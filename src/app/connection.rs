//! Step 1: connection to the car.

use super::*;

impl App {
    /// Card shown when Android Auto runs, with a button to stop it.
    pub(super) fn running_card(
        &mut self,
        ui: &mut egui::Ui,
        action: &mut Option<Action>,
        context: &str,
    ) {
        if !self.car_info.as_ref().is_some_and(|i| i.running) {
            return;
        }
        let busy = self.busy();
        Frame::none()
            .fill(tint(ORANGE, 24))
            .stroke(Stroke::new(1.0_f32, tint(ORANGE, 110)))
            .rounding(12.0)
            .inner_margin(Margin::symmetric(18.0, 14.0))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    icon_circle(ui, 30.0, ORANGE, "▶");
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 3.0;
                        ui.label(bold("Android Auto is running").size(16.0).color(TEXT));
                        ui.label(RichText::new(context).color(TEXT));
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if secondary_button(ui, "■  Stop Android Auto", !busy).clicked() {
                            *action = Some(Action::Stop);
                        }
                    });
                });
            });
        ui.add_space(6.0);
    }

    pub(super) fn page_connection(&mut self, ui: &mut egui::Ui, action: &mut Option<Action>) {
        page_header(
            ui,
            "Connect to the car",
            "Connect to the head unit over SSH.",
        );

        card(ui, |ui| {
            ui.horizontal(|ui| {
                icon_circle(ui, 34.0, tint(ACCENT, 90), "⚡");
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    ui.label(bold("Before you start").color(TEXT));
                    ui.label(
                        RichText::new(
                            "Park the car. Plug the ENET cable between the computer and the OBD port, \
                             then turn the ignition on.",
                        )
                        .color(MUTED),
                    );
                });
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                field(
                    ui,
                    "IP address",
                    &mut self.host,
                    220.0,
                    false,
                    "169.254.1.0",
                );
                field(ui, "Port", &mut self.port, 70.0, false, "22");
            });
            ui.horizontal(|ui| {
                field(ui, "User", &mut self.user, 220.0, false, "root");
                let r = field(
                    ui,
                    "Root password",
                    &mut self.password,
                    220.0,
                    true,
                    "Not provided by the project",
                );
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    *action = Some(Action::TestConnection);
                }
            });
            if toggle_row(ui, &mut self.remember_password, "Remember the password", "").changed()
                && !self.remember_password
            {
                crate::config::store_password(&self.user, "", false);
            }
            ui.add_space(6.0);
            let connecting = self.task == Some(Action::TestConnection);
            ui.horizontal(|ui| {
                let label = if self.car_info.is_some() {
                    "Reconnect"
                } else {
                    "Connect"
                };
                if primary_button(ui, label, !self.busy()).clicked() {
                    *action = Some(Action::TestConnection);
                }
                if connecting {
                    ui.add(egui::Spinner::new().size(18.0).color(ACCENT));
                    ui.label(RichText::new("Connecting…").color(MUTED));
                }
            });
        });

        let Some(info) = self.car_info.clone() else {
            return;
        };
        self.running_card(ui, action, "Stop it before installing or uninstalling.");
        card(ui, |ui| {
            card_title(ui, "Car", None);
            let latest = self.latest_stable();
            let (state, color) = match (&info.installed, &latest) {
                (Installed::None, _) => ("Not installed".to_string(), MUTED),
                (Installed::Unknown, _) => ("Unknown version".into(), ORANGE),
                (Installed::Version(v), Some(l)) => match compare_versions(v, l) {
                    Ordering::Less => (format!("{l} available"), ORANGE),
                    _ => ("Up to date".into(), GREEN),
                },
                (Installed::Version(_), None) => ("—".into(), MUTED),
            };
            let installed = match &info.installed {
                Installed::None => "—".to_string(),
                Installed::Unknown => "Installed (version not recognised)".into(),
                Installed::Version(v) => v.clone(),
            };
            ui.columns(3, |cols| {
                stat_tile(&mut cols[0], "System", &info.sysver, TEXT);
                stat_tile(&mut cols[1], "Android Auto", &installed, TEXT);
                stat_tile(&mut cols[2], "Update", &state, color);
            });
            ui.horizontal(|ui| {
                if info.running {
                    pill(ui, "RUNNING", ORANGE);
                } else {
                    pill(ui, "STOPPED", MUTED);
                }
                pill(
                    ui,
                    if info.usb_mode {
                        "USB MODE"
                    } else {
                        "WIRELESS MODE"
                    },
                    ACCENT,
                );
                if info.disabled {
                    pill(ui, "DISABLED", ORANGE);
                }
            });
            ui.add_space(6.0);
            if primary_button(ui, "Choose the version  →", true).clicked() {
                self.page = Page::Version;
            }
        });
    }
}
