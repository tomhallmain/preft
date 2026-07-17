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
        // Keep only the last 100 backup entries
        if self.backup_history.len() >= 100 {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn backup_entry(file_path: &str, success: bool) -> BackupEntry {
        BackupEntry {
            timestamp: Utc::now(),
            file_path: file_path.to_string(),
            file_size: None,
            success,
            error_message: None,
        }
    }

    #[test]
    fn new_defaults_to_current_year_filter_and_empty_state() {
        let settings = UserSettings::new();
        assert_eq!(settings.get_year_filter(), Some(chrono::Local::now().year()));
        assert!(settings.backup_history.is_empty());
        assert!(!settings.is_auto_backup_enabled());
        assert_eq!(settings.get_auto_backup_directory(), None);
    }

    #[test]
    fn toggle_category_visibility_round_trips() {
        let mut settings = UserSettings::new();
        assert!(!settings.is_category_hidden("groceries"));

        settings.toggle_category_visibility("groceries".to_string());
        assert!(settings.is_category_hidden("groceries"));

        settings.toggle_category_visibility("groceries".to_string());
        assert!(!settings.is_category_hidden("groceries"));
    }

    #[test]
    fn year_filter_round_trips() {
        let mut settings = UserSettings::new();
        settings.set_year_filter(Some(2020));
        assert_eq!(settings.get_year_filter(), Some(2020));

        settings.set_year_filter(None);
        assert_eq!(settings.get_year_filter(), None);
    }

    #[test]
    fn backup_history_caps_at_100_entries_evicting_oldest_first() {
        let mut settings = UserSettings::new();
        for i in 0..101 {
            settings.add_backup_entry(backup_entry(&format!("backup_{}", i), true));
        }

        assert_eq!(settings.backup_history.len(), 100);
        assert_eq!(settings.backup_history.first().unwrap().file_path, "backup_1");
        assert_eq!(settings.backup_history.last().unwrap().file_path, "backup_100");
    }

    #[test]
    fn get_last_successful_backup_skips_trailing_failures() {
        let mut settings = UserSettings::new();
        settings.add_backup_entry(backup_entry("ok_1", true));
        settings.add_backup_entry(backup_entry("fail_1", false));
        settings.add_backup_entry(backup_entry("ok_2", true));
        settings.add_backup_entry(backup_entry("fail_2", false));

        let last_success = settings.get_last_successful_backup().unwrap();
        assert_eq!(last_success.file_path, "ok_2");
    }

    #[test]
    fn get_last_successful_backup_none_when_no_successes() {
        let mut settings = UserSettings::new();
        settings.add_backup_entry(backup_entry("fail_1", false));
        assert!(settings.get_last_successful_backup().is_none());
    }
} 