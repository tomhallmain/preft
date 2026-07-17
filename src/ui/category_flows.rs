use eframe::egui;
use chrono::{Local, NaiveDate, Datelike};
use log::warn;

use crate::models::{Flow, Category};
use crate::app::PreftApp;
use crate::utils;

pub struct CategoryFlowsState {
    last_year_total: f64,
    this_year_total: f64,
    current_month_total: f64,
    tracking_ratio: Option<f64>,
    needs_update: bool,
}

impl CategoryFlowsState {
    pub fn new() -> Self {
        Self {
            last_year_total: 0.0,
            this_year_total: 0.0,
            current_month_total: 0.0,
            tracking_ratio: None,
            needs_update: true,
        }
    }

    pub fn mark_for_update(&mut self) {
        self.needs_update = true;
    }

    pub fn update_totals(&mut self, flows: &[Flow], category: &Category) {
        self.update_totals_as_of(flows, category, Local::now().naive_local().date());
    }

    /// Core of `update_totals`, parameterized on "today" so it's testable
    /// without depending on the wall clock.
    fn update_totals_as_of(&mut self, flows: &[Flow], category: &Category, as_of: NaiveDate) {
        if !self.needs_update {
            return;
        }

        let current_year = as_of.year();
        let current_month = as_of.month();

        self.last_year_total = flows.iter()
            .filter(|f| f.category_id == category.id && f.date.year() == current_year - 1)
            .map(|f| f.amount)
            .sum();

        self.this_year_total = flows.iter()
            .filter(|f| f.category_id == category.id && f.date.year() == current_year)
            .map(|f| f.amount)
            .sum();

        self.current_month_total = flows.iter()
            .filter(|f| f.category_id == category.id &&
                    f.date.year() == current_year &&
                    f.date.month() == current_month)
            .map(|f| f.amount)
            .sum();

        self.tracking_ratio = utils::calculate_tracking_ratio_as_of(flows, category, as_of);
        self.needs_update = false;
    }
}

pub fn show_category_flows(ui: &mut egui::Ui, app: &mut PreftApp, category: &Category) {
    // Get all data we need first
    let flows = app.flows.clone();
    let state = app.get_category_flows_state(&category.id);
    
    if state.needs_update {
        state.update_totals(&flows, category);
        state.tracking_ratio = utils::calculate_tracking_ratio(&flows, category);
        state.needs_update = false;
    }

    ui.heading(format!("{} Flows", category.name));
    ui.separator();

    // Display category totals
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.label("Last Year:");
            ui.label(format!("${:.2}", state.last_year_total));
            ui.add_space(20.0);
            
            ui.label("This Year:");
            ui.label(format!("${:.2}", state.this_year_total));
            ui.add_space(20.0);

            ui.label("Current Month:");
            ui.label(format!("${:.2}", state.current_month_total));
            ui.add_space(20.0);

            if let Some(ratio) = state.tracking_ratio {
                ui.label("Year Tracking Ratio:");
                let ratio_text = format!("{:.2}", ratio);
                let color = if ratio >= 1.0 {
                    egui::Color32::GREEN
                } else {
                    egui::Color32::RED
                };
                ui.label(egui::RichText::new(ratio_text).color(color));
            }
        });
    });

    if ui.button("Add Flow").clicked() {
        app.create_new_flow(category);
    }

    // Show flows table
    show_flows_table(ui, app, category);
}

fn show_flows_table(ui: &mut egui::Ui, app: &mut PreftApp, category: &Category) {
    egui::ScrollArea::vertical()
        .id_source(format!("flows_scroll_{}", category.id))
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Grid::new(format!("flows_grid_{}", category.id))
                .striped(true)
                .show(ui, |ui| {
                    // Header row
                    ui.label("Date");
                    ui.label("Amount");
                    ui.label("Description");
                    if category.tax_deduction.deduction_allowed {
                        ui.label("Tax Deductible");
                    }
                    for field in &category.fields {
                        ui.label(field.display_name());
                    }
                    ui.label(""); // Empty header for edit button column
                    ui.label(""); // Spacer
                    ui.label(""); // Empty header for delete button column
                    ui.end_row();

                    // Data rows
                    let mut flows: Vec<_> = app.flows.iter()
                        .filter(|f| f.category_id == category.id)
                        .filter(|f| {
                            if let Some(year) = app.user_settings.get_year_filter() {
                                f.date.year() == year
                            } else {
                                true
                            }
                        })
                        .cloned()
                        .collect();
                    
                    // Sort flows by date in descending order (newest first)
                    flows.sort_by(|a, b| b.date.cmp(&a.date));

                    for flow in flows {
                        // Date cell
                        ui.label(flow.date.to_string());
                        
                        // Amount cell
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(format!("${:.2}", flow.amount));
                        });
                        
                        // Description cell
                        ui.label(&flow.description);
                        
                        // Tax deductible cell
                        if category.tax_deduction.deduction_allowed {
                            let symbol = match flow.tax_deductible {
                                Some(true) => "[X]",
                                Some(false) => "[ ]",
                                None => "[ ]",
                            };
                            ui.label(symbol);
                        }

                        // Custom fields cells
                        for field in &category.fields {
                            if let Some(value) = flow.custom_fields.get(&field.name) {
                                match field.field_type {
                                    crate::models::FieldType::Boolean => {
                                        if value.parse::<bool>().unwrap_or(false) {
                                            ui.label("[X]");
                                        } else {
                                            ui.label("[ ]");
                                        }
                                    },
                                    crate::models::FieldType::Currency => {
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            if let Ok(num) = value.replace(['$', ','], "").parse::<f64>() {
                                                ui.label(format!("${:.2}", num));
                                            } else {
                                                ui.label(value);
                                            }
                                        });
                                    },
                                    crate::models::FieldType::Integer => {
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            if let Ok(num) = value.parse::<i64>() {
                                                ui.label(num.to_string());
                                            } else {
                                                ui.label(value);
                                            }
                                        });
                                    },
                                    crate::models::FieldType::Float => {
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            if let Ok(num) = value.parse::<f64>() {
                                                ui.label(format!("{:.2}", num));
                                            } else {
                                                ui.label(value);
                                            }
                                        });
                                    },
                                    _ => {
                                        let mut display_value = value.clone();
                                        if !display_value.is_empty() {
                                            let mut chars: Vec<char> = display_value.chars().collect();
                                            if let Some(first) = chars.first_mut() {
                                                *first = first.to_uppercase().next().unwrap_or(*first);
                                            }
                                            display_value = chars.into_iter().collect();
                                        }
                                        ui.label(&display_value);
                                    }
                                }
                            } else {
                                ui.label("");
                            }
                        }

                        // Edit button cell
                        if ui.button("Edit").clicked() {
                            app.set_editing_flow(flow.clone());
                            app.custom_field_values.clear();
                            for field in &category.fields {
                                if let Some(value) = flow.custom_fields.get(&field.name) {
                                    app.custom_field_values.insert(field.name.clone(), value.clone());
                                } else if let Some(default) = &field.default_value {
                                    app.custom_field_values.insert(field.name.clone(), default.clone());
                                }
                            }
                        }

                        ui.label("");

                        // Delete button
                        if ui.button("Delete").clicked() {
                            if let Err(e) = app.delete_flow(&flow.id) {
                                ui.label(egui::RichText::new(format!("Error deleting flow: {}", e))
                                    .color(egui::Color32::RED));
                            }
                        }

                        ui.end_row();
                    }
                });
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{FlowType, TaxDeductionInfo};
    use std::collections::HashMap;

    fn category(id: &str) -> Category {
        Category {
            id: id.to_string(),
            name: format!("Category {}", id),
            flow_type: FlowType::Expense,
            parent_id: None,
            fields: Vec::new(),
            tax_deduction: TaxDeductionInfo { deduction_allowed: false, default_value: false },
        }
    }

    fn flow(category_id: &str, date: NaiveDate, amount: f64) -> Flow {
        Flow {
            id: uuid::Uuid::new_v4().to_string(),
            date,
            amount,
            category_id: category_id.to_string(),
            description: String::new(),
            linked_flows: Vec::new(),
            custom_fields: HashMap::new(),
            tax_deductible: None,
        }
    }

    #[test]
    fn update_totals_computes_last_year_this_year_and_current_month() {
        let cat = category("cat-1");
        let as_of = NaiveDate::from_ymd_opt(2024, 6, 15).unwrap();
        let flows = vec![
            flow("cat-1", NaiveDate::from_ymd_opt(2023, 3, 1).unwrap(), 100.0), // last year
            flow("cat-1", NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(), 50.0),  // this year, not this month
            flow("cat-1", NaiveDate::from_ymd_opt(2024, 6, 10).unwrap(), 20.0), // this year, this month
            flow("other-cat", NaiveDate::from_ymd_opt(2024, 6, 10).unwrap(), 999.0), // different category
        ];

        let mut state = CategoryFlowsState::new();
        state.update_totals_as_of(&flows, &cat, as_of);

        assert_eq!(state.last_year_total, 100.0);
        assert_eq!(state.this_year_total, 70.0);
        assert_eq!(state.current_month_total, 20.0);
    }

    #[test]
    fn update_totals_skipped_until_marked_for_update_again() {
        let cat = category("cat-1");
        let as_of = NaiveDate::from_ymd_opt(2024, 6, 15).unwrap();
        let initial_flows = vec![flow("cat-1", as_of, 100.0)];

        let mut state = CategoryFlowsState::new(); // needs_update starts true
        state.update_totals_as_of(&initial_flows, &cat, as_of);
        assert_eq!(state.this_year_total, 100.0);

        // Totals shouldn't change on a second call without mark_for_update,
        // even though the flows passed in are different.
        let different_flows = vec![flow("cat-1", as_of, 500.0)];
        state.update_totals_as_of(&different_flows, &cat, as_of);
        assert_eq!(state.this_year_total, 100.0, "should not recompute until marked for update again");

        state.mark_for_update();
        state.update_totals_as_of(&different_flows, &cat, as_of);
        assert_eq!(state.this_year_total, 500.0, "should recompute after mark_for_update");
    }

    #[test]
    fn new_state_defaults_to_zero_and_needs_update() {
        let state = CategoryFlowsState::new();
        assert_eq!(state.last_year_total, 0.0);
        assert_eq!(state.this_year_total, 0.0);
        assert_eq!(state.current_month_total, 0.0);
        assert_eq!(state.tracking_ratio, None);
        assert!(state.needs_update);
    }
} 