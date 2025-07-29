use eframe::egui;

use crate::app::{PreftApp, PasswordDialogMode};

pub fn show_password_dialog(ctx: &egui::Context, app: &mut PreftApp) {
    let mut show_window = app.show_password_dialog;
    
    egui::Window::new("Password Management")
        .open(&mut show_window)
        .resizable(false)
        .collapsible(false)
        .default_size([400.0, 300.0])
        .show(ctx, |ui| {
            match app.password_dialog_mode {
                PasswordDialogMode::SetPassword => {
                    ui.heading("Set Database Password");
                    ui.label("Your database will be encrypted with this password.");
                    ui.label("Make sure to remember it - you'll need it to access your data.");
                    ui.separator();
                    
                    ui.label("Password:");
                    ui.add(egui::TextEdit::singleline(&mut app.password_input)
                        .password(true)
                        .desired_width(300.0));
                    
                    ui.label("Confirm Password:");
                    ui.add(egui::TextEdit::singleline(&mut app.password_confirm)
                        .password(true)
                        .desired_width(300.0));
                    
                    // Show status if any
                    if let Some(status) = &app.encryption_status {
                        ui.label(egui::RichText::new(status)
                            .color(egui::Color32::from_rgb(255, 140, 0)));
                    }
                    
                    ui.separator();
                    
                    ui.horizontal(|ui| {
                        if ui.button("Set Password").clicked() {
                            if app.password_input.is_empty() {
                                app.encryption_status = Some("Password cannot be empty".to_string());
                            } else if app.password_input != app.password_confirm {
                                app.encryption_status = Some("Passwords do not match".to_string());
                            } else if app.password_input.len() < 8 {
                                app.encryption_status = Some("Password must be at least 8 characters".to_string());
                            } else {
                                let password = app.password_input.clone();
                                if let Err(e) = app.set_password(&password) {
                                    app.encryption_status = Some(format!("Failed to set password: {}", e));
                                } else {
                                    app.show_password_dialog = false;
                                }
                            }
                        }
                        
                        if ui.button("Cancel").clicked() {
                            app.show_password_dialog = false;
                            app.clear_encryption_status();
                        }
                    });
                }
                
                PasswordDialogMode::EnterPassword => {
                    ui.heading("Enter Database Password");
                    ui.label("Your database is encrypted. Please enter your password to continue.");
                    ui.separator();
                    
                    ui.label("Password:");
                    ui.add(egui::TextEdit::singleline(&mut app.password_input)
                        .password(true)
                        .desired_width(300.0));
                    
                    // Show status if any
                    if let Some(status) = &app.encryption_status {
                        ui.label(egui::RichText::new(status)
                            .color(egui::Color32::from_rgb(255, 140, 0)));
                    }
                    
                    ui.separator();
                    
                    ui.horizontal(|ui| {
                        if ui.button("Unlock").clicked() {
                            if app.password_input.is_empty() {
                                app.encryption_status = Some("Password cannot be empty".to_string());
                            } else {
                                let password = app.password_input.clone();
                                match app.verify_password(&password) {
                                    Ok(true) => {
                                        app.show_password_dialog = false;
                                        app.clear_encryption_status();
                                    }
                                    Ok(false) => {
                                        // Status already set in verify_password
                                    }
                                    Err(e) => {
                                        app.encryption_status = Some(format!("Error: {}", e));
                                    }
                                }
                            }
                        }
                        
                        if ui.button("Cancel").clicked() {
                            app.show_password_dialog = false;
                            app.clear_encryption_status();
                        }
                    });
                }
                
                PasswordDialogMode::ChangePassword => {
                    ui.heading("Change Database Password");
                    ui.label("Enter your new password below.");
                    ui.separator();
                    
                    ui.label("New Password:");
                    ui.add(egui::TextEdit::singleline(&mut app.password_input)
                        .password(true)
                        .desired_width(300.0));
                    
                    ui.label("Confirm New Password:");
                    ui.add(egui::TextEdit::singleline(&mut app.password_confirm)
                        .password(true)
                        .desired_width(300.0));
                    
                    // Show status if any
                    if let Some(status) = &app.encryption_status {
                        ui.label(egui::RichText::new(status)
                            .color(egui::Color32::from_rgb(255, 140, 0)));
                    }
                    
                    ui.separator();
                    
                    ui.horizontal(|ui| {
                        if ui.button("Change Password").clicked() {
                            if app.password_input.is_empty() {
                                app.encryption_status = Some("Password cannot be empty".to_string());
                            } else if app.password_input != app.password_confirm {
                                app.encryption_status = Some("Passwords do not match".to_string());
                            } else if app.password_input.len() < 8 {
                                app.encryption_status = Some("Password must be at least 8 characters".to_string());
                            } else {
                                let password = app.password_input.clone();
                                if let Err(e) = app.change_password(&password) {
                                    app.encryption_status = Some(format!("Failed to change password: {}", e));
                                } else {
                                    app.show_password_dialog = false;
                                }
                            }
                        }
                        
                        if ui.button("Cancel").clicked() {
                            app.show_password_dialog = false;
                            app.clear_encryption_status();
                        }
                    });
                }
                
                PasswordDialogMode::DisableEncryption => {
                    ui.heading("Disable Database Encryption");
                    ui.label("Warning: This will remove encryption from your database.");
                    ui.label("Your data will no longer be encrypted and will be stored in plain text.");
                    ui.label("Make sure you understand the security implications before proceeding.");
                    ui.separator();
                    
                    ui.label("Current Password (for verification):");
                    ui.add(egui::TextEdit::singleline(&mut app.password_input)
                        .password(true)
                        .desired_width(300.0));
                    
                    ui.label("Type 'DISABLE' to confirm:");
                    ui.add(egui::TextEdit::singleline(&mut app.password_confirm)
                        .desired_width(300.0));
                    
                    // Show status if any
                    if let Some(status) = &app.encryption_status {
                        ui.label(egui::RichText::new(status)
                            .color(egui::Color32::from_rgb(255, 140, 0)));
                    }
                    
                    ui.separator();
                    
                    ui.horizontal(|ui| {
                        if ui.button("Disable Encryption").clicked() {
                            if app.password_input.is_empty() {
                                app.encryption_status = Some("Password cannot be empty".to_string());
                            } else if app.password_confirm != "DISABLE" {
                                app.encryption_status = Some("Please type 'DISABLE' to confirm".to_string());
                            } else {
                                // Verify the current password first
                                let password = app.password_input.clone();
                                match app.verify_password(&password) {
                                    Ok(true) => {
                                        // Password verified, now disable encryption
                                        if let Err(e) = app.disable_encryption() {
                                            app.encryption_status = Some(format!("Failed to disable encryption: {}", e));
                                        } else {
                                            app.show_password_dialog = false;
                                        }
                                    }
                                    Ok(false) => {
                                        app.encryption_status = Some("Incorrect password".to_string());
                                    }
                                    Err(e) => {
                                        app.encryption_status = Some(format!("Error: {}", e));
                                    }
                                }
                            }
                        }
                        
                        if ui.button("Cancel").clicked() {
                            app.show_password_dialog = false;
                            app.clear_encryption_status();
                        }
                    });
                }
            }
        });
    
    app.show_password_dialog = show_window;
}