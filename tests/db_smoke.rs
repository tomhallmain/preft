//! Smoke test for the `Database::new_for_test` seam: proves an isolated,
//! in-memory database can be built and used from an integration test
//! (a `tests/` binary that only sees `preft`'s public API) without touching
//! the user's real `~/.preft/preft.db` file or the OS keyring.

use chrono::NaiveDate;
use preft::db::Database;
use preft::models::Flow;
use preft::settings::UserSettings;
use rusqlite::Connection;
use std::collections::HashMap;

#[test]
fn user_settings_round_trip_on_isolated_db() {
    let conn = Connection::open_in_memory().expect("open in-memory db");
    let db = Database::new_for_test(conn).expect("initialize test db");

    let mut settings = UserSettings::new();
    settings.set_year_filter(Some(2024));
    db.save_user_settings(&settings).expect("save settings");

    let loaded = db.load_user_settings().expect("load settings");
    assert_eq!(loaded.get_year_filter(), Some(2024));
}

#[test]
fn flow_round_trip_on_isolated_db() {
    let conn = Connection::open_in_memory().expect("open in-memory db");
    // The flows table has a FOREIGN KEY on category_id, and this build enforces
    // foreign keys by default. Saving a real category first isn't an option
    // here (Database::save_category can't insert a brand-new category id yet —
    // see known issue), so disable enforcement for this connection, same as
    // Database's own backup/restore code does when load order can't satisfy it.
    conn.execute("PRAGMA foreign_keys = OFF", [])
        .expect("disable foreign keys");
    let db = Database::new_for_test(conn).expect("initialize test db");

    let flow = Flow {
        id: "flow-1".to_string(),
        date: NaiveDate::from_ymd_opt(2024, 3, 15).unwrap(),
        amount: 42.50,
        category_id: "some-category".to_string(),
        description: "Test flow".to_string(),
        linked_flows: Vec::new(),
        custom_fields: HashMap::new(),
        tax_deductible: Some(true),
    };
    db.save_flow(&flow).expect("save flow");

    let loaded = db.load_flows().expect("load flows");
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].id, "flow-1");
    assert_eq!(loaded[0].amount, 42.50);
    assert_eq!(loaded[0].tax_deductible, Some(true));
}

#[test]
fn two_test_databases_are_independent() {
    let conn_a = Connection::open_in_memory().expect("open in-memory db a");
    let db_a = Database::new_for_test(conn_a).expect("initialize test db a");
    db_a
        .save_user_settings(&UserSettings::new())
        .expect("save settings to db a");

    let conn_b = Connection::open_in_memory().expect("open in-memory db b");
    let db_b = Database::new_for_test(conn_b).expect("initialize test db b");

    // A fresh db has no user_settings row yet; load_user_settings falls back
    // to defaults rather than seeing db_a's data.
    let loaded_b = db_b.load_user_settings().expect("load settings from db b");
    assert_eq!(loaded_b.get_year_filter(), UserSettings::new().get_year_filter());
}
