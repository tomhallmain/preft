use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use chrono::{self, Datelike};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UserSettings {
    pub hidden_categories: HashSet<String>,  // Set of category IDs that are hidden
    pub year_filter: Option<i32>,  // Optional year to filter flows by, None means show all years
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
} 