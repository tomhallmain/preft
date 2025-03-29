use eframe::egui;
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
}

impl FlowEditor {
    pub fn new(flow: Flow, is_new_flow: bool) -> Self {
        Self {
            flow_data: flow,
            is_new_flow,
            has_set_focus: false,
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
                        let mut date_str = self.flow_data.date.to_string();
                        if ui.text_edit_singleline(&mut date_str).changed() {
                            if let Ok(date) = chrono::NaiveDate::parse_from_str(&date_str, "%Y-%m-%d") {
                                self.flow_data.date = date;
                            }
                        }
                    });

                    ui.horizontal(|ui| {
                        ui.label("Amount:");
                        let mut amount_str = self.flow_data.amount.to_string();
                        let amount_response = ui.text_edit_singleline(&mut amount_str);
                        if amount_response.changed() {
                            if let Ok(amount) = amount_str.parse::<f64>() {
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
                        let mut description = self.flow_data.description.clone();
                        if ui.text_edit_singleline(&mut description).changed() {
                            self.flow_data.description = description;
                        }
                    });

                    // Show tax_deductible checkbox for relevant categories
                    if category.tax_deduction.deduction_allowed {
                        ui.horizontal(|ui| {
                            ui.label("Tax Deductible:");
                            let mut is_deductible = self.flow_data.tax_deductible.unwrap_or(category.tax_deduction.default_value);
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
                            app.save_flow(self.flow_data.clone());
                        }
                        if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                            app.cancel_flow_edit();
                        }
                    });
                });
            });
    }
}

pub fn show_main_panel(ui: &mut egui::Ui, app: &mut PreftApp) {
    ui.horizontal(|ui| {
        ui.heading("Personal Finance Tracker");
        if ui.button("Add Category").clicked() {
            app.show_category_editor = true;
        }
    });

    // Category selector
    egui::ComboBox::from_label("Select Category")
        .selected_text(app.selected_category.as_deref().unwrap_or("Select a category"))
        .show_ui(ui, |ui| {
            for category in &app.categories {
                ui.selectable_value(
                    &mut app.selected_category,
                    Some(category.id.clone()),
                    &category.name,
                );
            }
        });

    // Show flows for selected category
    if let Some(category) = app.get_selected_category().cloned() {
        show_category_flows(ui, app, &category);
    }
}

fn show_category_flows(ui: &mut egui::Ui, app: &mut PreftApp, category: &Category) {
    ui.heading(&category.name);

    if ui.button("Add Flow").clicked() {
        app.create_new_flow(category);
    }

    egui::Grid::new(format!("flows_grid_{}", category.id))
        .striped(true)
        .show(ui, |ui| {
            // Header row
            ui.label("Date");
            ui.label("Amount");
            ui.label("Description");
            // Show tax_deductible for relevant categories
            if category.tax_deduction.deduction_allowed {
                ui.label("Tax Deductible");
            }
            for field in &category.fields {
                ui.label(&field.name);
            }
            ui.label(""); // Empty header for edit button column
            ui.end_row();

            // Data rows
            let flows: Vec<_> = app.flows.iter()
                .filter(|f| f.category_id == category.id)
                .cloned()
                .collect();

            for flow in flows {
                // Date cell
                ui.label(flow.date.to_string());
                
                // Amount cell
                ui.label(format!("${:.2}", flow.amount));
                
                // Description cell
                ui.label(&flow.description);
                
                // Tax deductible cell
                if category.tax_deduction.deduction_allowed {
                    let symbol = match flow.tax_deductible {
                        Some(true) => "[X]",
                        Some(false) => "[ ]",
                        None => "[ ]",
                    };
                    ui.label(symbol);
                }

                // Custom fields cells
                for field in &category.fields {
                    if let Some(value) = flow.custom_fields.get(&field.name) {
                        match field.field_type {
                            crate::models::FieldType::Boolean => {
                                if value.parse::<bool>().unwrap_or(false) {
                                    ui.label("[X]");
                                } else {
                                    ui.label("[ ]");
                                }
                            },
                            crate::models::FieldType::Number => {
                                if let Ok(num) = value.parse::<f64>() {
                                    ui.label(format!("${:.2}", num));
                                } else {
                                    ui.label(value);
                                }
                            },
                            _ => {
                                // For text, select, and date fields, capitalize first letter
                                let mut display_value = value.clone();
                                if !display_value.is_empty() {
                                    let mut chars: Vec<char> = display_value.chars().collect();
                                    if let Some(first) = chars.first_mut() {
                                        *first = first.to_uppercase().next().unwrap_or(*first);
                                    }
                                    display_value = chars.into_iter().collect();
                                }
                                ui.label(&display_value);
                            }
                        }
                    } else {
                        ui.label("");
                    }
                }

                // Edit button cell - always visible
                if ui.button("Edit").clicked() {
                    app.set_editing_flow(flow.clone());
                    // Initialize custom field values
                    app.custom_field_values.clear();
                    for field in &category.fields {
                        if let Some(value) = flow.custom_fields.get(&field.name) {
                            app.custom_field_values.insert(field.name.clone(), value.clone());
                        } else if let Some(default) = &field.default_value {
                            app.custom_field_values.insert(field.name.clone(), default.clone());
                        }
                    }
                }

                ui.end_row();
            }
        });
}