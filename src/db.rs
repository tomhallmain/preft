use anyhow::Result;
use rusqlite::{Connection, params, types::FromSql, types::ValueRef, types::FromSqlError, types::Type};
use chrono::NaiveDate;
use crate::models::{Flow, Category, FlowType, TaxDeductionInfo, CategoryField, get_default_categories};
use crate::settings::UserSettings;
use crate::encryption::DatabaseEncryption;
use crate::encryption_config::EncryptionConfig;
use log::info;
use log::error;
use std::path::Path;
mod migrations;

pub struct Database {
    conn: Connection,
    encryption: Option<DatabaseEncryption>,
    encryption_config: EncryptionConfig,
}

impl Database {
    pub fn new() -> Result<Self> {
        // Load encryption configuration from OS keystore
        let encryption_config = EncryptionConfig::load()
            .unwrap_or_else(|_| EncryptionConfig::default());
        
        // Get the user's home directory
        let home_dir = dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not find home directory"))?;
        
        // Create the app directory if it doesn't exist
        let app_dir = home_dir.join(".preft");
        std::fs::create_dir_all(&app_dir)?;
        
        // Open or create the database file
        let db_path = app_dir.join("preft.db");
        let conn = Connection::open(db_path)?;
        
        // Initialize the database
        let mut db = Database { conn, encryption: None, encryption_config };
        db.initialize()?;
        
        // Run migrations
        migrations::run_migrations(&mut db.conn)?;
        
        // Check if we have any categories, if not, save the defaults
        let count: i64 = db.conn.query_row("SELECT COUNT(*) FROM categories", [], |row| row.get(0))?;
        if count == 0 {
            for category in get_default_categories() {
                db.save_category(&category)?;
            }
        }

        // Initialize user settings if they don't exist
        let settings_count: i64 = db.conn.query_row("SELECT COUNT(*) FROM user_settings", [], |row| row.get(0))?;
        if settings_count == 0 {
            db.save_user_settings(&UserSettings::new())?;
        }
        
        Ok(db)
    }

    /// Create a new database with minimal initialization (for error recovery)
    pub fn new_minimal() -> Result<Self> {
        // Load encryption configuration from OS keystore
        let encryption_config = EncryptionConfig::load()
            .unwrap_or_else(|_| EncryptionConfig::default());
        
        // Get the user's home directory
        let home_dir = dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not find home directory"))?;
        
        // Create the app directory if it doesn't exist
        let app_dir = home_dir.join(".preft");
        std::fs::create_dir_all(&app_dir)?;
        
        // Open or create the database file
        let db_path = app_dir.join("preft.db");
        let conn = Connection::open(db_path)?;
        
        // Initialize the database with just the basic tables
        let mut db = Database { conn, encryption: None, encryption_config };
        db.initialize()?;
        
        Ok(db)
    }

    /// Create a database from an existing connection (for error recovery)
    pub fn from_connection(conn: Connection) -> Self {
        let encryption_config = EncryptionConfig::load()
            .unwrap_or_else(|_| EncryptionConfig::default());
        Database { conn, encryption: None, encryption_config }
    }

    /// Check if the database is encrypted by attempting to read a test value
    pub fn detect_encryption_state(&self) -> bool {
        // Try to read from user_settings table - if it fails with a specific error,
        // the database might be encrypted
        match self.conn.query_row("SELECT COUNT(*) FROM user_settings", [], |row| row.get::<_, i64>(0)) {
            Ok(_) => false, // Successfully read, likely unencrypted
            Err(_) => {
                // Check if the error suggests encryption
                // This is a simplified check - in practice you might want more sophisticated detection
                false // For now, assume unencrypted
            }
        }
    }

    /// Initialize encryption for the database
    pub fn initialize_encryption(&mut self, password: &str) -> Result<()> {
        if self.encryption_config.is_encryption_ready() {
            return Ok(()); // Already encrypted
        }

        // Set password in encryption config (this will generate salt and hash)
        self.encryption_config.set_password(password)?;
        
        // Create encryption instance
        let salt = self.encryption_config.get_salt()
            .ok_or_else(|| anyhow::anyhow!("Salt not found after setting password"))?;
        let encryption = DatabaseEncryption::new(password, salt)?;
        
        // Test encryption by encrypting and decrypting a test value
        let test_data = "encryption_test";
        let encrypted = encryption.encrypt(test_data)?;
        let decrypted = encryption.decrypt(&encrypted)?;
        
        if decrypted != test_data {
            return Err(anyhow::anyhow!("Encryption test failed"));
        }

        self.encryption = Some(encryption);
        
        info!("Database encryption initialized successfully");
        Ok(())
    }

    /// Set encryption state (for loading from settings)
    pub fn set_encryption_state(&mut self, enabled: bool, password: Option<&str>, salt: Option<&str>) -> Result<()> {
        if enabled {
            if let (Some(pwd), Some(salt_val)) = (password, salt) {
                let encryption = DatabaseEncryption::new(pwd, salt_val)?;
                self.encryption = Some(encryption);
            } else {
                return Err(anyhow::anyhow!("Password and salt required for encryption"));
            }
        } else {
            self.encryption = None;
        }
        Ok(())
    }

    /// Check if encryption is currently enabled
    pub fn is_encrypted(&self) -> bool {
        self.encryption_config.is_encryption_ready()
    }

    /// Encrypt sensitive data if encryption is enabled
    fn encrypt_data(&self, data: &str) -> Result<String> {
        if let Some(encryption) = &self.encryption {
            encryption.encrypt(data)
        } else {
            Ok(data.to_string()) // No encryption, return as-is
        }
    }

    /// Decrypt sensitive data if encryption is enabled
    fn decrypt_data(&self, data: &str) -> Result<String> {
        if let Some(encryption) = &self.encryption {
            encryption.decrypt(data)
        } else {
            Ok(data.to_string()) // No encryption, return as-is
        }
    }

    fn initialize(&self) -> Result<()> {
        // Create tables if they don't exist
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS categories (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                flow_type TEXT NOT NULL,
                fields TEXT NOT NULL,
                tax_deduction_allowed INTEGER NOT NULL,
                tax_deduction_default INTEGER NOT NULL
            )",
            [],
        )?;

        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS flows (
                id TEXT PRIMARY KEY,
                date TEXT NOT NULL,
                amount REAL NOT NULL,
                category_id TEXT NOT NULL,
                description TEXT NOT NULL,
                linked_flows TEXT NOT NULL,
                custom_fields TEXT NOT NULL,
                tax_deductible INTEGER,
                FOREIGN KEY (category_id) REFERENCES categories(id)
            )",
            [],
        )?;

        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS user_settings (
                id INTEGER PRIMARY KEY,
                settings_json TEXT NOT NULL
            )",
            [],
        )?;

        Ok(())
    }

    pub fn save_user_settings(&self, settings: &UserSettings) -> Result<()> {
        let settings_json = serde_json::to_string(settings)?;
        
        // Encrypt the settings if encryption is enabled
        let encrypted_json = self.encrypt_data(&settings_json)?;
        
        self.conn.execute(
            "INSERT OR REPLACE INTO user_settings (id, settings_json)
             VALUES (1, ?1)",
            params![encrypted_json],
        )?;

        Ok(())
    }

    pub fn load_user_settings(&self) -> Result<UserSettings> {
        let result = self.conn.query_row(
            "SELECT settings_json FROM user_settings WHERE id = 1",
            [],
            |row| row.get::<_, String>(0),
        );

        match result {
            Ok(encrypted_json) => {
                // Decrypt the settings if encryption is enabled
                let decrypted_json = self.decrypt_data(&encrypted_json)?;
                
                // Try to deserialize with backward compatibility
                match serde_json::from_str::<UserSettings>(&decrypted_json) {
                    Ok(settings) => Ok(settings),
                    Err(e) => {
                        // If deserialization fails, try to create default settings
                        eprintln!("Warning: Failed to deserialize user settings: {}", e);
                        eprintln!("This might be due to an old database format. Using default settings.");
                        Ok(UserSettings::new())
                    }
                }
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                // No user settings exist yet, return default settings
                Ok(UserSettings::new())
            }
            Err(e) => Err(e.into()),
        }
    }

    fn get_category(conn: &Connection, category_id: &str) -> Result<Option<Category>> {
        let mut stmt = conn.prepare("SELECT id, name, flow_type, fields, tax_deduction_allowed, tax_deduction_default FROM categories WHERE id = ?")?;
        let result = stmt.query_row(params![category_id], |row| {
            let id: String = row.get(0)?;
            let name: String = row.get(1)?;
            let flow_type_str: String = row.get(2)?;
            let fields_json: String = row.get(3)?;
            let tax_deduction_allowed: i64 = row.get(4)?;
            let tax_deduction_default: i64 = row.get(5)?;
            
            let flow_type = match flow_type_str.as_str() {
                "Income" => FlowType::Income,
                "Expense" => FlowType::Expense,
                _ => return Err(rusqlite::Error::InvalidParameterName(format!("Invalid flow type: {}", flow_type_str))),
            };
            
            let fields: Vec<CategoryField> = serde_json::from_str(&fields_json)
                .map_err(|e| rusqlite::Error::InvalidParameterName(e.to_string()))?;
            
            Ok(Category {
                id,
                name,
                flow_type,
                parent_id: None,
                fields,
                tax_deduction: TaxDeductionInfo {
                    deduction_allowed: tax_deduction_allowed != 0,
                    default_value: tax_deduction_default != 0,
                },
            })
        });

        match result {
            Ok(category) => Ok(Some(category)),
            Err(rusqlite::Error::ExecuteReturnedResults) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn save_category(&mut self, category: &Category) -> Result<()> {
        // Start transaction
        let tx = self.conn.transaction()?;

        // Get the old category before making any changes
        let old_category = Self::get_category(&tx, &category.id)?
            .ok_or_else(|| anyhow::anyhow!("Category not found: {}", category.id))?;

        // Save the category
        let fields_json = serde_json::to_string(&category.fields)?;
        tx.execute(
            "INSERT OR REPLACE INTO categories (id, name, flow_type, fields, tax_deduction_allowed, tax_deduction_default)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                category.id,
                category.name,
                category.flow_type.to_string(),
                fields_json,
                if category.tax_deduction.deduction_allowed { 1 } else { 0 },
                if category.tax_deduction.default_value { 1 } else { 0 }
            ],
        )?;

        // Run migrations if needed
        if migrations::has_schema_changes(&old_category, category) {
            migrations::migrate_flows_to_new_category(&tx, &old_category, category)?;
        }

        // Commit transaction
        tx.commit()?;
        Ok(())
    }

    pub fn save_flow(&self, flow: &Flow) -> Result<()> {
        let linked_flows_json = serde_json::to_string(&flow.linked_flows)?;
        let custom_fields_json = serde_json::to_string(&flow.custom_fields)?;
        
        self.conn.execute(
            "INSERT OR REPLACE INTO flows (id, date, amount, category_id, description, linked_flows, custom_fields, tax_deductible)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                flow.id,
                flow.date.to_string(),
                flow.amount,
                flow.category_id,
                flow.description,
                linked_flows_json,
                custom_fields_json,
                flow.tax_deductible.map(|b| if b { 1 } else { 0 })
            ],
        )?;

        Ok(())
    }

    pub fn load_categories(&self) -> Result<Vec<Category>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, flow_type, fields, tax_deduction_allowed, tax_deduction_default FROM categories"
        )?;

        let categories = stmt.query_map([], |row| {
            let flow_type_str: String = row.get(2)?;
            let flow_type = match flow_type_str.as_str() {
                "Income" => FlowType::Income,
                "Expense" => FlowType::Expense,
                _ => return Err(rusqlite::Error::FromSqlConversionFailure(
                    2,
                    rusqlite::types::Type::Text,
                    Box::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("Invalid flow type: {}", flow_type_str),
                    )),
                )),
            };

            let fields_json: String = row.get(3)?;
            let fields = serde_json::from_str(&fields_json)
                .map_err(|e| rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Text, Box::new(e)))?;

            let tax_deduction_allowed: i64 = row.get(4)?;
            let tax_deduction_default: i64 = row.get(5)?;

            Ok(Category {
                id: row.get(0)?,
                name: row.get(1)?,
                flow_type,
                parent_id: None,
                fields,
                tax_deduction: TaxDeductionInfo {
                    deduction_allowed: tax_deduction_allowed != 0,
                    default_value: tax_deduction_default != 0,
                },
            })
        })?;

        let mut result = Vec::new();
        for category in categories {
            result.push(category?);
        }

        Ok(result)
    }

    pub fn load_flows(&self) -> Result<Vec<Flow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, date, amount, category_id, description, linked_flows, custom_fields, tax_deductible FROM flows"
        )?;

        let flows = stmt.query_map([], |row| {
            let date_str: String = row.get(1)?;
            let date = NaiveDate::parse_from_str(&date_str, "%Y-%m-%d")
                .map_err(|e| rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Text, Box::new(e)))?;

            let linked_flows_json: String = row.get(5)?;
            let linked_flows = serde_json::from_str(&linked_flows_json)
                .map_err(|e| rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, Box::new(e)))?;

            let custom_fields_json: String = row.get(6)?;
            let custom_fields = serde_json::from_str(&custom_fields_json)
                .map_err(|e| rusqlite::Error::FromSqlConversionFailure(6, rusqlite::types::Type::Text, Box::new(e)))?;

            let tax_deductible: Option<i64> = row.get(7)?;
            let tax_deductible = tax_deductible.map(|i| i != 0);

            Ok(Flow {
                id: row.get(0)?,
                date,
                amount: row.get(2)?,
                category_id: row.get(3)?,
                description: row.get(4)?,
                linked_flows,
                custom_fields,
                tax_deductible,
            })
        })?;

        let mut result = Vec::new();
        for flow in flows {
            result.push(flow?);
        }

        Ok(result)
    }

    pub fn delete_category(&self, category_id: &str) -> Result<(), Box<dyn std::error::Error>> {
        // Delete the category
        self.conn.execute(
            "DELETE FROM categories WHERE id = ?",
            params![category_id],
        )?;

        Ok(())
    }

    pub fn delete_flows_by_category(&self, category_id: &str) -> Result<(), Box<dyn std::error::Error>> {
        // Delete all flows for this category
        self.conn.execute(
            "DELETE FROM flows WHERE category_id = ?",
            params![category_id],
        )?;

        Ok(())
    }

    pub fn delete_flow(&self, flow_id: &str) -> Result<(), Box<dyn std::error::Error>> {
        // Delete the flow
        self.conn.execute(
            "DELETE FROM flows WHERE id = ?",
            params![flow_id],
        )?;

        Ok(())
    }

    /// Create a backup of the database to the specified path
    /// 
    /// # Arguments
    /// * `backup_path` - Path where the backup file will be created
    /// * `encrypted_backup` - If true, creates an encrypted backup (requires password)
    ///                        If false, creates an unencrypted backup (for portability)
    pub fn backup_to_file(&self, backup_path: &Path, encrypted_backup: bool) -> Result<()> {
        if encrypted_backup && !self.is_encrypted() {
            return Err(anyhow::anyhow!("Cannot create encrypted backup: database is not encrypted"));
        }

        if encrypted_backup {
            // Create encrypted backup - this preserves the encryption
            self.backup_encrypted(backup_path)
        } else {
            // Create unencrypted backup - decrypt data before backing up
            self.backup_unencrypted(backup_path)
        }
    }

    /// Create an encrypted backup (preserves encryption)
    fn backup_encrypted(&self, backup_path: &Path) -> Result<()> {
        // Create a new connection to the backup file
        let mut backup_conn = Connection::open(backup_path)?;
        
        // Create a backup object
        let backup = rusqlite::backup::Backup::new(&self.conn, &mut backup_conn)?;
        
        // Perform the backup
        backup.run_to_completion(5, std::time::Duration::from_millis(100), Some(|progress| {
            info!("Encrypted backup progress: {} pages", progress.pagecount);
        }))?;
        
        info!("Encrypted database backup completed to: {:?}", backup_path);
        info!("Note: This backup requires the same password as the original database");
        Ok(())
    }

    /// Create an unencrypted backup (decrypts data for portability)
    fn backup_unencrypted(&self, backup_path: &Path) -> Result<()> {
        // Create a new connection to the backup file
        let mut backup_conn = Connection::open(backup_path)?;
        
        // Start a transaction and disable foreign key constraints
        let tx = backup_conn.transaction()?;
        tx.execute("PRAGMA foreign_keys = OFF", [])?;
        
        // Initialize the backup database with the same schema
        self.initialize_backup_database_transaction(&tx)?;
        
        // Copy all data, decrypting as we go
        self.copy_data_unencrypted_transaction(&tx)?;
        
        // Re-enable foreign key constraints
        tx.execute("PRAGMA foreign_keys = ON", [])?;
        
        // Commit the transaction
        tx.commit()?;
        
        info!("Unencrypted database backup completed to: {:?}", backup_path);
        info!("Note: This backup is unencrypted and should be stored securely");
        Ok(())
    }

    /// Initialize the backup database with the same schema within a transaction
    fn initialize_backup_database_transaction(&self, tx: &Connection) -> Result<()> {
        // Create tables with the same schema
        tx.execute(
            "CREATE TABLE IF NOT EXISTS categories (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                flow_type TEXT NOT NULL,
                fields TEXT NOT NULL,
                tax_deduction_allowed INTEGER NOT NULL,
                tax_deduction_default INTEGER NOT NULL
            )",
            [],
        )?;

        tx.execute(
            "CREATE TABLE IF NOT EXISTS flows (
                id TEXT PRIMARY KEY,
                date TEXT NOT NULL,
                amount REAL NOT NULL,
                category_id TEXT NOT NULL,
                description TEXT NOT NULL,
                linked_flows TEXT NOT NULL,
                custom_fields TEXT NOT NULL,
                tax_deductible INTEGER,
                FOREIGN KEY (category_id) REFERENCES categories(id)
            )",
            [],
        )?;

        tx.execute(
            "CREATE TABLE IF NOT EXISTS user_settings (
                id INTEGER PRIMARY KEY,
                settings_json TEXT NOT NULL
            )",
            [],
        )?;

        Ok(())
    }

    /// Copy all data from the encrypted database to the unencrypted backup within a transaction
    fn copy_data_unencrypted_transaction(&self, tx: &Connection) -> Result<()> {
        // Copy categories
        let mut stmt = self.conn.prepare("SELECT * FROM categories")?;
        let categories = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?, // id
                row.get::<_, String>(1)?, // name
                row.get::<_, String>(2)?, // flow_type
                row.get::<_, String>(3)?, // fields
                row.get::<_, i64>(4)?,    // tax_deduction_allowed
                row.get::<_, i64>(5)?,    // tax_deduction_default
            ))
        })?;

        for category in categories {
            let (id, name, flow_type, fields, tax_deduction_allowed, tax_deduction_default) = category?;
            tx.execute(
                "INSERT INTO categories (id, name, flow_type, fields, tax_deduction_allowed, tax_deduction_default)
                 VALUES (?, ?, ?, ?, ?, ?)",
                params![id, name, flow_type, fields, tax_deduction_allowed, tax_deduction_default],
            )?;
        }

        // Copy flows
        let mut stmt = self.conn.prepare("SELECT * FROM flows")?;
        let flows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?, // id
                row.get::<_, String>(1)?, // date
                row.get::<_, f64>(2)?,    // amount
                row.get::<_, String>(3)?, // category_id
                row.get::<_, String>(4)?, // description
                row.get::<_, String>(5)?, // linked_flows
                row.get::<_, String>(6)?, // custom_fields
                row.get::<_, Option<i64>>(7)?, // tax_deductible
            ))
        })?;

        for flow in flows {
            let (id, date, amount, category_id, description, linked_flows, custom_fields, tax_deductible) = flow?;
            tx.execute(
                "INSERT INTO flows (id, date, amount, category_id, description, linked_flows, custom_fields, tax_deductible)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                params![id, date, amount, category_id, description, linked_flows, custom_fields, tax_deductible],
            )?;
        }

        // Copy user settings (decrypt if necessary)
        let mut stmt = self.conn.prepare("SELECT settings_json FROM user_settings WHERE id = 1")?;
        if let Ok(encrypted_json) = stmt.query_row([], |row| row.get::<_, String>(0)) {
            // Decrypt the settings if encryption is enabled
            let decrypted_json = self.decrypt_data(&encrypted_json)?;
            tx.execute(
                "INSERT OR REPLACE INTO user_settings (id, settings_json) VALUES (1, ?)",
                params![decrypted_json],
            )?;
        }

        Ok(())
    }

    /// Restore the database from a backup file
    /// 
    /// # Arguments
    /// * `backup_path` - Path to the backup file
    /// * `password` - Password for encrypted backups (None for unencrypted backups)
    /// * `force_unencrypted_restore` - If true, forces restoration as unencrypted (for data recovery)
    pub fn restore_from_file(&mut self, backup_path: &Path, password: Option<&str>, force_unencrypted_restore: bool) -> Result<()> {
        info!("Starting restore from file: {:?}", backup_path);
        info!("Password provided: {}", password.is_some());
        info!("Force unencrypted restore: {}", force_unencrypted_restore);
        
        // Verify the backup file exists
        if !backup_path.exists() {
            return Err(anyhow::anyhow!("Backup file does not exist: {:?}", backup_path));
        }
        info!("Backup file exists");

        // Try to detect if the backup is encrypted by attempting to read it
        info!("Detecting backup encryption...");
        let is_encrypted_backup = self.detect_encrypted_backup(backup_path)?;
        info!("Backup encryption detected: {}", is_encrypted_backup);

        if is_encrypted_backup && password.is_none() && !force_unencrypted_restore {
            return Err(anyhow::anyhow!("Encrypted backup detected but no password provided. Use force_unencrypted_restore=true for data recovery (this will result in an unencrypted database)"));
        }

        if is_encrypted_backup && password.is_some() {
            info!("Using encrypted restore path");
            // Restore encrypted backup
            self.restore_encrypted(backup_path, password.unwrap())
        } else {
            info!("Using unencrypted restore path");
            // Restore as unencrypted (either it's unencrypted or we're forcing unencrypted restore)
            self.restore_unencrypted(backup_path)
        }
    }

    /// Detect if a backup file is encrypted
    pub fn detect_encrypted_backup(&self, backup_path: &Path) -> Result<bool> {
        // Try to open the backup file and read a simple query
        match Connection::open(backup_path) {
            Ok(conn) => {
                // Try to read from user_settings table
                match conn.query_row("SELECT COUNT(*) FROM user_settings", [], |row| row.get::<_, i64>(0)) {
                    Ok(_) => Ok(false), // Successfully read, likely unencrypted
                    Err(_) => Ok(true),  // Failed to read, likely encrypted
                }
            }
            Err(_) => Ok(true), // Can't open, assume encrypted
        }
    }

    /// Restore from an encrypted backup
    fn restore_encrypted(&mut self, backup_path: &Path, password: &str) -> Result<()> {
        info!("Starting encrypted restore from: {:?}", backup_path);
        
        // Verify password matches our current encryption config
        info!("Verifying password...");
        if !self.encryption_config.verify_password(password) {
            return Err(anyhow::anyhow!("Password does not match current encryption configuration"));
        }
        info!("Password verified successfully");

        // Create a connection to the backup file
        let backup_conn = Connection::open(backup_path)?;
        info!("Successfully opened encrypted backup connection");
        
        // Create a backup object (backup -> current)
        info!("Creating backup object for encrypted restore...");
        let backup = rusqlite::backup::Backup::new(&backup_conn, &mut self.conn)?;
        
        // Perform the restore
        info!("Performing encrypted restore...");
        backup.run_to_completion(5, std::time::Duration::from_millis(100), Some(|progress| {
            info!("Encrypted restore progress: {} pages", progress.pagecount);
        }))?;
        
        info!("Encrypted database restore completed from: {:?}", backup_path);
        Ok(())
    }

    /// Restore from an unencrypted backup
    fn restore_unencrypted(&mut self, backup_path: &Path) -> Result<()> {
        info!("Starting unencrypted restore from: {:?}", backup_path);
        
        // Create a connection to the backup file
        let backup_conn = Connection::open(backup_path)?;
        info!("Successfully opened backup connection");
        
        // Collect data from backup
        info!("Collecting data from backup...");
        let categories_data = self.collect_categories_from_backup(&backup_conn)?;
        info!("Collected {} categories from backup", categories_data.len());
        
        let flows_data = self.collect_flows_from_backup(&backup_conn)?;
        info!("Collected {} flows from backup", flows_data.len());
        
        let user_settings_data = self.collect_user_settings_from_backup(&backup_conn)?;
        info!("User settings collected: {}", user_settings_data.is_some());
        
        // Start a transaction and disable foreign key constraints
        info!("Starting transaction and disabling foreign key constraints...");
        let tx = self.conn.transaction()?;
        tx.execute("PRAGMA foreign_keys = OFF", [])?;
        
        // Verify foreign key constraints are actually disabled
        let fk_enabled: i64 = tx.query_row("PRAGMA foreign_keys", [], |row| row.get(0))?;
        info!("Foreign key constraints status after disabling: {}", fk_enabled);
        
        // Check if there are any foreign key constraints in the database
        let fk_count: i64 = tx.query_row(
            "SELECT COUNT(*) FROM pragma_foreign_key_list('flows')", 
            [], 
            |row| row.get(0)
        ).unwrap_or(0);
        info!("Number of foreign key constraints on flows table: {}", fk_count);
        
        // Clear current database
        info!("Clearing current database...");
        match tx.execute("DELETE FROM flows", []) {
            Ok(_) => info!("Flows cleared"),
            Err(e) => {
                error!("Failed to clear flows: {}", e);
                return Err(e.into());
            }
        }
        match tx.execute("DELETE FROM categories", []) {
            Ok(_) => info!("Categories cleared"),
            Err(e) => {
                error!("Failed to clear categories: {}", e);
                return Err(e.into());
            }
        }
        match tx.execute("DELETE FROM user_settings", []) {
            Ok(_) => info!("User settings cleared"),
            Err(e) => {
                error!("Failed to clear user settings: {}", e);
                return Err(e.into());
            }
        }
        
        // Insert collected data
        info!("Inserting categories...");
        Self::insert_categories_transaction(&categories_data, &tx)?;
        info!("Categories inserted successfully");
        
        info!("Inserting flows...");
        Self::insert_flows_transaction(&flows_data, &tx)?;
        info!("Flows inserted successfully");
        
        info!("Inserting user settings...");
        Self::insert_user_settings_transaction(&user_settings_data, &tx)?;
        info!("User settings inserted successfully");
        
        // Re-enable foreign key constraints
        info!("Re-enabling foreign key constraints...");
        tx.execute("PRAGMA foreign_keys = ON", [])?;
        info!("Foreign key constraints re-enabled");
        
        // Commit the transaction
        info!("Committing transaction...");
        tx.commit()?;
        info!("Transaction committed successfully");
        
        info!("Unencrypted database restore completed from: {:?}", backup_path);
        if self.is_encrypted() {
            info!("Note: Database is now unencrypted. Consider re-enabling encryption for security.");
        }
        Ok(())
    }

    /// Collect categories data from backup
    fn collect_categories_from_backup(&self, backup_conn: &Connection) -> Result<Vec<(String, String, String, String, i64, i64)>> {
        let mut stmt = backup_conn.prepare("SELECT * FROM categories")?;
        let categories = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?, // id
                row.get::<_, String>(1)?, // name
                row.get::<_, String>(2)?, // flow_type
                row.get::<_, String>(3)?, // fields
                row.get::<_, i64>(4)?,    // tax_deduction_allowed
                row.get::<_, i64>(5)?,    // tax_deduction_default
            ))
        })?;

        let mut result = Vec::new();
        for category in categories {
            result.push(category?);
        }
        Ok(result)
    }

    /// Collect flows data from backup
    fn collect_flows_from_backup(&self, backup_conn: &Connection) -> Result<Vec<(String, String, f64, String, String, String, String, Option<i64>)>> {
        let mut stmt = backup_conn.prepare("SELECT * FROM flows")?;
        let flows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?, // id
                row.get::<_, String>(1)?, // date
                row.get::<_, f64>(2)?,    // amount
                row.get::<_, String>(3)?, // category_id
                row.get::<_, String>(4)?, // description
                row.get::<_, String>(5)?, // linked_flows
                row.get::<_, String>(6)?, // custom_fields
                row.get::<_, Option<i64>>(7)?, // tax_deductible
            ))
        })?;

        let mut result = Vec::new();
        for flow in flows {
            result.push(flow?);
        }
        Ok(result)
    }

    /// Collect user settings data from backup
    fn collect_user_settings_from_backup(&self, backup_conn: &Connection) -> Result<Option<String>> {
        let mut stmt = backup_conn.prepare("SELECT settings_json FROM user_settings WHERE id = 1")?;
        match stmt.query_row([], |row| row.get::<_, String>(0)) {
            Ok(settings_json) => {
                // Encrypt the settings if encryption is enabled
                let encrypted_json = self.encrypt_data(&settings_json)?;
                Ok(Some(encrypted_json))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Insert categories data into transaction
    fn insert_categories_transaction(categories_data: &[(String, String, String, String, i64, i64)], tx: &Connection) -> Result<()> {
        info!("Inserting {} categories into transaction", categories_data.len());
        for (id, name, flow_type, fields, tax_deduction_allowed, tax_deduction_default) in categories_data {
            tx.execute(
                "INSERT INTO categories (id, name, flow_type, fields, tax_deduction_allowed, tax_deduction_default)
                 VALUES (?, ?, ?, ?, ?, ?)",
                params![id, name, flow_type, fields, tax_deduction_allowed, tax_deduction_default],
            )?;
        }
        info!("All categories inserted successfully");
        Ok(())
    }

    /// Insert flows data into transaction
    fn insert_flows_transaction(flows_data: &[(String, String, f64, String, String, String, String, Option<i64>)], tx: &Connection) -> Result<()> {
        info!("Inserting {} flows into transaction", flows_data.len());
        for (id, date, amount, category_id, description, linked_flows, custom_fields, tax_deductible) in flows_data {
            tx.execute(
                "INSERT INTO flows (id, date, amount, category_id, description, linked_flows, custom_fields, tax_deductible)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                params![id, date, amount, category_id, description, linked_flows, custom_fields, tax_deductible],
            )?;
        }
        info!("All flows inserted successfully");
        Ok(())
    }

    /// Insert user settings data into transaction
    fn insert_user_settings_transaction(user_settings_data: &Option<String>, tx: &Connection) -> Result<()> {
        if let Some(encrypted_json) = user_settings_data {
            tx.execute(
                "INSERT OR REPLACE INTO user_settings (id, settings_json) VALUES (1, ?)",
                params![encrypted_json],
            )?;
        }
        Ok(())
    }

    /// Create a SQL dump of the database to a text file
    pub fn dump_to_sql_file(&self, dump_path: &Path) -> Result<()> {
        // Get all tables
        let mut tables = self.conn.prepare(
            "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'"
        )?;
        
        let mut dump_content = String::new();
        
        // Add schema for each table
        for table_row in tables.query_map([], |row| row.get::<_, String>(0))? {
            let table_name = table_row?;
            let schema = self.conn.query_row(
                "SELECT sql FROM sqlite_master WHERE type='table' AND name = ?",
                params![table_name],
                |row| row.get::<_, String>(0)
            )?;
            dump_content.push_str(&format!("{}\n\n", schema));
        }
        
        // Add data for each table
        for table_row in tables.query_map([], |row| row.get::<_, String>(0))? {
            let table_name = table_row?;
            let mut data_stmt = self.conn.prepare(&format!("SELECT * FROM {}", table_name))?;
            let column_count = data_stmt.column_count();
            
            for row in data_stmt.query_map([], |row| {
                let mut values = Vec::new();
                for i in 0..column_count {
                    let value = row.get_ref(i)?;
                    match value.data_type() {
                        rusqlite::types::Type::Null => values.push("NULL".to_string()),
                        rusqlite::types::Type::Integer => {
                            let val: i64 = row.get(i)?;
                            values.push(val.to_string());
                        },
                        rusqlite::types::Type::Real => {
                            let val: f64 = row.get(i)?;
                            values.push(val.to_string());
                        },
                        rusqlite::types::Type::Text => {
                            let val: String = row.get(i)?;
                            values.push(format!("'{}'", val.replace("'", "''")));
                        },
                        rusqlite::types::Type::Blob => {
                            values.push("X''".to_string()); // Empty blob for simplicity
                        },
                    }
                }
                Ok(values.join(", "))
            })? {
                let values = row?;
                dump_content.push_str(&format!("INSERT INTO {} VALUES ({});\n", table_name, values));
            }
            dump_content.push('\n');
        }
        
        // Write to file
        std::fs::write(dump_path, dump_content)?;
        info!("SQL dump completed to: {:?}", dump_path);
        Ok(())
    }

    /// Restore the database from a SQL dump file
    pub fn restore_from_sql_file(&mut self, dump_path: &Path) -> Result<()> {
        // Verify the dump file exists
        if !dump_path.exists() {
            return Err(anyhow::anyhow!("Dump file does not exist: {:?}", dump_path));
        }

        // Read the dump file
        let dump_content = std::fs::read_to_string(dump_path)?;
        
        // Start a transaction
        let tx = self.conn.transaction()?;
        
        // Split by semicolon and execute each statement
        for statement in dump_content.split(';') {
            let statement = statement.trim();
            if !statement.is_empty() && !statement.starts_with("--") {
                tx.execute(statement, [])?;
            }
        }
        
        // Commit the transaction
        tx.commit()?;
        
        info!("Database restore from SQL dump completed from: {:?}", dump_path);
        Ok(())
    }

    /// Get the database file path
    pub fn get_database_path(&self) -> Result<std::path::PathBuf> {
        let home_dir = dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not find home directory"))?;
        Ok(home_dir.join(".preft").join("preft.db"))
    }
}

impl FromSql for FlowType {
    fn column_result(value: ValueRef<'_>) -> Result<Self, FromSqlError> {
        let text = value.as_str().map_err(|e| FromSqlError::Other(Box::new(e)))?;
        match text {
            "Income" => Ok(FlowType::Income),
            "Expense" => Ok(FlowType::Expense),
            _ => Err(FromSqlError::Other(Box::new(rusqlite::Error::InvalidColumnType(0, "text".to_string(), Type::Text)))),
        }
    }
} 