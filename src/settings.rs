use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use chrono::{self, Datelike, DateTime, Utc};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupEntry {
    pub timestamp: DateTime<Utc>,
    pub file_path: String,
    pub file_size: Option<u64>,
    pub success: bool,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UserSettings {
    #[serde(default)]
    pub hidden_categories: HashSet<String>,  // Set of category IDs that are hidden
    #[serde(default)]
    pub year_filter: Option<i32>,  // Optional year to filter flows by, None means show all years
    #[serde(default)]
    pub backup_history: Vec<BackupEntry>,  // History of backup operations
    #[serde(default)]
    pub last_backup_path: Option<String>,  // Path of the last successful backup
    #[serde(default)]
    pub auto_backup_enabled: bool,  // Whether automatic backups are enabled
    #[serde(default)]
    pub auto_backup_directory: Option<String>,  // Directory for automatic backups
    #[serde(default)]
    pub auto_backup_encrypted: Option<bool>,  // Whether automatic backups should be encrypted (None = use default)
    // Future settings can be added here, such as:
    // - preferred date format
    // - default currency
    // - theme preferences
    // - notification settings
    // - etc.
}

impl UserSettings {
    pub fn new() -> Self {
        Self {
            hidden_categories: HashSet::new(),
            year_filter: Some(chrono::Local::now().year()),  // Default to current year
            backup_history: Vec::new(),
            last_backup_path: None,
            auto_backup_enabled: false,
            auto_backup_directory: None,
            auto_backup_encrypted: None,
        }
    }

    pub fn is_category_hidden(&self, category_id: &str) -> bool {
        self.hidden_categories.contains(category_id)
    }

    pub fn toggle_category_visibility(&mut self, category_id: String) {
        if self.hidden_categories.contains(&category_id) {
            self.hidden_categories.remove(&category_id);
        } else {
            self.hidden_categories.insert(category_id);
        }
    }

    pub fn set_year_filter(&mut self, year: Option<i32>) {
        self.year_filter = year;
    }

    pub fn get_year_filter(&self) -> Option<i32> {
        self.year_filter
    }

    pub fn add_backup_entry(&mut self, entry: BackupEntry) {
        // Keep only the last 10 backup entries
        if self.backup_history.len() >= 10 {
            self.backup_history.remove(0);
        }
        self.backup_history.push(entry);
    }

    pub fn get_last_successful_backup(&self) -> Option<&BackupEntry> {
        self.backup_history.iter().rev().find(|entry| entry.success)
    }

    pub fn set_last_backup_path(&mut self, path: String) {
        self.last_backup_path = Some(path);
    }

    pub fn set_auto_backup_enabled(&mut self, enabled: bool) {
        self.auto_backup_enabled = enabled;
    }

    pub fn is_auto_backup_enabled(&self) -> bool {
        self.auto_backup_enabled
    }

    pub fn set_auto_backup_directory(&mut self, directory: Option<String>) {
        self.auto_backup_directory = directory;
    }

    pub fn get_auto_backup_directory(&self) -> Option<&String> {
        self.auto_backup_directory.as_ref()
    }

    pub fn set_auto_backup_encrypted(&mut self, encrypted: Option<bool>) {
        self.auto_backup_encrypted = encrypted;
    }

    pub fn get_auto_backup_encrypted(&self) -> Option<bool> {
        self.auto_backup_encrypted
    }
} 