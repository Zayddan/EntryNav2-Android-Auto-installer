// No console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod car;
mod config;
#[cfg(test)]
mod e2e;
mod github;
mod icon;
mod scripts;
mod self_update;
mod signature;
mod theme;
mod usb;
mod worker;

use eframe::egui;

const TITLE: &str = "Android Auto Installer for BMW / MINI EntryNav2 (unofficial)";

fn main() -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(TITLE)
            .with_inner_size([1120.0, 800.0])
            .with_min_inner_size([860.0, 600.0])
            .with_icon(egui::IconData {
                rgba: icon::rgba(64),
                width: 64,
                height: 64,
            }),
        centered: true,
        ..Default::default()
    };
    eframe::run_native(
        TITLE,
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc)))),
    )
}
