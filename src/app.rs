use anyhow::Result;
use eframe::egui;
use std::collections::HashMap;
use uuid::Uuid;
use std::fs::File;
use std::io::Write;
use chrono::{Datelike, Local};
use log::info;

use crate::models::{Flow, Category, CategoryField, get_default_categories};
use crate::ui::{show_main_panel, FlowEditorState};
use crate::db::Database;
use crate::settings::UserSettings;
use crate::reporting::{ReportRequest, ReportGenerator};
use crate::ui::dashboard::Dashboard;
use crate::ui::category_flows::CategoryFlowsState;
use rusqlite::Connection;
use crate::encryption_config::EncryptionConfig;

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
    // Backup-related fields
    pub show_backup_dialog: bool,
    pub backup_status: Option<String>,
    pub backup_in_progress: bool,
    // Encryption-related fields
    pub show_password_dialog: bool,
    pub password_dialog_mode: PasswordDialogMode,
    pub password_input: String,
    pub password_confirm: String,
    pub encryption_status: Option<String>,
    // Encryption configuration (loaded from OS keystore)
    pub encryption_config: EncryptionConfig,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PasswordDialogMode {
    SetPassword,      // First time setting password
    EnterPassword,    // Entering password to unlock encrypted database
    ChangePassword,   // Changing existing password
    DisableEncryption, // Disabling encryption entirely
}

impl PreftApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Initialize database
        let db = match Database::new() {
            Ok(db) => db,
            Err(e) => {
                eprintln!("Failed to initialize database: {}", e);
                eprintln!("This might happen if the database file is corrupted or inaccessible.");
                eprintln!("The application will start with default settings.");
                
                // Try to create a minimal database connection for basic functionality
                match Database::new_minimal() {
                    Ok(db) => {
                        eprintln!("Successfully created minimal database connection.");
                        db
                    }
                    Err(e2) => {
                        eprintln!("Failed to create minimal database: {}", e2);
                        eprintln!("Using in-memory database as last resort.");
                        
                        // Create an in-memory database as fallback
                        let conn = Connection::open_in_memory().expect("Failed to create in-memory database");
                        Database::from_connection(conn)
                    }
                }
            }
        };
        
        // Load categories from database or use defaults if none exist
        let categories = db.load_categories()
            .unwrap_or_else(|e| {
                eprintln!("Failed to load categories: {}", e);
                get_default_categories()
            });
            
        // Load flows from database
        let flows = db.load_flows().unwrap_or_else(|e| {
            eprintln!("Failed to load flows: {}", e);
            Vec::new()
        });

        // Load user settings
        let user_settings = db.load_user_settings().unwrap_or_else(|e| {
            eprintln!("Failed to load user settings: {}", e);
            UserSettings::new()
        });
        
        // Load encryption configuration
        let encryption_config = EncryptionConfig::load().unwrap_or_else(|e| {
            eprintln!("Failed to load encryption config: {}", e);
            EncryptionConfig::default()
        });
        
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
            // Backup-related fields
            show_backup_dialog: false,
            backup_status: None,
            backup_in_progress: false,
            // Encryption-related fields
            show_password_dialog: false,
            password_dialog_mode: PasswordDialogMode::SetPassword,
            password_input: String::new(),
            password_confirm: String::new(),
            encryption_status: None,
            // Encryption configuration (loaded from OS keystore)
            encryption_config,
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

    pub fn create_backup(&mut self) {
        if self.backup_in_progress {
            return;
        }

        self.backup_in_progress = true;
        self.backup_status = Some("Selecting backup location...".to_string());

        // Show file dialog for backup location
        if let Some(path) = rfd::FileDialog::new()
            .set_title("Save Backup As")
            .set_file_name(&format!("preft_backup_{}.db", chrono::Local::now().format("%Y%m%d_%H%M%S")))
            .add_filter("SQLite Database", &["db"])
            .add_filter("All Files", &["*"])
            .save_file()
        {
            self.backup_status = Some("Creating backup...".to_string());

            // Determine if we should create encrypted or unencrypted backup
            let encrypted_backup = self.db.is_encrypted();
            
            match self.db.backup_to_file(&path, encrypted_backup) {
                Ok(_) => {
                    let file_size = std::fs::metadata(&path)
                        .map(|m| m.len())
                        .ok();

                    let entry = crate::settings::BackupEntry {
                        timestamp: chrono::Utc::now(),
                        file_path: path.to_string_lossy().to_string(),
                        file_size,
                        success: true,
                        error_message: None,
                    };

                    self.user_settings.add_backup_entry(entry.clone());
                    self.user_settings.set_last_backup_path(path.to_string_lossy().to_string());

                    if let Err(e) = self.db.save_user_settings(&self.user_settings) {
                        eprintln!("Failed to save backup history: {}", e);
                    }

                    self.backup_status = Some(format!(
                        "Backup completed successfully! {}",
                        if encrypted_backup { "(Encrypted)" } else { "(Unencrypted)" }
                    ));
                }
                Err(e) => {
                    let entry = crate::settings::BackupEntry {
                        timestamp: chrono::Utc::now(),
                        file_path: path.to_string_lossy().to_string(),
                        file_size: None,
                        success: false,
                        error_message: Some(e.to_string()),
                    };

                    self.user_settings.add_backup_entry(entry);
                    if let Err(e) = self.db.save_user_settings(&self.user_settings) {
                        eprintln!("Failed to save backup history: {}", e);
                    }

                    self.backup_status = Some(format!("Backup failed: {}", e));
                }
            }
        } else {
            self.backup_status = Some("Backup cancelled".to_string());
        }

        self.backup_in_progress = false;
    }

    pub fn restore_backup(&mut self) {
        if self.backup_in_progress {
            return;
        }

        self.backup_in_progress = true;
        self.backup_status = Some("Selecting backup file...".to_string());

        // Show file dialog for backup file
        if let Some(path) = rfd::FileDialog::new()
            .set_title("Select Backup File")
            .add_filter("SQLite Database", &["db"])
            .add_filter("All Files", &["*"])
            .pick_file()
        {
            self.backup_status = Some("Restoring backup...".to_string());

            // Try to detect if the backup is encrypted
            let is_encrypted_backup = match self.db.detect_encrypted_backup(&path) {
                Ok(encrypted) => encrypted,
                Err(_) => false, // Assume unencrypted if we can't detect
            };

            let result = if is_encrypted_backup {
                // For encrypted backups, we need the password
                if !self.encryption_config.is_encryption_ready() {
                    Err(anyhow::anyhow!("Encrypted backup detected but no password is set. Please set a password first."))
                } else {
                    // For now, we'll use a simple approach - if the backup is encrypted and we have encryption set up,
                    // we'll try to restore it. In a real implementation, you might want to prompt the user for the password.
                    // For now, we'll assume the current password works (this is a simplification)
                    self.db.restore_from_file(&path, None, true) // Force unencrypted restore for now
                }
            } else {
                // For unencrypted backups, restore as unencrypted
                self.db.restore_from_file(&path, None, false)
            };

            match result {
                Ok(_) => {
                    // Reload all data from the restored database
                    self.categories = self.db.load_categories()
                        .unwrap_or_else(|e| { eprintln!("Failed to load categories: {}", e); Vec::new() });
                    self.flows = self.db.load_flows()
                        .unwrap_or_else(|e| { eprintln!("Failed to load flows: {}", e); Vec::new() });
                    self.user_settings = self.db.load_user_settings()
                        .unwrap_or_else(|e| { eprintln!("Failed to load user settings: {}", e); UserSettings::new() });

                    // Update UI components to reflect the restored data
                    self.dashboard.mark_for_update();
                    
                    // Update category flows states
                    self.category_flows_state.clear();
                    for category in &self.categories {
                        self.category_flows_state.insert(category.id.clone(), crate::ui::category_flows::CategoryFlowsState::new());
                    }

                    self.backup_status = Some("Backup restored successfully!".to_string());
                }
                Err(e) => {
                    self.backup_status = Some(format!("Restore failed: {}", e));
                }
            }
        } else {
            self.backup_status = Some("Restore cancelled".to_string());
        }

        self.backup_in_progress = false;
    }

    pub fn clear_backup_status(&mut self) {
        self.backup_status = None;
    }

    // Password management methods
    pub fn show_set_password_dialog(&mut self) {
        self.password_dialog_mode = PasswordDialogMode::SetPassword;
        self.password_input.clear();
        self.password_confirm.clear();
        self.show_password_dialog = true;
    }

    pub fn show_enter_password_dialog(&mut self) {
        self.password_dialog_mode = PasswordDialogMode::EnterPassword;
        self.password_input.clear();
        self.password_confirm.clear();
        self.show_password_dialog = true;
    }

    pub fn show_change_password_dialog(&mut self) {
        self.password_dialog_mode = PasswordDialogMode::ChangePassword;
        self.password_input.clear();
        self.password_confirm.clear();
        self.show_password_dialog = true;
    }

    pub fn show_disable_encryption_dialog(&mut self) {
        self.password_dialog_mode = PasswordDialogMode::DisableEncryption;
        self.password_input.clear();
        self.password_confirm.clear();
        self.show_password_dialog = true;
    }

    pub fn set_password(&mut self, password: &str) -> Result<(), anyhow::Error> {
        // Set password in encryption config (this will generate salt and hash)
        self.encryption_config.set_password(password)?;
        
        // Initialize encryption in database
        self.db.initialize_encryption(password)?;
        
        self.encryption_status = Some("Password set successfully".to_string());
        Ok(())
    }

    pub fn verify_password(&mut self, password: &str) -> Result<bool, anyhow::Error> {
        let is_valid = self.encryption_config.verify_password(password);
        
        if is_valid {
            // Initialize encryption with the correct password
            let salt = self.encryption_config.get_salt()
                .ok_or_else(|| anyhow::anyhow!("Salt not found"))?;
            self.db.set_encryption_state(true, Some(password), Some(salt))?;
            self.encryption_status = Some("Password verified successfully".to_string());
        } else {
            self.encryption_status = Some("Incorrect password".to_string());
        }
        
        Ok(is_valid)
    }

    pub fn change_password(&mut self, new_password: &str) -> Result<(), anyhow::Error> {
        // Set the new password (this will update the hash and salt)
        self.set_password(new_password)?;
        self.encryption_status = Some("Password changed successfully".to_string());
        Ok(())
    }

    pub fn disable_encryption(&mut self) -> Result<(), anyhow::Error> {
        // Disable encryption in the config
        self.encryption_config.disable_encryption()?;
        
        // Disable encryption in the database
        self.db.set_encryption_state(false, None, None)?;
        
        self.encryption_status = Some("Encryption disabled successfully".to_string());
        Ok(())
    }

    pub fn re_enable_encryption(&mut self) -> Result<(), anyhow::Error> {
        // Re-enable encryption configuration (without password)
        self.encryption_config.re_enable_encryption()?;
        
        // Database remains unencrypted until password is set
        self.db.set_encryption_state(false, None, None)?;
        
        self.encryption_status = Some("Encryption configuration re-enabled. Set a password to encrypt the database.".to_string());
        Ok(())
    }

    pub fn clear_encryption_status(&mut self) {
        self.encryption_status = None;
    }

    /// Create an automatic backup if enabled
    pub fn create_automatic_backup(&mut self) -> Result<(), anyhow::Error> {
        if !self.user_settings.is_auto_backup_enabled() {
            return Ok(());
        }

        let backup_dir = match self.user_settings.get_auto_backup_directory() {
            Some(dir) => std::path::PathBuf::from(dir),
            None => {
                // Use default backup directory in user's home directory
                let home_dir = dirs::home_dir().ok_or_else(|| {
                    anyhow::anyhow!("Could not determine home directory")
                })?;
                home_dir.join(".preft").join("auto_backups")
            }
        };

        // Check if backup directory is accessible
        if !backup_dir.exists() {
            // Try to create the directory, but don't fail if we can't
            if let Err(e) = std::fs::create_dir_all(&backup_dir) {
                eprintln!("Warning: Could not create backup directory {:?}: {}", backup_dir, e);
                return Ok(()); // Gracefully skip backup if directory creation fails
            }
        }

        // Check if directory is writable
        if let Err(e) = std::fs::metadata(&backup_dir) {
            eprintln!("Warning: Backup directory {:?} is not accessible: {}", backup_dir, e);
            return Ok(()); // Gracefully skip backup if directory is not accessible
        }

        // Generate backup filename with timestamp
        let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
        let backup_filename = format!("preft_auto_backup_{}.db", timestamp);
        let backup_path = backup_dir.join(backup_filename);

        // Determine if we should create encrypted or unencrypted backup based on settings
        let encrypted_backup = self.user_settings.auto_backup_encrypted.unwrap_or(false);
        
        // Create the backup
        if let Err(e) = self.db.backup_to_file(&backup_path, encrypted_backup) {
            eprintln!("Warning: Failed to create automatic backup: {}", e);
            return Ok(()); // Gracefully skip backup if creation fails
        }

        // Update user settings
        self.user_settings.set_last_backup_path(backup_path.to_string_lossy().to_string());
        
        // Add to backup history
        let file_size = std::fs::metadata(&backup_path).ok().map(|m| m.len());
        let entry = crate::settings::BackupEntry {
            timestamp: chrono::Utc::now(),
            file_path: backup_path.to_string_lossy().to_string(),
            file_size,
            success: true,
            error_message: None,
        };
        self.user_settings.add_backup_entry(entry);

        // Save updated settings (don't fail if this doesn't work)
        if let Err(e) = self.db.save_user_settings(&self.user_settings) {
            eprintln!("Warning: Failed to save backup history: {}", e);
        }

        // Clean up old automatic backups (keep only the 5 most recent)
        if let Err(e) = self.cleanup_old_automatic_backups(&backup_dir) {
            eprintln!("Warning: Failed to cleanup old automatic backups: {}", e);
        }

        Ok(())
    }

    /// Clean up old automatic backups, keeping only the 5 most recent
    fn cleanup_old_automatic_backups(&self, backup_dir: &std::path::Path) -> Result<(), anyhow::Error> {
        // Read all files in the backup directory
        let mut backup_files = Vec::new();
        
        if let Ok(entries) = std::fs::read_dir(backup_dir) {
            for entry in entries {
                if let Ok(entry) = entry {
                    let path = entry.path();
                    
                    // Only consider files that match our automatic backup pattern
                    if let Some(file_name) = path.file_name() {
                        if let Some(file_name_str) = file_name.to_str() {
                            if file_name_str.starts_with("preft_auto_backup_") && file_name_str.ends_with(".db") {
                                // Get file metadata for sorting by modification time
                                if let Ok(metadata) = std::fs::metadata(&path) {
                                    if let Ok(modified_time) = metadata.modified() {
                                        backup_files.push((path, modified_time));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Sort by modification time (newest first)
        backup_files.sort_by(|a, b| b.1.cmp(&a.1));

        // Remove files beyond the 5th one
        let files_to_remove = backup_files.len().saturating_sub(5);
        if files_to_remove > 0 {
            info!("Cleaning up {} old automatic backup(s)...", files_to_remove);
            for (file_path, _) in backup_files.iter().skip(5) {
                if let Err(e) = std::fs::remove_file(file_path) {
                    eprintln!("Warning: Failed to remove old backup file {:?}: {}", file_path, e);
                } else {
                    info!("Removed old backup: {:?}", file_path.file_name().unwrap_or_default());
                }
            }
        }

        Ok(())
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

            // Show backup dialog if needed
            if self.show_backup_dialog {
                crate::ui::show_backup_dialog(ctx, self);
            }

            // Show password dialog if needed
            if self.show_password_dialog {
                crate::ui::show_password_dialog(ctx, self);
            }
        });

        // Handle escape key to close the editor
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.cancel_flow_edit();
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // Create automatic backup if enabled
        if let Err(e) = self.create_automatic_backup() {
            eprintln!("Failed to create automatic backup on shutdown: {}", e);
        }
    }
} 