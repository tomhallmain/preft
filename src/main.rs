use eframe::egui;
use log::info;

use anyhow::Result;

mod models;
mod db;
mod app;
mod ui;
mod settings;
mod reporting;
mod dashboard;
mod utils;

fn main() -> Result<(), eframe::Error> {
    // Initialize logger
    env_logger::init();
    info!("Starting Preft application");

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([800.0, 600.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Preft",
        options,
        Box::new(|cc| Box::new(app::PreftApp::new(cc))),
    )
} 