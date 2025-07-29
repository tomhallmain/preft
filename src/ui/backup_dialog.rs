use eframe::egui;

use crate::app::PreftApp;

pub fn show_backup_dialog(ctx: &egui::Context, app: &mut PreftApp) {
    let mut show_window = app.show_backup_dialog;
    
    egui::Window::new("Backup & Restore")
        .open(&mut show_window)
        .resizable(true)
        .default_size([600.0, 400.0])
        .show(ctx, |ui| {
            ui.heading("Database Backup & Restore");
            ui.separator();
            
            // Status section
            ui.heading("Status");
            if let Some(status) = &app.backup_status {
                ui.label(status);
            } else {
                ui.label("No recent backup operations");
            }
            
            // Last backup info
            if let Some(last_backup) = app.user_settings.get_last_successful_backup() {
                ui.separator();
                ui.heading("Last Successful Backup");
                ui.label(format!("Date: {}", last_backup.timestamp.format("%Y-%m-%d %H:%M:%S UTC")));
                ui.label(format!("File: {}", last_backup.file_path));
                if let Some(size) = last_backup.file_size {
                    ui.label(format!("Size: {:.2} KB", size as f64 / 1024.0));
                }
            }
            
            ui.separator();
            
            // Action buttons
            ui.heading("Actions");
            ui.horizontal(|ui| {
                if ui.button("Create Backup").clicked() && !app.backup_in_progress {
                    app.create_backup();
                }
                
                if ui.button("Restore from Backup").clicked() && !app.backup_in_progress {
                    app.restore_backup();
                }
                
                if ui.button("Clear Status").clicked() {
                    app.clear_backup_status();
                }
            });
            
            // Show progress indicator
            if app.backup_in_progress {
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label("Operation in progress...");
                    ui.spinner();
                });
            }
            
            ui.separator();
            
            // Backup history
            ui.heading("Backup History");
            if app.user_settings.backup_history.is_empty() {
                ui.label("No backup history available");
            } else {
                egui::ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                    egui::Grid::new("backup_history_grid")
                        .striped(true)
                        .spacing([10.0, 4.0])
                        .show(ui, |ui| {
                            // Header
                            ui.strong("Date");
                            ui.strong("File");
                            ui.strong("Size");
                            ui.strong("Status");
                            ui.end_row();
                            
                            // History entries (show most recent first)
                            for entry in app.user_settings.backup_history.iter().rev() {
                                ui.label(entry.timestamp.format("%Y-%m-%d %H:%M").to_string());
                                
                                // Show just the filename, not the full path
                                let filename = std::path::Path::new(&entry.file_path)
                                    .file_name()
                                    .unwrap_or_default()
                                    .to_string_lossy();
                                ui.label(filename);
                                
                                if let Some(size) = entry.file_size {
                                    ui.label(format!("{:.1} KB", size as f64 / 1024.0));
                                } else {
                                    ui.label("N/A");
                                }
                                
                                if entry.success {
                                    ui.label(egui::RichText::new("✓ Success").color(egui::Color32::GREEN));
                                } else {
                                    ui.label(egui::RichText::new("✗ Failed").color(egui::Color32::RED));
                                }
                                ui.end_row();
                            }
                        });
                });
            }
            
            ui.separator();
            
            // Warning about restore
            ui.label(egui::RichText::new("⚠ Warning: Restoring a backup will replace all current data!")
                .color(egui::Color32::from_rgb(255, 140, 0)) // Dark orange/amber
                .strong());
            ui.label("Make sure to create a backup of your current data before restoring.");
            
            ui.separator();
            
            // Close button
            ui.horizontal(|ui| {
                ui.add_space(ui.available_width() - 60.0); // Push button to the right
                if ui.button("Close").clicked() {
                    app.show_backup_dialog = false;
                }
            });
        });
    
    app.show_backup_dialog = show_window;
}