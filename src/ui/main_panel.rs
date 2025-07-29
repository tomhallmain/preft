use eframe::egui;
use chrono::Datelike;

use crate::app::PreftApp;
use crate::ui::category_flows::show_category_flows;
use crate::ui::category_editor::show_category_editor;

pub fn show_main_panel(ui: &mut egui::Ui, app: &mut PreftApp) {
    ui.horizontal(|ui| {
        ui.heading("Personal Finance Tracker");
    });

    // Row for backup and encryption controls
    ui.horizontal(|ui| {
        if ui.button("Backup & Restore").clicked() {
            app.show_backup_dialog = true;
        }
        
        // Show encryption status and password management
        if app.encryption_config.enabled {
            if app.encryption_config.is_encryption_ready() {
                ui.label(egui::RichText::new("🔒 Encrypted").color(egui::Color32::GREEN));
                if ui.button("Change Password").clicked() {
                    app.show_change_password_dialog();
                }
                if ui.button("Disable Encryption").clicked() {
                    app.show_disable_encryption_dialog();
                }
            } else {
                ui.label(egui::RichText::new("🔓 Encryption Enabled (No Password)").color(egui::Color32::from_rgb(255, 140, 0))); // Dark orange/amber
                if ui.button("Set Password").clicked() {
                    app.show_set_password_dialog();
                }
            }
        } else {
            ui.label(egui::RichText::new("🔓 Unencrypted").color(egui::Color32::RED));
            if ui.button("Enable Encryption").clicked() {
                app.show_set_password_dialog();
            }
        }
    });


    // Row for main controls
    ui.horizontal(|ui| {
        if ui.button("Show Dashboard").clicked() {
            app.selected_category = None;
        }
        if ui.button("Add Category").clicked() {
            app.show_category_editor = true;
        }
        if ui.button("Generate Report").clicked() {
            app.show_report_dialog = true;
        }
    });

    // Show category editor if needed
    show_category_editor(ui, app);

    // Category selector with hide controls
    ui.horizontal(|ui| {
        egui::ComboBox::from_label("Select Category")
            .selected_text(
                app.selected_category
                    .as_ref()
                    .and_then(|id| app.categories.iter().find(|c| c.id == *id))
                    .map(|c| c.name.clone())
                    .unwrap_or_else(|| "Select a category".to_string())
            )
            .show_ui(ui, |ui| {
                for category in &app.categories {
                    if !app.is_category_hidden(&category.id) {
                        ui.selectable_value(
                            &mut app.selected_category,
                            Some(category.id.clone()),
                            &category.name,
                        );
                    }
                }
            });

        // Hide category button (only shown when a category is selected)
        if let Some(category_id) = &app.selected_category {
            if ui.button("Edit Category").clicked() {
                app.editing_category = Some(category_id.clone());
                app.show_category_editor = true;
            }
            if ui.button("Hide Category").clicked() {
                app.hide_category_confirmation = Some(category_id.clone());
            }
            if ui.button("Delete Category").clicked() {
                app.delete_category_confirmation = Some(category_id.clone());
            }
        }

        // Show confirmation dialog if needed
        if let Some(category_id) = app.hide_category_confirmation.clone() {
            egui::Window::new("Confirm Hide Category")
                .collapsible(false)
                .resizable(false)
                .show(ui.ctx(), |ui| {
                    ui.label("Are you sure you want to hide this category?");
                    ui.label("This will not delete any data - you can show the category again later.");
                    ui.label("All flows in this category will remain in the database.");
                    
                    ui.horizontal(|ui| {
                        if ui.button("Yes, Hide Category").clicked() {
                            app.toggle_category_visibility(category_id);
                            app.hide_category_confirmation = None;
                        }
                        if ui.button("Cancel").clicked() {
                            app.hide_category_confirmation = None;
                        }
                    });
                });
        }

        // Show delete confirmation dialog if needed
        if let Some(category_id) = app.delete_category_confirmation.clone() {
            egui::Window::new("Confirm Delete Category")
                .collapsible(false)
                .resizable(false)
                .show(ui.ctx(), |ui| {
                    ui.label("Are you sure you want to delete this category?");
                    ui.label("This will permanently delete the category and all its flows.");
                    ui.label("This action cannot be undone!");
                    
                    ui.horizontal(|ui| {
                        if ui.button("Yes, Delete Category").clicked() {
                            app.delete_category(category_id);
                            app.delete_category_confirmation = None;
                        }
                        if ui.button("Cancel").clicked() {
                            app.delete_category_confirmation = None;
                        }
                    });
                });
        }

        // Show hidden categories button
        if ui.button("Show Hidden Categories").clicked() {
            app.show_hidden_categories = !app.show_hidden_categories;
        }

        // Year filter control
        ui.horizontal(|ui| {
            ui.label("Year Filter:");
            let current_year = chrono::Local::now().year();
            let mut year_filter = app.user_settings.get_year_filter();
            
            egui::ComboBox::from_id_source("year_filter")
                .selected_text(match year_filter {
                    Some(year) => year.to_string(),
                    None => "All Years".to_string(),
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut year_filter, None, "All Years");
                    // Show last 5 years and next year
                    for year in (current_year - 5)..=(current_year + 1) {
                        ui.selectable_value(&mut year_filter, Some(year), year.to_string());
                    }
                });

            if year_filter != app.user_settings.get_year_filter() {
                app.user_settings.set_year_filter(year_filter);
                if let Err(e) = app.db.save_user_settings(&app.user_settings) {
                    eprintln!("Failed to save user settings: {}", e);
                }
                // Mark all category flows states for update
                for state in app.category_flows_state.values_mut() {
                    state.mark_for_update();
                }
            }
        });
    });

    // Show hidden categories management if enabled
    if app.show_hidden_categories {
        ui.separator();
        ui.heading("Hidden Categories");
        egui::Grid::new("hidden_categories_grid")
            .striped(true)
            .show(ui, |ui| {
                // Collect category IDs first to avoid borrow issues
                let hidden_category_ids: Vec<String> = app.categories
                    .iter()
                    .filter(|c| app.is_category_hidden(&c.id))
                    .map(|c| c.id.clone())
                    .collect();

                for category_id in hidden_category_ids {
                    // Find the category name
                    if let Some(category) = app.categories.iter().find(|c| c.id == category_id) {
                        ui.label(&category.name);
                        if ui.button("Show").clicked() {
                            app.toggle_category_visibility(category_id);
                        }
                        ui.end_row();
                    }
                }
            });
    }

    // Show flows for selected category or dashboard if no category is selected
    if let Some(category) = app.get_selected_category().cloned() {
        show_category_flows(ui, app, &category);
    } else {
        app.dashboard.show(ui, &app.flows, &app.categories);
    }
} 