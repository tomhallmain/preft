//! Category CRUD round trips against an isolated `Database::new_for_test` instance.
//!
//! These specifically cover the scenarios that were previously blocked by a bug
//! in `Database::get_category`/`save_category`: saving a *brand-new* category
//! (id not already in the table) used to error out unconditionally, so none of
//! this was testable until that was fixed.

use chrono::NaiveDate;
use preft::db::Database;
use preft::models::{Category, CategoryField, FieldType, Flow, FlowType, TaxDeductionInfo};
use rusqlite::Connection;
use std::collections::HashMap;

fn test_db() -> Database {
    Database::new_for_test(Connection::open_in_memory().expect("open in-memory db"))
        .expect("initialize test db")
}

fn category_with_fields(id: &str, fields: Vec<CategoryField>) -> Category {
    Category {
        id: id.to_string(),
        name: format!("Category {}", id),
        flow_type: FlowType::Expense,
        parent_id: None,
        fields,
        tax_deduction: TaxDeductionInfo { deduction_allowed: false, default_value: false },
    }
}

fn flow_with_custom_fields(id: &str, category_id: &str, custom_fields: HashMap<String, String>) -> Flow {
    Flow {
        id: id.to_string(),
        date: NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(),
        amount: 10.0,
        category_id: category_id.to_string(),
        description: String::new(),
        linked_flows: Vec::new(),
        custom_fields,
        tax_deductible: None,
    }
}

#[test]
fn save_category_inserts_a_brand_new_category() {
    let mut db = test_db();
    let category = category_with_fields("new-cat", vec![]);

    db.save_category(&category)
        .expect("saving a brand-new category should succeed");

    let loaded = db.load_categories().expect("load categories");
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].id, "new-cat");
    assert_eq!(loaded[0].name, "Category new-cat");
}

#[test]
fn save_category_round_trips_all_field_types() {
    let mut db = test_db();
    #[allow(deprecated)]
    let fields = vec![
        CategoryField { name: "notes".to_string(), field_type: FieldType::Text, required: true, default_value: None },
        CategoryField { name: "cost".to_string(), field_type: FieldType::Currency, required: true, default_value: None },
        CategoryField { name: "covered".to_string(), field_type: FieldType::Boolean, required: false, default_value: Some("false".to_string()) },
        CategoryField { name: "kind".to_string(), field_type: FieldType::Select(vec!["A".to_string(), "B".to_string()]), required: true, default_value: None },
        CategoryField { name: "legacy".to_string(), field_type: FieldType::Number, required: false, default_value: None },
        CategoryField { name: "count".to_string(), field_type: FieldType::Integer, required: false, default_value: None },
        CategoryField { name: "ratio".to_string(), field_type: FieldType::Float, required: false, default_value: None },
        CategoryField { name: "when".to_string(), field_type: FieldType::Date, required: false, default_value: None },
    ];
    let category = category_with_fields("field-types", fields.clone());
    db.save_category(&category).expect("save category");

    let loaded = db.load_categories().expect("load categories");
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].fields, fields);
}

#[test]
fn save_category_update_preserves_existing_id_and_changes_name() {
    let mut db = test_db();
    let category = category_with_fields("stable-cat", vec![]);
    db.save_category(&category).expect("initial save");

    let mut renamed = category.clone();
    renamed.name = "Renamed".to_string();
    db.save_category(&renamed).expect("update save");

    let loaded = db.load_categories().expect("load categories");
    assert_eq!(loaded.len(), 1, "update should replace, not duplicate, the category");
    assert_eq!(loaded[0].name, "Renamed");
}

#[test]
fn save_category_update_without_schema_change_leaves_flows_untouched() {
    let mut db = test_db();
    let fields = vec![CategoryField {
        name: "amount".to_string(),
        field_type: FieldType::Currency,
        required: true,
        default_value: None,
    }];
    let category = category_with_fields("stable-fields-cat", fields);
    db.save_category(&category).expect("initial save");

    let mut custom_fields = HashMap::new();
    custom_fields.insert("amount".to_string(), "$10.00".to_string());
    let flow = flow_with_custom_fields("flow-1", "stable-fields-cat", custom_fields);
    db.save_flow(&flow).expect("save flow");

    // Re-save the same category with only the name changed -- no field schema change.
    let mut renamed = category.clone();
    renamed.name = "Renamed".to_string();
    db.save_category(&renamed).expect("update save");

    let flows = db.load_flows().expect("load flows");
    assert_eq!(
        flows[0].custom_fields.get("amount"),
        Some(&"$10.00".to_string()),
        "flow custom fields should be untouched when the category schema didn't change"
    );
}

#[test]
fn save_category_schema_change_migrates_existing_flows() {
    let mut db = test_db();
    let original_fields = vec![
        CategoryField { name: "amount".to_string(), field_type: FieldType::Currency, required: true, default_value: None },
        CategoryField { name: "old_field".to_string(), field_type: FieldType::Text, required: false, default_value: None },
    ];
    let category = category_with_fields("evolving-cat", original_fields);
    db.save_category(&category).expect("initial save");

    let mut custom_fields = HashMap::new();
    custom_fields.insert("amount".to_string(), "$10.00".to_string());
    custom_fields.insert("old_field".to_string(), "some text".to_string());
    let flow = flow_with_custom_fields("flow-1", "evolving-cat", custom_fields);
    db.save_flow(&flow).expect("save flow");

    // Remove `old_field` from the category schema -- this is a schema change.
    let mut updated = category.clone();
    updated.fields.retain(|f| f.name != "old_field");
    db.save_category(&updated).expect("schema-changing save");

    let flows = db.load_flows().expect("load flows");
    let migrated_flow = flows.iter().find(|f| f.id == "flow-1").unwrap();
    assert!(
        !migrated_flow.custom_fields.contains_key("old_field"),
        "field removed from the category schema should be stripped from existing flows"
    );
    assert_eq!(
        migrated_flow.custom_fields.get("amount"),
        Some(&"10.00".to_string()),
        "surviving Currency fields are normalized (symbols/commas stripped) whenever a schema-changing save runs a migration"
    );
}

#[test]
fn delete_category_removes_it_from_load_categories() {
    let mut db = test_db();
    let category = category_with_fields("to-delete", vec![]);
    db.save_category(&category).expect("save category");
    assert_eq!(db.load_categories().unwrap().len(), 1);

    db.delete_category("to-delete").expect("delete category");
    assert!(db.load_categories().unwrap().is_empty());
}

#[test]
fn multiple_new_categories_can_be_saved_independently() {
    let mut db = test_db();
    db.save_category(&category_with_fields("cat-a", vec![])).expect("save cat-a");
    db.save_category(&category_with_fields("cat-b", vec![])).expect("save cat-b");

    let mut loaded_ids: Vec<String> = db.load_categories().unwrap().into_iter().map(|c| c.id).collect();
    loaded_ids.sort();
    assert_eq!(loaded_ids, vec!["cat-a".to_string(), "cat-b".to_string()]);
}
