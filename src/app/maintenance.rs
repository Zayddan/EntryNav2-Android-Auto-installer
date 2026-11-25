//! Maintenance tools, offered only when they make sense.

use super::*;

impl App {
    pub(super) fn page_maintenance(&mut self, ui: &mut egui::Ui, action: &mut Option<Action>) {
        page_header(
            ui,
            "Maintenance",
            "Tools for the car. They use the connection from step 1.",
        );
        let stop_block = self.maintenance_block(Action::Stop);
        let reboot_block = self.maintenance_block(Action::Reboot);
        let enable_block = self.maintenance_block(Action::Enable);
        let disable_block = self.maintenance_block(Action::Disable);
        let uninstall_block = self.maintenance_block(Action::Uninstall);
        let installed = self
            .car_info
            .as_ref()
            .is_some_and(|i| i.installed != Installed::None);

        card(ui, |ui| {
            card_title(
                ui,
                "State of the car",
                Some("As last read. Refresh after using the iDrive menu."),
            );
            ui.horizontal(|ui| {
                match self.car_info.as_ref().map(|i| &i.installed) {
                    Some(Installed::Version(v)) => pill(ui, &format!("ANDROID AUTO {v}"), GREEN),
                    Some(Installed::Unknown) => pill(ui, "ANDROID AUTO (UNKNOWN VERSION)", ORANGE),
                    _ => pill(ui, "ANDROID AUTO NOT INSTALLED", MUTED),
                }
                if let Some(i) = &self.car_info {
                    if i.running {
                        pill(ui, "RUNNING", ORANGE);
                    } else if installed {
                        pill(ui, "STOPPED", MUTED);
                    }
                    if installed && i.disabled {
                        pill(ui, "DISABLED (app.disable)", ORANGE);
                    }
                    if installed && i.guard {
                        pill(ui, "GUARD COUNTER PRESENT", ORANGE);
                    }
                }
            });
            if secondary_button(ui, "Refresh status", !self.busy()).clicked() {
                *action = Some(Action::TestConnection);
            }
        });

        card(ui, |ui| {
            card_title(
                ui,
                "Stop Android Auto",
                Some("Same as Stop in the iDrive menu."),
            );
            ui.horizontal(|ui| {
                if secondary_button(ui, "Stop Android Auto", stop_block.is_none()).clicked() {
                    *action = Some(Action::Stop);
                }
                hint(ui, stop_block);
            });
        });

        card(ui, |ui| {
            card_title(
                ui,
                "Reboot the head unit",
                Some("Restarts the iDrive, for example after an installation."),
            );
            ui.horizontal(|ui| {
                if secondary_button(ui, "Reboot", reboot_block.is_none()).clicked() {
                    let running = self.car_info.as_ref().is_some_and(|i| i.running);
                    let mut lines = vec![
                        "The iDrive screen goes dark and restarts (about a minute).".to_string(),
                        "Keep the ignition on until it is back.".into(),
                    ];
                    if running {
                        lines.insert(1, "The Android Auto session in progress ends.".into());
                    }
                    self.confirm = Some(Confirm {
                        action: Action::Reboot,
                        title: "Reboot the head unit?".into(),
                        lines,
                        warnings: Vec::new(),
                        button: "Reboot".into(),
                        danger: false,
                    });
                }
                hint(ui, reboot_block);
            });
        });

        card(ui, |ui| {
            card_title(
                ui,
                "Enable and safety guard",
                Some(
                    "After 3 quick restarts of the iDrive, the safety guard switches the app off.",
                ),
            );

            if self.car_info.as_ref().is_some_and(|i| installed && i.guard) {
                ui.label(
                    RichText::new("Guard counter present: normal for 2 minutes after a start.")
                        .size(12.5)
                        .color(MUTED),
                );
            }
            if enable_block.is_none() {
                banner(
                    ui,
                    ORANGE,
                    Icon::Text("!"),
                    "Only re-enable after fixing the cause",
                    &["Otherwise the restart loop comes back.".into()],
                );
            }
            if enable_block.is_none() || disable_block.is_none() {
                toggle_row(
                    ui,
                    &mut self.reboot_after,
                    "Reboot the head unit afterwards",
                    "",
                );
            }
            ui.horizontal(|ui| {
                if secondary_button(ui, "Re-enable Android Auto", enable_block.is_none())
                    .clicked()
                {
                    self.confirm = Some(Confirm {
                        action: Action::Enable,
                        title: "Re-enable Android Auto?".into(),
                        lines: vec![
                            "The iDrive will load the app again at its next start.".into(),
                            "The safety guard counter is reset.".into(),
                        ],
                        warnings: vec![
                            "Only do this once the cause of the restart loop is fixed, \
                             otherwise the iDrive will restart in a loop again."
                                .into(),
                        ],
                        button: "Re-enable".into(),
                        danger: false,
                    });
                }
                if secondary_button(
                    ui,
                    "Disable without uninstalling",
                    disable_block.is_none(),
                )
                .clicked()
                {
                    let mut lines = vec![
                        "The iDrive will no longer load Android Auto at its next start (file app.disable)."
                            .to_string(),
                        "Nothing is uninstalled: Re-enable brings it back.".into(),
                    ];
                    if self.reboot_after {
                        lines.push("The head unit reboots afterwards.".into());
                    }
                    self.confirm = Some(Confirm {
                        action: Action::Disable,
                        title: "Disable Android Auto?".into(),
                        lines,
                        warnings: Vec::new(),
                        button: "Disable".into(),
                        danger: false,
                    });
                }
            });
            match (enable_block, disable_block) {
                (Some(a), Some(b)) if a == b => hint(ui, Some(a)),
                (a, b) => {
                    hint(ui, a.map(|r| format!("Re-enable: {r}")).as_deref());
                    hint(ui, b.map(|r| format!("Disable: {r}")).as_deref());
                }
            }
        });

        Frame::none()
            .fill(tint(RED, 12))
            .stroke(Stroke::new(1.0_f32, tint(RED, 90)))
            .rounding(14.0)
            .inner_margin(Margin::same(20.0))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                card_title(
                    ui,
                    "Uninstall Android Auto",
                    Some("Android Auto is stopped first if it runs. Only folders created by the project are removed."),
                );
                if uninstall_block.is_none() {
                    toggle_row(ui, &mut self.keep_settings, "Keep settings", "Folder /var/local/data/AA");
                    toggle_row(ui, &mut self.reboot_after, "Reboot the head unit afterwards", "");
                }
                ui.horizontal(|ui| {
                    if danger_button(ui, "Uninstall", uninstall_block.is_none()).clicked() {
                        self.confirm = Some(self.uninstall_confirm());
                    }
                    hint(ui, uninstall_block);
                });
            });
    }

    /// Why a maintenance action cannot, or need not, be done now. `None`: it makes sense.
    pub(super) fn maintenance_block(&self, action: Action) -> Option<&'static str> {
        if self.busy() {
            return Some("Wait for the current task to finish");
        }
        let Some(info) = &self.car_info else {
            return Some("Connect to the car first");
        };
        let installed = info.installed != Installed::None;
        match action {
            Action::Stop if !info.running => Some("Android Auto is not running"),
            Action::Uninstall | Action::Enable | Action::Disable if !installed => {
                Some("Android Auto is not installed on this car")
            }
            Action::Enable if !info.disabled && !info.guard => {
                Some("Android Auto is already enabled")
            }
            Action::Disable if info.disabled => Some("Android Auto is already disabled"),
            _ => None,
        }
    }

    pub(super) fn uninstall_confirm(&self) -> Confirm {
        let what = match self.installed() {
            Some(Installed::Version(v)) => format!("Android Auto {v}"),
            _ => "Android Auto".into(),
        };
        let mut lines = vec![format!(
            "{what} will be removed from the car (programs and iDrive plugin)."
        )];
        if self.car_info.as_ref().is_some_and(|i| i.running) {
            lines.push("It is running: it will be stopped first (the phone session ends).".into());
        }
        lines.push(if self.keep_settings {
            "Your settings in /var/local/data/AA are kept.".into()
        } else {
            "Your settings in /var/local/data/AA are deleted too.".into()
        });
        if self.reboot_after {
            lines.push("The head unit reboots at the end.".into());
        }
        Confirm {
            action: Action::Uninstall,
            title: "Uninstall Android Auto?".into(),
            lines,
            warnings: Vec::new(),
            button: "Uninstall".into(),
            danger: true,
        }
    }
}
