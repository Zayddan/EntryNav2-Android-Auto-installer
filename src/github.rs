//! GitHub releases of the project, and archive downloads.

use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use std::time::Duration;

pub const REPO: &str = "Jiraiya43/EntryNav2-Android-Auto";
pub const ASSET_PREFIX: &str = "entrynav2-android-auto-";
pub const ASSET_SUFFIX: &str = "-armhf.tar";

#[derive(Debug, Clone, Deserialize)]
pub struct Asset {
    pub name: String,
    pub browser_download_url: String,
    #[serde(default)]
    pub size: u64,
    /// Provided by GitHub since 2025 as "sha256:<hex>".
    #[serde(default)]
    pub digest: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Release {
    pub tag_name: String,
    #[serde(default)]
    pub prerelease: bool,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub published_at: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub assets: Vec<Asset>,
}

/// Installable archive of a release.
#[derive(Debug, Clone)]
pub struct Package {
    pub version: String,
    pub asset: Asset,
    pub sha_asset: Option<Asset>,
}

impl Release {
    /// Why this installer cannot install this release, if it cannot.
    pub fn unsupported_reason(&self) -> Option<&'static str> {
        self.package().is_none().then_some(
            "Older archive format (entry-aad-…, before 0.12.0-rc.12) with its own installation procedure. \
             This installer does not handle it: follow the INSTALLATION.md inside its archive.",
        )
    }

    /// Version number shown for any release, supported or not.
    pub fn display_version(&self) -> String {
        match self.package() {
            Some(p) => p.version,
            None => self.tag_name.trim_start_matches('v').to_string(),
        }
    }

    /// The release notes say it was not tried in a car yet.
    pub fn not_tried_in_car(&self) -> bool {
        let body = self.body.as_deref().unwrap_or("").to_lowercase();
        body.contains("pas encore essayée en voiture") || body.contains("not yet tried in a car")
    }

    pub fn published_date(&self) -> &str {
        self.published_at
            .as_deref()
            .and_then(|d| d.get(..10))
            .unwrap_or("")
    }

    pub fn package(&self) -> Option<Package> {
        let asset = self
            .assets
            .iter()
            .find(|a| version_from_filename(&a.name).is_some())?;
        let sha_name = format!("{}.sha256", asset.name);
        Some(Package {
            version: version_from_filename(&asset.name)?,
            sha_asset: self.assets.iter().find(|a| a.name == sha_name).cloned(),
            asset: asset.clone(),
        })
    }
}

/// SemVer order: `0.12.0-rc.12 < 0.12.0 < 0.13.0-rc.1 < 0.13.0-rc.10`.
pub fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    fn split(v: &str) -> (Vec<u64>, Option<&str>) {
        let v = v.trim().trim_start_matches('v');
        let (core, pre) = match v.split_once('-') {
            Some((c, p)) => (c, Some(p)),
            None => (v, None),
        };
        (
            core.split('.').map(|x| x.parse().unwrap_or(0)).collect(),
            pre,
        )
    }
    let (ca, pa) = split(a);
    let (cb, pb) = split(b);
    for i in 0..ca.len().max(cb.len()) {
        let o = ca.get(i).unwrap_or(&0).cmp(cb.get(i).unwrap_or(&0));
        if o != Ordering::Equal {
            return o;
        }
    }
    match (pa, pb) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(pa), Some(pb)) => {
            let (ia, ib): (Vec<&str>, Vec<&str>) =
                (pa.split('.').collect(), pb.split('.').collect());
            for (x, y) in ia.iter().zip(ib.iter()) {
                let o = match (x.parse::<u64>(), y.parse::<u64>()) {
                    (Ok(x), Ok(y)) => x.cmp(&y),
                    (Ok(_), Err(_)) => Ordering::Less,
                    (Err(_), Ok(_)) => Ordering::Greater,
                    (Err(_), Err(_)) => x.cmp(y),
                };
                if o != Ordering::Equal {
                    return o;
                }
            }
            ia.len().cmp(&ib.len())
        }
    }
}

/// `entrynav2-android-auto-0.12.0-armhf.tar` -> `0.12.0`
pub fn version_from_filename(name: &str) -> Option<String> {
    let v = name
        .strip_prefix(ASSET_PREFIX)?
        .strip_suffix(ASSET_SUFFIX)?;
    crate::scripts::is_safe_token(v).then(|| v.to_string())
}

pub fn archive_name(version: &str) -> String {
    format!("{ASSET_PREFIX}{version}{ASSET_SUFFIX}")
}

/// A SHA-256 written as 64 hex characters.
pub fn normalize_sha256(s: &str) -> Option<String> {
    let s = s.trim().to_ascii_lowercase();
    let s = s.strip_prefix("sha256:").unwrap_or(&s).to_string();
    (s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit())).then_some(s)
}

/// Downloads are only accepted from the project's GitHub release pages.
fn check_url(url: &str) -> Result<()> {
    let allowed = format!("https://github.com/{REPO}/releases/download/");
    if !url.starts_with(&allowed) {
        bail!("refusing to download from an unexpected address: {url}");
    }
    Ok(())
}

fn client(timeout: Duration) -> Result<reqwest::blocking::Client> {
    Ok(reqwest::blocking::Client::builder()
        .user_agent(concat!("entrynav2-installer/", env!("CARGO_PKG_VERSION")))
        .https_only(true)
        .connect_timeout(Duration::from_secs(15))
        .timeout(timeout)
        .build()?)
}

/// Short time limit for API calls; downloads get longer.
const API_TIMEOUT: Duration = Duration::from_secs(30);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(900);

/// An HTTP status from GitHub, in plain words.
fn status_text(status: reqwest::StatusCode) -> String {
    match status.as_u16() {
        403 | 429 => {
            "GitHub limits the number of requests for now. Try again in a few minutes.".into()
        }
        404 => "Not found on GitHub (it may have been removed).".into(),
        code @ 500..=599 => {
            format!("GitHub is having trouble right now (error {code}). Try again in a moment.")
        }
        code => format!("GitHub answered with error {code}."),
    }
}

/// A network error, in plain words (no URLs or internals).
fn network_text(e: &reqwest::Error) -> String {
    if let Some(status) = e.status() {
        status_text(status)
    } else if e.is_timeout() {
        "GitHub did not answer in time. Check the internet connection and try again.".into()
    } else if e.is_connect() {
        "Cannot reach GitHub. Check the internet connection.".into()
    } else {
        "Network error while talking to GitHub. Try again.".into()
    }
}

/// GET with up to two retries on temporary failures.
fn get(url: &str, accept: Option<&str>, timeout: Duration) -> Result<reqwest::blocking::Response> {
    let client = client(timeout)?;
    let mut last = String::new();
    for attempt in 0..3u64 {
        if attempt > 0 {
            std::thread::sleep(Duration::from_secs(2 * attempt));
        }
        let mut request = client.get(url);
        if let Some(a) = accept {
            request = request.header("Accept", a);
        }
        match request.send() {
            Ok(resp) if resp.status().is_success() => return Ok(resp),
            Ok(resp) => {
                last = status_text(resp.status());
                if !resp.status().is_server_error() {
                    break;
                }
            }
            Err(e) => {
                last = network_text(&e);
                if !(e.is_timeout() || e.is_connect()) {
                    break;
                }
            }
        }
    }
    bail!("{last}")
}

pub fn list_releases() -> Result<Vec<Release>> {
    let url = format!("https://api.github.com/repos/{REPO}/releases?per_page=50");
    let resp = get(&url, Some("application/vnd.github+json"), API_TIMEOUT)?;
    let releases: Vec<Release> = resp
        .json()
        .map_err(|_| anyhow!("GitHub sent an incomplete answer. Try again."))?;
    Ok(releases
        .into_iter()
        .filter(|r| !r.draft && r.assets.iter().any(|a| a.name.ends_with(".tar")))
        .collect())
}

/// Reference SHA-256 published for the archive, if any.
pub fn expected_sha256(pkg: &Package) -> Result<Option<String>> {
    if let Some(hex) = pkg.asset.digest.as_deref().and_then(normalize_sha256) {
        return Ok(Some(hex));
    }
    if let Some(sha) = &pkg.sha_asset {
        check_url(&sha.browser_download_url)?;
        let text = get(&sha.browser_download_url, None, API_TIMEOUT)
            .context("could not get the published SHA-256")?
            .text()
            .map_err(|_| anyhow!("GitHub sent an incomplete answer. Try again."))?;
        let first = text
            .split_whitespace()
            .next()
            .ok_or_else(|| anyhow!("empty .sha256 file"))?;
        let hex = normalize_sha256(first).ok_or_else(|| anyhow!("invalid .sha256 file"))?;
        return Ok(Some(hex));
    }
    Ok(None)
}

/// Downloads the archive to `dest` and returns its SHA-256. `progress` returns
/// false to cancel.
pub fn download(
    asset: &Asset,
    dest: &Path,
    mut progress: impl FnMut(u64, u64) -> bool,
) -> Result<String> {
    check_url(&asset.browser_download_url)?;
    let mut resp = get(&asset.browser_download_url, None, DOWNLOAD_TIMEOUT)?;
    let total = resp.content_length().unwrap_or(asset.size);
    let tmp = dest.with_extension("part");
    let mut file = File::create(&tmp)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    let mut done = 0u64;
    loop {
        let n = match resp.read(&mut buf) {
            Ok(n) => n,
            Err(_) => {
                drop(file);
                let _ = std::fs::remove_file(&tmp);
                bail!("The download was interrupted. Check the internet connection and try again.");
            }
        };
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])?;
        hasher.update(&buf[..n]);
        done += n as u64;
        if !progress(done, total) {
            drop(file);
            let _ = std::fs::remove_file(&tmp);
            return Err(crate::worker::Cancelled.into());
        }
    }
    file.flush()?;
    drop(file);
    if total > 0 && done != total {
        let _ = std::fs::remove_file(&tmp);
        bail!("The download is incomplete. Try again.");
    }
    std::fs::rename(&tmp, dest)?;
    Ok(hex::encode(hasher.finalize()))
}

pub fn sha256_file(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_parsing() {
        assert_eq!(
            version_from_filename("entrynav2-android-auto-0.13.0-rc.9-armhf.tar").as_deref(),
            Some("0.13.0-rc.9")
        );
        assert_eq!(version_from_filename("entry-aad-0.11.0.tar"), None);
        assert_eq!(
            version_from_filename("entrynav2-android-auto-x;rm-armhf.tar"),
            None
        );
        assert_eq!(
            archive_name("0.12.0"),
            "entrynav2-android-auto-0.12.0-armhf.tar"
        );
    }

    #[test]
    fn version_ordering() {
        use std::cmp::Ordering::*;
        let order = [
            "0.12.0-rc.1",
            "0.12.0-rc.12",
            "0.12.0",
            "0.12.1-rc.4",
            "0.13.0-alpha.2",
            "0.13.0-rc.1",
            "0.13.0-rc.9",
            "0.13.0-rc.10",
            "0.13.0",
        ];
        for w in order.windows(2) {
            assert_eq!(compare_versions(w[0], w[1]), Less, "{} < {}", w[0], w[1]);
            assert_eq!(compare_versions(w[1], w[0]), Greater);
        }
        assert_eq!(compare_versions("0.12.0", "v0.12.0"), Equal);
    }

    #[test]
    fn errors_are_short_and_plain() {
        use reqwest::StatusCode;
        for (code, needle) in [
            (504, "trouble right now (error 504)"),
            (502, "trouble right now"),
            (403, "limits the number of requests"),
            (429, "limits the number of requests"),
            (404, "Not found"),
        ] {
            let text = status_text(StatusCode::from_u16(code).unwrap());
            assert!(text.contains(needle), "{code}: {text}");
            assert!(!text.contains("http") && text.len() < 100, "{text}");
        }
    }

    #[test]
    fn sha_and_url_checks() {
        let h = "5bc0b84c990e09ce38513feb572a9a36daab7db4ff1da4e2f5cde1f0df4f1062";
        assert_eq!(normalize_sha256(&format!("sha256:{h}")).as_deref(), Some(h));
        assert_eq!(normalize_sha256(&h.to_uppercase()).as_deref(), Some(h));
        assert_eq!(normalize_sha256("abc"), None);
        assert!(check_url(&format!(
            "https://github.com/{REPO}/releases/download/v1/x.tar"
        ))
        .is_ok());
        assert!(check_url("https://evil.example/x.tar").is_err());
        assert!(check_url(&format!(
            "http://github.com/{REPO}/releases/download/v1/x.tar"
        ))
        .is_err());
    }
}
