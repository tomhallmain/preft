use eframe::egui;
use log::info;

use anyhow::Result;

mod db;
mod models;
mod settings;
mod encryption;
mod encryption_config;
mod reporting;
mod utils;
mod ui;
mod app;

fn main() -> Result<(), eframe::Error> {
    // Initialize logger with default level if RUST_LOG is not set
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
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