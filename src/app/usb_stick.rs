//! USB update stick (since 0.13.0-rc.10). Does not touch the car.

use super::*;
use crate::usb;

/// First version that can update itself from a USB stick.
const USB_UPDATE_SINCE: &str = "0.13.0-rc.10";

impl App {
    /// Releases signed by the project (0.13.0-rc.10 and newer).
    fn usb_releases(&self) -> Vec<&Release> {
        self.releases
            .iter()
            .filter(|r| {
                r.package().is_some_and(|p| {
                    compare_versions(&p.version, USB_UPDATE_SINCE) != Ordering::Less
                })
            })
            .collect()
    }

    /// The chosen release, else the newest stable, else the newest.
    fn usb_release(&self) -> Option<&Release> {
        let list = self.usb_releases();
        self.usb_tag
            .as_ref()
            .and_then(|t| list.iter().find(|r| &r.tag_name == t).copied())
            .or_else(|| list.iter().find(|r| !r.prerelease).copied())
            .or_else(|| list.first().copied())
    }

    pub(super) fn usb_package(&self) -> Option<github::Package> {
        self.usb_release()?.package()
    }

    fn refresh_drives(&mut self) {
        self.usb_scanned = true;
        self.usb_drives = usb::removable_drives();
        if self.usb_target.as_ref().is_some_and(|t| !t.root.exists()) {
            self.usb_target = None;
        }
        if self.usb_target.is_none() {
            let supported: Vec<_> = self.usb_drives.iter().filter(|d| d.supported()).collect();
            if supported.len() == 1 {
                self.usb_target = Some(supported[0].clone());
            }
        }
    }

    /// Why the stick cannot be written now.
    fn usb_block(&self) -> Option<&'static str> {
        if self.busy() {
            return Some("Wait for the current task to finish");
        }
        if self.usb_package().is_none() {
            return Some("No release can be installed from a USB stick");
        }
        match &self.usb_target {
            None => Some("Choose a USB stick"),
            Some(t) if !t.supported() => {
                Some("The car cannot read this stick (FAT32, exFAT or NTFS needed)")
            }
            Some(t) if !t.root.exists() => Some("The stick was unplugged: Refresh"),
            Some(_) => None,
        }
    }

    pub(super) fn page_usb_stick(&mut self, ui: &mut egui::Ui) {
        if !self.usb_scanned {
            self.refresh_drives();
        }
        page_header(
            ui,
            "USB update stick",
            "Update the car without a computer. Needs 0.13.0-rc.10 or newer on the car.",
        );

        let too_old = match self.installed() {
            Some(Installed::Version(v))
                if compare_versions(v, USB_UPDATE_SINCE) == Ordering::Less =>
            {
                Some(format!("The connected car runs {v}."))
            }
            Some(Installed::None) => {
                Some("Android Auto is not installed on the connected car.".into())
            }
            _ => None,
        };
        if let Some(what) = too_old {
            banner(
                ui,
                ORANGE,
                "!",
                "This car cannot update from a USB stick yet",
                &[format!(
                    "{what} Install {USB_UPDATE_SINCE} or newer once with steps 1 to 4; later updates can then use the stick."
                )],
            );
        }

        let installed = self.installed_version().map(String::from);
        let chosen = self.usb_release().map(|r| r.tag_name.clone());
        let rows: Vec<(String, String, bool, String)> = self
            .usb_releases()
            .iter()
            .filter_map(|r| {
                Some((
                    r.tag_name.clone(),
                    r.package()?.version,
                    r.prerelease,
                    r.published_date().to_string(),
                ))
            })
            .collect();
        card(ui, |ui| {
            card_title(
                ui,
                "Version",
                Some("Only releases signed by the project (0.13.0-rc.10 and newer) install from a USB stick."),
            );
            if rows.is_empty() {
                if self.task == Some(Action::FetchReleases) {
                    ui.horizontal(|ui| {
                        ui.add(egui::Spinner::new().color(ACCENT));
                        ui.label(RichText::new("Loading versions…").color(MUTED));
                    });
                } else {
                    ui.label(
                        RichText::new("No release can be installed from a USB stick yet.")
                            .color(MUTED),
                    );
                }
            }
            for (tag, version, pre, date) in rows.iter().take(6) {
                ui.horizontal(|ui| {
                    let selected = chosen.as_deref() == Some(tag.as_str());
                    if ui
                        .radio(selected, bold(version).size(15.0).color(TEXT))
                        .clicked()
                    {
                        self.usb_tag = Some(tag.clone());
                    }
                    ui.label(RichText::new(date).size(12.0).color(MUTED));
                    if *pre {
                        pill(ui, "PRE-RELEASE", ORANGE);
                    } else {
                        pill(ui, "STABLE", GREEN);
                    }
                    if installed.as_deref() == Some(version.as_str()) {
                        pill(ui, "ON CAR", GREEN);
                    }
                });
            }
        });

        card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(bold("USB stick").size(17.0).color(TEXT));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if secondary_button(ui, "Choose a drive…", !self.busy()).clicked() {
                        if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                            match usb::drive_for_folder(&folder) {
                                Ok(d) => {
                                    if !self.usb_drives.contains(&d) {
                                        self.usb_drives.push(d.clone());
                                    }
                                    self.usb_target = Some(d);
                                }
                                Err(e) => self.log(Level::Error, e),
                            }
                        }
                    }
                    if secondary_button(ui, "⟳  Refresh", !self.busy()).clicked() {
                        self.refresh_drives();
                    }
                });
            });
            ui.label(
                RichText::new("FAT32, exFAT or NTFS. The archive goes to its root, as is.")
                    .size(12.5)
                    .color(MUTED),
            );
            if self.usb_drives.is_empty() {
                ui.label(
                    RichText::new("No USB stick found. Plug one in, then Refresh.").color(MUTED),
                );
            }
            let drives = self.usb_drives.clone();
            for d in drives {
                ui.horizontal(|ui| {
                    let selected = self.usb_target.as_ref().map(|t| &t.root) == Some(&d.root);
                    let r = ui
                        .add_enabled(d.supported(), egui::RadioButton::new(selected, d.display()));
                    if r.clicked() {
                        self.usb_target = Some(d.clone());
                    }
                    if !d.supported() {
                        pill(ui, "NOT FAT32 / EXFAT / NTFS", MUTED);
                    }
                });
            }

            let Some(target) = self.usb_target.clone() else {
                return;
            };
            let name = self.usb_package().map(|p| p.asset.name);
            let others: Vec<String> = usb::archives_on(&target.root)
                .into_iter()
                .filter(|n| Some(n) != name.as_ref())
                .collect();
            if !others.is_empty() {
                ui.add_space(4.0);
                toggle_row(
                    ui,
                    &mut self.usb_remove_others,
                    "Remove the other EntryNav2 archives from the stick",
                    &format!(
                        "The car takes the newest one it finds. On the stick: {}",
                        others.join(", ")
                    ),
                );
            }
            if let Some(report) = usb::read_report(&target.root) {
                ui.add_space(4.0);
                egui::CollapsingHeader::new(
                    bold("Report of the car's last update from this stick")
                        .size(15.0)
                        .color(TEXT),
                )
                .id_salt("usb_report")
                .default_open(true)
                .show(ui, |ui| {
                    ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                        ui.label(RichText::new(report).monospace().color(TEXT));
                    });
                });
            }
        });

        let block = self.usb_block();
        let writing = self.task == Some(Action::WriteUsb);
        ui.horizontal(|ui| {
            let version = self.usb_package().map(|p| p.version).unwrap_or_default();
            if primary_button(
                ui,
                &format!("Write {version} to the USB stick"),
                block.is_none(),
            )
            .clicked()
            {
                let root = self
                    .usb_target
                    .as_ref()
                    .map(|t| t.root.display().to_string())
                    .unwrap_or_default();
                let name = github::archive_name(&version);
                let others: Vec<String> = self
                    .usb_target
                    .as_ref()
                    .map(|t| usb::archives_on(&t.root))
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|n| *n != name)
                    .collect();
                if self.usb_remove_others && !others.is_empty() {
                    self.confirm = Some(Confirm {
                        action: Action::WriteUsb,
                        title: "Write the update to the USB stick?".into(),
                        lines: vec![
                            format!("Android Auto {version} is copied to {root}."),
                            format!(
                                "These archives are removed from the stick: {}.",
                                others.join(", ")
                            ),
                        ],
                        warnings: Vec::new(),
                        button: "Write".into(),
                        danger: false,
                    });
                } else {
                    let ctx = ui.ctx().clone();
                    self.start(Action::WriteUsb, &ctx);
                }
            }
            if writing {
                ui.add(egui::Spinner::new().size(18.0).color(ACCENT));
                let cancelling = self
                    .cancel
                    .as_ref()
                    .is_some_and(|c| c.load(AtomicOrdering::Relaxed));
                if !cancelling && secondary_button(ui, "Cancel", true).clicked() {
                    if let Some(c) = &self.cancel {
                        c.store(true, AtomicOrdering::Relaxed);
                    }
                }
            } else {
                hint(ui, block);
            }
        });
    }
}
