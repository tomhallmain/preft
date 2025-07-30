use anyhow::Result;
use rusqlite::{Connection, params};
use serde_json::Value;
use log::{info, warn, error};
use crate::models::{Category, FieldType, CategoryField, FlowType, TaxDeductionInfo, Flow};
use std::collections::HashMap;

pub fn run_migrations(conn: &mut Connection) -> Result<()> {
    log::info!("Starting database migrations...");

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
    log::info!("Migrations table verified/created");

    // Get list of applied migrations
    let applied_migrations: Vec<(String, i64)> = {
        let mut stmt = conn.prepare("SELECT name, version FROM migrations ORDER BY version")?;
        stmt.query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?
        .collect::<Result<Vec<(String, i64)>, _>>()?
    };
    
    log::info!("Previously applied migrations: {:?}", applied_migrations);

    // Check if we've already run the number to float migration
    let migration_name = "convert_number_to_float";
    let migration_version: i64 = 1;
    let migration_applied: bool = {
        let mut stmt = conn.prepare("SELECT COUNT(*) > 0 FROM migrations WHERE name = ? AND version = ?")?;
        stmt.query_row(params![migration_name, migration_version], |row| row.get(0))?
    };

    if !migration_applied {
        log::info!("Running migration: {} (version {})", migration_name, migration_version);
        
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
                    log::info!("Migration record added to database");
                    
                    // Commit transaction
                    tx.commit()?;
                    log::info!("Successfully completed migration: {} (version {})", migration_name, migration_version);
                } else {
                    log::error!("Migration validation failed, rolling back");
                    return Err(anyhow::anyhow!("Migration validation failed"));
                }
            }
            Err(e) => {
                log::error!("Failed to run migration {}: {}", migration_name, e);
                return Err(e);
            }
        }
    } else {
        log::info!("Migration {} (version {}) already applied, skipping", migration_name, migration_version);
    }

    log::info!("Database migrations completed successfully");
    Ok(())
}

fn convert_number_to_float(conn: &Connection) -> Result<()> {
    log::info!("Starting conversion of Number fields to Float...");

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
                log::info!("Converting field '{}' in category '{}' from Number to Float", 
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
            log::info!("Updated category '{}' with {} converted fields", 
                category.name, fields_converted);
        }
    }

    log::info!("Migration summary:");
    log::info!("- Total categories processed: {}", total_categories);
    log::info!("- Categories modified: {}", modified_categories);
    log::info!("- Total fields converted: {}", total_fields_converted);

    Ok(())
}

fn validate_migration(conn: &Connection) -> Result<bool> {
    log::info!("Validating migration...");
    
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
                log::error!("Validation failed: Found unconverted Number field '{}'", field.name);
                return Ok(false);
            }
        }
    }

    log::info!("Migration validation successful");
    Ok(true)
}

/// Compares two category field schemas to determine if a migration is needed
pub fn has_schema_changes(old_category: &Category, new_category: &Category) -> bool {
    log::info!("Comparing schemas for category '{}':", new_category.name);
    log::info!("Old category fields: {:?}", old_category.fields);
    log::info!("New category fields: {:?}", new_category.fields);

    // Create maps of field names to their types for easy comparison
    let old_fields: HashMap<&str, &FieldType> = old_category.fields
        .iter()
        .map(|f| (f.name.as_str(), &f.field_type))
        .collect();
    
    let new_fields: HashMap<&str, &FieldType> = new_category.fields
        .iter()
        .map(|f| (f.name.as_str(), &f.field_type))
        .collect();

    log::info!("Old field types: {:?}", old_fields);
    log::info!("New field types: {:?}", new_fields);

    let mut has_changes = false;
    let mut changes = Vec::new();

    // Check for removed fields
    for field_name in old_fields.keys() {
        if !new_fields.contains_key(field_name) {
            changes.push(format!("Field '{}' was removed", field_name));
            has_changes = true;
            log::info!("Field '{}' exists in old schema but not in new", field_name);
        }
    }

    // Check for added fields
    for field_name in new_fields.keys() {
        if !old_fields.contains_key(field_name) {
            changes.push(format!("Field '{}' was added", field_name));
            has_changes = true;
            log::info!("Field '{}' exists in new schema but not in old", field_name);
        }
    }

    // Check for type changes
    for (field_name, new_type) in &new_fields {
        if let Some(old_type) = old_fields.get(field_name) {
            if old_type != new_type {
                changes.push(format!("Field '{}' type changed from {:?} to {:?}", 
                    field_name, old_type, new_type));
                has_changes = true;
                log::info!("Field '{}' type changed: {:?} -> {:?}", field_name, old_type, new_type);
            } else {
                log::info!("Field '{}' type unchanged: {:?}", field_name, old_type);
            }
        }
    }

    if has_changes {
        log::info!("Schema changes detected for category '{}':", new_category.name);
        for change in changes {
            log::info!("- {}", change);
        }
    } else {
        log::info!("No schema changes detected for category '{}'", new_category.name);
    }

    has_changes
}

/// Migrates flows to match a new category structure
pub fn migrate_flows_to_new_category(conn: &Connection, old_category: &Category, new_category: &Category) -> Result<()> {
    // Check if we actually need to migrate
    if !has_schema_changes(old_category, new_category) {
        log::info!("No schema changes detected for category '{}', skipping flow migration", new_category.name);
        return Ok(());
    }

    log::info!("Starting flow migration for category '{}'", new_category.name);
    
    // Get all flows for this category
    let mut stmt = conn.prepare(
        "SELECT id, custom_fields FROM flows WHERE category_id = ?"
    )?;
    
    let flows = stmt.query_map(params![new_category.id], |row| {
        let id: String = row.get(0)?;
        let custom_fields_json: String = row.get(1)?;
        let custom_fields: HashMap<String, String> = serde_json::from_str(&custom_fields_json)
            .map_err(|e| rusqlite::Error::InvalidParameterName(e.to_string()))?;
        Ok((id, custom_fields))
    })?;

    let mut total_flows = 0;
    let mut migrated_flows = 0;
    let mut skipped_fields = 0;

    // Create a map of old field names to new field types
    let field_type_map: HashMap<String, FieldType> = new_category.fields
        .iter()
        .map(|f| (f.name.clone(), f.field_type.clone()))
        .collect();

    // Process each flow
    for flow_result in flows {
        total_flows += 1;
        let (flow_id, mut custom_fields) = flow_result?;
        let mut modified = false;

        // Check each field in the flow
        let fields_to_remove: Vec<String> = custom_fields.keys()
            .filter(|field_name| !field_type_map.contains_key(*field_name))
            .cloned()
            .collect();

        // Remove fields that no longer exist in the category
        for field_name in fields_to_remove {
            custom_fields.remove(&field_name);
            modified = true;
            skipped_fields += 1;
            log::info!("Removed field '{}' from flow {}", field_name, flow_id);
        }

        // Validate and convert field values based on new types
        for (field_name, field_type) in &field_type_map {
            if let Some(value) = custom_fields.get(field_name) {
                // Skip empty values
                if value.trim().is_empty() {
                    continue;
                }

                // Clone the value to avoid borrow checker issues
                let value = value.clone();

                match field_type {
                    FieldType::Integer => {
                        if let Ok(_) = value.parse::<i64>() {
                            // Value is already valid
                        } else if let Ok(float_val) = value.parse::<f64>() {
                            // Convert float to integer
                            custom_fields.insert(field_name.clone(), (float_val as i64).to_string());
                            modified = true;
                            log::info!("Converted field '{}' to integer in flow {}", field_name, flow_id);
                        } else {
                            // Invalid value, remove it
                            custom_fields.remove(field_name);
                            modified = true;
                            skipped_fields += 1;
                            log::warn!("Invalid integer value '{}' for field '{}' in category '{}'", 
                                value, field_name, new_category.name);
                        }
                    },
                    FieldType::Float => {
                        if let Ok(_) = value.parse::<f64>() {
                            // Value is already valid
                        } else if let Ok(int_val) = value.parse::<i64>() {
                            // Convert integer to float
                            custom_fields.insert(field_name.clone(), (int_val as f64).to_string());
                            modified = true;
                            log::info!("Converted field '{}' to float in flow {}", field_name, flow_id);
                        } else {
                            // Invalid value, remove it
                            custom_fields.remove(field_name);
                            modified = true;
                            skipped_fields += 1;
                            log::warn!("Invalid float value '{}' for field '{}' in category '{}'", 
                                value, field_name, new_category.name);
                        }
                    },
                    FieldType::Currency => {
                        // Remove currency symbols and commas, then validate
                        let clean_value = value.replace(['$', ','], "");
                        if let Ok(_) = clean_value.parse::<f64>() {
                            // Value is valid, update with cleaned version
                            custom_fields.insert(field_name.clone(), clean_value);
                            modified = true;
                            log::info!("Cleaned currency field '{}' in flow {}", field_name, flow_id);
                        } else {
                            // Invalid value, remove it
                            custom_fields.remove(field_name);
                            modified = true;
                            skipped_fields += 1;
                            log::warn!("Invalid currency value '{}' for field '{}' in category '{}'", 
                                value, field_name, new_category.name);
                        }
                    },
                    FieldType::Boolean => {
                        match value.to_lowercase().as_str() {
                            "true" | "1" | "yes" | "y" => {
                                custom_fields.insert(field_name.clone(), "true".to_string());
                                modified = true;
                            },
                            "false" | "0" | "no" | "n" => {
                                custom_fields.insert(field_name.clone(), "false".to_string());
                                modified = true;
                            },
                            _ => {
                                // Invalid value, remove it
                                custom_fields.remove(field_name);
                                modified = true;
                                skipped_fields += 1;
                                log::warn!("Invalid boolean value '{}' for field '{}' in category '{}'", 
                                    value, field_name, new_category.name);
                            }
                        }
                    },
                    FieldType::Date => {
                        // Try to parse the date in various formats
                        if chrono::NaiveDate::parse_from_str(&value, "%Y-%m-%d").is_ok() {
                            // Already in correct format
                        } else if let Ok(date) = chrono::NaiveDate::parse_from_str(&value, "%m/%d/%Y") {
                            // Convert to standard format
                            custom_fields.insert(field_name.clone(), date.format("%Y-%m-%d").to_string());
                            modified = true;
                            log::info!("Converted date field '{}' to standard format in flow {}", field_name, flow_id);
                        } else {
                            // Invalid value, remove it
                            custom_fields.remove(field_name);
                            modified = true;
                            skipped_fields += 1;
                            log::warn!("Invalid date value '{}' for field '{}' in category '{}'", 
                                value, field_name, new_category.name);
                        }
                    },
                    _ => {
                        // Text and Select fields don't need validation
                    }
                }
            }
        }

        // Update the flow if any changes were made
        if modified {
            let custom_fields_json = serde_json::to_string(&custom_fields)?;
            conn.execute(
                "UPDATE flows SET custom_fields = ? WHERE id = ?",
                params![custom_fields_json, flow_id],
            )?;
            migrated_flows += 1;
        }
    }

    log::info!("Flow migration summary for category '{}':", new_category.name);
    log::info!("- Total flows processed: {}", total_flows);
    log::info!("- Flows modified: {}", migrated_flows);
    log::info!("- Fields skipped/removed: {}", skipped_fields);

    Ok(())
} 