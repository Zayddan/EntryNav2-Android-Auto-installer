//! USB sticks for the project's own update (since 0.13.0-rc.10).
//!
//! The archive is copied as is to the root of the stick; the car writes its
//! report to `EntryAA-update.txt`.

use std::path::{Path, PathBuf};

pub const REPORT_FILE: &str = "EntryAA-update.txt";

#[derive(Debug, Clone, PartialEq)]
pub struct Drive {
    /// Root of the stick ("E:\" on Windows, "/Volumes/NAME" on macOS).
    pub root: PathBuf,
    pub label: String,
    /// File system as the system names it (FAT32, exFAT, NTFS, msdos…).
    pub fs: String,
    pub free: Option<u64>,
}

impl Drive {
    /// File systems the car reads: FAT32, exFAT, NTFS.
    pub fn supported(&self) -> bool {
        matches!(
            self.fs.to_ascii_lowercase().as_str(),
            "fat32" | "exfat" | "ntfs" | "msdos"
        )
    }

    pub fn display(&self) -> String {
        let free = self
            .free
            .map(|b| format!(" · {:.1} GB free", b as f64 / 1e9))
            .unwrap_or_default();
        let label = if self.label.is_empty() {
            "USB stick"
        } else {
            &self.label
        };
        format!("{}  {label} ({}){free}", self.root.display(), self.fs)
    }
}

/// `entrynav2-android-auto-….tar` (the car's own rule, case-insensitive).
pub fn is_archive_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.starts_with("entrynav2-android-auto-") && lower.ends_with(".tar")
}

/// The car's report of its last update from this stick, if any.
pub fn read_report(root: &Path) -> Option<String> {
    let text = std::fs::read(root.join(REPORT_FILE)).ok()?;
    let text = String::from_utf8_lossy(&text[..text.len().min(64 * 1024)]).to_string();
    (!text.trim().is_empty()).then_some(text)
}

/// The archives of the project already at the root of the stick.
pub fn archives_on(root: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(root)
        .map(|it| {
            it.filter_map(|e| e.ok())
                .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
                .filter_map(|e| e.file_name().to_str().map(String::from))
                .filter(|n| is_archive_name(n))
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

#[cfg(windows)]
mod sys {
    use super::Drive;
    use std::path::{Component, Path, PathBuf};
    use windows_sys::Win32::Storage::FileSystem::{
        GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW,
    };

    const DRIVE_REMOVABLE: u32 = 2;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn from_wide(buf: &[u16]) -> String {
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        String::from_utf16_lossy(&buf[..end])
    }

    pub fn describe(root: &Path) -> Option<Drive> {
        let root_str = root.to_str()?;
        let w = wide(root_str);
        let mut label = [0u16; 261];
        let mut fs = [0u16; 261];
        let ok = unsafe {
            GetVolumeInformationW(
                w.as_ptr(),
                label.as_mut_ptr(),
                label.len() as u32,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                fs.as_mut_ptr(),
                fs.len() as u32,
            )
        };
        if ok == 0 {
            return None;
        }
        let mut free = 0u64;
        let has_free = unsafe {
            GetDiskFreeSpaceExW(
                w.as_ptr(),
                &mut free,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        } != 0;
        Some(Drive {
            root: root.to_path_buf(),
            label: from_wide(&label),
            fs: from_wide(&fs),
            free: has_free.then_some(free),
        })
    }

    pub fn removable_drives() -> Vec<Drive> {
        let mask = unsafe { GetLogicalDrives() };
        (0..26u8)
            .filter(|i| mask & (1 << i) != 0)
            .map(|i| format!("{}:\\", (b'A' + i) as char))
            .filter(|root| unsafe { GetDriveTypeW(wide(root).as_ptr()) } == DRIVE_REMOVABLE)
            .filter_map(|root| describe(Path::new(&root)))
            .collect()
    }

    /// "E:\" for any path on drive E.
    pub fn root_of(path: &Path) -> Option<PathBuf> {
        match path.components().next()? {
            Component::Prefix(p) => Some(PathBuf::from(format!("{}\\", p.as_os_str().to_str()?))),
            _ => None,
        }
    }
}

#[cfg(target_os = "macos")]
mod sys {
    use super::Drive;
    use std::ffi::{CStr, CString};
    use std::path::{Path, PathBuf};

    pub fn describe(root: &Path) -> Option<Drive> {
        let c = CString::new(root.to_str()?).ok()?;
        let mut st: libc::statfs = unsafe { std::mem::zeroed() };
        if unsafe { libc::statfs(c.as_ptr(), &mut st) } != 0 {
            return None;
        }
        let fs = unsafe { CStr::from_ptr(st.f_fstypename.as_ptr()) }
            .to_string_lossy()
            .to_string();
        Some(Drive {
            root: root.to_path_buf(),
            label: root.file_name()?.to_string_lossy().to_string(),
            fs,
            free: Some(st.f_bavail as u64 * st.f_bsize as u64),
        })
    }

    /// Volumes mounted under /Volumes, except the system disk.
    pub fn removable_drives() -> Vec<Drive> {
        let Ok(entries) = std::fs::read_dir("/Volumes") else {
            return Vec::new();
        };
        entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                std::fs::canonicalize(p)
                    .map(|c| c != Path::new("/"))
                    .unwrap_or(false)
            })
            .filter_map(|p| describe(&p))
            .filter(|d| d.supported())
            .collect()
    }

    /// "/Volumes/NAME" for any path on that volume.
    pub fn root_of(path: &Path) -> Option<PathBuf> {
        let mut it = path.components();
        let (a, b, c) = (it.next()?, it.next()?, it.next()?);
        (b.as_os_str() == "Volumes").then(|| PathBuf::from(a.as_os_str()).join(b).join(c))
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod sys {
    use super::Drive;
    use std::path::{Path, PathBuf};

    pub fn describe(_root: &Path) -> Option<Drive> {
        None
    }
    pub fn removable_drives() -> Vec<Drive> {
        Vec::new()
    }
    pub fn root_of(_path: &Path) -> Option<PathBuf> {
        None
    }
}

/// The disk the system runs from (C:\ on Windows, / on macOS).
fn is_system_drive(root: &Path) -> bool {
    #[cfg(windows)]
    {
        let system = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
        root.to_string_lossy()
            .trim_end_matches('\\')
            .eq_ignore_ascii_case(system.trim_end_matches('\\'))
    }
    #[cfg(not(windows))]
    {
        std::fs::canonicalize(root)
            .map(|c| c == Path::new("/"))
            .unwrap_or(false)
    }
}

/// USB sticks plugged in now.
pub fn removable_drives() -> Vec<Drive> {
    sys::removable_drives()
}

/// A folder chosen by hand: accepted only if it is the root of a drive.
pub fn drive_for_folder(folder: &Path) -> Result<Drive, String> {
    let root = sys::root_of(folder).ok_or("This is not a drive.")?;
    if is_system_drive(&root) {
        return Err("This is the computer's own disk, not a USB stick.".into());
    }
    if root.as_path() != folder {
        return Err(format!(
            "Choose the root of the USB stick ({}), not a folder inside it: the car only looks there.",
            root.display()
        ));
    }
    sys::describe(&root).ok_or_else(|| "Cannot read this drive.".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Lists the sticks plugged in (read only):
    /// `cargo test list_drives -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn list_drives() {
        for d in removable_drives() {
            println!("stick: {} supported={}", d.display(), d.supported());
        }
    }

    #[cfg(windows)]
    #[test]
    fn system_disk_and_subfolders_are_refused() {
        assert!(drive_for_folder(Path::new("C:\\"))
            .unwrap_err()
            .contains("computer's own disk"));
        assert!(drive_for_folder(Path::new("C:\\Windows")).is_err());
    }

    #[test]
    fn archive_names_follow_the_cars_rule() {
        assert!(is_archive_name(
            "entrynav2-android-auto-0.13.0-rc.10-armhf.tar"
        ));
        assert!(is_archive_name("EntryNav2-Android-Auto-0.13.0 (1).TAR"));
        assert!(!is_archive_name("entry-aad-0.12.0-armhf.tar"));
        assert!(!is_archive_name(
            "entrynav2-android-auto-0.13.0-armhf.tar.part"
        ));
    }

    #[test]
    fn supported_file_systems() {
        let d = |fs: &str| Drive {
            root: PathBuf::from("E:\\"),
            label: String::new(),
            fs: fs.into(),
            free: None,
        };
        for fs in ["FAT32", "exFAT", "NTFS", "msdos"] {
            assert!(d(fs).supported(), "{fs}");
        }
        for fs in ["FAT", "apfs", "hfs"] {
            assert!(!d(fs).supported(), "{fs}");
        }
    }

    #[test]
    fn report_and_archives_on_a_folder() {
        let dir = std::env::temp_dir().join(format!("entrynav2-usb-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("entrynav2-android-auto-0.13.0-rc.10-armhf.tar"),
            b"x",
        )
        .unwrap();
        std::fs::write(dir.join("notes.txt"), b"x").unwrap();
        std::fs::write(dir.join(REPORT_FILE), "Installed 0.13.0-rc.10\n").unwrap();
        assert_eq!(
            archives_on(&dir),
            vec!["entrynav2-android-auto-0.13.0-rc.10-armhf.tar"]
        );
        assert!(read_report(&dir).unwrap().contains("Installed"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
