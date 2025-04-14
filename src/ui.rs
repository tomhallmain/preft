use eframe::egui;
use crate::models::{Flow, Category, CategoryField, FieldType};
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
    date_input: String,  // Store raw date input
    date_error: Option<String>,  // Store date validation error
    amount_input: String,  // Store raw amount input
    description_input: String,  // Store raw description input
}

impl FlowEditor {
    pub fn new(flow: Flow, is_new_flow: bool) -> Self {
        let flow_clone = flow.clone();
        let date_input = flow_clone.date.to_string();  // Get the date string from the clone
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
                                .color(egui::Color32::from_rgb(200, 100, 0))  // Darker orange color
                                .size(16.0));  // Slightly larger
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
                            if let Ok(date) = chrono::NaiveDate::parse_from_str(&self.date_input, "%Y-%m-%d") {
                                self.flow_data.date = date;
                                self.date_error = None;  // Clear any previous error
                                app.save_flow(self.flow_data.clone());
                            } else {
                                // Store error message
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

pub fn show_main_panel(ui: &mut egui::Ui, app: &mut PreftApp) {
    ui.horizontal(|ui| {
        ui.heading("Personal Finance Tracker");
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
                app.categories.push(category);
                app.db.save_category(&app.categories.last().unwrap()).expect("Failed to save category");
                app.show_category_editor = false;
            } else if should_cancel {
                app.show_category_editor = false;
            } else {
                // Put the category back if neither save nor cancel was clicked
                app.new_category = Some(category);
            }
        }
    }

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

fn show_category_flows(ui: &mut egui::Ui, app: &mut PreftApp, category: &Category) {
    ui.heading(&category.name);

    if ui.button("Add Flow").clicked() {
        app.create_new_flow(category);
    }

    // TODO Remove the header row from the scroll area once the column widths are fixed
    egui::ScrollArea::vertical()
        .id_source(format!("flows_scroll_{}", category.id))
        .auto_shrink([false, false])
        .show(ui, |ui| {
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

                        // Add spacing between buttons
                        ui.label("");

                        // Delete button
                        if ui.button("Delete").clicked() {
                            if let Err(e) = app.delete_flow(&flow.id) {
                                // Show error in UI
                                ui.label(egui::RichText::new(format!("Error deleting flow: {}", e))
                                    .color(egui::Color32::RED));
                            }
                        }

                        ui.end_row();
                    }
                });
        });
}