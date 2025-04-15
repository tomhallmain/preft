use eframe::egui;

use crate::models::{Category, CategoryField, FieldType};
use crate::app::PreftApp;

pub fn show_category_editor(ui: &mut egui::Ui, app: &mut PreftApp) {
    if app.show_category_editor {
        // Initialize new category if needed
        if app.new_category.is_none() {
            app.new_category = Some(Category::new("New Category".to_string()));
        }

        // Take the category out of the Option to avoid borrowing issues
        if let Some(mut category) = app.new_category.take() {
            let mut should_save = false;
            let mut should_cancel = false;

            egui::Window::new("Add Category")
                .collapsible(false)
                .resizable(false)
                .show(ui.ctx(), |ui| {
                    ui.vertical(|ui| {
                        ui.heading("New Category");
                        
                        // Category name
                        ui.horizontal(|ui| {
                            ui.label("Name:");
                            if ui.text_edit_singleline(&mut category.name).changed() {
                                // Name is updated directly in the category
                            }
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
                            egui::Grid::new("fields_grid")
                                .striped(true)
                                .show(ui, |ui| {
                                    for field in &category.fields {
                                        ui.label(&field.name);
                                        ui.label(format!("{:?}", field.field_type));
                                        if let Some(default) = &field.default_value {
                                            ui.label(default);
                                        } else {
                                            ui.label("No default");
                                        }
                                        if ui.button("Edit").clicked() {
                                            app.editing_field = Some(field.clone());
                                            app.show_field_editor = true;
                                        }
                                        ui.end_row();
                                    }
                                });
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
                app.add_category(category);
                app.show_category_editor = false;
            } else if should_cancel {
                app.show_category_editor = false;
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
                        egui::ComboBox::from_label("")
                            .selected_text(format!("{:?}", field_type))
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut field_type, FieldType::Text, "Text");
                                ui.selectable_value(&mut field_type, FieldType::Number, "Number");
                                ui.selectable_value(&mut field_type, FieldType::Boolean, "Boolean");
                                ui.selectable_value(&mut field_type, FieldType::Date, "Date");
                            });
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
                category.fields.push(field);
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