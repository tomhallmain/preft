use anyhow::Result;
use rusqlite::{Connection, params, types::FromSql, types::ValueRef, types::FromSqlError, types::Type};
use chrono::NaiveDate;
use crate::models::{Flow, Category, FlowType, TaxDeductionInfo, CategoryField, get_default_categories};
use crate::settings::UserSettings;
use log::{error, info};
mod migrations;

pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn new() -> Result<Self> {
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
        let mut db = Database { conn };
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
        
        self.conn.execute(
            "INSERT OR REPLACE INTO user_settings (id, settings_json)
             VALUES (1, ?1)",
            params![settings_json],
        )?;

        Ok(())
    }

    pub fn load_user_settings(&self) -> Result<UserSettings> {
        let settings_json: String = self.conn.query_row(
            "SELECT settings_json FROM user_settings WHERE id = 1",
            [],
            |row| row.get(0),
        )?;

        let settings = serde_json::from_str(&settings_json)?;
        Ok(settings)
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