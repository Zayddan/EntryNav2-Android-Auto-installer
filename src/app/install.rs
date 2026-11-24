//! Step 4: installation.

use super::*;

impl App {
    pub(super) fn page_install(&mut self, ui: &mut egui::Ui, action: &mut Option<Action>) {
        page_header(
            ui,
            "Install",
            "Replaces the version on the car with the selected one, then reboots the head unit.",
        );

        if let Some(v) = &self.just_installed {
            banner(
                ui,
                GREEN,
                "✔",
                &format!("Android Auto {v} is installed"),
                &[
                    "Once the head unit has restarted: iDrive › Media › Android Auto › Start."
                        .into(),
                ],
            );
        }
        if let Some(e) = &self.install_error {
            banner(
                ui,
                RED,
                "✖",
                "The installation did not complete",
                std::slice::from_ref(e),
            );
        }

        let target = self.target_version();
        let target_s = target.clone().unwrap_or_else(|| "—".into());
        let current = match self.installed() {
            Some(Installed::Version(v)) => v.clone(),
            Some(Installed::Unknown) => "Unknown version".into(),
            Some(Installed::None) => "None".into(),
            None => "Not connected".into(),
        };

        card(ui, |ui| {
            ui.columns(2, |cols| {
                stat_tile(&mut cols[0], "On the car now", &current, MUTED);
                stat_tile(&mut cols[1], "Will be installed", &target_s, ACCENT_HOVER);
            });
        });

        if self.has_current_install() {
            let relation = match (self.installed_version(), &target) {
                (Some(cur), Some(t)) => match compare_versions(t, cur) {
                    Ordering::Greater => None,
                    Ordering::Equal => Some(("Same version: it will be reinstalled.", MUTED)),
                    Ordering::Less => Some((
                        "The selected version is OLDER than the installed one (downgrade). \
                         Settings written by a newer version may not be understood.",
                        ORANGE,
                    )),
                },
                _ => None,
            };
            Frame::none()
                .fill(tint(ORANGE, 14))
                .stroke(Stroke::new(1.0_f32, tint(ORANGE, 100)))
                .rounding(14.0)
                .inner_margin(Margin::same(20.0))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    card_title(
                        ui,
                        "Replace the installed version",
                        Some("Your settings are kept."),
                    );
                    if let Some((text, color)) = relation {
                        ui.label(RichText::new(text).color(color));
                    }
                });
            ui.add_space(6.0);
        }

        self.running_card(
            ui,
            action,
            "It has to be stopped before the files are replaced. Stop it now, or let the installer do it.",
        );

        card(ui, |ui| {
            card_title(ui, "Options", None);
            toggle_row(
                ui,
                &mut self.auto_stop,
                "Stop Android Auto automatically",
                "If it runs, it is stopped the way the iDrive menu does, before anything is replaced",
            );
            if self.wired_only() {
                let already = self.car_info.as_ref().is_some_and(|i| i.usb_mode);
                toggle_row(
                    ui,
                    &mut self.usb_mode,
                    "Select USB cable mode",
                    if already {
                        "Already selected on this car"
                    } else {
                        "Wireless is not compatible with this car: Android Auto will use a USB cable"
                    },
                );
            }
            toggle_row(
                ui,
                &mut self.vehicle_enabled,
                "Vehicle file",
                "Optional: model, year and fuel shown on the phone",
            );
            if self.vehicle_enabled {
                ui.indent("vehicle", |ui| {
                    ui.horizontal(|ui| {
                        field(
                            ui,
                            "Model (24 characters max.)",
                            &mut self.model,
                            200.0,
                            false,
                            "e.g. 320d",
                        );
                        field(ui, "Year", &mut self.year, 90.0, false, "e.g. 2019");
                    });
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.label(RichText::new("Fuel").size(12.5).color(MUTED));
                            segmented(
                                ui,
                                &mut self.fuel,
                                &[(Fuel::Petrol, "Petrol"), (Fuel::Diesel, "Diesel")],
                            );
                        });
                        ui.vertical(|ui| {
                            ui.label(RichText::new("Driver side").size(12.5).color(MUTED));
                            segmented(
                                ui,
                                &mut self.side,
                                &[
                                    (Side::Auto, "Auto"),
                                    (Side::Left, "Left"),
                                    (Side::Right, "Right"),
                                ],
                            );
                        });
                    });
                    if let Some(Err(e)) = self.vehicle().map(|v| v.validate()) {
                        ui.label(RichText::new(e).color(RED));
                    }
                });
            }
            toggle_row(
                ui,
                &mut self.reboot_after,
                "Reboot when done",
                "Needed to load the new version. Never done if the installation fails.",
            );
        });

        let check = self.can_install();
        let installing = self.task == Some(Action::Install);
        ui.horizontal(|ui| {
            let label = match (&target, self.has_current_install()) {
                (Some(v), true) => format!("Replace {current} with {v}"),
                (Some(v), false) => format!("Install version {v}"),
                (None, _) => "Install".into(),
            };
            if primary_button(ui, &label, !self.busy() && check.is_ok()).clicked() {
                self.confirm = Some(self.install_confirm());
            }
            if installing {
                ui.add(egui::Spinner::new().size(18.0).color(ACCENT));
                ui.label(RichText::new("Installing. Keep the ignition on.").color(ORANGE));
            } else if let Err(why) = check {
                ui.label(RichText::new(why).color(MUTED));
            }
        });
    }

    pub(super) fn install_confirm(&self) -> Confirm {
        let v = self.target_version().unwrap_or_default();
        let running = self.car_info.as_ref().is_some_and(|i| i.running);
        let mut lines = Vec::new();
        let mut warnings = Vec::new();

        match self.installed() {
            Some(Installed::Version(cur)) => {
                lines.push(format!(
                    "Android Auto {cur} will be deleted from the car and replaced by {v}."
                ));
                match compare_versions(&v, cur) {
                    Ordering::Less => warnings.push(format!(
                        "{v} is OLDER than the installed {cur} (downgrade): settings written by a newer \
                         version may not be understood."
                    )),
                    Ordering::Equal => lines.push("It is the same version: it will be reinstalled.".into()),
                    Ordering::Greater => {}
                }
            }
            Some(Installed::Unknown) => lines.push(format!(
                "The version on the car (not recognised) will be deleted and replaced by {v}."
            )),
            _ => lines.push(format!("Android Auto {v} will be installed on the car.")),
        }
        if self.has_current_install() {
            lines.push("Your settings in /var/local/data/AA are kept.".into());
        }
        if running {
            lines.push(
                "Android Auto is running: it will be stopped first (the phone session ends)."
                    .into(),
            );
        }
        lines.push(if self.reboot_after {
            "The head unit reboots at the end.".into()
        } else {
            "You will have to reboot the head unit yourself.".into()
        });
        lines.push("Keep the ignition on until the end (about a minute).".into());

        if self.is_prerelease_selected() {
            let not_in_car = self
                .selected_release()
                .is_some_and(|r| r.not_tried_in_car());
            warnings.push(if not_in_car {
                "This is a pre-release that, according to its notes, was not tried in a car yet."
                    .into()
            } else {
                "This is a pre-release.".into()
            });
        }
        if self
            .report_for_target()
            .is_some_and(|r| !r.archive_verified)
        {
            warnings.push("The archive's SHA-256 could not be verified.".into());
        }

        Confirm {
            action: Action::Install,
            title: if self.has_current_install() {
                format!("Replace Android Auto with {v}?")
            } else {
                format!("Install Android Auto {v}?")
            },
            lines,
            warnings,
            button: if self.has_current_install() {
                format!("Replace with {v}")
            } else {
                format!("Install {v}")
            },
            danger: false,
        }
    }
}
