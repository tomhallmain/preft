use eframe::egui;
use std::collections::HashMap;
use uuid::Uuid;

use crate::models::{Flow, Category, CategoryField, get_default_categories};
use crate::ui::{show_main_panel, FlowEditorState};
use crate::db::Database;
use crate::settings::UserSettings;

pub struct PreftApp {
    pub categories: Vec<Category>,
    pub flows: Vec<Flow>,
    pub selected_category: Option<String>,
    pub show_category_editor: bool,
    pub show_hidden_categories: bool,
    pub new_flow: Option<Flow>,
    pub editing_flow: Option<Flow>,
    pub custom_field_values: HashMap<String, String>,
    pub user_settings: UserSettings,
    flow_editor_state: FlowEditorState,
    pub db: Database,
    pub hide_category_confirmation: Option<String>,  // Track which category is being confirmed for hiding
    pub new_category: Option<Category>,  // This will now track all fields being added
    pub show_field_editor: bool,  // Track if field editor is open
    pub editing_field: Option<CategoryField>,  // Track the field being edited
}

impl PreftApp {
    pub fn new() -> Self {
        // Initialize database
        let db = Database::new().expect("Failed to initialize database");
        
        // Load categories from database or use defaults if none exist
        let categories = db.load_categories()
            .unwrap_or_else(|_| get_default_categories());
            
        // Load flows from database
        let flows = db.load_flows().unwrap_or_default();

        // Load user settings
        let user_settings = db.load_user_settings().unwrap_or_default();
        
        Self {
            categories,
            flows,
            selected_category: None,
            show_category_editor: false,
            show_hidden_categories: false,
            new_flow: None,
            editing_flow: None,
            custom_field_values: HashMap::new(),
            user_settings,
            flow_editor_state: FlowEditorState::new(),
            db,
            hide_category_confirmation: None,
            new_category: None,  // Initialize as None
            show_field_editor: false,
            editing_field: None,
        }
    }

    pub fn toggle_category_visibility(&mut self, category_id: String) {
        self.user_settings.toggle_category_visibility(category_id);
        if let Err(e) = self.db.save_user_settings(&self.user_settings) {
            eprintln!("Failed to save user settings: {}", e);
        }
    }

    pub fn is_category_hidden(&self, category_id: &str) -> bool {
        self.user_settings.is_category_hidden(category_id)
    }

    pub fn create_new_flow(&mut self, category: &Category) {
        let new_flow = Flow {
            id: Uuid::new_v4().to_string(),
            date: chrono::Local::now().naive_local().date(),
            amount: 0.0,
            category_id: category.id.clone(),
            description: String::new(),
            linked_flows: Vec::new(),
            custom_fields: HashMap::new(),
            tax_deductible: None,
        };
        self.new_flow = Some(new_flow.clone());
        self.flow_editor_state.set_editor(new_flow, true);
        // Initialize custom field values
        self.custom_field_values.clear();
        for field in &category.fields {
            if let Some(default) = &field.default_value {
                self.custom_field_values.insert(field.name.clone(), default.clone());
            }
        }
    }

    pub fn save_flow(&mut self, mut flow_data: Flow) {
        // Copy all custom field values to the flow's custom_fields
        for (name, value) in &self.custom_field_values {
            flow_data.custom_fields.insert(name.clone(), value.clone());
        }

        // Save to database
        if let Err(e) = self.db.save_flow(&flow_data) {
            eprintln!("Failed to save flow: {}", e);
            return;
        }

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
                    tax_deductible: None,
                });
                // Update the editor with the new flow
                self.flow_editor_state.set_editor(self.new_flow.as_ref().unwrap().clone(), true);
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

    pub fn set_editing_flow(&mut self, flow: Flow) {
        self.editing_flow = Some(flow.clone());
        self.flow_editor_state.set_editor(flow, false);
    }
}

impl eframe::App for PreftApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            // First show the main panel
            show_main_panel(ui, self);
            
            // Then show the flow editor if needed
            if self.flow_editor_state.has_editor() {
                // Get the category before showing the editor to avoid multiple mutable borrows
                let category = self.get_selected_category().cloned();
                if let Some(category) = category {
                    // Take the editor temporarily to avoid multiple mutable borrows
                    if let Some(mut editor) = self.flow_editor_state.take_editor() {
                        editor.show(ui, self, &category);
                        // Only set the editor back if we're still in edit mode
                        if self.new_flow.is_some() || self.editing_flow.is_some() {
                            self.flow_editor_state.put_editor_back(editor);
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