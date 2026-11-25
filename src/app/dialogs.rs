//! Windows over the app: start-up safety rules, confirmations, results, credits, closing.

use super::*;

impl App {
    pub(super) fn new_operation(&self, kind: Action) -> Operation {
        let (previous, had_previous) = match self.installed() {
            Some(Installed::Version(v)) => (v.clone(), true),
            Some(Installed::Unknown) => ("a version not recognised".into(), true),
            _ => (String::new(), false),
        };
        let (version, target, removed_others) = if kind == Action::WriteUsb {
            let version = self.usb_package().map(|p| p.version).unwrap_or_default();
            let target = self.usb_target.as_ref();
            let others = target.is_some_and(|t| {
                let name = github::archive_name(&version);
                crate::usb::archives_on(&t.root).iter().any(|n| *n != name)
            });
            (
                version,
                target
                    .map(|t| t.root.display().to_string())
                    .unwrap_or_default(),
                others && self.usb_remove_others,
            )
        } else {
            (
                self.target_version().unwrap_or_default(),
                String::new(),
                false,
            )
        };
        Operation {
            kind,
            version,
            target,
            removed_others,
            previous,
            had_previous,
            was_running: self.car_info.as_ref().is_some_and(|i| i.running),
            reboot: self.reboot_after,
            usb: self.wired_only() && self.usb_mode,
            vehicle: self.vehicle_enabled,
            keep_settings: self.keep_settings,
            log_start: self.logs.len(),
            succeeded: false,
        }
    }

    pub(super) fn result_dialog(&self, op: Operation) -> Dialog {
        let logs = self.logs.get(op.log_start..).unwrap_or(&[]);
        let logged = |needle: &str| logs.iter().any(|(_, s)| s.contains(needle));
        let warnings: Vec<String> = logs
            .iter()
            .filter(|(l, s)| *l == Level::Warn && !s.starts_with("Android Auto is running"))
            .map(|(_, s)| s.clone())
            .collect();
        let reboot_row = |what: &str| {
            if op.reboot {
                (
                    RowState::Done,
                    "Head unit rebooting".to_string(),
                    format!("The iDrive restarts in about a minute{what}"),
                )
            } else {
                (
                    RowState::Warn,
                    "Reboot still needed".to_string(),
                    "Reboot the head unit from the Maintenance page".to_string(),
                )
            }
        };

        if !op.succeeded {
            let reason = self
                .install_error
                .clone()
                .filter(|_| op.kind == Action::Install)
                .or_else(|| {
                    logs.iter()
                        .rev()
                        .find(|(l, _)| *l == Level::Error)
                        .map(|(_, s)| s.trim_start_matches("Error: ").to_string())
                })
                .unwrap_or_else(|| "Unknown error: see the log.".into());
            if op.kind == Action::WriteUsb {
                return Dialog {
                    success: false,
                    title: "The USB stick was not prepared".into(),
                    subtitle: reason,
                    rows: Vec::new(),
                    warnings,
                    steps_title: "What to do",
                    steps: vec![
                        "Nothing was changed on the car.".into(),
                        "Check that the stick is plugged in, not full and not read-only, then try again.".into(),
                    ],
                };
            }
            let what = if op.kind == Action::Install {
                "installation"
            } else {
                "uninstallation"
            };
            return Dialog {
                success: false,
                title: format!("The {what} did not complete"),
                subtitle: reason,
                rows: Vec::new(),
                warnings,
                steps_title: "What to do",
                steps: vec![
                    "The head unit was not rebooted. Keep the ignition on.".into(),
                    "Read the log for the details (button below).".into(),
                    "Fix the cause, then run the check and the installation again.".into(),
                ],
            };
        }

        if op.kind == Action::WriteUsb {
            let mut rows = vec![
                (
                    RowState::Done,
                    "Archive signed by the project".to_string(),
                    "Signature and every file checked".to_string(),
                ),
                (
                    RowState::Done,
                    "Copied to the stick and read back".to_string(),
                    format!("At the root of {}, as is (not extracted)", op.target),
                ),
            ];
            if op.removed_others {
                rows.push((
                    RowState::Done,
                    "Other archives removed from the stick".into(),
                    "The car takes the newest archive it finds: only this one is left".into(),
                ));
            }
            return Dialog {
                success: true,
                title: "The USB stick is ready".into(),
                subtitle: format!("Android Auto {} is on {}.", op.version, op.target),
                rows,
                warnings,
                steps_title: "In the car",
                steps: vec![
                    "Eject the stick before unplugging it."
                        .into(),
                    "Plug it into the car (parked, ignition on), then iDrive › Media › Android Auto › Update from USB stick."
                        .into(),
                    "Press again within 30 seconds to install. The head unit restarts by itself."
                        .into(),
                    "Plug the stick back into this computer to see the car's report.".into(),
                ],
            };
        }

        if op.kind == Action::Uninstall {
            let mut rows = vec![(
                RowState::Done,
                "Programs and iDrive plugin removed".to_string(),
                "/opt/aa and /opt/hmi/data/ro/Plugins/EntryAad".to_string(),
            )];
            if op.was_running {
                rows.push((
                    RowState::Done,
                    "Android Auto was stopped first".into(),
                    "The car's Bluetooth was given back".into(),
                ));
            }
            rows.push(if op.keep_settings {
                (
                    RowState::Done,
                    "Settings kept".into(),
                    "/var/local/data/AA, for a later installation".into(),
                )
            } else {
                (
                    RowState::Done,
                    "Settings deleted".into(),
                    "/var/local/data/AA".into(),
                )
            });
            rows.push(reboot_row(""));
            return Dialog {
                success: true,
                title: "Android Auto is uninstalled".into(),
                subtitle: format!(
                    "{} was removed from the car.",
                    if op.had_previous {
                        op.previous.as_str()
                    } else {
                        "Android Auto"
                    }
                ),
                rows,
                warnings,
                steps_title: "Next steps",
                steps: vec![
                    "After the restart, Android Auto is no longer in the iDrive Media menu.".into(),
                    "You can unplug the ENET cable.".into(),
                ],
            };
        }

        let mut rows = vec![(
            RowState::Done,
            "Files copied and verified".to_string(),
            "md5sum of every installed file".to_string(),
        )];
        if op.was_running {
            rows.push((
                RowState::Done,
                "Android Auto was stopped first".into(),
                "The car's Bluetooth was given back".into(),
            ));
        }
        if op.vehicle {
            rows.push(if logged("Could not write the vehicle file") {
                (
                    RowState::Warn,
                    "Vehicle file not written".into(),
                    "Not blocking".into(),
                )
            } else {
                (
                    RowState::Done,
                    "Vehicle file written".into(),
                    "Model, year and fuel shown on the phone".into(),
                )
            });
        }
        if op.usb {
            rows.push(if logged("Could not select USB mode") {
                (
                    RowState::Warn,
                    "USB mode not selected".into(),
                    "Tick it in the Android Auto menu".into(),
                )
            } else {
                (
                    RowState::Done,
                    "USB cable mode selected".into(),
                    "Wireless is not compatible with this car".into(),
                )
            });
        }
        rows.push(reboot_row(" to load the new version"));

        let mut steps = Vec::new();
        if op.reboot {
            steps.push("Wait until the iDrive has restarted (about a minute).".to_string());
        }
        steps.push("On the iDrive: Media › Android Auto › Start.".into());
        if op.usb {
            steps.push(
                "Plug the phone into a USB port with a data cable. The first time, allow Android Auto on the phone."
                    .into(),
            );
        } else {
            steps.push(
                "First time: pair the phone over Bluetooth and accept the car on the phone.".into(),
            );
        }
        steps.push("You can unplug the ENET cable.".into());
        if compare_versions(&op.version, "0.13.0-rc.10") != Ordering::Less {
            steps.push("Next updates can be done with a USB stick (USB stick page).".into());
        }

        Dialog {
            success: true,
            title: format!("Android Auto {} is installed", op.version),
            subtitle: if op.had_previous {
                format!("It replaced {}. Your settings were kept.", op.previous)
            } else {
                "Installed on the car for the first time.".into()
            },
            rows,
            warnings,
            steps_title: "Next steps",
            steps,
        }
    }

    /// The result window, over a dimmed app.
    pub(super) fn ui_dialog(&mut self, ctx: &egui::Context) {
        let Some(d) = &self.dialog else { return };
        let screen = ctx.screen_rect();
        egui::Area::new(egui::Id::new("dialog_overlay"))
            .order(egui::Order::Foreground)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                let (rect, _) = ui.allocate_exact_size(screen.size(), Sense::click());
                ui.painter()
                    .rect_filled(rect, 0.0, Color32::from_black_alpha(170));
            });

        let (mut close, mut show_log) = (false, false);
        let (color, glyph) = if d.success {
            (GREEN, Icon::Check)
        } else {
            (RED, Icon::Cross)
        };
        egui::Area::new(egui::Id::new("dialog"))
            .order(egui::Order::Tooltip)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                Frame::none()
                    .fill(CARD)
                    .stroke(Stroke::new(1.0_f32, tint(color, 140)))
                    .rounding(18.0)
                    .inner_margin(Margin::same(28.0))
                    .show(ui, |ui| {
                        ui.set_width(540.0_f32.min(screen.width() - 80.0));
                        ui.vertical_centered(|ui| {
                            icon_circle(ui, 64.0, color, glyph);
                            ui.add_space(6.0);
                            ui.label(bold(&d.title).size(22.0).color(TEXT));
                            ui.add(
                                egui::Label::new(RichText::new(&d.subtitle).color(MUTED)).wrap(),
                            );
                        });
                        ui.add_space(10.0);

                        for (state, title, detail) in &d.rows {
                            status_row(ui, *state, title, detail);
                        }

                        if !d.warnings.is_empty() {
                            ui.add_space(4.0);
                            Frame::none()
                                .fill(tint(ORANGE, 22))
                                .rounding(10.0)
                                .inner_margin(Margin::symmetric(12.0, 8.0))
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    for w in &d.warnings {
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(w.to_string()).color(ORANGE),
                                            )
                                            .wrap(),
                                        );
                                    }
                                });
                        }

                        if !d.steps.is_empty() {
                            ui.add_space(8.0);
                            ui.label(bold(d.steps_title).size(15.0).color(TEXT));
                            for (i, step) in d.steps.iter().enumerate() {
                                ui.horizontal_top(|ui| {
                                    icon_circle(
                                        ui,
                                        22.0,
                                        tint(color, 170),
                                        Icon::Text(&(i + 1).to_string()),
                                    );
                                    ui.add(
                                        egui::Label::new(RichText::new(step).color(TEXT)).wrap(),
                                    );
                                });
                            }
                        }

                        ui.add_space(14.0);
                        ui.horizontal(|ui| {
                            if primary_button(ui, if d.success { "Done" } else { "Close" }, true)
                                .clicked()
                            {
                                close = true;
                            }
                            if secondary_button(ui, "Show log", true).clicked() {
                                show_log = true;
                                close = true;
                            }
                        });
                    });
            });

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            close = true;
        }
        if show_log {
            self.show_log = true;
        }
        if close {
            self.dialog = None;
        }
    }

    pub(super) fn ui_about(&mut self, ctx: &egui::Context) {
        if !self.show_about {
            return;
        }
        Self::overlay(ctx, "about_overlay");
        let screen = ctx.screen_rect();
        let mut close = false;
        egui::Area::new(egui::Id::new("about"))
            .order(egui::Order::Tooltip)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                Self::window_frame(ACCENT).show(ui, |ui| {
                    ui.set_width(560.0_f32.min(screen.width() - 80.0));
                    {
                        {
                            ui.vertical_centered(|ui| {
                                logo(ui);
                                ui.add_space(6.0);
                                ui.label(bold("About & credits").size(22.0).color(TEXT));
                                ui.label(
                                    RichText::new(format!("Installer version {}", env!("CARGO_PKG_VERSION")))
                                        .size(12.5)
                                        .color(MUTED),
                                );
                            });
                            ui.add_space(10.0);

                            let section = |ui: &mut egui::Ui, title: &str, lines: &[&str]| {
                                ui.add_space(6.0);
                                ui.label(bold(title).size(15.0).color(TEXT));
                                for l in lines {
                                    ui.add(egui::Label::new(RichText::new(*l).color(MUTED)).wrap());
                                }
                            };

                            section(
                                ui,
                                "Android Auto for EntryNav2",
                                &[
                                    "Made by Jiraiya43 and contributors.",
                                    "Licence: PolyForm Noncommercial 1.0.0.",
                                ],
                            );
                            ui.hyperlink_to(
                                RichText::new(format!("github.com/{}", github::REPO)).size(13.0),
                                format!("https://github.com/{}", github::REPO),
                            );

                            section(
                                ui,
                                "This installer",
                                &[
                                    "Unofficial tool that automates the project's installation procedure. Not supported by its authors.",
                                    "Report installer problems here, not to the project.",
                                ],
                            );

                            section(
                                ui,
                                "Built with",
                                &[
                                    "egui / eframe (interface), egui_commonmark (release notes), \
                                     ssh2 / libssh2 (SSH), reqwest and rustls (downloads), sha2 (SHA-256), \
                                     rfd (file picker), anyhow, serde.",
                                ],
                            );

                            section(
                                ui,
                                "Trademarks",
                                &[
                                    "Not affiliated with BMW, MINI or Google. Android Auto is a trademark of \
                                     Google LLC. BMW, MINI and iDrive are trademarks of BMW AG.",
                                ],
                            );

                            ui.add_space(14.0);
                            if primary_button(ui, "Close", true).clicked() {
                                close = true;
                            }
                        }
                    }
                });
            });
        if close || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.show_about = false;
        }
    }

    /// Dims the app and blocks clicks on it while a window is open.
    pub(super) fn overlay(ctx: &egui::Context, id: &str) {
        let screen = ctx.screen_rect();
        egui::Area::new(egui::Id::new(id))
            .order(egui::Order::Foreground)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                let (rect, _) = ui.allocate_exact_size(screen.size(), Sense::click());
                ui.painter()
                    .rect_filled(rect, 0.0, Color32::from_black_alpha(170));
            });
    }

    pub(super) fn window_frame(color: Color32) -> Frame {
        Frame::none()
            .fill(CARD)
            .stroke(Stroke::new(1.0_f32, tint(color, 140)))
            .rounding(18.0)
            .inner_margin(Margin::same(28.0))
    }

    /// Shown once at start-up: the safety rules, accepted for the session.
    pub(super) fn ui_welcome(&mut self, ctx: &egui::Context) {
        if self.safety_accepted {
            return;
        }
        Self::overlay(ctx, "welcome_overlay");
        let screen = ctx.screen_rect();
        let (mut accept, mut quit) = (false, false);
        egui::Area::new(egui::Id::new("welcome"))
            .order(egui::Order::Tooltip)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                Self::window_frame(ORANGE).show(ui, |ui| {
                    ui.set_width(560.0_f32.min(screen.width() - 80.0));
                    ui.vertical_centered(|ui| {
                        logo(ui);
                        ui.add_space(10.0);
                        ui.label(bold("Before you start").size(22.0).color(TEXT));
                        ui.label(
                            RichText::new("Read once before using the app.")
                                .color(MUTED),
                        );
                    });
                    ui.add_space(12.0);
                    let rules: [(&str, &str); 5] = [
                        ("Park the car", "Never use the app, or the head unit's shell, while driving."),
                        ("Ignition on, all the way", "Never turn it off during an installation."),
                        (
                            "Experimental software",
                            "It changes the head unit's system. Use at your own risk.",
                        ),
                        (
                            "Not affiliated with BMW or Google",
                            "Only on your own car. It may affect your warranty.",
                        ),
                        (
                            "Everything is checked first",
                            "Nothing is written before the check passes, and every change is confirmed.",
                        ),
                    ];
                    for (i, (title, text)) in rules.into_iter().enumerate() {
                        ui.horizontal_top(|ui| {
                            let n = (i + 1).to_string();
                            icon_circle(ui, 28.0, tint(ACCENT, 150), Icon::Text(&n));
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 2.0;
                                ui.label(bold(title).color(TEXT));
                                ui.add(egui::Label::new(RichText::new(text).size(13.0).color(MUTED)).wrap());
                            });
                        });
                        ui.add_space(2.0);
                    }
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        if primary_button(ui, "I understand, continue", true).clicked() {
                            accept = true;
                        }
                        if secondary_button(ui, "Quit", true).clicked() {
                            quit = true;
                        }
                    });
                });
            });
        if accept {
            self.safety_accepted = true;
            self.log(Level::Info, "Safety rules accepted for this session");
        }
        if quit {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    /// Confirmation of an action that changes the car.
    pub(super) fn ui_confirm(&mut self, ctx: &egui::Context) {
        let Some(c) = &self.confirm else { return };
        Self::overlay(ctx, "confirm_overlay");
        let screen = ctx.screen_rect();
        let color = if c.danger { RED } else { ACCENT };
        let (mut go, mut cancel) = (false, false);
        egui::Area::new(egui::Id::new("confirm"))
            .order(egui::Order::Tooltip)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                Self::window_frame(color).show(ui, |ui| {
                    ui.set_width(500.0_f32.min(screen.width() - 80.0));
                    ui.vertical_centered(|ui| {
                        icon_circle(ui, 56.0, color, Icon::Text("?"));
                        ui.add_space(6.0);
                        ui.label(bold(&c.title).size(21.0).color(TEXT));
                    });
                    ui.add_space(10.0);
                    for l in &c.lines {
                        ui.horizontal_top(|ui| {
                            ui.label(RichText::new("•").color(MUTED));
                            ui.add(egui::Label::new(RichText::new(l).color(TEXT)).wrap());
                        });
                    }
                    if !c.warnings.is_empty() {
                        ui.add_space(6.0);
                        Frame::none()
                            .fill(tint(ORANGE, 22))
                            .rounding(10.0)
                            .inner_margin(Margin::symmetric(12.0, 8.0))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                for w in &c.warnings {
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(w.to_string()).color(ORANGE),
                                        )
                                        .wrap(),
                                    );
                                }
                            });
                    }
                    ui.add_space(14.0);
                    ui.horizontal(|ui| {
                        let clicked = if c.danger {
                            danger_button(ui, &c.button, true).clicked()
                        } else {
                            primary_button(ui, &c.button, true).clicked()
                        };
                        if clicked {
                            go = true;
                        }
                        if secondary_button(ui, "Cancel", true).clicked() {
                            cancel = true;
                        }
                    });
                });
            });
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            cancel = true;
        }
        if go {
            let action = c.action;
            self.confirm = None;
            self.start(action, ctx);
        } else if cancel {
            self.confirm = None;
        }
    }

    /// Refuses to close while the system partition is being written.
    pub(super) fn guard_close(&mut self, ctx: &egui::Context) {
        if self.close_now {
            self.close_now = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        let critical = self.task.is_some_and(Action::is_critical);
        let close_requested = ctx.input(|i| i.viewport().close_requested());
        if critical && close_requested {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.close_blocked = true;
        }
        // Leaving after a check without installing: clean the car's /tmp first.
        if close_requested && !self.busy() && !self.close_after_task && self.needs_discard() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.close_after_task = true;
            self.log(
                Level::Info,
                "Removing the checked files from the car before closing…",
            );
            self.start(Action::Discard, ctx);
            if !self.busy() {
                self.close_now = true;
            }
        }
        if self.close_blocked && critical {
            egui::Window::new("Please wait")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label(
                        RichText::new(
                            "The head unit's system is being modified.\n\
                             The window can be closed once the operation is over.\n\
                             Keep the ignition on.",
                        )
                        .color(TEXT),
                    );
                    if ui.button("OK").clicked() {
                        self.close_blocked = false;
                    }
                });
        }
    }

    /// A checked archive is still in the car's /tmp.
    pub(super) fn needs_discard(&self) -> bool {
        let Some(r) = &self.report else { return false };
        !self.check_used
            && !matches!(r.verdict(), Verdict::Blocked(_))
            && self.car_id.as_deref() == Some(r.host.as_str())
            && self
                .car_info
                .as_ref()
                .is_some_and(|i| i.staged.contains(&r.version))
    }
}
