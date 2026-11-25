//! Replaces this installer with a newer release, then restarts it.

use crate::github::{self, InstallerUpdate, INSTALLER_REPO};
use anyhow::{anyhow, bail, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn download(u: &InstallerUpdate, progress: impl FnMut(u64, u64) -> bool) -> Result<PathBuf> {
    let expected = github::published_sha256(INSTALLER_REPO, &u.asset, u.sha_asset.as_ref())?
        .ok_or_else(|| {
            anyhow!(
                "no SHA-256 is published for this version: update from the release page instead"
            )
        })?;
    let dir = std::env::temp_dir().join("entrynav2-installer-update");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    let file = dir.join(&u.asset.name);
    let sha = github::download_from(INSTALLER_REPO, &u.asset, &file, progress)?;
    if sha != expected {
        let _ = std::fs::remove_dir_all(&dir);
        bail!("the download does not match its published SHA-256. Nothing was changed.");
    }
    Ok(file)
}

fn manual() -> String {
    format!("update from https://github.com/{INSTALLER_REPO}/releases/latest instead")
}

#[cfg(target_os = "windows")]
fn old_copy(target: &Path) -> PathBuf {
    target.with_extension("old")
}

#[cfg(target_os = "macos")]
fn old_copy(target: &Path) -> PathBuf {
    PathBuf::from(format!("{}.old", target.display()))
}

#[cfg(target_os = "windows")]
fn target() -> Result<PathBuf> {
    Ok(std::env::current_exe()?)
}

#[cfg(target_os = "macos")]
fn target() -> Result<PathBuf> {
    let exe = std::env::current_exe()?.canonicalize()?;
    if exe.to_string_lossy().contains("/AppTranslocation/") {
        bail!("macOS runs this app from a temporary place. Move it to Applications, open it again, then update.");
    }
    exe.ancestors()
        .nth(3)
        .filter(|p| p.extension().is_some_and(|e| e == "app"))
        .map(Path::to_path_buf)
        .ok_or_else(|| anyhow!("the installer is not running from its .app: {}", manual()))
}

#[cfg(target_os = "windows")]
pub fn replace(downloaded: &Path) -> Result<PathBuf> {
    let exe = target()?;
    let (new, old) = (exe.with_extension("new"), old_copy(&exe));
    std::fs::copy(downloaded, &new)
        .map_err(|e| anyhow!("cannot write next to the installer ({e}): {}", manual()))?;
    let _ = std::fs::remove_file(&old);
    if let Err(e) = std::fs::rename(&exe, &old) {
        let _ = std::fs::remove_file(&new);
        bail!("cannot replace the installer ({e}): {}", manual());
    }
    if let Err(e) = std::fs::rename(&new, &exe) {
        let _ = std::fs::rename(&old, &exe);
        let _ = std::fs::remove_file(&new);
        bail!("cannot replace the installer ({e}): {}", manual());
    }
    Ok(exe)
}

#[cfg(target_os = "macos")]
pub fn replace(downloaded: &Path) -> Result<PathBuf> {
    let app = target()?;
    let parent = app
        .parent()
        .ok_or_else(|| anyhow!("unexpected location: {}", manual()))?;
    let stage = parent.join(".entrynav2-installer-update");
    let _ = std::fs::remove_dir_all(&stage);
    std::fs::create_dir(&stage)
        .map_err(|e| anyhow!("cannot write in {} ({e}): {}", parent.display(), manual()))?;
    let unpacked = Command::new("/usr/bin/ditto")
        .args(["-x", "-k"])
        .arg(downloaded)
        .arg(&stage)
        .status()
        .is_ok_and(|s| s.success());
    let new_app = std::fs::read_dir(&stage)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .find(|p| p.extension().is_some_and(|e| e == "app"));
    let Some(new_app) = new_app.filter(|_| unpacked) else {
        let _ = std::fs::remove_dir_all(&stage);
        bail!("could not unpack the update: {}", manual());
    };
    let old = old_copy(&app);
    let _ = std::fs::remove_dir_all(&old);
    if let Err(e) = std::fs::rename(&app, &old) {
        let _ = std::fs::remove_dir_all(&stage);
        bail!("cannot replace the installer ({e}): {}", manual());
    }
    if let Err(e) = std::fs::rename(&new_app, &app) {
        let _ = std::fs::rename(&old, &app);
        let _ = std::fs::remove_dir_all(&stage);
        bail!("cannot replace the installer ({e}): {}", manual());
    }
    let _ = std::fs::remove_dir_all(&stage);
    Ok(app)
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub fn replace(_downloaded: &Path) -> Result<PathBuf> {
    bail!(
        "automatic update is not available on this system: {}",
        manual()
    )
}

pub fn relaunch(target: &Path) -> Result<()> {
    #[cfg(target_os = "macos")]
    Command::new("/usr/bin/open")
        .arg("-n")
        .arg(target)
        .spawn()?;
    #[cfg(not(target_os = "macos"))]
    Command::new(target).spawn()?;
    Ok(())
}

pub fn cleanup() {
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    if let Ok(t) = target() {
        let old = old_copy(&t);
        let _ = std::fs::remove_file(&old);
        let _ = std::fs::remove_dir_all(&old);
    }
    let _ = std::fs::remove_dir_all(std::env::temp_dir().join("entrynav2-installer-update"));
}
