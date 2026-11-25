//! Sidebar, status bar and log.

use super::*;

impl App {
    pub(super) fn ui_sidebar(&mut self, ui: &mut egui::Ui) {
        egui::TopBottomPanel::bottom("sidebar_status")
            .frame(Frame::none())
            .show_separator_line(false)
            .show_inside(ui, |ui| self.ui_sidebar_status(ui));

        ui.add_space(6.0);
        logo(ui);
        ui.add_space(18.0);

        section_label(ui, "INSTALLATION");
        let steps = [
            (Page::Connection, "1", "Connection"),
            (Page::Version, "2", "Version"),
            (Page::Check, "3", "Check"),
            (Page::Install, "4", "Install"),
        ];
        for (page, badge, title) in steps {
            let (state, sub) = self.step_state(page);
            if nav_item(ui, badge, title, &sub, state, self.page == page).clicked() {
                self.page = page;
            }
        }
        ui.add_space(8.0);
        section_label(ui, "TOOLS");
        let (state, sub) = self.step_state(Page::Maintenance);
        if nav_item(
            ui,
            "M",
            "Maintenance",
            &sub,
            state,
            self.page == Page::Maintenance,
        )
        .clicked()
        {
            self.page = Page::Maintenance;
        }
        let (state, sub) = self.step_state(Page::UsbStick);
        if nav_item(
            ui,
            "U",
            "USB update stick",
            &sub,
            state,
            self.page == Page::UsbStick,
        )
        .clicked()
        {
            self.page = Page::UsbStick;
        }
    }

    pub(super) fn ui_sidebar_status(&mut self, ui: &mut egui::Ui) {
        Frame::none()
            .fill(CARD)
            .stroke(Stroke::new(1.0_f32, BORDER))
            .rounding(12.0)
            .inner_margin(Margin::same(12.0))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 4.0;
                    ui.horizontal(|ui| {
                        let (c, t) = match &self.car_info {
                            Some(_) => (GREEN, "Car detected"),
                            None => (MUTED, "Not connected"),
                        };
                        dot(ui, c);
                        ui.label(bold(t).size(13.0).color(TEXT));
                    });
                    ui.label(
                        RichText::new(format!("{}@{}", self.user, self.host))
                            .size(12.0)
                            .color(MUTED),
                    );
                    if let Some(info) = &self.car_info {
                        let aa = match &info.installed {
                            Installed::None => "Android Auto: not installed".to_string(),
                            Installed::Unknown => "Android Auto: unknown version".into(),
                            Installed::Version(v) => format!("Android Auto: {v}"),
                        };
                        ui.label(RichText::new(aa).size(12.0).color(MUTED));
                        if info.running {
                            dot(ui, ORANGE);
                            ui.label(RichText::new("Running").size(12.0).color(ORANGE));
                        }
                    }
                });
            });
        ui.add_space(2.0);
        let about = ui
            .add(
                egui::Label::new(
                    RichText::new("About & credits")
                        .size(12.5)
                        .color(ACCENT_HOVER),
                )
                .sense(Sense::click()),
            )
            .on_hover_cursor(CursorIcon::PointingHand);
        if about.clicked() {
            self.show_about = true;
        }
    }

    pub(super) fn ui_statusbar(&mut self, ui: &mut egui::Ui) {
        // Right side first, so the status text is cut instead of overlapping.
        let total_errors = self.logs.iter().filter(|(l, _)| *l == Level::Error).count();
        if self.show_log {
            self.errors_seen = total_errors;
        }
        let new_errors = total_errors.saturating_sub(self.errors_seen);

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let label = if self.show_log {
                "Hide log"
            } else {
                "Show log"
            };
            if small_button(ui, label, TEXT, BORDER_STRONG).clicked() {
                self.show_log = !self.show_log;
            }
            if new_errors > 0 {
                let text = if new_errors == 1 {
                    "1 new error · view".to_string()
                } else {
                    format!("{new_errors} new errors · view")
                };
                if small_button(ui, &text, RED, tint(RED, 150)).clicked() {
                    self.show_log = true;
                }
            }
            ui.add_space(8.0);
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                self.ui_status_text(ui);
            });
        });
    }

    pub(super) fn ui_status_text(&mut self, ui: &mut egui::Ui) {
        {
            match (&self.progress, self.busy()) {
                (Some((label, f)), _) if *f >= 0.0 => {
                    ui.add(egui::Spinner::new().size(14.0).color(ACCENT));
                    ui.label(RichText::new(label).size(13.0).color(TEXT));
                    ui.add(
                        egui::ProgressBar::new(*f)
                            .desired_width(220.0)
                            .fill(ACCENT)
                            .show_percentage(),
                    );
                }
                (Some((label, _)), _) => {
                    ui.add(egui::Spinner::new().size(14.0).color(ACCENT));
                    ui.label(RichText::new(label).size(13.0).color(TEXT));
                }
                (None, true) => {
                    ui.add(egui::Spinner::new().size(14.0).color(ACCENT));
                    ui.label(RichText::new("Working…").size(13.0).color(TEXT));
                }
                (None, false) => {
                    let (level, last) = self
                        .logs
                        .iter()
                        .rev()
                        .find(|(l, _)| *l != Level::Remote)
                        .map(|(l, s)| (*l, s.as_str()))
                        .unwrap_or((Level::Info, "Ready"));
                    let (dot_color, text_color) = match level {
                        Level::Error => (RED, RED),
                        Level::Warn => (ORANGE, ORANGE),
                        _ => (GREEN, MUTED),
                    };
                    let first_line = last.lines().next().unwrap_or("");
                    dot(ui, dot_color);
                    ui.add(
                        egui::Label::new(RichText::new(first_line).size(13.0).color(text_color))
                            .truncate(),
                    )
                    .on_hover_text(last);
                }
            }
        }
    }

    pub(super) fn ui_log(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(bold("Log").size(13.0).color(TEXT));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if small_button(ui, "Clear", TEXT, BORDER_STRONG).clicked() {
                    self.logs.clear();
                    self.errors_seen = 0;
                }
                if small_button(ui, "Copy", TEXT, BORDER_STRONG).clicked() {
                    let text: String = self.logs.iter().map(|(_, s)| format!("{s}\n")).collect();
                    ui.ctx().copy_text(text);
                }
            });
        });
        ScrollArea::vertical()
            .stick_to_bottom(true)
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 3.0;
                for (lvl, line) in &self.logs {
                    let (prefix, color) = match lvl {
                        Level::Info => ("-", TEXT),
                        Level::Remote => ("|", MUTED),
                        Level::Ok => ("+", GREEN),
                        Level::Warn => ("!", ORANGE),
                        Level::Error => ("x", RED),
                    };
                    ui.label(
                        RichText::new(format!("{prefix} {line}"))
                            .monospace()
                            .color(color),
                    );
                }
            });
    }

    pub(super) fn ui_update_banner(&mut self, ui: &mut egui::Ui) {
        if self.update_hidden {
            return;
        }
        let Some(u) = &self.installer_update else {
            return;
        };
        let (version, page) = (u.version.clone(), u.page.clone());
        let updating = self.task == Some(Action::UpdateInstaller);
        let busy = self.busy();
        let (mut update, mut later) = (false, false);
        Frame::none()
            .fill(tint(ACCENT, 30))
            .stroke(Stroke::new(1.0_f32, tint(ACCENT, 120)))
            .rounding(12.0)
            .inner_margin(Margin::symmetric(16.0, 10.0))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        ui.label(bold(format!("Installer {version} is available")).color(TEXT));
                        let sub = if updating {
                            "Updating… the installer restarts by itself.".to_string()
                        } else if busy {
                            "You can update once the current task is over.".to_string()
                        } else {
                            format!(
                                "You have {}. Nothing on the car is changed.",
                                env!("CARGO_PKG_VERSION")
                            )
                        };
                        ui.label(RichText::new(sub).size(12.5).color(MUTED));
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if !updating {
                            later = small_button(ui, "Later", MUTED, BORDER_STRONG).clicked();
                        }
                        if small_button(ui, "What's new", TEXT, BORDER_STRONG).clicked() {
                            ui.ctx().open_url(egui::OpenUrl::new_tab(&page));
                        }
                        let btn =
                            egui::Button::new(bold("Update").size(13.0).color(Color32::WHITE))
                                .fill(if busy { tint(ACCENT, 70) } else { ACCENT })
                                .stroke(Stroke::NONE)
                                .rounding(BUTTON_ROUNDING)
                                .min_size(egui::vec2(90.0, 28.0));
                        update = ui
                            .add_enabled(!busy, btn)
                            .on_hover_cursor(CursorIcon::PointingHand)
                            .clicked();
                    });
                });
            });
        ui.add_space(14.0);
        if later {
            self.update_hidden = true;
        }
        if update {
            self.confirm = Some(Confirm {
                action: Action::UpdateInstaller,
                title: format!("Update the installer to {version}?"),
                lines: vec![
                    "The new version is downloaded from GitHub, checked (SHA-256), then replaces this one."
                        .into(),
                    "The installer then restarts. Nothing on the car is changed.".into(),
                ],
                warnings: vec![],
                button: "Update and restart".into(),
                danger: false,
            });
        }
    }
}
