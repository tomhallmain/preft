use eframe::egui;
use std::collections::HashMap;
use uuid::Uuid;
use std::fs::File;
use std::io::Write;
use chrono::{Datelike, Local};

use crate::models::{Flow, Category, CategoryField, get_default_categories};
use crate::ui::{show_main_panel, FlowEditorState};
use crate::db::Database;
use crate::settings::UserSettings;
use crate::reporting::{ReportRequest, ReportGenerator};
use crate::ui::dashboard::Dashboard;
use crate::ui::category_flows::CategoryFlowsState;

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
    pub delete_category_confirmation: Option<String>,
    pub new_category: Option<Category>,  // This will now track all fields being added
    pub show_field_editor: bool,  // Track if field editor is open
    pub editing_field: Option<CategoryField>,  // Track the field being edited
    pub report_request: ReportRequest,
    pub show_report_dialog: bool,
    pub dashboard: Dashboard,
    pub category_flows_state: HashMap<String, CategoryFlowsState>,
    pub editing_category: Option<String>,  // Track which category is being edited
}

impl PreftApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Initialize database
        let db = Database::new().expect("Failed to initialize database");
        
        // Load categories from database or use defaults if none exist
        let categories = db.load_categories()
            .unwrap_or_else(|_| get_default_categories());
            
        // Load flows from database
        let flows = db.load_flows().unwrap_or_default();

        // Load user settings
        let user_settings = db.load_user_settings().unwrap_or_default();
        
        // Initialize category flows state for all categories
        let mut category_flows_state = HashMap::new();
        for category in &categories {
            category_flows_state.insert(category.id.clone(), CategoryFlowsState::new());
        }
        
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
            delete_category_confirmation: None,
            new_category: None,
            show_field_editor: false,
            editing_field: None,
            report_request: ReportRequest::default(),
            show_report_dialog: false,
            dashboard: Dashboard::new(),
            category_flows_state,
            editing_category: None,
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
                let category_id = flow_data.category_id.clone();
                let new_flow = Flow {
                    id: Uuid::new_v4().to_string(),
                    date: chrono::Local::now().naive_local().date(),
                    amount: 0.0,
                    category_id: category_id.clone(),
                    description: String::new(),
                    linked_flows: Vec::new(),
                    custom_fields: HashMap::new(),
                    tax_deductible: None,
                };
                self.new_flow = Some(new_flow.clone());
                // Update the editor with the new flow
                self.flow_editor_state.set_editor(new_flow, true);
                
                // TODO: The amount and description fields in the flow editor are not being reset to their default values
                // after saving a flow. This needs to be fixed by properly updating the FlowEditor's internal state
                // (amount_input and description_input) when creating a new flow.
                
                // Reinitialize default values for the new flow
                if let Some(category) = self.categories.iter().find(|c| c.id == category_id) {
                    self.custom_field_values.clear();
                    for field in &category.fields {
                        if let Some(default) = &field.default_value {
                            self.custom_field_values.insert(field.name.clone(), default.clone());
                        }
                    }
                }
                self.dashboard.mark_for_update();
                let state = self.category_flows_state.get_mut(&flow_data.category_id)
                    .expect("Category state should exist");
                state.mark_for_update();
            }
        } else if self.editing_flow.is_some() {
            if let Some(editing_flow) = self.editing_flow.take() {
                if let Some(existing_flow) = self.flows.iter_mut()
                    .find(|f| f.id == editing_flow.id) {
                    *existing_flow = flow_data;
                    self.dashboard.mark_for_update();
                    let state = self.category_flows_state.get_mut(&existing_flow.category_id)
                        .expect("Category state should exist");
                    state.mark_for_update();
                }
            }
        }
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

    pub fn generate_report(&self) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let generator = ReportGenerator::new(
            self.flows.clone(),
            self.categories.iter()
                .map(|cat| (cat.id.clone(), cat.name.clone()))
                .collect()
        );
        generator.generate_report(&self.report_request)
    }

    fn get_category_fields(&self) -> Vec<CategoryField> {
        self.get_selected_category()
            .map(|c| c.fields.clone())
            .unwrap_or_default()
    }

    fn show_group_by_selection(ui: &mut egui::Ui, group_by: &mut Option<String>, fields: &[CategoryField]) {
        ui.horizontal(|ui| {
            ui.label("Group By:");
            egui::ComboBox::from_id_source("group_by")
                .selected_text(group_by.as_deref().unwrap_or("None"))
                .show_ui(ui, |ui| {
                    ui.selectable_value(group_by, None, "None");
                    for field in fields {
                        ui.selectable_value(group_by, 
                            Some(field.name.clone()), &field.name);
                    }
                });
        });
    }

    pub fn delete_category(&mut self, category_id: String) {
        // Remove the category from the database
        if let Err(e) = self.db.delete_category(&category_id) {
            eprintln!("Failed to delete category: {}", e);
            return;
        }

        // Remove the category from memory
        self.categories.retain(|c| c.id != category_id);

        // Remove all flows associated with this category
        self.flows.retain(|f| f.category_id != category_id);
        if let Err(e) = self.db.delete_flows_by_category(&category_id) {
            eprintln!("Failed to delete flows for category: {}", e);
        }

        // Clear selection if the deleted category was selected
        if self.selected_category.as_ref() == Some(&category_id) {
            self.selected_category = None;
        }
        self.dashboard.mark_for_update();
        self.category_flows_state.remove(&category_id);
    }

    pub fn delete_flow(&mut self, flow_id: &str) -> Result<(), Box<dyn std::error::Error>> {
        // Remove the flow from the database
        self.db.delete_flow(flow_id)?;

        // Remove the flow from memory
        if let Some(pos) = self.flows.iter().position(|f| f.id == flow_id) {
            let flow = self.flows.remove(pos);
            self.dashboard.mark_for_update();
            let state = self.category_flows_state.get_mut(&flow.category_id)
                .expect("Category state should exist");
            state.mark_for_update();
        }

        Ok(())
    }

    pub fn add_category(&mut self, category: Category) {
        self.categories.push(category.clone());
        self.category_flows_state.insert(category.id.clone(), CategoryFlowsState::new());
        if let Err(e) = self.db.save_category(&category) {
            eprintln!("Failed to save category: {}", e);
        }
    }

    pub fn get_category_flows_state(&mut self, category_id: &str) -> &mut CategoryFlowsState {
        self.category_flows_state
            .entry(category_id.to_string())
            .or_insert_with(CategoryFlowsState::new)
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

            // Show report dialog if needed
            if self.show_report_dialog {
                let mut report_request = self.report_request.clone();
                let fields = self.get_category_fields();
                let flows = self.flows.clone();
                let mut should_close = false;
                let mut pdf_data = None;
                let mut show_window = true;
                
                egui::Window::new("Generate Report")
                    .open(&mut show_window)
                    .show(ctx, |ui| {
                        ui.heading("Report Settings");
                        
                        // Time period selection
                        ui.horizontal(|ui| {
                            ui.label("Time Period:");
                            egui::ComboBox::from_id_source("time_period")
                                .selected_text(format!("{:?}", self.report_request.time_period))
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut self.report_request.time_period, crate::reporting::TimePeriod::LastYear, "Last Year");
                                    ui.selectable_value(&mut self.report_request.time_period, crate::reporting::TimePeriod::ThisYear, "This Year");
                                    ui.selectable_value(&mut self.report_request.time_period, crate::reporting::TimePeriod::Custom(
                                        chrono::Local::now().date_naive().with_month(1).unwrap().with_day(1).unwrap(),
                                        chrono::Local::now().date_naive()
                                    ), "Custom");
                                });
                        });

                        // Group by selection
                        Self::show_group_by_selection(ui, &mut report_request.group_by, &fields);

                        // Title and subtitle
                        ui.horizontal(|ui| {
                            ui.label("Title:");
                            ui.text_edit_singleline(&mut report_request.title);
                        });
                        ui.horizontal(|ui| {
                            ui.label("Subtitle:");
                            ui.text_edit_singleline(&mut report_request.subtitle);
                        });

                        // Font settings
                        ui.separator();
                        ui.heading("Font Settings");

                        // Title font
                        ui.horizontal(|ui| {
                            ui.label("Title Font:");
                            egui::ComboBox::from_id_source("title_font")
                                .selected_text(report_request.font_settings.title_font.get_display_name())
                                .show_ui(ui, |ui| {
                                    for variant in [
                                        crate::reporting::FontVariant::RobotoRegular,
                                        crate::reporting::FontVariant::RobotoBold,
                                        crate::reporting::FontVariant::RobotoItalic,
                                        crate::reporting::FontVariant::RobotoBoldItalic,
                                        crate::reporting::FontVariant::TimesRegular,
                                        crate::reporting::FontVariant::TimesBold,
                                        crate::reporting::FontVariant::TimesItalic,
                                        crate::reporting::FontVariant::TimesBoldItalic,
                                    ] {
                                        ui.selectable_value(
                                            &mut report_request.font_settings.title_font,
                                            variant,
                                            variant.get_display_name(),
                                        );
                                    }
                                });
                        });

                        // Subtitle font
                        ui.horizontal(|ui| {
                            ui.label("Subtitle Font:");
                            egui::ComboBox::from_id_source("subtitle_font")
                                .selected_text(report_request.font_settings.subtitle_font.get_display_name())
                                .show_ui(ui, |ui| {
                                    for variant in [
                                        crate::reporting::FontVariant::RobotoRegular,
                                        crate::reporting::FontVariant::RobotoBold,
                                        crate::reporting::FontVariant::RobotoItalic,
                                        crate::reporting::FontVariant::RobotoBoldItalic,
                                        crate::reporting::FontVariant::TimesRegular,
                                        crate::reporting::FontVariant::TimesBold,
                                        crate::reporting::FontVariant::TimesItalic,
                                        crate::reporting::FontVariant::TimesBoldItalic,
                                    ] {
                                        ui.selectable_value(
                                            &mut report_request.font_settings.subtitle_font,
                                            variant,
                                            variant.get_display_name(),
                                        );
                                    }
                                });
                        });

                        // Header font
                        ui.horizontal(|ui| {
                            ui.label("Header Font:");
                            egui::ComboBox::from_id_source("header_font")
                                .selected_text(report_request.font_settings.header_font.get_display_name())
                                .show_ui(ui, |ui| {
                                    for variant in [
                                        crate::reporting::FontVariant::RobotoRegular,
                                        crate::reporting::FontVariant::RobotoBold,
                                        crate::reporting::FontVariant::RobotoItalic,
                                        crate::reporting::FontVariant::RobotoBoldItalic,
                                        crate::reporting::FontVariant::TimesRegular,
                                        crate::reporting::FontVariant::TimesBold,
                                        crate::reporting::FontVariant::TimesItalic,
                                        crate::reporting::FontVariant::TimesBoldItalic,
                                    ] {
                                        ui.selectable_value(
                                            &mut report_request.font_settings.header_font,
                                            variant,
                                            variant.get_display_name(),
                                        );
                                    }
                                });
                        });

                        // Body font
                        ui.horizontal(|ui| {
                            ui.label("Body Font:");
                            egui::ComboBox::from_id_source("body_font")
                                .selected_text(report_request.font_settings.body_font.get_display_name())
                                .show_ui(ui, |ui| {
                                    for variant in [
                                        crate::reporting::FontVariant::RobotoRegular,
                                        crate::reporting::FontVariant::RobotoBold,
                                        crate::reporting::FontVariant::RobotoItalic,
                                        crate::reporting::FontVariant::RobotoBoldItalic,
                                        crate::reporting::FontVariant::TimesRegular,
                                        crate::reporting::FontVariant::TimesBold,
                                        crate::reporting::FontVariant::TimesItalic,
                                        crate::reporting::FontVariant::TimesBoldItalic,
                                    ] {
                                        ui.selectable_value(
                                            &mut report_request.font_settings.body_font,
                                            variant,
                                            variant.get_display_name(),
                                        );
                                    }
                                });
                        });

                        // Generate button
                        if ui.button("Generate Report").clicked() {
                            let generator = ReportGenerator::new(
                                flows,
                                self.categories.iter()
                                    .map(|cat| (cat.id.clone(), cat.name.clone()))
                                    .collect()
                            );
                            if let Ok(data) = generator.generate_report(&report_request) {
                                pdf_data = Some(data);
                                should_close = true;
                            }
                        }
                    });
                
                if should_close || !show_window {
                    if let Some(data) = pdf_data {
                        // Save the PDF file
                        if let Some(path) = rfd::FileDialog::new()
                            .set_title("Save Report")
                            .set_file_name("financial_report.pdf")
                            .save_file() {
                            if let Ok(mut file) = File::create(path) {
                                if let Err(e) = file.write_all(&data) {
                                    eprintln!("Failed to save PDF: {}", e);
                                }
                            }
                        }
                    }
                    self.report_request = report_request;
                    self.show_report_dialog = false;
                }
            }
        });

        // Handle escape key to close the editor
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.cancel_flow_edit();
        }
    }
} 