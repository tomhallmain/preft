use anyhow::Result;
use rusqlite::{Connection, params, types::FromSql, types::ValueRef, types::FromSqlError, types::Type};
use chrono::NaiveDate;
use crate::models::{Flow, Category, FlowType, get_default_categories};

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
        let db = Database { conn };
        db.initialize()?;
        
        // Check if we have any categories, if not, save the defaults
        let count: i64 = db.conn.query_row("SELECT COUNT(*) FROM categories", [], |row| row.get(0))?;
        if count == 0 {
            for category in get_default_categories() {
                db.save_category(&category)?;
            }
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
                fields TEXT NOT NULL
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
                FOREIGN KEY (category_id) REFERENCES categories(id)
            )",
            [],
        )?;

        Ok(())
    }

    pub fn save_category(&self, category: &Category) -> Result<()> {
        let fields_json = serde_json::to_string(&category.fields)?;
        
        self.conn.execute(
            "INSERT OR REPLACE INTO categories (id, name, flow_type, fields)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                category.id,
                category.name,
                category.flow_type.to_string(),
                fields_json
            ],
        )?;

        Ok(())
    }

    pub fn save_flow(&self, flow: &Flow) -> Result<()> {
        let linked_flows_json = serde_json::to_string(&flow.linked_flows)?;
        let custom_fields_json = serde_json::to_string(&flow.custom_fields)?;
        
        self.conn.execute(
            "INSERT OR REPLACE INTO flows (id, date, amount, category_id, description, linked_flows, custom_fields)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                flow.id,
                flow.date.to_string(),
                flow.amount,
                flow.category_id,
                flow.description,
                linked_flows_json,
                custom_fields_json
            ],
        )?;

        Ok(())
    }

    pub fn load_categories(&self) -> Result<Vec<Category>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, flow_type, fields FROM categories"
        )?;

        let categories = stmt.query_map([], |row| {
            let fields_json: String = row.get(3)?;
            let fields = serde_json::from_str(&fields_json)
                .map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e)))?;

            Ok(Category {
                id: row.get(0)?,
                name: row.get(1)?,
                flow_type: row.get(2)?,
                fields,
                parent_id: None,
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
            "SELECT id, date, amount, category_id, description, linked_flows, custom_fields FROM flows"
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

            Ok(Flow {
                id: row.get(0)?,
                date,
                amount: row.get(2)?,
                category_id: row.get(3)?,
                description: row.get(4)?,
                linked_flows,
                custom_fields,
            })
        })?;

        let mut result = Vec::new();
        for flow in flows {
            result.push(flow?);
        }

        Ok(result)
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