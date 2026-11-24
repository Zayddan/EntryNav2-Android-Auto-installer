//! The project's signature on release archives (since 0.13.0-rc.10).
//!
//! Checks the archive structure (regular files only, nothing outside its folder),
//! then `UPDATE.sig` (RSA PKCS #1 v1.5 / SHA-256) and every file against `UPDATE`.

use anyhow::{anyhow, bail, Context, Result};
use rsa::pkcs1v15::{Signature, VerifyingKey};
use rsa::pkcs8::DecodePublicKey;
use rsa::signature::Verifier;
use rsa::RsaPublicKey;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Component, Path};

/// The project's public keys (DER SubjectPublicKeyInfo), copied from
/// `platform/update_key.hpp`. A new project key is added next to the old one.
/// update-key-1.der: SHA-256 4bd305847ad5ab466a786421629fe7f5136c50f23950f1828090c2649a44aabe
pub const PROJECT_KEYS: &[&[u8]] = &[include_bytes!("../keys/update-key-1.der")];

const UPDATE_FIRST_LINE: &str = "entrynav2-android-auto update 1";

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Status {
    /// Signed by the project, and every file matches the signed list.
    Signed,
    /// No signature (before 0.13.0-rc.10, or self-built): structure checked only.
    Unsigned,
}

struct ArchiveFile {
    mode: u32,
    size: u64,
    sha256: String,
}

/// Checks the archive of `version` with the project's keys.
pub fn check_archive(path: &Path, version: &str) -> Result<Status> {
    check_archive_with_keys(path, version, PROJECT_KEYS)
}

pub fn check_archive_with_keys(path: &Path, version: &str, keys: &[&[u8]]) -> Result<Status> {
    let top = format!("entrynav2-android-auto-{version}");
    let file =
        std::fs::File::open(path).with_context(|| format!("cannot open {}", path.display()))?;
    let mut archive = tar::Archive::new(file);

    let mut update: Option<Vec<u8>> = None;
    let mut update_sig: Option<Vec<u8>> = None;
    let mut files: BTreeMap<String, ArchiveFile> = BTreeMap::new();

    for entry in archive.entries().context("the archive cannot be read")? {
        let mut entry = entry.context("the archive cannot be read")?;
        let kind = entry.header().entry_type();
        let raw = String::from_utf8_lossy(&entry.path_bytes()).to_string();

        if !(kind.is_file() || kind.is_dir()) {
            bail!("the archive contains links or special files ({raw}): refused");
        }
        let p = Path::new(&raw);
        if p.components()
            .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
        {
            bail!("unsafe path in the archive ({raw}): refused");
        }
        let rel = match raw.trim_end_matches('/').strip_prefix(&top) {
            Some("") => continue,
            Some(rest) if rest.starts_with('/') => rest[1..].to_string(),
            _ => bail!("unexpected path in the archive ({raw}): refused"),
        };
        if kind.is_dir() {
            continue;
        }

        let mode = entry.header().mode().unwrap_or(0) & 0o777;
        let mut data = Vec::new();
        entry
            .read_to_end(&mut data)
            .context("the archive cannot be read")?;
        match rel.as_str() {
            "UPDATE" => update = Some(data),
            "UPDATE.sig" => update_sig = Some(data),
            name if name.starts_with("opt/") => {
                files.insert(
                    name.to_string(),
                    ArchiveFile {
                        mode,
                        size: data.len() as u64,
                        sha256: hex::encode(Sha256::digest(&data)),
                    },
                );
            }
            _ => {}
        }
    }

    let Some(sig) = update_sig else {
        return Ok(Status::Unsigned);
    };
    let list = update.ok_or_else(|| anyhow!("signed archive without its UPDATE list: refused"))?;

    let signature =
        Signature::try_from(sig.as_slice()).map_err(|_| anyhow!("malformed signature: refused"))?;
    let trusted = keys.iter().any(|der| {
        RsaPublicKey::from_public_key_der(der)
            .map(|key| {
                VerifyingKey::<Sha256>::new(key)
                    .verify(&list, &signature)
                    .is_ok()
            })
            .unwrap_or(false)
    });
    if !trusted {
        bail!("the archive's signature is not the project's: refused");
    }

    // The signed list must describe exactly the files the archive installs.
    let text = String::from_utf8(list).map_err(|_| anyhow!("unreadable UPDATE list: refused"))?;
    let mut lines = text.lines();
    if lines.next() != Some(UPDATE_FIRST_LINE) {
        bail!("unknown format of the signed list: this installer is too old for this archive");
    }
    match lines.next().and_then(|l| l.strip_prefix("version ")) {
        Some(v) if v == version => {}
        Some(v) => bail!("the signed list is for version {v}, not {version}: refused"),
        None => bail!("the signed list has no version: refused"),
    }
    let mut listed = 0;
    for line in lines.filter(|l| !l.trim().is_empty()) {
        let f: Vec<&str> = line.splitn(5, ' ').collect();
        let [kind, mode, size, sha, name] = f.as_slice() else {
            bail!("unreadable line in the signed list: refused");
        };
        if *kind != "file" {
            bail!("unknown entry in the signed list ({kind}): refused");
        }
        let actual = files.get(*name).ok_or_else(|| {
            anyhow!("{name} is in the signed list but not in the archive: refused")
        })?;
        let mode =
            u32::from_str_radix(mode, 8).map_err(|_| anyhow!("bad mode in the signed list"))?;
        if actual.size.to_string() != *size
            || actual.sha256 != sha.to_ascii_lowercase()
            || actual.mode != mode
        {
            bail!("{name} does not match the signed list (modified archive): refused");
        }
        listed += 1;
    }
    if listed != files.len() {
        bail!("the archive installs files that are not in the signed list: refused");
    }
    Ok(Status::Signed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rsa::pkcs1v15::SigningKey;
    use rsa::pkcs8::EncodePublicKey;
    use rsa::signature::{SignatureEncoding, Signer};
    use rsa::RsaPrivateKey;

    const V: &str = "9.9.9";

    struct Kit {
        key: RsaPrivateKey,
        public_der: Vec<u8>,
    }

    fn kit() -> Kit {
        let key = RsaPrivateKey::new(&mut rand::thread_rng(), 1024).unwrap();
        let public_der = key
            .to_public_key()
            .to_public_key_der()
            .unwrap()
            .as_bytes()
            .to_vec();
        Kit { key, public_der }
    }

    fn append(b: &mut tar::Builder<Vec<u8>>, name: &str, mode: u32, data: &[u8]) {
        let mut h = tar::Header::new_ustar();
        h.set_size(data.len() as u64);
        h.set_mode(mode);
        h.set_entry_type(tar::EntryType::Regular);
        h.set_path(name).unwrap();
        h.set_cksum();
        b.append(&h, data).unwrap();
    }

    /// An archive like the project's, signed with `key` or not.
    fn archive(kit: &Kit, sign: bool, tamper: bool) -> tempfile_path::TempFile {
        let top = format!("entrynav2-android-auto-{V}");
        let bin: &[u8] = b"\x7fELF fake program";
        let lib: &[u8] = b"fake plugin";
        let update = format!(
            "{UPDATE_FIRST_LINE}\nversion {V}\nfile 755 {} {} opt/aa/entry-aad\nfile 644 {} {} opt/hmi/data/ro/Plugins/EntryAad/x.so\n",
            bin.len(),
            hex::encode(Sha256::digest(bin)),
            lib.len(),
            hex::encode(Sha256::digest(lib)),
        );
        let mut b = tar::Builder::new(Vec::new());
        append(&mut b, &format!("{top}/MD5SUMS"), 0o644, b"");
        append(
            &mut b,
            &format!("{top}/opt/aa/entry-aad"),
            0o755,
            if tamper { b"evil" } else { bin },
        );
        append(
            &mut b,
            &format!("{top}/opt/hmi/data/ro/Plugins/EntryAad/x.so"),
            0o644,
            lib,
        );
        append(&mut b, &format!("{top}/UPDATE"), 0o644, update.as_bytes());
        if sign {
            let sig = SigningKey::<Sha256>::new(kit.key.clone())
                .sign(update.as_bytes())
                .to_vec();
            append(&mut b, &format!("{top}/UPDATE.sig"), 0o644, &sig);
        }
        tempfile_path::TempFile::with(b.into_inner().unwrap())
    }

    mod tempfile_path {
        pub struct TempFile(pub std::path::PathBuf);
        impl TempFile {
            pub fn with(data: Vec<u8>) -> Self {
                use std::sync::atomic::{AtomicUsize, Ordering};
                static N: AtomicUsize = AtomicUsize::new(0);
                let p = std::env::temp_dir().join(format!(
                    "entrynav2-sigtest-{}-{}.tar",
                    std::process::id(),
                    N.fetch_add(1, Ordering::SeqCst)
                ));
                std::fs::write(&p, data).unwrap();
                TempFile(p)
            }
        }
        impl Drop for TempFile {
            fn drop(&mut self) {
                let _ = std::fs::remove_file(&self.0);
            }
        }
    }

    #[test]
    fn signed_unsigned_tampered_and_wrong_key() {
        let k = kit();
        let keys: &[&[u8]] = &[&k.public_der];
        let a = archive(&k, true, false);
        assert_eq!(
            check_archive_with_keys(&a.0, V, keys).unwrap(),
            Status::Signed
        );

        let a = archive(&k, false, false);
        assert_eq!(
            check_archive_with_keys(&a.0, V, keys).unwrap(),
            Status::Unsigned
        );

        let a = archive(&k, true, true);
        let e = check_archive_with_keys(&a.0, V, keys)
            .unwrap_err()
            .to_string();
        assert!(e.contains("does not match the signed list"), "{e}");

        let other = kit();
        let a = archive(&other, true, false);
        let e = check_archive_with_keys(&a.0, V, keys)
            .unwrap_err()
            .to_string();
        assert!(e.contains("not the project's"), "{e}");

        let a = archive(&k, true, false);
        let e = check_archive_with_keys(&a.0, "1.0.0", keys)
            .unwrap_err()
            .to_string();
        assert!(e.contains("unexpected path"), "{e}");
    }

    #[test]
    fn unsafe_entries_are_refused() {
        // A path escaping the folder, written raw (tar::Builder refuses "..").
        let mut b = tar::Builder::new(Vec::new());
        let mut h = tar::Header::new_ustar();
        let name = format!("entrynav2-android-auto-{V}/../../etc/passwd");
        h.as_old_mut().name[..name.len()].copy_from_slice(name.as_bytes());
        h.set_size(1);
        h.set_mode(0o644);
        h.set_entry_type(tar::EntryType::Regular);
        h.set_cksum();
        b.append(&h, &b"x"[..]).unwrap();
        let a = tempfile_path::TempFile::with(b.into_inner().unwrap());
        let e = check_archive_with_keys(&a.0, V, &[])
            .unwrap_err()
            .to_string();
        assert!(e.contains("unsafe path"), "{e}");

        let mut b = tar::Builder::new(Vec::new());
        let mut h = tar::Header::new_ustar();
        h.set_entry_type(tar::EntryType::Symlink);
        h.set_path(format!("entrynav2-android-auto-{V}/opt/aa/entry-aad"))
            .unwrap();
        h.set_link_name("/etc/shadow").unwrap();
        h.set_size(0);
        h.set_cksum();
        b.append(&h, &b""[..]).unwrap();
        let a = tempfile_path::TempFile::with(b.into_inner().unwrap());
        let e = check_archive_with_keys(&a.0, V, &[])
            .unwrap_err()
            .to_string();
        assert!(e.contains("links or special files"), "{e}");
    }

    #[test]
    fn project_key_is_valid() {
        for der in PROJECT_KEYS {
            assert!(RsaPublicKey::from_public_key_der(der).is_ok());
        }
    }

    /// The real archive of a signed release, e.g.
    /// `ENTRYNAV_TEST_ARCHIVE=…/entrynav2-android-auto-0.13.0-rc.10-armhf.tar
    ///  ENTRYNAV_TEST_VERSION=0.13.0-rc.10 cargo test real_release -- --ignored`
    #[test]
    #[ignore]
    fn real_release_is_signed_by_the_project() {
        let path = std::env::var("ENTRYNAV_TEST_ARCHIVE").expect("ENTRYNAV_TEST_ARCHIVE");
        let version = std::env::var("ENTRYNAV_TEST_VERSION").expect("ENTRYNAV_TEST_VERSION");
        assert_eq!(
            check_archive(Path::new(&path), &version).unwrap(),
            Status::Signed
        );
    }
}
