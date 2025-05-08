use anyhow::Result;
use rusqlite::{Connection, params};
use serde_json::Value;
use log::{info, warn, error};
use crate::models::{Category, FieldType, CategoryField, FlowType, TaxDeductionInfo};

pub fn run_migrations(conn: &mut Connection) -> Result<()> {
    info!("Starting database migrations...");

    // Create migrations table if it doesn't exist
    conn.execute(
        "CREATE TABLE IF NOT EXISTS migrations (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            version INTEGER NOT NULL,
            applied_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
        )",
        [],
    )?;
    info!("Migrations table verified/created");

    // Get list of applied migrations
    let applied_migrations: Vec<(String, i64)> = {
        let mut stmt = conn.prepare("SELECT name, version FROM migrations ORDER BY version")?;
        stmt.query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?
        .collect::<Result<Vec<(String, i64)>, _>>()?
    };
    
    info!("Previously applied migrations: {:?}", applied_migrations);

    // Check if we've already run the number to float migration
    let migration_name = "convert_number_to_float";
    let migration_version: i64 = 1;
    let migration_applied: bool = {
        let mut stmt = conn.prepare("SELECT COUNT(*) > 0 FROM migrations WHERE name = ? AND version = ?")?;
        stmt.query_row(params![migration_name, migration_version], |row| row.get(0))?
    };

    if !migration_applied {
        info!("Running migration: {} (version {})", migration_name, migration_version);
        
        // Start transaction
        let tx = conn.transaction()?;
        
        match convert_number_to_float(&tx) {
            Ok(_) => {
                // Validate the migration
                if validate_migration(&tx)? {
                    // Mark migration as applied
                    tx.execute(
                        "INSERT INTO migrations (name, version) VALUES (?, ?)",
                        params![migration_name, migration_version],
                    )?;
                    info!("Migration record added to database");
                    
                    // Commit transaction
                    tx.commit()?;
                    info!("Successfully completed migration: {} (version {})", migration_name, migration_version);
                } else {
                    error!("Migration validation failed, rolling back");
                    return Err(anyhow::anyhow!("Migration validation failed"));
                }
            }
            Err(e) => {
                error!("Failed to run migration {}: {}", migration_name, e);
                return Err(e);
            }
        }
    } else {
        info!("Migration {} (version {}) already applied, skipping", migration_name, migration_version);
    }

    info!("Database migrations completed successfully");
    Ok(())
}

fn convert_number_to_float(conn: &Connection) -> Result<()> {
    info!("Starting conversion of Number fields to Float...");

    // Get all categories
    let mut stmt = conn.prepare("SELECT id, name, flow_type, fields, tax_deduction_allowed, tax_deduction_default FROM categories")?;
    let categories = stmt.query_map([], |row| {
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
    })?;

    let mut total_categories = 0;
    let mut modified_categories = 0;
    let mut total_fields_converted = 0;

    // Convert each category's Number fields to Float
    for category_result in categories {
        total_categories += 1;
        let mut category = category_result?;
        let mut modified = false;
        let mut fields_converted = 0;

        // Update field types from Number to Float
        for field in &mut category.fields {
            #[allow(deprecated)]
            if field.field_type == FieldType::Number {
                field.field_type = FieldType::Float;
                modified = true;
                fields_converted += 1;
                info!("Converting field '{}' in category '{}' from Number to Float", 
                    field.name, category.name);
            }
        }

        // If any fields were modified, update the category in the database
        if modified {
            let fields_json = serde_json::to_string(&category.fields)?;
            conn.execute(
                "UPDATE categories SET fields = ? WHERE id = ?",
                params![fields_json, category.id],
            )?;
            modified_categories += 1;
            total_fields_converted += fields_converted;
            info!("Updated category '{}' with {} converted fields", 
                category.name, fields_converted);
        }
    }

    info!("Migration summary:");
    info!("- Total categories processed: {}", total_categories);
    info!("- Categories modified: {}", modified_categories);
    info!("- Total fields converted: {}", total_fields_converted);

    Ok(())
}

fn validate_migration(conn: &Connection) -> Result<bool> {
    info!("Validating migration...");
    
    // Check if any Number fields still exist
    let mut stmt = conn.prepare("SELECT fields FROM categories")?;
    let categories = stmt.query_map([], |row| {
        let fields_json: String = row.get(0)?;
        let fields: Vec<CategoryField> = serde_json::from_str(&fields_json)
            .map_err(|e| rusqlite::Error::InvalidParameterName(e.to_string()))?;
        Ok(fields)
    })?;

    for fields_result in categories {
        let fields = fields_result?;
        for field in fields {
            #[allow(deprecated)]
            if field.field_type == FieldType::Number {
                error!("Validation failed: Found unconverted Number field '{}'", field.name);
                return Ok(false);
            }
        }
    }

    info!("Migration validation successful");
    Ok(true)
} 