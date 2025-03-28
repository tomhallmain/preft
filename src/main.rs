use eframe::egui;

mod models;
mod app;
mod ui;

use app::PreftApp;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([800.0, 600.0]),
        ..Default::default()
    };
    
    eframe::run_native(
        "Preft",
        options,
        Box::new(|_cc| Box::new(PreftApp::default())),
    )
} 