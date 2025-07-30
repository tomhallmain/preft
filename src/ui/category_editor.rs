use eframe::egui;
use log::{info, warn, error};

use crate::models::{Category, CategoryField, FieldType};
use crate::app::PreftApp;

pub fn show_category_editor(ui: &mut egui::Ui, app: &mut PreftApp) {
    if app.show_category_editor {
        // Initialize new category or get existing category for editing
        if app.new_category.is_none() {
            if let Some(category_id) = &app.editing_category {
                // Find the category to edit
                if let Some(category) = app.categories.iter().find(|c| c.id == *category_id) {
                    app.new_category = Some(category.clone());
                }
            } else {
                app.new_category = Some(Category::new("New Category".to_string()));
            }
        }

        // Take the category out of the Option to avoid borrowing issues
        if let Some(mut category) = app.new_category.take() {
            let mut should_save = false;
            let mut should_cancel = false;

            egui::Window::new(if app.editing_category.is_some() { "Edit Category" } else { "Add Category" })
                .collapsible(false)
                .resizable(false)
                .show(ui.ctx(), |ui| {
                    ui.vertical(|ui| {
                        ui.heading(if app.editing_category.is_some() { "Edit Category" } else { "New Category" });
                        
                        // Category name
                        ui.horizontal(|ui| {
                            ui.label("Name:");
                            if ui.text_edit_singleline(&mut category.name).changed() {
                                // Name is updated directly in the category
                            }
                        });

                        // Flow type
                        ui.horizontal(|ui| {
                            ui.label("Type:");
                            let mut flow_type = category.flow_type.clone();
                            egui::ComboBox::from_label("")
                                .selected_text(format!("{:?}", flow_type))
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut flow_type, crate::models::FlowType::Income, "Income");
                                    ui.selectable_value(&mut flow_type, crate::models::FlowType::Expense, "Expense");
                                });
                            category.flow_type = flow_type;
                        });

                        // Tax deduction settings
                        ui.horizontal(|ui| {
                            ui.label("Allow Tax Deduction:");
                            if ui.checkbox(&mut category.tax_deduction.deduction_allowed, "").changed() {
                                // Tax deduction setting is updated directly
                            }
                        });

                        // Default tax deduction value
                        ui.horizontal(|ui| {
                            ui.label("Default Tax Deductible:");
                            if ui.checkbox(&mut category.tax_deduction.default_value, "").changed() {
                                // Default deductible value is updated directly
                            }
                        });

                        ui.separator();

                        // Show existing fields
                        if !category.fields.is_empty() {
                            ui.heading("Fields");
                            let mut indices_to_remove = Vec::new();
                            egui::Grid::new("fields_grid")
                                .striped(true)
                                .show(ui, |ui| {
                                    for (index, field) in category.fields.iter().enumerate() {
                                        ui.label(&field.name);
                                        ui.label(match field.field_type {
                                            FieldType::Text => "Text",
                                            FieldType::Integer => "Whole Number",
                                            FieldType::Float => "Decimal Number",
                                            FieldType::Currency => "Currency",
                                            FieldType::Boolean => "Boolean",
                                            FieldType::Date => "Date",
                                            #[allow(deprecated)]
                                            FieldType::Number => "Decimal Number",
                                            FieldType::Select(_) => "Select",
                                        });
                                        if let Some(default) = &field.default_value {
                                            ui.label(default);
                                        } else {
                                            ui.label("No default");
                                        }
                                        if ui.button("Edit").clicked() {
                                            app.editing_field = Some(field.clone());
                                            app.show_field_editor = true;
                                        }
                                        if ui.button("Remove").clicked() && !indices_to_remove.contains(&index) {
                                            indices_to_remove.push(index);
                                        }
                                        ui.end_row();
                                    }
                                });
                            
                            // Remove fields in reverse order to avoid index shifting
                            if !indices_to_remove.is_empty() {
                                indices_to_remove.sort_unstable();
                                indices_to_remove.dedup();
                                for &index in indices_to_remove.iter().rev() {
                                    if index < category.fields.len() {
                                        category.fields.remove(index);
                                    }
                                }
                            }
                        }

                        // Add field button
                        if ui.button("Add Field").clicked() {
                            app.editing_field = Some(CategoryField {
                                name: String::new(),
                                field_type: FieldType::Text,
                                required: false,
                                default_value: None,
                            });
                            app.show_field_editor = true;
                        }

                        ui.separator();

                        // Show field editor if needed
                        if app.show_field_editor {
                            show_field_editor(ui, app, &mut category);
                        }

                        ui.separator();

                        // Save/Cancel buttons
                        ui.horizontal(|ui| {
                            if ui.button("Save").clicked() {
                                should_save = true;
                            }
                            if ui.button("Cancel").clicked() {
                                should_cancel = true;
                            }
                        });
                    });
                });

            // Handle save/cancel after the window is closed
            if should_save {
                if app.editing_category.is_some() {
                    // Update existing category
                    if let Some(pos) = app.categories.iter().position(|c| c.id == category.id) {
                        app.categories[pos] = category.clone();
                        if let Err(e) = app.db.save_category(&category) {
                            log::error!("Failed to save category: {}", e);
                        }
                    }
                    app.editing_category = None;
                } else {
                    // Add new category
                    app.add_category(category);
                }
                app.show_category_editor = false;
            } else if should_cancel {
                app.show_category_editor = false;
                app.editing_category = None;
            } else {
                // Put the category back if neither save nor cancel was clicked
                app.new_category = Some(category);
            }
        }
    }
}

fn show_field_editor(ui: &mut egui::Ui, app: &mut PreftApp, category: &mut Category) {
    if let Some(mut field) = app.editing_field.take() {
        let mut should_save = false;
        let mut should_cancel = false;

        egui::Window::new("Edit Field")
            .collapsible(false)
            .resizable(false)
            .show(ui.ctx(), |ui| {
                ui.vertical(|ui| {
                    ui.heading(if field.name.is_empty() { "New Field" } else { "Edit Field" });
                    
                    // Field name
                    ui.horizontal(|ui| {
                        ui.label("Name:");
                        if ui.text_edit_singleline(&mut field.name).changed() {
                            // Name is updated directly in the field
                        }
                    });

                    // Field type
                    ui.horizontal(|ui| {
                        ui.label("Type:");
                        let mut field_type = field.field_type.clone();
                        let old_type = field_type.clone();
                        egui::ComboBox::from_label("")
                            .selected_text(match field_type {
                                FieldType::Text => "Text",
                                FieldType::Integer => "Whole Number",
                                FieldType::Float => "Decimal Number",
                                FieldType::Currency => "Currency",
                                FieldType::Boolean => "Boolean",
                                FieldType::Date => "Date",
                                #[allow(deprecated)]
                                FieldType::Number => "Decimal Number",
                                FieldType::Select(_) => "Select",
                            })
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut field_type, FieldType::Text, "Text");
                                ui.selectable_value(&mut field_type, FieldType::Integer, "Whole Number");
                                ui.selectable_value(&mut field_type, FieldType::Float, "Decimal Number");
                                ui.selectable_value(&mut field_type, FieldType::Currency, "Currency");
                                ui.selectable_value(&mut field_type, FieldType::Boolean, "Boolean");
                                ui.selectable_value(&mut field_type, FieldType::Date, "Date");
                            });
                        
                        // Handle default value conversion when type changes
                        if field_type != old_type {
                            field.default_value = if let Some(value) = &field.default_value {
                                match field_type {
                                    FieldType::Integer => {
                                        if let Ok(_) = value.parse::<i64>() {
                                            Some(value.clone())
                                        } else if let Ok(float_val) = value.parse::<f64>() {
                                            Some((float_val as i64).to_string())
                                        } else {
                                            None
                                        }
                                    },
                                    FieldType::Float => {
                                        if let Ok(_) = value.parse::<f64>() {
                                            Some(value.clone())
                                        } else if let Ok(int_val) = value.parse::<i64>() {
                                            Some((int_val as f64).to_string())
                                        } else {
                                            None
                                        }
                                    },
                                    FieldType::Currency => {
                                        let clean_value = value.replace(['$', ','], "");
                                        if let Ok(_) = clean_value.parse::<f64>() {
                                            Some(clean_value)
                                        } else {
                                            None
                                        }
                                    },
                                    FieldType::Boolean => {
                                        match value.to_lowercase().as_str() {
                                            "true" | "1" | "yes" | "y" => Some("true".to_string()),
                                            "false" | "0" | "no" | "n" => Some("false".to_string()),
                                            _ => None
                                        }
                                    },
                                    FieldType::Date => {
                                        if chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d").is_ok() {
                                            Some(value.clone())
                                        } else if let Ok(date) = chrono::NaiveDate::parse_from_str(value, "%m/%d/%Y") {
                                            Some(date.format("%Y-%m-%d").to_string())
                                        } else {
                                            None
                                        }
                                    },
                                    _ => None, // Text and Select fields don't need conversion
                                }
                            } else {
                                None
                            };
                        }
                        field.field_type = field_type;
                    });

                    // Default value
                    ui.horizontal(|ui| {
                        ui.label("Default Value:");
                        let mut default_value = field.default_value.clone().unwrap_or_default();
                        if ui.text_edit_singleline(&mut default_value).changed() {
                            field.default_value = Some(default_value);
                        }
                    });

                    ui.separator();

                    // Save/Cancel buttons
                    ui.horizontal(|ui| {
                        if ui.button("Save").clicked() {
                            should_save = true;
                        }
                        if ui.button("Cancel").clicked() {
                            should_cancel = true;
                        }
                    });
                });
            });

        // Handle save/cancel after the window is closed
        if should_save {
            // If this is a new field, add it to the category
            if !field.name.is_empty() {
                // Check if we're editing an existing field
                if let Some(existing_field) = category.fields.iter_mut()
                    .find(|f| f.name == field.name) {
                    // Update the existing field
                    *existing_field = field;
                } else {
                    // Add as new field
                    category.fields.push(field);
                }
            }
            app.show_field_editor = false;
        } else if should_cancel {
            app.show_field_editor = false;
        } else {
            // Put the field back if neither save nor cancel was clicked
            app.editing_field = Some(field);
        }
    }
} 