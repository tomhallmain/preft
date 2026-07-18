//! `Database::is_dirty` -- whether any *financial* data (flows/categories)
//! has changed since construction. Used by `PreftApp::on_exit` to skip the
//! automatic backup when nothing worth backing up changed this session.
//! Deliberately excludes `UserSettings` (year filter, hidden categories,
//! backup bookkeeping, ...) -- see `save_user_settings_does_not_mark_dirty`.

use chrono::NaiveDate;
use preft::db::Database;
use preft::models::{Category, FlowType, Flow, TaxDeductionInfo};
use preft::settings::UserSettings;
use rusqlite::Connection;
use std::collections::HashMap;

fn test_db() -> Database {
    Database::new_for_test(Connection::open_in_memory().expect("open in-memory db"))
        .expect("initialize test db")
}

fn category(id: &str) -> Category {
    Category {
        id: id.to_string(),
        name: format!("Category {}", id),
        flow_type: FlowType::Expense,
        parent_id: None,
        fields: vec![],
        tax_deduction: TaxDeductionInfo { deduction_allowed: false, default_value: false },
    }
}

fn flow(id: &str, category_id: &str) -> Flow {
    Flow {
        id: id.to_string(),
        date: NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(),
        amount: 10.0,
        category_id: category_id.to_string(),
        description: String::new(),
        linked_flows: Vec::new(),
        custom_fields: HashMap::new(),
        tax_deductible: None,
    }
}

#[test]
fn fresh_database_is_not_dirty() {
    let db = test_db();
    assert!(!db.is_dirty());
}

#[test]
fn save_category_marks_dirty() {
    let mut db = test_db();
    db.save_category(&category("cat-1")).expect("save category");
    assert!(db.is_dirty());
}

#[test]
fn save_flow_marks_dirty() {
    let mut db = test_db();
    db.save_category(&category("cat-1")).expect("save category");
    db.save_flow(&flow("flow-1", "cat-1")).expect("save flow");
    assert!(db.is_dirty());
}

#[test]
fn delete_flow_marks_dirty() {
    let mut db = test_db();
    db.save_category(&category("cat-1")).expect("save category");
    db.save_flow(&flow("flow-1", "cat-1")).expect("save flow");
    db.delete_flow("flow-1").expect("delete flow");
    assert!(db.is_dirty());
}

#[test]
fn delete_category_marks_dirty() {
    let mut db = test_db();
    db.save_category(&category("cat-1")).expect("save category");
    db.delete_category("cat-1").expect("delete category");
    assert!(db.is_dirty());
}

#[test]
fn save_user_settings_does_not_mark_dirty() {
    // UserSettings covers UI/app preferences (year filter, hidden
    // categories, backup directory, backup history, ...), not financial
    // records -- and every real call site is a routine preference change
    // (e.g. switching the main window's year filter), so treating it as
    // "dirty" would trigger a full automatic backup on the next exit just
    // from browsing the UI.
    let db = test_db();
    db.save_user_settings(&UserSettings::new()).expect("save user settings");
    assert!(!db.is_dirty());
}

#[test]
fn loading_data_does_not_mark_dirty() {
    let db = test_db();
    let _ = db.load_categories().expect("load categories");
    let _ = db.load_flows().expect("load flows");
    assert!(!db.is_dirty());
}
