//! Long-running tasks, executed off the UI thread.

use crate::car::{Car, ConnParams};
use crate::github::{self, Package};
use crate::scripts::{self, CarInfo, CheckReport, CompatLevel, Vehicle, Verdict};
use crate::signature;
use anyhow::{bail, Result};
use eframe::egui;
use std::fmt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;

/// The user cancelled the task (download, upload): not an error.
#[derive(Debug)]
pub struct Cancelled;

impl fmt::Display for Cancelled {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "cancelled")
    }
}

impl std::error::Error for Cancelled {}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Level {
    Info,
    Ok,
    Warn,
    Error,
    Remote,
}

pub enum Msg {
    Log(Level, String),
    Progress(Option<(String, f32)>),
    Releases(Result<Vec<github::Release>, String>),
    CarInfo(CarInfo),
    Checked(CheckReport),
    Installed(String),
    /// The installation did not complete; the text says what to do.
    InstallFailed(String),
    Uninstalled,
    /// A USB update stick is ready.
    UsbWritten,
    /// The archive cannot be verified: the user must agree before its program runs.
    NeedsConsent(String),
    InstallerUpdate(github::InstallerUpdate),
    InstallerReplaced,
    TaskEnd,
}

#[derive(Clone)]
pub struct Ctx {
    tx: Sender<Msg>,
    egui: egui::Context,
    cancel: Arc<AtomicBool>,
}

impl Ctx {
    pub fn new(tx: Sender<Msg>, egui: egui::Context) -> Self {
        Self {
            tx,
            egui,
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }
    /// Set to true to cancel the task at its next safe point.
    pub fn cancel_flag(&self) -> Arc<AtomicBool> {
        self.cancel.clone()
    }
    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
    fn stop_if_cancelled(&self) -> Result<()> {
        if self.cancelled() {
            return Err(Cancelled.into());
        }
        Ok(())
    }
    fn send(&self, m: Msg) {
        let _ = self.tx.send(m);
        self.egui.request_repaint();
    }
    fn log(&self, l: Level, s: impl Into<String>) {
        self.send(Msg::Log(l, s.into()));
    }
    fn info(&self, s: impl Into<String>) {
        self.log(Level::Info, s)
    }
    fn ok(&self, s: impl Into<String>) {
        self.log(Level::Ok, s)
    }
    fn warn(&self, s: impl Into<String>) {
        self.log(Level::Warn, s)
    }
    fn progress(&self, label: &str, done: u64, total: u64) {
        let f = if total > 0 {
            done as f32 / total as f32
        } else {
            0.0
        };
        self.send(Msg::Progress(Some((label.to_string(), f))));
    }
    fn busy(&self, label: &str) {
        self.send(Msg::Progress(Some((label.to_string(), -1.0))));
    }
}

/// Runs `f` on a thread; errors are shown in the log.
pub fn spawn(ctx: Ctx, f: impl FnOnce(&Ctx) -> Result<()> + Send + 'static) {
    std::thread::spawn(move || {
        if let Err(e) = f(&ctx) {
            if e.downcast_ref::<Cancelled>().is_some() {
                ctx.info("Cancelled. Nothing was changed on the car.");
            } else {
                ctx.log(Level::Error, format!("Error: {e:#}"));
            }
        }
        ctx.send(Msg::Progress(None));
        ctx.send(Msg::TaskEnd);
    });
}

pub enum Source {
    GitHub(Package),
    /// Path, version, optional SHA-256 typed by the user.
    Local(PathBuf, String, Option<String>),
}

impl Source {
    pub fn version(&self) -> &str {
        match self {
            Source::GitHub(p) => &p.version,
            Source::Local(_, v, _) => v,
        }
    }

    /// Identifies the exact archive a check result belongs to.
    pub fn key(&self) -> String {
        match self {
            Source::GitHub(p) => format!("github:{}", p.asset.browser_download_url),
            Source::Local(path, _, _) => {
                let meta = std::fs::metadata(path).ok();
                let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
                let modified = meta
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                format!("local:{}|{size}|{modified}", path.display())
            }
        }
    }
}

fn connect(ctx: &Ctx, conn: &ConnParams) -> Result<Car> {
    ctx.info(format!(
        "Connecting to {}@{} over SSH…",
        conn.user,
        conn.id()
    ));
    let car = Car::connect(conn)?;
    ctx.ok(format!(
        "Connected (head unit key SHA-256 {})",
        car.fingerprint
    ));
    Ok(car)
}

/// Runs a script and copies its output (except markers) to the log.
fn run_logged(ctx: &Ctx, car: &Car, script: &str) -> Result<(i32, Vec<String>)> {
    let mut lines = Vec::new();
    let rc = car.run(script, |l| {
        if let Some(w) = l.strip_prefix("@@WARN|") {
            ctx.warn(w);
        } else if !l.starts_with("@@") && !l.is_empty() {
            ctx.log(Level::Remote, l);
        }
        lines.push(l.to_string());
    })?;
    Ok((rc, lines))
}

/// Release numbers looked for in the car's program (see `scripts::info_script`).
static KNOWN_VERSIONS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

pub fn set_known_versions(releases: &[github::Release]) {
    let versions = releases.iter().map(|r| r.display_version()).collect();
    if let Ok(mut known) = KNOWN_VERSIONS.lock() {
        *known = versions;
    }
}

fn read_info(ctx: &Ctx, car: &Car) -> Result<CarInfo> {
    let known = KNOWN_VERSIONS.lock().map(|k| k.clone()).unwrap_or_default();
    let (_, lines) = car.run_collect(&scripts::info_script(&known))?;
    let info = scripts::parse_info(&lines);
    ctx.info(format!("Head unit system version: {}", info.sysver));
    ctx.info(match &info.installed {
        scripts::Installed::None => "Android Auto: not installed".to_string(),
        scripts::Installed::Unknown => {
            "Android Auto: installed, version not recognised (not a GitHub release, or versions not loaded)"
                .into()
        }
        scripts::Installed::Version(v) => format!("Android Auto: version {v} installed"),
    });
    if info.running {
        ctx.warn("Android Auto is running. It must be stopped before installing or uninstalling.");
    }
    if info.usb_mode {
        ctx.info("Android Auto is set to USB cable mode (usb file present)");
    }
    if info.disabled {
        ctx.warn("Android Auto is disabled (app.disable file present)");
    }
    if info.guard {
        ctx.info("Safety guard counter present (app-guard): normal within 2 minutes of a start");
    }
    ctx.send(Msg::CarInfo(info.clone()));
    Ok(info)
}

fn cleanup_tmp(ctx: &Ctx, car: &Car, version: &str) {
    if car.run_collect(&scripts::cleanup_script(version)).is_ok() {
        ctx.info("Temporary files removed from /tmp");
    }
}

/// Stops Android Auto cleanly if it runs (same sequence as the iDrive menu).
fn stop_android_auto(ctx: &Ctx, car: &Car) -> Result<()> {
    ctx.info("Stopping Android Auto (the phone session ends, the car's Bluetooth is given back)…");
    ctx.busy("Stopping Android Auto…");
    let (_, lines) = run_logged(ctx, car, scripts::STOP_SCRIPT)?;
    if let Some(err) = scripts::markers(&lines, "@@ERR|").first() {
        bail!("{err}");
    }
    match scripts::markers(&lines, "@@STOP_OK|").first() {
        Some(&"not running") => ctx.info("Android Auto was not running"),
        Some(_) => ctx.ok("Android Auto stopped"),
        None => bail!("could not confirm that Android Auto stopped"),
    }
    Ok(())
}

/// Stops Android Auto first when it runs and `auto_stop` allows it.
fn ensure_stopped(ctx: &Ctx, car: &Car, auto_stop: bool) -> Result<()> {
    let info = read_info(ctx, car)?;
    if info.running {
        if !auto_stop {
            bail!("Android Auto is running. Stop it first.");
        }
        stop_android_auto(ctx, car)?;
        read_info(ctx, car)?;
    }
    Ok(())
}

pub fn stop(ctx: &Ctx, conn: ConnParams) -> Result<()> {
    let car = connect(ctx, &conn)?;
    stop_android_auto(ctx, &car)?;
    read_info(ctx, &car)?;
    Ok(())
}

pub fn fetch_releases(ctx: &Ctx) -> Result<()> {
    ctx.info("Fetching versions from GitHub…");
    match github::list_releases() {
        Ok(r) => {
            ctx.ok(format!("{} version(s) found", r.len()));
            set_known_versions(&r);
            ctx.send(Msg::Releases(Ok(r)));
            Ok(())
        }
        Err(e) => {
            ctx.send(Msg::Releases(Err(format!("{e:#}"))));
            Err(e)
        }
    }
}

pub fn check_installer_update(ctx: Ctx) {
    std::thread::spawn(move || {
        if let Ok(Some(u)) = github::installer_update() {
            ctx.send(Msg::InstallerUpdate(u));
        }
    });
}

pub fn update_installer(ctx: &Ctx, update: github::InstallerUpdate) -> Result<()> {
    ctx.info(format!("Downloading installer {}…", update.version));
    let label = format!("Downloading installer {}", update.version);
    let file = crate::self_update::download(&update, |d, t| {
        ctx.progress(&label, d, t);
        !ctx.cancelled()
    })?;
    ctx.ok("Installer download verified (SHA-256)");
    ctx.busy("Replacing the installer…");
    let target = crate::self_update::replace(&file)?;
    let _ = std::fs::remove_file(&file);
    crate::self_update::relaunch(&target).map_err(|e| {
        anyhow::anyhow!(
            "installer {} is installed but could not restart by itself ({e}). Close it and open it again.",
            update.version
        )
    })?;
    ctx.ok(format!(
        "Installer {} installed. Restarting…",
        update.version
    ));
    ctx.send(Msg::InstallerReplaced);
    Ok(())
}

pub fn test_connection(ctx: &Ctx, conn: ConnParams) -> Result<()> {
    let car = connect(ctx, &conn)?;
    read_info(ctx, &car)?;
    Ok(())
}

/// The archive on this computer, and what was verified about it.
struct Obtained {
    path: PathBuf,
    /// Matched a published or user-supplied SHA-256.
    hash_verified: bool,
    /// Carries the project's valid signature.
    signed: bool,
}

/// Gets the archive, checks its SHA-256 and signature, and refuses an unsafe one.
fn obtain_package(ctx: &Ctx, source: &Source) -> Result<Obtained> {
    let (path, hash_verified, downloaded) = match source {
        Source::Local(path, _, expected) => {
            ctx.info(format!("Local archive: {}", path.display()));
            let sha = github::sha256_file(path)?;
            match expected {
                Some(exp) if *exp == sha => {
                    ctx.ok(format!("SHA-256 verified: {sha}"));
                    (path.clone(), true, false)
                }
                Some(exp) => bail!("SHA-256 mismatch!\n  expected: {exp}\n  actual:   {sha}"),
                None => {
                    ctx.info(format!("SHA-256: {sha} (no reference hash given)"));
                    (path.clone(), false, false)
                }
            }
        }
        Source::GitHub(pkg) => {
            let dir = std::env::temp_dir().join("entrynav2-installer");
            std::fs::create_dir_all(&dir)?;
            let dest = dir.join(&pkg.asset.name);
            let expected = github::expected_sha256(pkg)?;

            let cached = match (&expected, dest.exists()) {
                (Some(exp), true) => &github::sha256_file(&dest)? == exp,
                _ => false,
            };
            if cached {
                ctx.ok("Archive already downloaded and verified (SHA-256)");
                (dest, true, false)
            } else {
                ctx.info(format!("Downloading {}…", pkg.asset.name));
                let label = format!("Downloading {}", pkg.version);
                let sha = github::download(&pkg.asset, &dest, |d, t| {
                    ctx.progress(&label, d, t);
                    !ctx.cancelled()
                })?;
                match expected {
                    Some(exp) if exp == sha => {
                        ctx.ok(format!("SHA-256 verified: {sha}"));
                        (dest, true, true)
                    }
                    Some(exp) => {
                        let _ = std::fs::remove_file(&dest);
                        bail!("SHA-256 mismatch! The file was deleted.\n  expected: {exp}\n  actual:   {sha}");
                    }
                    None => {
                        ctx.info(format!(
                            "SHA-256: {sha} (no hash published for this version)"
                        ));
                        (dest, false, true)
                    }
                }
            }
        }
    };

    let signed = match signature::check_archive(&path, source.version()) {
        Ok(signature::Status::Signed) => {
            ctx.ok(
                "Signed by the EntryNav2-Android-Auto project: signature and every file checked",
            );
            true
        }
        Ok(signature::Status::Unsigned) => {
            ctx.info("No project signature (releases before 0.13.0-rc.10 are not signed)");
            false
        }
        Err(e) => {
            if downloaded {
                let _ = std::fs::remove_file(&path);
            }
            bail!("{e:#}");
        }
    };
    if !hash_verified && !signed {
        ctx.warn("This archive cannot be verified: no project signature and no reference SHA-256.");
    }
    Ok(Obtained {
        path,
        hash_verified,
        signed,
    })
}

/// Download, upload, check. Only /tmp is written on the car, and cleaned on failure.
pub fn prepare(ctx: &Ctx, conn: ConnParams, source: Source, allow_unverified: bool) -> Result<()> {
    let version = source.version().to_string();
    let ob = obtain_package(ctx, &source)?;
    let verified = ob.hash_verified || ob.signed;
    if !verified && !allow_unverified {
        ctx.send(Msg::NeedsConsent(version));
        return Ok(());
    }
    ctx.stop_if_cancelled()?;
    let local = ob.path.clone();
    let size_kb = std::fs::metadata(&local)?.len() / 1024 + 1;

    let car = connect(ctx, &conn)?;
    read_info(ctx, &car)?;
    let _ = run_logged(ctx, &car, &scripts::clean_others_script(&version));

    // /tmp is in RAM: keep a margin.
    let (_, out) = car.run_collect(scripts::TMP_SPACE_SCRIPT)?;
    let free_kb = out
        .iter()
        .rev()
        .find_map(|l| scripts::parse_df_available(l))
        .unwrap_or(0);
    let needed_kb = size_kb * 2 + 16 * 1024;
    if free_kb < needed_kb {
        bail!(
            "not enough space in /tmp on the head unit ({} MB free, {} MB needed). Nothing was sent.",
            free_kb / 1024,
            needed_kb / 1024
        );
    }

    ctx.stop_if_cancelled()?;
    let remote = format!("/tmp/{}", github::archive_name(&version));
    ctx.info(format!("Uploading the archive to {remote}…"));
    if let Err(e) = car.upload(&local, &remote, |d, t| {
        ctx.progress("Uploading to the car", d, t);
        !ctx.cancelled()
    }) {
        cleanup_tmp(ctx, &car, &version);
        return Err(e);
    }
    ctx.send(Msg::Progress(None));
    ctx.ok("Archive uploaded");
    if ctx.cancelled() {
        cleanup_tmp(ctx, &car, &version);
        return Err(Cancelled.into());
    }

    ctx.info("Running the compatibility check (up to 1 minute)…");
    ctx.busy("Checking on the car…");
    let (_, lines) = run_logged(ctx, &car, &scripts::check_script(&version))?;
    let mut report = scripts::parse_check(&version, &lines);
    report.host = conn.id();
    report.source = source.key();
    report.archive_verified = verified;
    report.signed = ob.signed;

    for c in &report.compat {
        let lvl = match c.level {
            CompatLevel::Ok => Level::Ok,
            CompatLevel::Warn => Level::Warn,
            CompatLevel::Block if c.wireless_only => Level::Warn,
            CompatLevel::Block => Level::Error,
        };
        ctx.log(lvl, format!("compat: {}", c.text));
    }
    match report.verdict() {
        Verdict::Ok => ctx.ok(format!(
            "Version {version} is compatible and ready to install"
        )),
        Verdict::WiredOnly => ctx.warn(format!(
            "Version {version} can be installed for USB only (wireless is not compatible). \
             Choose USB mode before the first Start."
        )),
        Verdict::Blocked(reasons) => {
            for r in reasons {
                ctx.log(Level::Error, r);
            }
            ctx.log(Level::Error, "DO NOT INSTALL this version on this car.");
            cleanup_tmp(ctx, &car, &version);
        }
    }
    ctx.send(Msg::Checked(report));
    Ok(())
}

/// Copies a signed release to the root of a USB stick. Nothing touches the car.
pub fn write_usb(ctx: &Ctx, pkg: Package, root: PathBuf, remove_others: bool) -> Result<()> {
    let version = pkg.version.clone();
    let ob = obtain_package(ctx, &Source::GitHub(pkg.clone()))?;
    if !ob.signed {
        bail!(
            "{version} is not signed by the project: the car only installs signed archives from a USB stick \
             (0.13.0-rc.10 and newer)."
        );
    }
    ctx.stop_if_cancelled()?;

    let name = pkg.asset.name.clone();
    let dest = root.join(&name);
    // Written under another name first: the car ignores it until it is complete.
    let part = root.join(format!("{name}.part"));
    let expected = github::sha256_file(&ob.path)?;
    ctx.info(format!("Writing {name} to {}…", root.display()));
    {
        use std::io::{Read, Write};
        let total = std::fs::metadata(&ob.path)?.len();
        let mut src = std::fs::File::open(&ob.path)?;
        let mut out = std::fs::File::create(&part).map_err(|e| {
            anyhow::anyhow!("cannot write to the USB stick ({e}). Is it read-only?")
        })?;
        let mut buf = vec![0u8; 256 * 1024];
        let mut done = 0u64;
        loop {
            let n = src.read(&mut buf)?;
            if n == 0 {
                break;
            }
            if let Err(e) = out.write_all(&buf[..n]) {
                drop(out);
                let _ = std::fs::remove_file(&part);
                bail!("writing to the USB stick failed ({e}). Is it full or unplugged?");
            }
            done += n as u64;
            ctx.progress("Writing to the USB stick", done, total);
            if ctx.cancelled() {
                drop(out);
                let _ = std::fs::remove_file(&part);
                return Err(Cancelled.into());
            }
        }
        out.sync_all()?;
    }
    std::fs::rename(&part, &dest)?;

    if github::sha256_file(&dest)? != expected {
        let _ = std::fs::remove_file(&dest);
        bail!(
            "the copy on the USB stick does not match the archive (faulty stick?). It was removed."
        );
    }
    ctx.ok(format!(
        "{name} written and read back from the USB stick (SHA-256 checked)"
    ));

    if remove_others {
        for other in crate::usb::archives_on(&root) {
            if other != name {
                match std::fs::remove_file(root.join(&other)) {
                    Ok(()) => ctx.info(format!("Removed another archive from the stick: {other}")),
                    Err(e) => ctx.warn(format!("Could not remove {other} from the stick: {e}")),
                }
            }
        }
    }
    ctx.send(Msg::UsbWritten);
    Ok(())
}

/// Removes a checked archive from the car's /tmp.
pub fn discard(ctx: &Ctx, conn: ConnParams, version: String) -> Result<()> {
    let car = connect(ctx, &conn)?;
    cleanup_tmp(ctx, &car, &version);
    Ok(())
}

pub struct InstallOptions {
    pub version: String,
    pub vehicle: Option<Vehicle>,
    /// Stop Android Auto first if it runs.
    pub auto_stop: bool,
    /// Select USB cable mode (wireless not compatible).
    pub usb_mode: bool,
    pub reboot: bool,
}

/// Stop, install, vehicle file, USB mode, cleanup, reboot.
pub fn install(ctx: &Ctx, conn: ConnParams, opts: InstallOptions) -> Result<()> {
    let InstallOptions {
        version,
        vehicle,
        auto_stop,
        usb_mode,
        reboot,
    } = opts;
    let car = connect(ctx, &conn)?;
    ensure_stopped(ctx, &car, auto_stop)?;
    ctx.info(format!(
        "Installing version {version}… DO NOT TURN OFF THE IGNITION."
    ));
    ctx.busy("Installing… keep the ignition on");

    let (rc, lines) = match run_logged(ctx, &car, &scripts::install_script(&version)) {
        Ok(r) => r,
        Err(e) => {
            let msg = "Connection lost during the installation. It continues on the car: wait a minute, then reconnect.";
            ctx.send(Msg::InstallFailed(msg.into()));
            bail!("{msg} ({e:#})");
        }
    };

    if rc != 0 || !lines.iter().any(|l| l == "@@INSTALL_OK") {
        let reason = scripts::markers(&lines, "@@ERR|")
            .first()
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("the script stopped with code {rc}"));
        let msg = format!(
            "Installation failed: {reason} The head unit was NOT rebooted. \
             See the log before doing anything else."
        );
        ctx.send(Msg::InstallFailed(msg.clone()));
        bail!("{msg}");
    }
    ctx.ok(format!("Version {version} installed and verified"));

    if let Some(v) = vehicle {
        match run_logged(ctx, &car, &v.script()) {
            Ok((0, lines)) if lines.iter().any(|l| l == "@@VEHICLE_OK") => {
                ctx.ok("Vehicle file written to /var/local/data/AA/vehicle")
            }
            _ => ctx.warn("Could not write the vehicle file (not blocking)"),
        }
    }

    if usb_mode {
        match run_logged(ctx, &car, scripts::USB_MODE_SCRIPT) {
            Ok((0, lines)) if lines.iter().any(|l| l == "@@USB_OK") => {
                ctx.ok("USB cable mode selected (/var/local/data/AA/usb)")
            }
            _ => ctx.warn(
                "Could not select USB mode: tick 'USB cable instead of wireless' in the Android Auto menu",
            ),
        }
    }

    cleanup_tmp(ctx, &car, &version);
    let _ = read_info(ctx, &car);
    ctx.send(Msg::Installed(version));

    if reboot {
        ctx.info("Rebooting the head unit…");
        car.reboot();
        ctx.ok("Reboot started. Then: iDrive > Media > Android Auto > Start.");
    } else {
        ctx.warn("Reboot the head unit to load the new version.");
    }
    Ok(())
}

pub fn uninstall(
    ctx: &Ctx,
    conn: ConnParams,
    keep_settings: bool,
    auto_stop: bool,
    reboot: bool,
) -> Result<()> {
    let car = connect(ctx, &conn)?;
    if read_info(ctx, &car)?.installed == scripts::Installed::None {
        bail!(
            "Android Auto is not installed on this car: nothing to uninstall. Nothing was changed."
        );
    }
    ensure_stopped(ctx, &car, auto_stop)?;
    ctx.info("Uninstalling…");
    ctx.busy("Uninstalling… keep the ignition on");
    let (rc, lines) = run_logged(ctx, &car, &scripts::uninstall_script(keep_settings))?;
    if rc != 0 || !lines.iter().any(|l| l == "@@UNINSTALL_OK") {
        let reason = scripts::markers(&lines, "@@ERR|")
            .first()
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("the script stopped with code {rc}"));
        bail!("uninstall failed: {reason}");
    }
    ctx.ok("Android Auto uninstalled");
    let _ = read_info(ctx, &car);
    ctx.send(Msg::Uninstalled);
    if reboot {
        car.reboot();
        ctx.ok("Reboot started");
    }
    Ok(())
}

/// Turns the app on (`enable = true`) or off at iDrive start-up.
pub fn set_enabled(ctx: &Ctx, conn: ConnParams, enable: bool, reboot: bool) -> Result<()> {
    let car = connect(ctx, &conn)?;
    let info = read_info(ctx, &car)?;
    if info.installed == scripts::Installed::None {
        bail!("Android Auto is not installed on this car. Nothing was changed.");
    }
    if enable && !info.disabled && !info.guard {
        bail!("Android Auto is already enabled. Nothing was changed.");
    }
    if !enable && info.disabled {
        bail!("Android Auto is already disabled. Nothing was changed.");
    }
    let (script, marker) = if enable {
        (scripts::ENABLE_SCRIPT, "@@ENABLE_OK")
    } else {
        (scripts::DISABLE_SCRIPT, "@@DISABLE_OK")
    };
    let (_, lines) = run_logged(ctx, &car, script)?;
    if !lines.iter().any(|l| l == marker) {
        bail!("the command failed");
    }
    if enable {
        ctx.ok("Android Auto re-enabled (app.disable and app-guard counter removed)");
    } else {
        ctx.ok("Android Auto disabled (app.disable created): the iDrive will no longer load it");
    }
    read_info(ctx, &car)?;
    if reboot {
        car.reboot();
        ctx.ok("Reboot started");
    }
    Ok(())
}

pub fn reboot(ctx: &Ctx, conn: ConnParams) -> Result<()> {
    let car = connect(ctx, &conn)?;
    car.reboot();
    ctx.ok("Reboot started");
    Ok(())
}
