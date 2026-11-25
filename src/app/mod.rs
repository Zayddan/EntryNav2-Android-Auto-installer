//! User interface: step-by-step wizard with a sidebar.

mod check;
mod chrome;
mod connection;
mod dialogs;
mod install;
mod maintenance;
mod usb_stick;
mod version;

use crate::car::ConnParams;
use crate::github::{self, compare_versions, Release};
use crate::scripts::{CarInfo, CheckReport, CompatLevel, Fuel, Installed, Side, Vehicle, Verdict};
use crate::theme::*;
use crate::worker::{self, Ctx, InstallOptions, Level, Msg, Source};
use eframe::egui::{
    self, Align, Color32, CursorIcon, Frame, Layout, Margin, RichText, ScrollArea, Sense, Stroke,
};
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use std::cmp::Ordering;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

#[derive(PartialEq, Clone, Copy)]
enum SourceMode {
    GitHub,
    Local,
}

#[derive(PartialEq, Eq, Hash, Clone, Copy)]
enum Page {
    Connection,
    Version,
    Check,
    Install,
    Maintenance,
    /// Prepares a USB stick for the project's own update (no PC in the car).
    UsbStick,
}

#[derive(PartialEq, Clone, Copy)]
enum Action {
    FetchReleases,
    TestConnection,
    Prepare,
    /// The check of an archive that cannot be verified, after the user agreed.
    PrepareUnverified,
    /// Removes a checked archive from the car's /tmp (leaving without installing).
    Discard,
    Install,
    Uninstall,
    Stop,
    Enable,
    Disable,
    Reboot,
    /// Writes a release archive to a USB stick (does not touch the car).
    WriteUsb,
    UpdateInstaller,
}

impl Action {
    /// Tasks that write to the head unit: the window cannot close while they run.
    fn is_critical(self) -> bool {
        matches!(self, Action::Install | Action::Uninstall)
    }
}

/// What an install or uninstall was asked to do, kept to describe the result.
struct Operation {
    kind: Action,
    version: String,
    previous: String,
    had_previous: bool,
    was_running: bool,
    reboot: bool,
    usb: bool,
    vehicle: bool,
    keep_settings: bool,
    /// USB stick the archive is written to (WriteUsb).
    target: String,
    /// Other archives were removed from the stick (WriteUsb).
    removed_others: bool,
    log_start: usize,
    succeeded: bool,
}

/// Result window shown over the app at the end of an install or uninstall.
struct Dialog {
    success: bool,
    title: String,
    subtitle: String,
    rows: Vec<(RowState, String, String)>,
    warnings: Vec<String>,
    steps_title: &'static str,
    steps: Vec<String>,
}

/// Confirmation window opened by a button that changes the car.
struct Confirm {
    action: Action,
    title: String,
    lines: Vec<String>,
    warnings: Vec<String>,
    button: String,
    danger: bool,
}

pub struct App {
    /// The safety rules were accepted in the window shown at start-up.
    safety_accepted: bool,
    show_about: bool,
    confirm: Option<Confirm>,
    operation: Option<Operation>,
    dialog: Option<Dialog>,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    task: Option<Action>,
    progress: Option<(String, f32)>,
    logs: Vec<(Level, String)>,
    show_log: bool,
    /// Errors already seen in the open log (the status bar counts the new ones).
    errors_seen: usize,
    page: Page,
    close_blocked: bool,
    /// Close the window once the running task (clean-up) is over.
    close_after_task: bool,
    close_now: bool,
    /// Cancels the running task (download, upload).
    cancel: Option<Arc<AtomicBool>>,
    /// Address (host:port) of the running task, and of the car `car_info` describes.
    task_conn_id: Option<String>,
    car_id: Option<String>,
    md_cache: CommonMarkCache,

    host: String,
    port: String,
    user: String,
    password: String,
    /// Remember the password on this computer.
    remember_password: bool,
    car_info: Option<CarInfo>,

    // `browse_tag`: release in the details panel; `selected_tag`: the confirmed one.
    releases: Vec<Release>,
    releases_error: Option<String>,
    show_prerelease: bool,
    browse_tag: Option<String>,
    selected_tag: Option<String>,
    source_mode: SourceMode,
    local_path: Option<PathBuf>,
    local_sha: String,
    local_confirmed: bool,

    report: Option<CheckReport>,
    /// The checked archive was installed: a new install needs a new check.
    check_used: bool,
    just_installed: Option<String>,
    install_error: Option<String>,
    auto_stop: bool,
    usb_mode: bool,
    vehicle_enabled: bool,
    model: String,
    year: String,
    fuel: Fuel,
    side: Side,
    reboot_after: bool,

    keep_settings: bool,

    /// Release chosen for the stick.
    usb_tag: Option<String>,
    usb_drives: Vec<crate::usb::Drive>,
    usb_target: Option<crate::usb::Drive>,
    usb_scanned: bool,
    usb_remove_others: bool,

    installer_update: Option<github::InstallerUpdate>,
    update_hidden: bool,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        apply(&cc.egui_ctx);
        let (tx, rx) = channel();
        let mut app = Self {
            safety_accepted: false,
            show_about: false,
            confirm: None,
            operation: None,
            dialog: None,
            tx,
            rx,
            task: None,
            progress: None,
            logs: Vec::new(),
            show_log: false,
            errors_seen: 0,
            page: Page::Connection,
            close_blocked: false,
            close_after_task: false,
            close_now: false,
            cancel: None,
            task_conn_id: None,
            car_id: None,
            md_cache: CommonMarkCache::default(),
            host: "169.254.1.0".into(),
            port: "22".into(),
            user: "root".into(),
            password: String::new(),
            remember_password: true,
            car_info: None,
            releases: Vec::new(),
            releases_error: None,
            show_prerelease: false,
            browse_tag: None,
            selected_tag: None,
            source_mode: SourceMode::GitHub,
            local_path: None,
            local_sha: String::new(),
            local_confirmed: false,
            report: None,
            check_used: false,
            just_installed: None,
            install_error: None,
            auto_stop: true,
            usb_mode: true,
            vehicle_enabled: false,
            model: String::new(),
            year: String::new(),
            fuel: Fuel::Petrol,
            side: Side::Auto,
            reboot_after: true,
            keep_settings: true,
            usb_tag: None,
            usb_drives: Vec::new(),
            usb_target: None,
            usb_scanned: false,
            usb_remove_others: true,
            installer_update: None,
            update_hidden: false,
        };
        if let Some(c) = crate::config::load_connection() {
            (app.host, app.port, app.user) = (c.host, c.port, c.user);
            app.remember_password = c.remember_password;
        }
        if app.remember_password {
            app.password = crate::config::load_password(&app.user).unwrap_or_default();
        }
        app.start(Action::FetchReleases, &cc.egui_ctx);
        crate::self_update::cleanup();
        worker::check_installer_update(Ctx::new(app.tx.clone(), cc.egui_ctx.clone()));
        app
    }

    fn busy(&self) -> bool {
        self.task.is_some()
    }

    fn visible_releases(&self) -> Vec<&Release> {
        self.releases
            .iter()
            .filter(|r| self.show_prerelease || !r.prerelease)
            .collect()
    }

    fn release(&self, tag: Option<&String>) -> Option<&Release> {
        let tag = tag?;
        self.releases.iter().find(|r| &r.tag_name == tag)
    }

    fn selected_release(&self) -> Option<&Release> {
        self.release(self.selected_tag.as_ref())
    }

    fn recommended_tag(&self) -> Option<String> {
        self.releases
            .iter()
            .find(|r| !r.prerelease && r.unsupported_reason().is_none())
            .map(|r| r.tag_name.clone())
    }

    /// Keeps the details panel on a visible release.
    fn fix_browse(&mut self) {
        let visible = self
            .browse_tag
            .as_ref()
            .is_some_and(|t| self.visible_releases().iter().any(|r| &r.tag_name == t));
        if !visible {
            self.browse_tag = self
                .recommended_tag()
                .filter(|t| self.visible_releases().iter().any(|r| &r.tag_name == t))
                .or_else(|| self.visible_releases().first().map(|r| r.tag_name.clone()));
        }
    }

    fn latest_stable(&self) -> Option<String> {
        self.release(self.recommended_tag().as_ref())
            .and_then(|r| r.package())
            .map(|p| p.version)
    }

    fn installed(&self) -> Option<&Installed> {
        self.car_info.as_ref().map(|i| &i.installed)
    }

    fn installed_version(&self) -> Option<&str> {
        match self.installed()? {
            Installed::Version(v) => Some(v),
            _ => None,
        }
    }

    fn is_prerelease_selected(&self) -> bool {
        self.source_mode == SourceMode::GitHub
            && self.selected_release().is_some_and(|r| r.prerelease)
    }

    /// The version the user confirmed (none until confirmed).
    fn target_version(&self) -> Option<String> {
        match self.source_mode {
            SourceMode::GitHub => self.selected_release()?.package().map(|p| p.version),
            SourceMode::Local => {
                if !self.local_confirmed {
                    return None;
                }
                self.local_file_version()
            }
        }
    }

    fn local_file_version(&self) -> Option<String> {
        let name = self.local_path.as_ref()?.file_name()?.to_str()?;
        github::version_from_filename(name)
    }

    fn source(&self) -> Result<Source, String> {
        match self.source_mode {
            SourceMode::GitHub => self
                .selected_release()
                .and_then(|r| r.package())
                .map(Source::GitHub)
                .ok_or_else(|| "No version selected".into()),
            SourceMode::Local => {
                let path = self.local_path.clone().ok_or("No file chosen")?;
                let v = self.target_version().ok_or(
                    "Unrecognised file name: it must look like entrynav2-android-auto-<version>-armhf.tar",
                )?;
                let sha = if self.local_sha.trim().is_empty() {
                    None
                } else {
                    Some(
                        github::normalize_sha256(&self.local_sha)
                            .ok_or("The SHA-256 must be 64 hex characters")?,
                    )
                };
                Ok(Source::Local(path, v, sha))
            }
        }
    }

    fn conn(&self) -> Result<ConnParams, String> {
        let port = self
            .port
            .trim()
            .parse()
            .map_err(|_| "Invalid port".to_string())?;
        if self.host.trim().is_empty() {
            return Err("Missing IP address".into());
        }
        Ok(ConnParams {
            host: self.host.trim().into(),
            port,
            user: self.user.trim().into(),
            password: self.password.clone(),
        })
    }

    /// host:port as entered (same form as `ConnParams::id`).
    fn conn_id(&self) -> String {
        self.conn().map(|c| c.id()).unwrap_or_default()
    }

    fn vehicle(&self) -> Option<Vehicle> {
        self.vehicle_enabled.then(|| Vehicle {
            model: self.model.trim().into(),
            year: self.year.trim().into(),
            fuel: self.fuel,
            side: self.side,
        })
    }

    /// The check report, only if made for this exact archive on this car.
    fn report_for_target(&self) -> Option<&CheckReport> {
        let r = self.report.as_ref()?;
        let same_archive =
            self.source().ok().map(|s| s.key()).as_deref() == Some(r.source.as_str());
        (Some(&r.version) == self.target_version().as_ref()
            && r.host == self.conn_id()
            && same_archive)
            .then_some(r)
    }

    fn wired_only(&self) -> bool {
        self.report_for_target()
            .is_some_and(|r| r.verdict() == Verdict::WiredOnly)
    }

    /// A version of Android Auto is on the car and will be replaced.
    fn has_current_install(&self) -> bool {
        matches!(
            self.installed(),
            Some(Installed::Version(_) | Installed::Unknown)
        )
    }

    fn can_install(&self) -> Result<(), &'static str> {
        let Some(r) = self.report_for_target() else {
            return Err("Run the check for this version on this car first");
        };
        if self.check_used {
            return Err("Installed. To install again, run the check again first");
        }
        if matches!(r.verdict(), Verdict::Blocked(_)) {
            return Err("The check failed: installation is not possible");
        }
        if self.car_info.as_ref().is_some_and(|i| i.running) && !self.auto_stop {
            return Err("Android Auto is running: stop it first, or let the installer stop it");
        }
        if let Some(v) = self.vehicle() {
            if v.validate().is_err() {
                return Err("Invalid vehicle file");
            }
        }
        Ok(())
    }

    /// Why a page cannot be opened yet (the previous step is not done).
    fn locked_reason(&self, page: Page) -> Option<&'static str> {
        let connected = self.car_info.is_some();
        match page {
            Page::Connection | Page::UsbStick => None,
            Page::Version | Page::Maintenance if !connected => Some("Connect to the car first"),
            Page::Version | Page::Maintenance => None,
            Page::Check if !connected => Some("Connect to the car first"),
            Page::Check if self.target_version().is_none() => Some("Confirm a version first"),
            Page::Check => None,
            Page::Install => {
                // Stays open after an installation, to show its result.
                if self.just_installed.is_some() || self.install_error.is_some() {
                    return None;
                }
                match self.report_for_target().map(|r| r.verdict()) {
                    _ if !connected => Some("Connect to the car first"),
                    _ if self.target_version().is_none() => Some("Confirm a version first"),
                    None => Some("Run the check first"),
                    Some(Verdict::Blocked(_)) => Some("The check failed"),
                    Some(_) => None,
                }
            }
        }
    }

    /// The furthest step that can be opened, to fall back to.
    fn furthest_open_step(&self) -> Page {
        [Page::Install, Page::Check, Page::Version]
            .into_iter()
            .find(|p| self.locked_reason(*p).is_none())
            .unwrap_or(Page::Connection)
    }

    fn step_state(&self, page: Page) -> (StepState, String) {
        if let Some(reason) = self.locked_reason(page) {
            return (StepState::Locked, reason.into());
        }
        match page {
            Page::Connection => match &self.car_info {
                Some(i) if i.running => (StepState::Done, "Car detected · AA running".into()),
                Some(_) => (StepState::Done, "Car detected".into()),
                None => (StepState::Pending, "SSH over ENET cable".into()),
            },
            Page::Version => match self.target_version() {
                Some(v) => (StepState::Done, format!("{v} confirmed")),
                None => match self.pending_choice() {
                    Some(v) => (StepState::Warn, format!("{v} · confirm it")),
                    None => (StepState::Pending, "Pick a version".into()),
                },
            },
            Page::Check => match self.report_for_target().map(|r| r.verdict()) {
                Some(Verdict::Ok) => (StepState::Done, "Compatible".into()),
                Some(Verdict::WiredOnly) => (StepState::Warn, "USB only".into()),
                Some(Verdict::Blocked(_)) => (StepState::Error, "Not compatible".into()),
                None => (StepState::Pending, "Not run yet".into()),
            },
            Page::Install => match (
                &self.just_installed,
                &self.install_error,
                self.installed_version(),
            ) {
                (Some(v), _, _) => (StepState::Done, format!("{v} installed")),
                (None, Some(_), _) => (StepState::Error, "Failed".into()),
                (None, None, Some(v)) if Some(v.to_string()) == self.target_version() => {
                    (StepState::Done, "Already installed".into())
                }
                _ => (StepState::Pending, "Waiting".into()),
            },
            Page::Maintenance => (StepState::Pending, "Stop, troubleshoot, uninstall".into()),
            Page::UsbStick => (StepState::Pending, "Update without a PC".into()),
        }
    }

    fn log(&mut self, l: Level, s: impl Into<String>) {
        if l == Level::Error {
            self.show_log = true;
        }
        self.logs.push((l, s.into()));
    }

    /// A version picked but not confirmed yet.
    fn pending_choice(&self) -> Option<String> {
        match self.source_mode {
            SourceMode::GitHub => {
                if self.browse_tag == self.selected_tag {
                    return None;
                }
                self.release(self.browse_tag.as_ref())?
                    .package()
                    .map(|p| p.version)
            }
            SourceMode::Local => {
                let sha_ok = self.local_sha.trim().is_empty()
                    || github::normalize_sha256(&self.local_sha).is_some();
                (!self.local_confirmed && sha_ok)
                    .then(|| self.local_file_version())
                    .flatten()
            }
        }
    }

    /// Confirms the version picked on the Version page and goes to the next step.
    fn confirm_pending(&mut self) {
        match self.source_mode {
            SourceMode::GitHub => self.confirm_browsed(),
            SourceMode::Local => {
                self.local_confirmed = true;
                if let Some(v) = self.target_version() {
                    self.log(Level::Info, format!("Local archive {v} confirmed"));
                }
                self.page = Page::Check;
            }
        }
    }

    /// Confirms the release in the details panel and goes to the next step.
    fn confirm_browsed(&mut self) {
        if self
            .release(self.browse_tag.as_ref())
            .is_none_or(|r| r.unsupported_reason().is_some())
        {
            return;
        }
        if self.selected_tag != self.browse_tag {
            self.selected_tag = self.browse_tag.clone();
            if let Some(v) = self.target_version() {
                self.log(Level::Info, format!("Version {v} confirmed"));
            }
        }
        self.page = Page::Check;
    }

    fn start(&mut self, action: Action, ctx: &egui::Context) {
        if self.busy() {
            return;
        }
        // Never start an action that is impossible or useless for the car's state.
        if matches!(
            action,
            Action::Stop | Action::Uninstall | Action::Enable | Action::Disable | Action::Reboot
        ) {
            if let Some(why) = self.maintenance_block(action) {
                return self.log(Level::Warn, why);
            }
        }
        let c = Ctx::new(self.tx.clone(), ctx.clone());

        match action {
            Action::FetchReleases => {
                self.task_conn_id = None;
                self.cancel = Some(c.cancel_flag());
                worker::spawn(c, worker::fetch_releases);
                self.task = Some(action);
                return;
            }
            Action::WriteUsb => {
                let (Some(pkg), Some(target)) = (self.usb_package(), self.usb_target.clone())
                else {
                    return;
                };
                let remove = self.usb_remove_others;
                self.task_conn_id = None;
                self.cancel = Some(c.cancel_flag());
                self.operation = Some(self.new_operation(Action::WriteUsb));
                worker::spawn(c, move |c| worker::write_usb(c, pkg, target.root, remove));
                self.task = Some(action);
                return;
            }
            Action::UpdateInstaller => {
                let Some(update) = self.installer_update.clone() else {
                    return;
                };
                self.task_conn_id = None;
                self.cancel = Some(c.cancel_flag());
                worker::spawn(c, move |c| worker::update_installer(c, update));
                self.task = Some(action);
                return;
            }
            _ => {}
        }

        let conn = match self.conn() {
            Ok(conn) => conn,
            Err(e) => return self.log(Level::Error, e),
        };
        self.task_conn_id = Some(conn.id());
        self.cancel = Some(c.cancel_flag());

        match action {
            Action::FetchReleases | Action::WriteUsb | Action::UpdateInstaller => {
                unreachable!("handled above")
            }
            Action::TestConnection => {
                worker::spawn(c, move |c| worker::test_connection(c, conn));
            }
            Action::Stop => {
                worker::spawn(c, move |c| worker::stop(c, conn));
            }
            Action::Prepare | Action::PrepareUnverified => {
                let source = match self.source() {
                    Ok(s) => s,
                    Err(e) => return self.log(Level::Error, e),
                };
                self.report = None;
                self.check_used = false;
                self.just_installed = None;
                self.install_error = None;
                let allow_unverified = action == Action::PrepareUnverified;
                worker::spawn(c, move |c| {
                    worker::prepare(c, conn, source, allow_unverified)
                });
            }
            Action::Discard => {
                let Some(version) = self.report.as_ref().map(|r| r.version.clone()) else {
                    return;
                };
                worker::spawn(c, move |c| worker::discard(c, conn, version));
            }
            Action::Install => {
                if let Err(e) = self.can_install() {
                    return self.log(Level::Error, e);
                }
                let Some(version) = self.target_version() else {
                    return self.log(Level::Error, "Choose and confirm a version first");
                };
                self.install_error = None;
                self.operation = Some(self.new_operation(Action::Install));
                let opts = InstallOptions {
                    version,
                    vehicle: self.vehicle(),
                    auto_stop: self.auto_stop,
                    usb_mode: self.wired_only() && self.usb_mode,
                    reboot: self.reboot_after,
                };
                worker::spawn(c, move |c| worker::install(c, conn, opts));
            }
            Action::Uninstall | Action::Enable | Action::Disable | Action::Reboot => {
                let reboot = self.reboot_after;
                match action {
                    Action::Uninstall => {
                        let (keep, auto_stop) = (self.keep_settings, self.auto_stop);
                        self.operation = Some(self.new_operation(Action::Uninstall));
                        worker::spawn(c, move |c| {
                            worker::uninstall(c, conn, keep, auto_stop, reboot)
                        });
                    }
                    Action::Reboot => worker::spawn(c, move |c| worker::reboot(c, conn)),
                    _ => {
                        let enable = action == Action::Enable;
                        worker::spawn(c, move |c| worker::set_enabled(c, conn, enable, reboot));
                    }
                }
            }
        }
        self.task = Some(action);
    }

    fn drain_messages(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Log(l, s) => self.log(l, s),
                Msg::Progress(p) => self.progress = p,
                Msg::Releases(Ok(r)) => {
                    self.releases = r;
                    self.releases_error = None;
                    if self.selected_release().is_none() {
                        self.selected_tag = None;
                    }
                    self.fix_browse();
                }
                Msg::Releases(Err(e)) => self.releases_error = Some(e),
                Msg::CarInfo(i) => {
                    // The checked archive is gone from /tmp (the car restarted). Not during a check or install.
                    let stale = self.report.as_ref().is_some_and(|r| {
                        Some(&r.host) == self.task_conn_id.as_ref()
                            && !self.check_used
                            && !matches!(r.verdict(), Verdict::Blocked(_))
                            && !i.staged.contains(&r.version)
                    }) && !matches!(
                        self.task,
                        Some(Action::Prepare | Action::PrepareUnverified | Action::Install)
                    );
                    if stale {
                        self.report = None;
                        self.log(
                            Level::Warn,
                            "The checked files are no longer on the car (it restarted?): run the check again.",
                        );
                    }
                    self.car_info = Some(i);
                    self.car_id = self.task_conn_id.clone();
                    if self.task == Some(Action::TestConnection) {
                        // Remember what worked.
                        crate::config::save_connection(&crate::config::Connection {
                            host: self.host.clone(),
                            port: self.port.clone(),
                            user: self.user.clone(),
                            remember_password: self.remember_password,
                        });
                        crate::config::store_password(
                            &self.user,
                            &self.password,
                            self.remember_password,
                        );
                        if self.page == Page::Connection {
                            self.page = Page::Version;
                        }
                    }
                }
                Msg::NeedsConsent(version) => {
                    self.confirm = Some(Confirm {
                        action: Action::PrepareUnverified,
                        title: "Use an archive that cannot be verified?".into(),
                        lines: vec![
                            format!(
                                "Archive {version} has no project signature and no reference SHA-256 \
                                 to compare with."
                            ),
                            "The check runs its program (aa-supervisor --compat) as root on the car."
                                .into(),
                        ],
                        warnings: vec![
                            "Only continue if you fully trust where this file comes from.".into(),
                        ],
                        button: "Check it anyway".into(),
                        danger: true,
                    });
                }
                Msg::InstallerUpdate(u) => self.installer_update = Some(u),
                Msg::InstallerReplaced => self.close_after_task = true,
                Msg::Checked(r) => self.report = Some(r),
                Msg::Installed(v) => {
                    // The check stays shown as done; its archive is gone from the car.
                    self.check_used = true;
                    self.just_installed = Some(v);
                    if let Some(op) = &mut self.operation {
                        op.succeeded = true;
                    }
                }
                Msg::InstallFailed(e) => {
                    // Still valid: the install script verifies the archive again before writing.
                    self.install_error = Some(e);
                }
                Msg::UsbWritten => {
                    if let Some(op) = &mut self.operation {
                        op.succeeded = true;
                    }
                }
                Msg::Uninstalled => {
                    self.just_installed = None;
                    if let Some(op) = &mut self.operation {
                        op.succeeded = true;
                    }
                }
                Msg::TaskEnd => {
                    if self.task == Some(Action::Discard) {
                        self.report = None;
                    }
                    self.task = None;
                    self.progress = None;
                    self.cancel = None;
                    self.close_blocked = false;
                    if let Some(op) = self.operation.take() {
                        let cancelled = self
                            .logs
                            .get(op.log_start..)
                            .unwrap_or(&[])
                            .iter()
                            .any(|(_, s)| s.starts_with("Cancelled."));
                        if op.succeeded || !cancelled {
                            self.dialog = Some(self.result_dialog(op));
                        }
                    }
                    if self.close_after_task {
                        self.close_now = true;
                    }
                }
            }
        }
    }
}

/// Says why a button next to it is greyed out.
fn hint(ui: &mut egui::Ui, why: Option<&str>) {
    if let Some(why) = why {
        ui.label(RichText::new(why.to_string()).size(12.5).color(ORANGE));
    }
}

fn radio_dot(ui: &mut egui::Ui, selected: bool) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), Sense::hover());
    let p = ui.painter();
    if selected {
        p.circle_filled(rect.center(), 9.0, ACCENT);
        p.circle_filled(rect.center(), 3.5, Color32::WHITE);
    } else {
        p.circle_stroke(rect.center(), 8.5, Stroke::new(1.5_f32, BORDER_STRONG));
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain_messages();
        // The address changed after connecting: lock everything again.
        if self.car_info.is_some()
            && !self.busy()
            && self.car_id.as_deref() != Some(self.conn_id().as_str())
        {
            self.car_info = None;
            self.car_id = None;
            self.log(Level::Info, "The car's address changed: connect again.");
        }
        // Never stay on a step that is locked (e.g. a new connection, another version).
        if self.locked_reason(self.page).is_some() {
            self.page = self.furthest_open_step();
        }
        if self.busy() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        self.guard_close(ctx);
        let mut action = None;

        egui::SidePanel::left("nav")
            .exact_width(250.0)
            .resizable(false)
            .frame(
                Frame::none()
                    .fill(SIDEBAR)
                    .inner_margin(Margin::symmetric(14.0, 16.0))
                    .stroke(Stroke::new(1.0_f32, BORDER)),
            )
            .show(ctx, |ui| self.ui_sidebar(ui));

        egui::TopBottomPanel::bottom("status")
            .exact_height(38.0)
            .frame(
                Frame::none()
                    .fill(SIDEBAR)
                    .inner_margin(Margin::symmetric(16.0, 6.0))
                    .stroke(Stroke::new(1.0_f32, BORDER)),
            )
            .show(ctx, |ui| self.ui_statusbar(ui));

        if self.show_log {
            egui::TopBottomPanel::bottom("log")
                .resizable(true)
                .default_height(200.0)
                .min_height(90.0)
                .frame(
                    Frame::none()
                        .fill(CONSOLE)
                        .inner_margin(Margin::symmetric(16.0, 10.0)),
                )
                .show(ctx, |ui| self.ui_log(ui));
        }

        egui::CentralPanel::default()
            .frame(Frame::none().fill(BG))
            .show(ctx, |ui| {
                ScrollArea::vertical()
                    .id_salt(("page", self.page))
                    .auto_shrink([false; 2])
                    .show(ui, |ui| {
                        Frame::none()
                            .inner_margin(Margin {
                                left: 32.0,
                                right: 32.0,
                                top: 28.0,
                                bottom: 28.0,
                            })
                            .show(ui, |ui| {
                                ui.set_max_width(ui.available_width().min(960.0));
                                self.ui_update_banner(ui);
                                match self.page {
                                    Page::Connection => self.page_connection(ui, &mut action),
                                    Page::Version => self.page_version(ui, &mut action),
                                    Page::Check => self.page_check(ui, &mut action),
                                    Page::Install => self.page_install(ui, &mut action),
                                    Page::Maintenance => self.page_maintenance(ui, &mut action),
                                    Page::UsbStick => self.page_usb_stick(ui),
                                }
                            });
                    });
            });

        self.ui_dialog(ctx);
        self.ui_confirm(ctx);
        self.ui_about(ctx);
        self.ui_welcome(ctx);

        if let Some(a) = action {
            self.start(a, ctx);
        }
    }
}
