use anyhow::Result;
use eframe::egui;

pub mod app;
pub mod db;
pub mod encryption;
pub mod encryption_config;
pub mod logging;
pub mod models;
pub mod reporting;
pub mod settings;
pub mod ui;
pub mod utils;

/// Runs the desktop application. Extracted from `main` so the rest of the
/// crate is importable (by integration tests, etc.) without pulling in the
/// eframe event loop.
pub fn run() -> Result<(), eframe::Error> {
    logging::init_logging();
    log::info!("Starting Preft application");

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
