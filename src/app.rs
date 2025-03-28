use eframe::egui;
use std::collections::HashMap;
use uuid::Uuid;

use crate::models::{Flow, Category, get_default_categories};
use crate::ui::{show_main_panel, FlowEditorState};

pub struct PreftApp {
    pub categories: Vec<Category>,
    pub flows: Vec<Flow>,
    pub selected_category: Option<String>,
    pub show_category_editor: bool,
    pub new_flow: Option<Flow>,
    pub editing_flow: Option<Flow>,
    pub custom_field_values: HashMap<String, String>,
    flow_editor_state: FlowEditorState,
}

impl Default for PreftApp {
    fn default() -> Self {
        Self {
            categories: get_default_categories(),
            flows: Vec::new(),
            selected_category: None,
            show_category_editor: false,
            new_flow: None,
            editing_flow: None,
            custom_field_values: HashMap::new(),
            flow_editor_state: FlowEditorState::new(),
        }
    }
}

impl PreftApp {
    pub fn create_new_flow(&mut self, category: &Category) {
        let new_flow = Flow {
            id: Uuid::new_v4().to_string(),
            date: chrono::Local::now().naive_local().date(),
            amount: 0.0,
            category_id: category.id.clone(),
            description: String::new(),
            linked_flows: Vec::new(),
            custom_fields: HashMap::new(),
        };
        self.new_flow = Some(new_flow.clone());
        self.flow_editor_state.set_editor(new_flow);
        // Initialize custom field values
        self.custom_field_values.clear();
        for field in &category.fields {
            if let Some(default) = &field.default_value {
                self.custom_field_values.insert(field.name.clone(), default.clone());
            }
        }
    }

    pub fn save_flow(&mut self, flow_data: Flow) {
        if self.new_flow.is_some() {
            if let Some(_) = self.new_flow.take() {
                self.flows.push(flow_data.clone());
                // Create a new flow for the next entry
                self.new_flow = Some(Flow {
                    id: Uuid::new_v4().to_string(),
                    date: chrono::Local::now().naive_local().date(),
                    amount: 0.0,
                    category_id: flow_data.category_id.clone(),
                    description: String::new(),
                    linked_flows: Vec::new(),
                    custom_fields: HashMap::new(),
                });
                // Update the editor with the new flow
                self.flow_editor_state.set_editor(self.new_flow.as_ref().unwrap().clone());
            }
        } else if self.editing_flow.is_some() {
            if let Some(editing_flow) = self.editing_flow.take() {
                if let Some(existing_flow) = self.flows.iter_mut()
                    .find(|f| f.id == editing_flow.id) {
                    *existing_flow = flow_data;
                }
            }
        }
        self.custom_field_values.clear();
    }

    pub fn cancel_flow_edit(&mut self) {
        self.new_flow = None;
        self.editing_flow = None;
        self.custom_field_values.clear();
        self.flow_editor_state.clear_editor();
    }

    pub fn get_selected_category(&self) -> Option<&Category> {
        self.selected_category.as_ref()
            .and_then(|id| self.categories.iter().find(|c| c.id == *id))
    }
}

impl eframe::App for PreftApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            // First show the main panel
            show_main_panel(ui, self);
            
            // Then show the flow editor if needed
            if self.flow_editor_state.has_editor() {
                // Take the editor temporarily to avoid multiple mutable borrows
                if let Some(mut editor) = self.flow_editor_state.take_editor() {
                    // Get the category before showing the editor to avoid multiple mutable borrows
                    let category = self.get_selected_category().cloned();
                    if let Some(category) = category {
                        editor.show(ui, self, &category);
                        // Only set the editor back if we're still in edit mode
                        if self.new_flow.is_some() || self.editing_flow.is_some() {
                            self.flow_editor_state.set_editor(editor.take_flow_data());
                        }
                    }
                }
            }
        });

        // Handle escape key to close the editor
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.cancel_flow_edit();
        }
    }
} 