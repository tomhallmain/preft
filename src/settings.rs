use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UserSettings {
    pub hidden_categories: HashSet<String>,  // Set of category IDs that are hidden
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
} 