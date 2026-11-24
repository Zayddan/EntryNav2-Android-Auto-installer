//! Step 3: check on the car.

use super::*;

impl App {
    pub(super) fn page_check(&mut self, ui: &mut egui::Ui, action: &mut Option<Action>) {
        page_header(
            ui,
            "Check",
            "Everything is verified before installing. This step changes nothing on the car.",
        );
        let Some(version) = self.target_version() else {
            match self.pending_choice() {
                Some(v) => {
                    banner(
                        ui,
                        ORANGE,
                        "!",
                        &format!("You chose version {v}: confirm it"),
                        &["The check runs on the confirmed version only.".into()],
                    );
                    ui.horizontal(|ui| {
                        if primary_button(ui, &format!("Confirm version {v}"), true).clicked() {
                            self.confirm_pending();
                        }
                        if secondary_button(ui, "Back to the versions", true).clicked() {
                            self.page = Page::Version;
                        }
                    });
                }
                None => {
                    banner(
                        ui,
                        ORANGE,
                        "!",
                        "No version chosen yet",
                        &["Pick a version in step 2, then confirm it.".into()],
                    );
                    if secondary_button(ui, "Go to the versions", true).clicked() {
                        self.page = Page::Version;
                    }
                }
            }
            return;
        };
        let running = matches!(self.task, Some(Action::Prepare | Action::PrepareUnverified));
        let report = self.report_for_target().cloned();
        let progress_label = self
            .progress
            .as_ref()
            .map(|p| p.0.clone())
            .unwrap_or_default();

        if let Some(r) = &report {
            match r.verdict() {
                Verdict::Ok => banner(
                    ui,
                    GREEN,
                    "✔",
                    &format!("Version {} is compatible", r.version),
                    &["You can go on to the installation.".into()],
                ),
                Verdict::WiredOnly => banner(
                    ui,
                    ORANGE,
                    "!",
                    &format!("Version {} is compatible for USB only", r.version),
                    &["Wireless Android Auto is not available on this car. The installer can select USB mode for you.".into()],
                ),
                Verdict::Blocked(reasons) => {
                    let lines: Vec<String> = reasons.iter().map(|x| format!("• {x}")).collect();
                    banner(ui, RED, "✖", "Do not install this version", &lines);
                }
            }
            if self.check_used {
                ui.label(
                    RichText::new(
                        "✔ This version was installed after this check. Its files were then removed from \
                         the car, so installing again needs a new check.",
                    )
                    .color(MUTED),
                );
            } else if !matches!(r.verdict(), Verdict::Blocked(_))
                && primary_button(ui, "Continue to installation  →", true).clicked()
            {
                self.page = Page::Install;
            }
            ui.add_space(8.0);
        }

        card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(bold("Checks").size(17.0).color(TEXT));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    pill(ui, &version, ACCENT)
                });
            });
            ui.add_space(2.0);

            let phase = |keyword: &str| running && progress_label.contains(keyword);
            let r = report.as_ref();

            let dl = if phase("Downloading") {
                RowState::Running
            } else {
                match r {
                    None => RowState::Pending,
                    Some(r) if r.archive_verified => RowState::Done,
                    Some(_) => RowState::Warn,
                }
            };
            let dl_detail = match r {
                Some(r) if r.signed => "Signed by the EntryNav2-Android-Auto project: signature and every file checked",
                Some(r) if r.archive_verified => "SHA-256 matches the published one (releases before 0.13.0-rc.10 are not signed)",
                Some(_) => "NOT verified: no project signature and no reference SHA-256",
                None => "Project signature, or published SHA-256 for older releases",
            };
            status_row(ui, dl, "Archive origin", dl_detail);

            let up = match r {
                _ if phase("Uploading") => RowState::Running,
                Some(_) => RowState::Done,
                None => RowState::Pending,
            };
            status_row(
                ui,
                up,
                "Upload to the car",
                "Copied to /tmp (RAM), free space checked first",
            );

            let verifying = phase("Checking");
            let st = |ok: Option<bool>| match ok {
                _ if verifying => RowState::Running,
                None => RowState::Pending,
                Some(true) => RowState::Done,
                Some(false) => RowState::Error,
            };
            status_row(
                ui,
                st(r.map(|r| {
                    r.error.is_none() && r.md5_rc == Some(0) && r.md5_failures.is_empty()
                })),
                "Archive contents and integrity",
                "The 3 programs are present, md5sum -c MD5SUMS",
            );
            let free = r
                .and_then(|r| r.free_kb)
                .map(|k| format!("{} MB free", k / 1024))
                .unwrap_or_else(|| "16 MB minimum".into());
            status_row(
                ui,
                st(r.map(|r| r.free_kb.is_some_and(|k| k >= crate::scripts::MIN_FREE_KB))),
                "Disk space",
                &free,
            );
            let libs = match r {
                Some(r) if !r.ld_issues.is_empty() => format!("{} problem(s)", r.ld_issues.len()),
                _ => "System libraries and symbols".into(),
            };
            status_row(
                ui,
                st(r.map(|r| r.complete && r.ld_issues.is_empty())),
                "Libraries",
                &libs,
            );
            let compat_state = match r {
                _ if verifying => RowState::Running,
                None => RowState::Pending,
                Some(r) if r.compat.is_empty() || matches!(r.compat_rc, Some(124 | 137 | 143)) => {
                    RowState::Error
                }
                Some(r)
                    if r.compat
                        .iter()
                        .any(|c| c.level == CompatLevel::Block && !c.wireless_only) =>
                {
                    RowState::Error
                }
                Some(r) if r.compat.iter().any(|c| c.level != CompatLevel::Ok) => RowState::Warn,
                Some(_) => RowState::Done,
            };
            status_row(
                ui,
                compat_state,
                "Official compatibility test",
                "aa-supervisor --compat",
            );

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let label = if report.is_some() {
                    "Run the check again"
                } else {
                    "Run the check"
                };
                if primary_button(ui, label, !self.busy()).clicked() {
                    *action = Some(Action::Prepare);
                }
                if running {
                    if let Some((_, f)) = &self.progress {
                        if *f >= 0.0 {
                            ui.add(
                                egui::ProgressBar::new(*f)
                                    .desired_width(200.0)
                                    .fill(ACCENT)
                                    .show_percentage(),
                            );
                        }
                    }
                    // Download and upload can be cancelled; the check itself cannot.
                    let cancellable = phase("Downloading") || phase("Uploading");
                    let cancelling = self
                        .cancel
                        .as_ref()
                        .is_some_and(|c| c.load(AtomicOrdering::Relaxed));
                    if cancellable && !cancelling && secondary_button(ui, "Cancel", true).clicked()
                    {
                        if let Some(c) = &self.cancel {
                            c.store(true, AtomicOrdering::Relaxed);
                        }
                    }
                    if cancelling {
                        ui.label(RichText::new("Cancelling…").color(MUTED));
                    }
                }
            });
        });

        let Some(r) = report else { return };
        if !r.compat.is_empty() {
            card(ui, |ui| {
                egui::CollapsingHeader::new(
                    bold(format!("Compatibility test details ({})", r.compat.len()))
                        .size(15.0)
                        .color(TEXT),
                )
                .id_salt("compat")
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 6.0;
                    for c in &r.compat {
                        let (col, tag) = match c.level {
                            CompatLevel::Ok => (GREEN, "OK"),
                            CompatLevel::Warn => (ORANGE, "WARNING"),
                            CompatLevel::Block if c.wireless_only => (ORANGE, "WIRELESS"),
                            CompatLevel::Block => (RED, "BLOCKING"),
                        };
                        ui.horizontal(|ui| {
                            pill(ui, tag, col);
                            ui.label(RichText::new(&c.text).color(TEXT));
                        });
                    }
                });
            });
        }
    }
}
