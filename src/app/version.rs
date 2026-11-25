//! Step 2: choice of the version.

use super::*;

impl App {
    pub(super) fn page_version(&mut self, ui: &mut egui::Ui, action: &mut Option<Action>) {
        page_header(
            ui,
            "Choose the version",
            "Look at a version and its release notes, then confirm it.",
        );

        ui.horizontal(|ui| {
            segmented(
                ui,
                &mut self.source_mode,
                &[
                    (SourceMode::GitHub, "GitHub releases"),
                    (SourceMode::Local, "Local file"),
                ],
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if let Some(v) = self.target_version() {
                    pill(ui, &format!("CONFIRMED: {v}"), GREEN);
                } else if let Some(v) = self.pending_choice() {
                    pill(ui, &format!("{v} CHOSEN · CONFIRM IT TO CONTINUE"), ORANGE);
                } else {
                    pill(ui, "PICK A VERSION", MUTED);
                }
            });
        });
        ui.add_space(4.0);

        match self.source_mode {
            SourceMode::GitHub => self.version_github(ui, action),
            SourceMode::Local => self.version_local(ui),
        }
    }

    pub(super) fn version_github(&mut self, ui: &mut egui::Ui, action: &mut Option<Action>) {
        if let Some(e) = &self.releases_error {
            banner(
                ui,
                RED,
                Icon::Text("!"),
                "Could not list the versions",
                std::slice::from_ref(e),
            );
        }

        card(ui, |ui| {
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(270.0);
                    ui.horizontal(|ui| {
                        ui.label(bold("Versions").size(16.0).color(TEXT));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui
                                .add_enabled(
                                    !self.busy(),
                                    egui::Button::new("Refresh").rounding(8.0),
                                )
                                .on_hover_text("Refresh")
                                .clicked()
                            {
                                *action = Some(Action::FetchReleases);
                            }
                        });
                    });
                    ui.horizontal(|ui| {
                        if toggle(ui, &mut self.show_prerelease).changed() {
                            self.fix_browse();
                        }
                        ui.label(RichText::new("Show pre-releases").size(13.0).color(MUTED));
                    });
                    if self.releases.is_empty() && self.task == Some(Action::FetchReleases) {
                        ui.horizontal(|ui| {
                            ui.add(egui::Spinner::new().color(ACCENT));
                            ui.label(RichText::new("Loading…").color(MUTED));
                        });
                    }

                    let installed = self.installed_version().map(String::from);
                    let recommended = self.recommended_tag();
                    let rows: Vec<(String, String, String, bool, bool)> = self
                        .visible_releases()
                        .iter()
                        .map(|r| {
                            (
                                r.tag_name.clone(),
                                r.display_version(),
                                r.published_date().to_string(),
                                r.prerelease,
                                r.unsupported_reason().is_none(),
                            )
                        })
                        .collect();

                    ScrollArea::vertical()
                        .id_salt("release_list")
                        .max_height(460.0)
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing.y = 6.0;
                            for (tag, version, date, pre, supported) in rows {
                                let browsed = self.browse_tag.as_deref() == Some(tag.as_str());
                                let selected = self.selected_tag.as_deref() == Some(tag.as_str());
                                let kind = if !supported {
                                    ("Not supported", MUTED)
                                } else if pre {
                                    ("Pre-release", ORANGE)
                                } else if recommended.as_deref() == Some(tag.as_str()) {
                                    ("Recommended", ACCENT_HOVER)
                                } else {
                                    ("Stable", MUTED)
                                };
                                let resp = Frame::none()
                                    .fill(if browsed {
                                        tint(ACCENT, 30)
                                    } else {
                                        CARD_INNER
                                    })
                                    .stroke(Stroke::new(
                                        1.0_f32,
                                        if browsed { ACCENT } else { BORDER },
                                    ))
                                    .rounding(10.0)
                                    .inner_margin(Margin::symmetric(12.0, 8.0))
                                    .show(ui, |ui| {
                                        ui.set_width(ui.available_width());
                                        ui.horizontal(|ui| {
                                            if selected {
                                                icon_circle(ui, 20.0, GREEN, Icon::Check);
                                            } else {
                                                radio_dot(ui, browsed);
                                            }
                                            ui.vertical(|ui| {
                                                ui.spacing_mut().item_spacing.y = 1.0;
                                                let vcolor = if supported { TEXT } else { MUTED };
                                                ui.label(bold(&version).size(15.0).color(vcolor));
                                                ui.horizontal(|ui| {
                                                    ui.spacing_mut().item_spacing.x = 4.0;
                                                    ui.label(
                                                        RichText::new(&date)
                                                            .size(11.5)
                                                            .color(MUTED),
                                                    );
                                                    ui.label(
                                                        RichText::new("·").size(11.5).color(MUTED),
                                                    );
                                                    ui.label(
                                                        RichText::new(kind.0)
                                                            .size(11.5)
                                                            .color(kind.1),
                                                    );
                                                });
                                            });
                                            if installed.as_deref() == Some(version.as_str()) {
                                                ui.with_layout(
                                                    Layout::right_to_left(Align::Center),
                                                    |ui| {
                                                        pill(ui, "ON CAR", GREEN);
                                                    },
                                                );
                                            }
                                        });
                                    })
                                    .response
                                    .interact(Sense::click())
                                    .on_hover_cursor(CursorIcon::PointingHand);
                                if resp.clicked() {
                                    self.browse_tag = Some(tag.clone());
                                }
                                if resp.double_clicked() && supported {
                                    self.browse_tag = Some(tag);
                                    self.confirm_browsed();
                                }
                            }
                        });
                });

                ui.add_space(8.0);
                ui.separator();
                ui.add_space(8.0);

                ui.vertical(|ui| {
                    ui.set_width(ui.available_width());
                    self.release_details(ui);
                });
            });
        });
    }

    pub(super) fn release_details(&mut self, ui: &mut egui::Ui) {
        let Some(r) = self.release(self.browse_tag.as_ref()).cloned() else {
            ui.label(RichText::new("Pick a version on the left.").color(MUTED));
            return;
        };
        if let Some(reason) = r.unsupported_reason() {
            ui.horizontal(|ui| {
                ui.label(bold(r.display_version()).size(26.0).color(MUTED));
                pill(ui, "NOT SUPPORTED", MUTED);
            });
            ui.label(
                RichText::new(format!("Published {}", r.published_date()))
                    .size(12.5)
                    .color(MUTED),
            );
            banner(
                ui,
                ORANGE,
                Icon::Text("!"),
                "This installer cannot install this version",
                &[reason.into()],
            );
            ui.hyperlink_to(
                RichText::new("Open the release on GitHub").size(12.5),
                format!(
                    "https://github.com/{}/releases/tag/{}",
                    github::REPO,
                    r.tag_name
                ),
            );
            return;
        }
        let Some(pkg) = r.package() else { return };
        let version = pkg.version.clone();
        let selected = self.selected_tag.as_deref() == Some(r.tag_name.as_str());

        ui.horizontal(|ui| {
            ui.label(bold(&version).size(26.0).color(TEXT));
            ui.add_space(6.0);
            if r.prerelease {
                pill(ui, "PRE-RELEASE", ORANGE);
            } else {
                pill(ui, "STABLE", GREEN);
            }
            if self.recommended_tag().as_deref() == Some(r.tag_name.as_str()) {
                pill(ui, "RECOMMENDED", ACCENT);
            }
            if r.not_tried_in_car() {
                pill(ui, "NOT TRIED IN A CAR YET", ORANGE);
            }
        });
        ui.label(
            RichText::new(format!(
                "Published {} · {:.1} MB",
                r.published_date(),
                pkg.asset.size as f64 / 1_048_576.0
            ))
            .size(12.5)
            .color(MUTED),
        );

        let (text, color) = match self.installed() {
            None => (
                "Connect to compare with the version on the car.".to_string(),
                MUTED,
            ),
            Some(Installed::None) => (
                "Android Auto is not installed on this car yet.".into(),
                MUTED,
            ),
            Some(Installed::Unknown) => (
                "A version this app does not recognise (not a GitHub release) is on the car. It will be replaced.".into(),
                ORANGE,
            ),
            Some(Installed::Version(cur)) => match compare_versions(&version, cur) {
                Ordering::Greater => (format!("Update: {cur} to {version}"), GREEN),
                Ordering::Equal => (
                    format!("{version} is already installed (reinstall)."),
                    MUTED,
                ),
                Ordering::Less => (
                    format!("Older than the installed {cur}: this is a downgrade."),
                    ORANGE,
                ),
            },
        };
        ui.label(RichText::new(text).color(color));
        ui.add_space(4.0);

        ui.horizontal(|ui| {
            if selected {
                if primary_button(ui, "Continue to the check", true).clicked() {
                    self.page = Page::Check;
                }
                pill(ui, "CONFIRMED", GREEN);
            } else {
                if primary_button(ui, &format!("Confirm {version} and continue"), true).clicked() {
                    self.confirm_browsed();
                }
                ui.label(
                    RichText::new("Goes straight to the check.")
                        .size(12.5)
                        .color(MUTED),
                );
            }
        });

        ui.add_space(6.0);
        ui.separator();
        ui.label(bold("Release notes").size(15.0).color(TEXT));
        let body = r.body.clone().unwrap_or_default();
        if body.trim().is_empty() {
            ui.label(RichText::new("No release notes.").color(MUTED));
        } else {
            Frame::none()
                .fill(CARD_INNER)
                .rounding(10.0)
                .inner_margin(Margin::symmetric(14.0, 10.0))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ScrollArea::vertical()
                        .id_salt(("notes", r.tag_name.as_str()))
                        .max_height(340.0)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing = egui::vec2(6.0, 4.0);
                            CommonMarkViewer::new().show(ui, &mut self.md_cache, body.trim());
                        });
                });
            ui.hyperlink_to(
                RichText::new("Open the release on GitHub").size(12.5),
                format!(
                    "https://github.com/{}/releases/tag/{}",
                    github::REPO,
                    r.tag_name
                ),
            );
        }
    }

    pub(super) fn version_local(&mut self, ui: &mut egui::Ui) {
        card(ui, |ui| {
            card_title(
                ui,
                "Local archive",
                Some("Expected name: entrynav2-android-auto-<version>-armhf.tar"),
            );
            ui.horizontal(|ui| {
                if secondary_button(ui, "Choose file…", !self.busy()).clicked() {
                    if let Some(p) = rfd::FileDialog::new()
                        .add_filter("Archive", &["tar"])
                        .pick_file()
                    {
                        self.local_path = Some(p);
                        self.local_confirmed = false;
                    }
                }
                match &self.local_path {
                    Some(p) => ui.label(RichText::new(p.display().to_string()).color(TEXT)),
                    None => ui.label(RichText::new("No file").color(MUTED)),
                };
            });
            let file_version = self.local_file_version();
            if self.local_path.is_some() && file_version.is_none() {
                banner(
                    ui,
                    RED,
                    Icon::Text("!"),
                    "Unrecognised file name",
                    &["Rename it to the format above.".into()],
                );
            }
            ui.add_space(4.0);
            let before = self.local_sha.clone();
            field(
                ui,
                "Expected SHA-256 (from the release's .sha256 file) — strongly recommended",
                &mut self.local_sha,
                560.0,
                false,
                "64 hex characters",
            );
            if before != self.local_sha {
                self.local_confirmed = false;
            }
            let sha_ok = self.local_sha.trim().is_empty()
                || github::normalize_sha256(&self.local_sha).is_some();
            if !sha_ok {
                ui.label(RichText::new("Must be 64 hex characters").color(RED));
            } else if self.local_sha.trim().is_empty() {
                ui.label(
                    RichText::new("Without it the archive cannot be verified.")
                        .size(12.5)
                        .color(ORANGE),
                );
            }
            if let Some(v) = file_version {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if self.local_confirmed {
                        if primary_button(ui, "Continue to the check", true).clicked() {
                            self.page = Page::Check;
                        }
                        pill(ui, &format!("{v} CONFIRMED"), GREEN);
                    } else {
                        if primary_button(
                            ui,
                            &format!("Confirm this file ({v}) and continue"),
                            sha_ok,
                        )
                        .clicked()
                        {
                            self.confirm_pending();
                        }
                        ui.label(
                            RichText::new("Goes straight to the check.")
                                .size(12.5)
                                .color(MUTED),
                        );
                    }
                });
            }
        });
    }
}
