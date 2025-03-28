use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use chrono::NaiveDate;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum FlowType {
    Income,
    Expense,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Category {
    pub id: String,
    pub name: String,
    pub flow_type: FlowType,
    pub parent_id: Option<String>,
    pub fields: Vec<CategoryField>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategoryField {
    pub name: String,
    pub field_type: FieldType,
    pub required: bool,
    pub default_value: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FieldType {
    Text,
    Number,
    Date,
    Boolean,
    Select(Vec<String>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Flow {
    pub id: String,
    pub date: NaiveDate,
    pub amount: f64,
    pub category_id: String,
    pub description: String,
    pub linked_flows: Vec<String>, // IDs of linked flows
    pub custom_fields: HashMap<String, String>,
}

// Default categories that will be pre-defined
pub fn get_default_categories() -> Vec<Category> {
    vec![
        Category {
            id: "salary".to_string(),
            name: "Salary".to_string(),
            flow_type: FlowType::Income,
            parent_id: None,
            fields: vec![
                CategoryField {
                    name: "employer".to_string(),
                    field_type: FieldType::Text,
                    required: true,
                    default_value: None,
                },
                CategoryField {
                    name: "pay_period".to_string(),
                    field_type: FieldType::Select(vec!["Monthly".to_string(), "Bi-weekly".to_string(), "Weekly".to_string()]),
                    required: true,
                    default_value: Some("Monthly".to_string()),
                },
            ],
        },
        Category {
            id: "passive_income".to_string(),
            name: "Passive Income".to_string(),
            flow_type: FlowType::Income,
            parent_id: None,
            fields: vec![
                CategoryField {
                    name: "source".to_string(),
                    field_type: FieldType::Text,
                    required: true,
                    default_value: None,
                },
                CategoryField {
                    name: "type".to_string(),
                    field_type: FieldType::Select(vec!["Investment".to_string(), "Rental".to_string(), "Royalty".to_string(), "Other".to_string()]),
                    required: true,
                    default_value: None,
                },
            ],
        },
        Category {
            id: "taxes_paid".to_string(),
            name: "Taxes Paid".to_string(),
            flow_type: FlowType::Expense,
            parent_id: None,
            fields: vec![
                CategoryField {
                    name: "tax_type".to_string(),
                    field_type: FieldType::Select(vec!["Federal".to_string(), "State".to_string(), "Local".to_string(), "Property".to_string(), "Other".to_string()]),
                    required: true,
                    default_value: None,
                },
                CategoryField {
                    name: "tax_year".to_string(),
                    field_type: FieldType::Number,
                    required: true,
                    default_value: None,
                },
            ],
        },
        Category {
            id: "cash_donations".to_string(),
            name: "Cash Donations".to_string(),
            flow_type: FlowType::Expense,
            parent_id: None,
            fields: vec![
                CategoryField {
                    name: "recipient".to_string(),
                    field_type: FieldType::Text,
                    required: true,
                    default_value: None,
                },
                CategoryField {
                    name: "tax_deductible".to_string(),
                    field_type: FieldType::Boolean,
                    required: true,
                    default_value: Some("true".to_string()),
                },
            ],
        },
        Category {
            id: "in_kind_donations".to_string(),
            name: "In-Kind Donations".to_string(),
            flow_type: FlowType::Expense,
            parent_id: None,
            fields: vec![
                CategoryField {
                    name: "recipient".to_string(),
                    field_type: FieldType::Text,
                    required: true,
                    default_value: None,
                },
                CategoryField {
                    name: "item_description".to_string(),
                    field_type: FieldType::Text,
                    required: true,
                    default_value: None,
                },
                CategoryField {
                    name: "tax_deductible".to_string(),
                    field_type: FieldType::Boolean,
                    required: true,
                    default_value: Some("true".to_string()),
                },
            ],
        },
        Category {
            id: "medical".to_string(),
            name: "Medical".to_string(),
            flow_type: FlowType::Expense,
            parent_id: None,
            fields: vec![
                CategoryField {
                    name: "provider".to_string(),
                    field_type: FieldType::Text,
                    required: true,
                    default_value: None,
                },
                CategoryField {
                    name: "type".to_string(),
                    field_type: FieldType::Select(vec!["Doctor Visit".to_string(), "Prescription".to_string(), "Procedure".to_string(), "Equipment".to_string(), "Other".to_string()]),
                    required: true,
                    default_value: None,
                },
                CategoryField {
                    name: "insurance_covered".to_string(),
                    field_type: FieldType::Boolean,
                    required: true,
                    default_value: Some("false".to_string()),
                },
            ],
        },
        Category {
            id: "dental".to_string(),
            name: "Dental".to_string(),
            flow_type: FlowType::Expense,
            parent_id: None,
            fields: vec![
                CategoryField {
                    name: "provider".to_string(),
                    field_type: FieldType::Text,
                    required: true,
                    default_value: None,
                },
                CategoryField {
                    name: "type".to_string(),
                    field_type: FieldType::Select(vec!["Cleaning".to_string(), "Checkup".to_string(), "Procedure".to_string(), "Orthodontics".to_string(), "Other".to_string()]),
                    required: true,
                    default_value: None,
                },
                CategoryField {
                    name: "insurance_covered".to_string(),
                    field_type: FieldType::Boolean,
                    required: true,
                    default_value: Some("false".to_string()),
                },
            ],
        },
        Category {
            id: "other_expense".to_string(),
            name: "Other Expense".to_string(),
            flow_type: FlowType::Expense,
            parent_id: None,
            fields: vec![
                CategoryField {
                    name: "description".to_string(),
                    field_type: FieldType::Text,
                    required: true,
                    default_value: None,
                },
                CategoryField {
                    name: "recurring".to_string(),
                    field_type: FieldType::Boolean,
                    required: true,
                    default_value: Some("false".to_string()),
                },
            ],
        },
        Category {
            id: "other_income".to_string(),
            name: "Other Income".to_string(),
            flow_type: FlowType::Income,
            parent_id: None,
            fields: vec![
                CategoryField {
                    name: "source".to_string(),
                    field_type: FieldType::Text,
                    required: true,
                    default_value: None,
                },
                CategoryField {
                    name: "recurring".to_string(),
                    field_type: FieldType::Boolean,
                    required: true,
                    default_value: Some("false".to_string()),
                },
            ],
        },
    ]
} 