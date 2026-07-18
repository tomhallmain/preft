use anyhow::Result;
use eframe::egui;
use std::collections::HashMap;
use uuid::Uuid;
use log::{info, warn, error};

use crate::models::{Flow, Category, CategoryField, get_default_categories};
use crate::ui::{show_main_panel, FlowEditorState};
use crate::db::Database;
use crate::settings::UserSettings;
use crate::reporting::ReportRequest;
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
    /// Set while a manual backup's final move-into-place is running on a
    /// background thread (see `create_backup`); polled once per frame by
    /// `poll_pending_backup`.
    pending_backup: Option<std::sync::mpsc::Receiver<BackupMoveOutcome>>,
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

/// Result of a background thread's attempt to move a completed backup (see
/// `create_backup`) from its local temp path to the destination the user
/// picked. Carries everything `poll_pending_backup` needs to finish the
/// backup-history bookkeeping without touching the filesystem again.
struct BackupMoveOutcome {
    dest_path: std::path::PathBuf,
    encrypted: bool,
    file_size: Option<u64>,
    error: Option<String>,
}

/// Moves `temp_path` to `dest_path`. Tries a plain rename first (fast,
/// atomic, and the common case since both are usually on the same
/// filesystem); falls back to copy-then-remove if that fails, since rename
/// can't cross filesystem boundaries (e.g. the destination is a different
/// drive, a network share, or removable media) and that's exactly the kind
/// of destination this is meant to support without blocking the UI.
fn move_backup_file(temp_path: &std::path::Path, dest_path: &std::path::Path) -> std::io::Result<()> {
    if std::fs::rename(temp_path, dest_path).is_ok() {
        return Ok(());
    }
    std::fs::copy(temp_path, dest_path)?;
    std::fs::remove_file(temp_path)?;
    Ok(())
}

impl PreftApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Initialize database
        let db = match Database::new() {
            Ok(db) => db,
            Err(e) => {
                log::error!("Failed to initialize database: {}", e);
                log::error!("This might happen if the database file is corrupted or inaccessible.");
                log::error!("The application will start with default settings.");
                
                // Try to create a minimal database connection for basic functionality
                match Database::new_minimal() {
                    Ok(db) => {
                        log::info!("Successfully created minimal database connection.");
                        db
                    }
                    Err(e2) => {
                        log::error!("Failed to create minimal database: {}", e2);
                        log::error!("Using in-memory database as last resort.");
                        
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
                log::error!("Failed to load categories: {}", e);
                get_default_categories()
            });
            
        // Load flows from database
        let flows = db.load_flows().unwrap_or_else(|e| {
            log::error!("Failed to load flows: {}", e);
            Vec::new()
        });

        // Load user settings
        let user_settings = db.load_user_settings().unwrap_or_else(|e| {
            log::error!("Failed to load user settings: {}", e);
            UserSettings::new()
        });
        
        // Load encryption configuration
        let encryption_config = EncryptionConfig::load().unwrap_or_else(|e| {
            log::error!("Failed to load encryption config: {}", e);
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
            pending_backup: None,
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
            log::error!("Failed to save user settings: {}", e);
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
            log::error!("Failed to save flow: {}", e);
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
                // Update the editor with the new flow. FlowEditor::new()
                // resets amount_input/description_input from the fresh Flow's
                // defaults; see the has_editor() check in update() that keeps
                // this fresh editor from being overwritten by the stale one.
                self.flow_editor_state.set_editor(new_flow, true);

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

    pub fn delete_category(&mut self, category_id: String) {
        // Remove the category from the database
        if let Err(e) = self.db.delete_category(&category_id) {
            log::error!("Failed to delete category: {}", e);
            return;
        }

        // Remove the category from memory
        self.categories.retain(|c| c.id != category_id);

        // Remove all flows associated with this category
        self.flows.retain(|f| f.category_id != category_id);
        if let Err(e) = self.db.delete_flows_by_category(&category_id) {
            log::error!("Failed to delete flows for category: {}", e);
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
            log::error!("Failed to save category: {}", e);
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
        let Some(dest_path) = rfd::FileDialog::new()
            .set_title("Save Backup As")
            .set_file_name(&format!("preft_backup_{}.db", chrono::Local::now().format("%Y%m%d_%H%M%S")))
            .add_filter("SQLite Database", &["db"])
            .add_filter("All Files", &["*"])
            .save_file()
        else {
            self.backup_status = Some("Backup cancelled".to_string());
            self.backup_in_progress = false;
            return;
        };

        self.backup_status = Some("Creating backup...".to_string());

        // Determine if we should create encrypted or unencrypted backup
        let encrypted_backup = self.db.is_encrypted();

        // The actual SQLite copy needs live access to `self.db` (it reads
        // the in-memory encryption key for an unencrypted/decrypted backup),
        // so it can't safely run on another thread without a much bigger
        // refactor -- but it's local-disk-to-local-disk on a personal-
        // finance-sized database, so it's fast. Write it to a local temp
        // file first, then hand that off to a background thread to move
        // into place. That move is the part that can actually be slow (the
        // user picked destination could be a network drive or a USB stick),
        // and it touches nothing but plain files, so it's safe to run off
        // the UI thread without touching `self.db` at all.
        let temp_path = std::env::temp_dir().join(format!(
            "preft_backup_tmp_{}_{}.db",
            std::process::id(),
            chrono::Local::now().format("%Y%m%d%H%M%S"),
        ));

        match self.db.backup_to_file(&temp_path, encrypted_backup) {
            Ok(()) => {
                self.backup_status = Some("Finishing backup...".to_string());

                let (tx, rx) = std::sync::mpsc::channel();
                self.pending_backup = Some(rx);

                std::thread::spawn(move || {
                    let outcome = match move_backup_file(&temp_path, &dest_path) {
                        Ok(()) => BackupMoveOutcome {
                            dest_path: dest_path.clone(),
                            encrypted: encrypted_backup,
                            file_size: std::fs::metadata(&dest_path).ok().map(|m| m.len()),
                            error: None,
                        },
                        Err(e) => {
                            let _ = std::fs::remove_file(&temp_path); // best-effort cleanup
                            BackupMoveOutcome {
                                dest_path: dest_path.clone(),
                                encrypted: encrypted_backup,
                                file_size: None,
                                error: Some(e.to_string()),
                            }
                        }
                    };
                    // If the receiver's gone (e.g. the app is shutting down),
                    // there's nothing left to report the outcome to.
                    let _ = tx.send(outcome);
                });
            }
            Err(e) => {
                let _ = std::fs::remove_file(&temp_path); // may not exist; best-effort

                let entry = crate::settings::BackupEntry {
                    timestamp: chrono::Utc::now(),
                    file_path: dest_path.to_string_lossy().to_string(),
                    file_size: None,
                    success: false,
                    error_message: Some(e.to_string()),
                };
                self.user_settings.add_backup_entry(entry);
                if let Err(e) = self.db.save_user_settings(&self.user_settings) {
                    log::error!("Failed to save backup history: {}", e);
                }

                self.backup_status = Some(format!("Backup failed: {}", e));
                self.backup_in_progress = false;
            }
        }
    }

    /// Checks whether a manual backup's background move-into-place (started
    /// by `create_backup`) has finished, and if so, finalizes the backup
    /// history entry and clears `backup_in_progress`. Called once per frame
    /// from `update()`; a no-op (a single non-blocking channel poll) unless
    /// a backup is actually pending.
    pub fn poll_pending_backup(&mut self) {
        let Some(rx) = &self.pending_backup else { return };

        let outcome = match rx.try_recv() {
            Ok(outcome) => outcome,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                // The thread panicked before sending anything -- surface
                // that rather than leaving backup_in_progress stuck forever.
                self.pending_backup = None;
                self.backup_status = Some("Backup failed: background thread did not complete".to_string());
                self.backup_in_progress = false;
                return;
            }
        };
        self.pending_backup = None;

        let entry = crate::settings::BackupEntry {
            timestamp: chrono::Utc::now(),
            file_path: outcome.dest_path.to_string_lossy().to_string(),
            file_size: outcome.file_size,
            success: outcome.error.is_none(),
            error_message: outcome.error.clone(),
        };
        self.user_settings.add_backup_entry(entry);
        if outcome.error.is_none() {
            self.user_settings.set_last_backup_path(outcome.dest_path.to_string_lossy().to_string());
        }
        if let Err(e) = self.db.save_user_settings(&self.user_settings) {
            log::error!("Failed to save backup history: {}", e);
        }

        self.backup_status = Some(match &outcome.error {
            None => format!(
                "Backup completed successfully! {}",
                if outcome.encrypted { "(Encrypted)" } else { "(Unencrypted)" }
            ),
            Some(e) => format!("Backup failed: {}", e),
        });

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
                        .unwrap_or_else(|e| { log::error!("Failed to load categories: {}", e); Vec::new() });
                    self.flows = self.db.load_flows()
                        .unwrap_or_else(|e| { log::error!("Failed to load flows: {}", e); Vec::new() });
                    self.user_settings = self.db.load_user_settings()
                        .unwrap_or_else(|e| { log::error!("Failed to load user settings: {}", e); UserSettings::new() });

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

        // No financial data (flows/categories) changed this session -- an
        // automatic backup would just be an identical duplicate of the most
        // recent one, so skip it. Deliberately not affected by UI-only
        // changes like the year filter or a hidden-category toggle, since
        // those are preferences, not records worth backing up.
        if !self.db.is_dirty() {
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
                log::warn!("Warning: Could not create backup directory {:?}: {}", backup_dir, e);
                return Ok(()); // Gracefully skip backup if directory creation fails
            }
        }

        // Check if directory is writable
        if let Err(e) = std::fs::metadata(&backup_dir) {
            log::warn!("Warning: Backup directory {:?} is not accessible: {}", backup_dir, e);
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
            log::warn!("Warning: Failed to create automatic backup: {}", e);
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
            log::warn!("Warning: Failed to save backup history: {}", e);
        }

        // Clean up old automatic backups (keep only the 5 most recent)
        if let Err(e) = self.cleanup_old_automatic_backups(&backup_dir) {
            log::warn!("Warning: Failed to cleanup old automatic backups: {}", e);
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
            log::info!("Cleaning up {} old automatic backup(s)...", files_to_remove);
            for (file_path, _) in backup_files.iter().skip(5) {
                if let Err(e) = std::fs::remove_file(file_path) {
                    log::warn!("Warning: Failed to remove old backup file {:?}: {}", file_path, e);
                } else {
                    log::info!("Removed old backup: {:?}", file_path.file_name().unwrap_or_default());
                }
            }
        }

        Ok(())
    }
}

impl eframe::App for PreftApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_pending_backup();
        if self.pending_backup.is_some() {
            // Keep polling at a modest rate while the background move is in
            // flight -- egui doesn't repaint on its own between input
            // events, and nothing else here would otherwise wake it up
            // until the move finishes.
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }

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
                        // save_flow() may have already installed a fresh editor
                        // (e.g. for the next new-flow entry, with amount/description
                        // reset to their defaults) during the call above -- don't
                        // clobber it by putting the stale one we took out back.
                        if !self.flow_editor_state.has_editor()
                            && (self.new_flow.is_some() || self.editing_flow.is_some())
                        {
                            self.flow_editor_state.put_editor_back(editor);
                        }
                    }
                }
            }

            // Show report dialog if needed
            if self.show_report_dialog {
                crate::ui::show_report_dialog(ctx, self);
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
        // If a manual backup's background move (see `create_backup`) is
        // still in flight, the process exiting would kill that thread
        // mid-copy and could leave a truncated file at the destination the
        // user picked. Give it a bounded window to finish first -- long
        // enough for even a slow destination in the common case, but not
        // so long that a stuck thread hangs shutdown indefinitely.
        let manual_backup_was_pending = self.pending_backup.is_some();
        if let Some(rx) = self.pending_backup.take() {
            let _ = rx.recv_timeout(std::time::Duration::from_secs(10));
        }

        // A manual backup that was still wrapping up when the app closed
        // already captures the current state -- an automatic backup right
        // on top of it would just be a redundant duplicate.
        if manual_backup_was_pending {
            return;
        }

        // Create automatic backup if enabled (and only if something
        // actually changed since startup -- see `Database::is_dirty`).
        if let Err(e) = self.create_automatic_backup() {
            log::error!("Failed to create automatic backup on shutdown: {}", e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // `move_backup_file` is the one piece of the backgrounded-backup change
    // that's pure enough to unit test without a constructible `PreftApp`
    // (see docs/APP_STATE_REFACTOR.md) -- it only touches plain files.

    #[test]
    fn move_backup_file_moves_content_to_the_destination() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let src = dir.path().join("source.db");
        let dest = dir.path().join("dest.db");
        std::fs::write(&src, b"backup contents").expect("write source file");

        move_backup_file(&src, &dest).expect("move should succeed");

        assert!(!src.exists(), "source file should be gone after a successful move");
        assert_eq!(std::fs::read(&dest).expect("read dest"), b"backup contents");
    }

    #[test]
    fn move_backup_file_errors_when_source_is_missing() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let src = dir.path().join("does_not_exist.db");
        let dest = dir.path().join("dest.db");

        assert!(move_backup_file(&src, &dest).is_err());
    }
}