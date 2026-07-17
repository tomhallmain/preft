//! Backup/restore/dump round trips for `Database`, using `tempfile` so nothing
//! touches the user's real `~/.preft` directory.
//!
//! Two tests here (`restore_from_file_on_encrypted_backup_silently_resets_settings_to_defaults`
//! and `restore_from_sql_file_fails_against_an_already_initialized_database`)
//! intentionally assert *current, real* buggy behavior rather than desired
//! behavior. They exist to document and regression-lock what the code
//! actually does today; see the comments on each for the root cause. Per
//! request, these are not being fixed in this pass.

use preft::db::Database;
use preft::encryption::DatabaseEncryption;
use preft::models::{Category, CategoryField, FlowType, TaxDeductionInfo};
use preft::settings::UserSettings;
use rusqlite::Connection;
use std::collections::HashMap;
use std::io::Write;

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

// --- encrypt_data/decrypt_data (indirectly, via the public save/load API) ---

#[test]
fn user_settings_round_trip_through_encryption() {
    let salt = DatabaseEncryption::generate_salt();
    let mut db = test_db();
    db.enable_encryption_for_test("s3cret", &salt).expect("set up encryption");
    assert!(db.is_encrypted());

    let mut settings = UserSettings::new();
    settings.set_year_filter(Some(2019));
    db.save_user_settings(&settings).expect("save settings");

    let loaded = db.load_user_settings().expect("load settings");
    assert_eq!(loaded.get_year_filter(), Some(2019));
}

// --- backup_to_file / restore_from_file (binary rusqlite backup) ---

#[test]
fn unencrypted_backup_and_restore_round_trip() {
    let mut db1 = test_db();
    db1.save_category(&category_with_fields("cat-1", vec![])).expect("save category");

    let mut custom_fields = HashMap::new();
    custom_fields.insert("note".to_string(), "hello".to_string());
    let flow = preft::models::Flow {
        id: "flow-1".to_string(),
        date: chrono::NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(),
        amount: 42.0,
        category_id: "cat-1".to_string(),
        description: "Test flow".to_string(),
        linked_flows: Vec::new(),
        custom_fields,
        tax_deductible: Some(true),
    };
    db1.save_flow(&flow).expect("save flow");

    let mut settings = UserSettings::new();
    settings.set_year_filter(Some(2022));
    db1.save_user_settings(&settings).expect("save settings");

    let backup_dir = tempfile::tempdir().expect("create tempdir");
    let backup_path = backup_dir.path().join("backup.db");
    db1.backup_to_file(&backup_path, false).expect("unencrypted backup should succeed");

    let mut db2 = test_db(); // fresh, empty
    db2.restore_from_file(&backup_path, None, false).expect("restore should succeed");

    let categories = db2.load_categories().expect("load categories");
    assert_eq!(categories.len(), 1);
    assert_eq!(categories[0].id, "cat-1");

    let flows = db2.load_flows().expect("load flows");
    assert_eq!(flows.len(), 1);
    assert_eq!(flows[0].custom_fields.get("note"), Some(&"hello".to_string()));
    assert_eq!(flows[0].tax_deductible, Some(true));

    let loaded_settings = db2.load_user_settings().expect("load settings");
    assert_eq!(loaded_settings.get_year_filter(), Some(2022));
}

#[test]
fn backup_to_file_encrypted_errors_when_database_not_encrypted() {
    let db = test_db();
    let backup_dir = tempfile::tempdir().expect("create tempdir");
    let backup_path = backup_dir.path().join("backup.db");

    let result = db.backup_to_file(&backup_path, true);
    assert!(result.is_err());
}

#[test]
fn backup_to_file_encrypted_succeeds_when_database_is_encrypted() {
    let salt = DatabaseEncryption::generate_salt();
    let mut db = test_db();
    db.enable_encryption_for_test("s3cret", &salt).expect("set up encryption");

    let backup_dir = tempfile::tempdir().expect("create tempdir");
    let backup_path = backup_dir.path().join("backup.db");

    db.backup_to_file(&backup_path, true).expect("encrypted backup should succeed");
    assert!(backup_path.exists());
    assert!(std::fs::metadata(&backup_path).unwrap().len() > 0);
}

#[test]
fn detect_encrypted_backup_false_for_valid_unencrypted_backup() {
    let db1 = test_db();
    let backup_dir = tempfile::tempdir().expect("create tempdir");
    let backup_path = backup_dir.path().join("backup.db");
    db1.backup_to_file(&backup_path, false).expect("backup should succeed");

    assert_eq!(db1.detect_encrypted_backup(&backup_path).unwrap(), false);
}

#[test]
fn detect_encrypted_backup_true_for_malformed_file() {
    let db = test_db();
    let backup_dir = tempfile::tempdir().expect("create tempdir");
    let garbage_path = backup_dir.path().join("garbage.db");
    let mut file = std::fs::File::create(&garbage_path).expect("create garbage file");
    file.write_all(b"this is not a sqlite database").expect("write garbage bytes");
    drop(file);

    assert_eq!(db.detect_encrypted_backup(&garbage_path).unwrap(), true);
}

#[test]
fn restore_from_file_on_encrypted_backup_silently_resets_settings_to_defaults() {
    // Known limitation, not desired behavior: this app only encrypts the
    // *value* of the `user_settings.settings_json` column (AES-GCM+base64),
    // never the database file itself. `detect_encrypted_backup` decides
    // "encrypted or not" by running `SELECT COUNT(*) FROM user_settings`,
    // which succeeds either way -- the column is still valid TEXT whether its
    // content is readable JSON or ciphertext. So it always reports `false`
    // for backups this app produces, even ones made via the encrypted path.
    // As a result `restore_from_file` always takes the "unencrypted" restore
    // path, ignoring any password passed in, and writes the raw ciphertext
    // into the target as if it were plaintext JSON. `load_user_settings`
    // then fails to parse it and *silently* falls back to `UserSettings::new()`
    // instead of surfacing an error. This test locks in that current,
    // reproducible behavior so a future fix will need to update it.
    let salt = DatabaseEncryption::generate_salt();
    let mut db1 = test_db();
    db1.enable_encryption_for_test("s3cret", &salt).expect("set up encryption");

    let mut settings = UserSettings::new();
    settings.set_year_filter(Some(2021));
    db1.save_user_settings(&settings).expect("save settings");

    let backup_dir = tempfile::tempdir().expect("create tempdir");
    let backup_path = backup_dir.path().join("backup.db");
    db1.backup_to_file(&backup_path, true).expect("encrypted backup should succeed");

    // Finding #1, asserted directly: detection misreports this as unencrypted.
    assert_eq!(db1.detect_encrypted_backup(&backup_path).unwrap(), false);

    let mut db2 = test_db(); // unencrypted target
    db2.restore_from_file(&backup_path, Some("s3cret"), false)
        .expect("the restore call itself does not return an error");

    // Finding #2: the restored settings are silently the defaults, not the
    // originally-saved (year_filter = Some(2021)) values.
    let restored = db2.load_user_settings().expect("load settings");
    assert_eq!(
        restored.get_year_filter(),
        UserSettings::new().get_year_filter(),
        "settings were silently reset to defaults instead of being correctly decrypted"
    );
}

// --- dump_to_sql_file / restore_from_sql_file ---

#[test]
fn dump_to_sql_file_produces_expected_inserts_with_escaped_quotes() {
    let mut db = test_db();
    db.save_category(&category_with_fields("cat-1", vec![])).expect("save category");

    let flow = preft::models::Flow {
        id: "flow-1".to_string(),
        date: chrono::NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(),
        amount: 12.5,
        category_id: "cat-1".to_string(),
        description: "Tom's Coffee".to_string(),
        linked_flows: Vec::new(),
        custom_fields: HashMap::new(),
        tax_deductible: None,
    };
    db.save_flow(&flow).expect("save flow");

    let dump_dir = tempfile::tempdir().expect("create tempdir");
    let dump_path = dump_dir.path().join("dump.sql");
    db.dump_to_sql_file(&dump_path).expect("dump should succeed");

    let dump_content = std::fs::read_to_string(&dump_path).expect("read dump file");
    assert!(dump_content.contains("CREATE TABLE"), "dump should contain table schemas");
    assert!(dump_content.contains("INSERT INTO categories"));
    assert!(dump_content.contains("INSERT INTO flows"));
    assert!(
        dump_content.contains("'Tom''s Coffee'"),
        "embedded single quotes should be escaped by doubling, per the dump's own escaping rule"
    );
}

#[test]
fn restore_from_sql_file_fails_against_an_already_initialized_database() {
    // Known limitation, not desired behavior: unlike `restore_unencrypted`
    // (the binary backup path), `restore_from_sql_file` never clears existing
    // rows before replaying the dumped INSERT statements -- it just executes
    // whatever schema/data statements were dumped. Every
    // `Database::new()`/`new_for_test()` instance already has exactly one
    // bookkeeping row in the `migrations` table (id=1, from its own
    // `run_migrations()` call during construction), so replaying a dump taken
    // from another such database always collides on that primary key, even
    // with zero categories/flows involved. This test locks in that current,
    // reproducible behavior so a future fix will need to update it.
    let db1 = test_db();
    let dump_dir = tempfile::tempdir().expect("create tempdir");
    let dump_path = dump_dir.path().join("dump.sql");
    db1.dump_to_sql_file(&dump_path).expect("dump should succeed");

    let mut db2 = test_db(); // already has its own migrations bookkeeping row
    let result = db2.restore_from_sql_file(&dump_path);
    assert!(
        result.is_err(),
        "restoring into an already-initialized database currently fails on a migrations-table primary key collision"
    );
}
