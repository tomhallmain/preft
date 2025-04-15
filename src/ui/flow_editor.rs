use eframe::egui;
use chrono::{NaiveDate, Datelike};

use crate::models::{Flow, Category};
use crate::app::PreftApp;

pub struct FlowEditorState {
    pub editor: Option<FlowEditor>,
}

impl FlowEditorState {
    pub fn new() -> Self {
        Self {
            editor: None,
        }
    }

    pub fn has_editor(&self) -> bool {
        self.editor.is_some()
    }

    pub fn show(&mut self, ui: &mut egui::Ui, app: &mut PreftApp) {
        if let Some(editor) = &mut self.editor {
            // Get the category before showing the editor to avoid multiple mutable borrows
            let category = app.get_selected_category().cloned();
            if let Some(category) = category {
                editor.show(ui, app, &category);
            }
        }
    }

    pub fn take_editor(&mut self) -> Option<FlowEditor> {
        self.editor.take()
    }

    pub fn set_editor(&mut self, flow: Flow, is_new_flow: bool) {
        self.editor = Some(FlowEditor::new(flow, is_new_flow));
    }

    pub fn put_editor_back(&mut self, editor: FlowEditor) {
        self.editor = Some(editor);
    }

    pub fn clear_editor(&mut self) {
        self.editor = None;
    }
}

pub struct FlowEditor {
    flow_data: Flow,
    is_new_flow: bool,
    has_set_focus: bool,
    date_input: String,
    date_error: Option<String>,
    amount_input: String,
    description_input: String,
}

impl FlowEditor {
    pub fn new(flow: Flow, is_new_flow: bool) -> Self {
        let flow_clone = flow.clone();
        let date_input = flow_clone.date.to_string();
        Self {
            flow_data: flow_clone,
            is_new_flow,
            has_set_focus: false,
            date_input,
            date_error: None,
            amount_input: flow.amount.to_string(),
            description_input: flow.description.clone(),
        }
    }

    pub fn get_flow_data(&self) -> &Flow {
        &self.flow_data
    }

    pub fn take_flow_data(self) -> Flow {
        self.flow_data
    }

    pub fn show(&mut self, ui: &mut egui::Ui, app: &mut PreftApp, category: &Category) {
        let window_id = egui::Id::new("flow_editor_window");
        egui::Window::new("Edit Flow")
            .id(window_id)
            .collapsible(false)
            .resizable(true)
            .show(ui.ctx(), |ui| {                
                ui.vertical(|ui| {
                    // Basic flow information
                    ui.horizontal(|ui| {
                        ui.label("Date:");
                        let _response = ui.add(
                            egui::TextEdit::singleline(&mut self.date_input)
                                .hint_text("YYYY-MM-DD")
                                .desired_width(100.0)
                        );
                        
                        // Show visual feedback about the date format
                        if !self.date_input.is_empty() && self.date_input.len() != 10 {
                            let warning = ui.label(egui::RichText::new("⚠")
                                .color(egui::Color32::from_rgb(200, 100, 0))
                                .size(16.0));
                            warning.on_hover_text("Date must be in YYYY-MM-DD format");
                        }
                    });

                    // Show date error message if it exists
                    if let Some(error) = &self.date_error {
                        ui.label(egui::RichText::new(error).color(egui::Color32::RED));
                    }

                    ui.horizontal(|ui| {
                        ui.label("Amount:");
                        let amount_response = ui.text_edit_singleline(&mut self.amount_input);
                        if amount_response.changed() {
                            if let Ok(amount) = self.amount_input.parse::<f64>() {
                                self.flow_data.amount = amount;
                            }
                        }
                        if !self.has_set_focus {
                            amount_response.request_focus();
                            self.has_set_focus = true;
                        }
                    });

                    ui.horizontal(|ui| {
                        ui.label("Description:");
                        if ui.text_edit_singleline(&mut self.description_input).changed() {
                            self.flow_data.description = self.description_input.clone();
                        }
                    });

                    // Show tax_deductible checkbox for relevant categories
                    if category.tax_deduction.deduction_allowed {
                        ui.horizontal(|ui| {
                            ui.label("Tax Deductible:");
                            // Initialize with category default if not set
                            if self.flow_data.tax_deductible.is_none() {
                                self.flow_data.tax_deductible = Some(category.tax_deduction.default_value);
                            }
                            let mut is_deductible = self.flow_data.tax_deductible.unwrap();
                            if ui.checkbox(&mut is_deductible, "").changed() {
                                self.flow_data.tax_deductible = Some(is_deductible);
                            }
                        });
                    }

                    ui.separator();

                    // Category-specific fields
                    for field in &category.fields {
                        ui.horizontal(|ui| {
                            ui.label(format!("{}:", field.name));
                            match field.field_type {
                                crate::models::FieldType::Text => {
                                    let value = app.custom_field_values
                                        .entry(field.name.clone())
                                        .or_insert_with(String::new);
                                    if ui.text_edit_singleline(value).changed() {
                                        self.flow_data.custom_fields.insert(field.name.clone(), value.clone());
                                    }
                                },
                                crate::models::FieldType::Number => {
                                    let value = app.custom_field_values
                                        .entry(field.name.clone())
                                        .or_insert_with(String::new);
                                    if ui.text_edit_singleline(value).changed() {
                                        self.flow_data.custom_fields.insert(field.name.clone(), value.clone());
                                    }
                                },
                                crate::models::FieldType::Boolean => {
                                    let mut value = app.custom_field_values
                                        .entry(field.name.clone())
                                        .or_insert_with(|| field.default_value.clone().unwrap_or_else(|| "false".to_string()))
                                        .parse()
                                        .unwrap_or(false);
                                    if ui.checkbox(&mut value, "").changed() {
                                        let value_str = value.to_string();
                                        self.flow_data.custom_fields.insert(field.name.clone(), value_str.clone());
                                        app.custom_field_values.insert(field.name.clone(), value_str);
                                    }
                                },
                                crate::models::FieldType::Select(ref options) => {
                                    let mut selected = app.custom_field_values
                                        .entry(field.name.clone())
                                        .or_insert_with(|| field.default_value.clone().unwrap_or_else(|| options[0].clone()))
                                        .clone();
                                    egui::ComboBox::from_label("")
                                        .selected_text(&selected)
                                        .show_ui(ui, |ui| {
                                            for option in options {
                                                ui.selectable_value(&mut selected, option.clone(), option);
                                            }
                                        });
                                    if selected != app.custom_field_values[&field.name] {
                                        self.flow_data.custom_fields.insert(field.name.clone(), selected.clone());
                                        app.custom_field_values.insert(field.name.clone(), selected);
                                    }
                                },
                                crate::models::FieldType::Date => {
                                    let value = app.custom_field_values
                                        .entry(field.name.clone())
                                        .or_insert_with(String::new);
                                    if ui.text_edit_singleline(value).changed() {
                                        self.flow_data.custom_fields.insert(field.name.clone(), value.clone());
                                    }
                                },
                            }
                        });
                    }

                    ui.separator();

                    // Save/Cancel buttons
                    ui.horizontal(|ui| {
                        if ui.button("Save").clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            // Parse date only when saving
                            if let Ok(date) = NaiveDate::parse_from_str(&self.date_input, "%Y-%m-%d") {
                                self.flow_data.date = date;
                                self.date_error = None;
                                app.save_flow(self.flow_data.clone());
                            } else {
                                self.date_error = Some("Invalid date format or date. Please use YYYY-MM-DD".to_string());
                            }
                        }
                        if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                            app.cancel_flow_edit();
                        }
                    });
                });
            });
    }
} 