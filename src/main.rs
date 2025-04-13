mod models;
mod db;
mod app;
mod ui;
mod settings;

use eframe::egui;
use anyhow::Result;

fn main() -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([800.0, 600.0]),
        ..Default::default()
    };
    
    eframe::run_native(
        "Preft",
        options,
        Box::new(|_cc| Box::new(app::PreftApp::new())),
    )
} 