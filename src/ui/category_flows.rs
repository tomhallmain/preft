use eframe::egui;
use chrono::{Local, NaiveDate, Datelike};
use log::warn;

use crate::models::{Flow, Category};
use crate::app::PreftApp;
use crate::utils;

#[derive(Debug, Clone, Copy, PartialEq)]
enum SortColumn {
    Date,
    Amount,
    Description,
}

impl SortColumn {
    /// Direction a column starts in the first time it's selected.
    fn default_ascending(self) -> bool {
        match self {
            SortColumn::Date => false,        // newest first
            SortColumn::Amount => false,      // largest first
            SortColumn::Description => true,  // A-Z
        }
    }
}

/// Sorts flows in place by the given column/direction. `Description` sorts
/// case-insensitively so e.g. "apple" comes before "Banana".
fn sort_flows(flows: &mut [Flow], column: SortColumn, ascending: bool) {
    flows.sort_by(|a, b| {
        let ordering = match column {
            SortColumn::Date => a.date.cmp(&b.date),
            SortColumn::Amount => a.amount.partial_cmp(&b.amount).unwrap_or(std::cmp::Ordering::Equal),
            SortColumn::Description => a.description.to_lowercase().cmp(&b.description.to_lowercase()),
        };
        if ascending { ordering } else { ordering.reverse() }
    });
}

pub struct CategoryFlowsState {
    last_year_total: f64,
    this_year_total: f64,
    current_month_total: f64,
    tracking_ratio: Option<f64>,
    needs_update: bool,
    sort_column: SortColumn,
    sort_ascending: bool,
}

impl CategoryFlowsState {
    pub fn new() -> Self {
        Self {
            last_year_total: 0.0,
            this_year_total: 0.0,
            current_month_total: 0.0,
            tracking_ratio: None,
            needs_update: true,
            sort_column: SortColumn::Date,
            sort_ascending: false, // newest first, matching the table's prior hardcoded behavior
        }
    }

    pub fn mark_for_update(&mut self) {
        self.needs_update = true;
    }

    /// Clicking the active column's header flips its direction; clicking a
    /// different column switches to it at that column's default direction.
    fn toggle_sort(&mut self, column: SortColumn) {
        if self.sort_column == column {
            self.sort_ascending = !self.sort_ascending;
        } else {
            self.sort_column = column;
            self.sort_ascending = column.default_ascending();
        }
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

/// Renders a clickable column header, with a ▲/▼ indicator when it's the
/// active sort column, and returns the response so the caller can check
/// `.clicked()`.
fn sortable_header(ui: &mut egui::Ui, label: &str, column: SortColumn, active_column: SortColumn, ascending: bool) -> egui::Response {
    let text = if column == active_column {
        format!("{} {}", label, if ascending { "\u{25B2}" } else { "\u{25BC}" })
    } else {
        label.to_string()
    };
    ui.button(text)
}

fn show_flows_table(ui: &mut egui::Ui, app: &mut PreftApp, category: &Category) {
    let (sort_column, sort_ascending) = {
        let state = app.get_category_flows_state(&category.id);
        (state.sort_column, state.sort_ascending)
    };

    egui::ScrollArea::vertical()
        .id_source(format!("flows_scroll_{}", category.id))
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Grid::new(format!("flows_grid_{}", category.id))
                .striped(true)
                .show(ui, |ui| {
                    // Header row -- Date/Amount/Description are sortable by
                    // clicking; custom fields aren't (they're typed per-field
                    // and would need type-aware comparisons, unlike these three).
                    if sortable_header(ui, "Date", SortColumn::Date, sort_column, sort_ascending).clicked() {
                        app.get_category_flows_state(&category.id).toggle_sort(SortColumn::Date);
                    }
                    if sortable_header(ui, "Amount", SortColumn::Amount, sort_column, sort_ascending).clicked() {
                        app.get_category_flows_state(&category.id).toggle_sort(SortColumn::Amount);
                    }
                    if sortable_header(ui, "Description", SortColumn::Description, sort_column, sort_ascending).clicked() {
                        app.get_category_flows_state(&category.id).toggle_sort(SortColumn::Description);
                    }
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
                    
                    sort_flows(&mut flows, sort_column, sort_ascending);

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
        assert_eq!(state.sort_column, SortColumn::Date);
        assert!(!state.sort_ascending, "should default to newest-first, matching the table's prior hardcoded behavior");
    }

    // --- toggle_sort ---

    #[test]
    fn toggle_sort_flips_direction_when_clicking_the_active_column_again() {
        let mut state = CategoryFlowsState::new(); // Date, descending
        state.toggle_sort(SortColumn::Date);
        assert_eq!(state.sort_column, SortColumn::Date);
        assert!(state.sort_ascending);

        state.toggle_sort(SortColumn::Date);
        assert!(!state.sort_ascending);
    }

    #[test]
    fn toggle_sort_switches_column_and_resets_to_its_default_direction() {
        let mut state = CategoryFlowsState::new(); // Date, descending
        state.toggle_sort(SortColumn::Date); // Date, ascending now

        state.toggle_sort(SortColumn::Description);
        assert_eq!(state.sort_column, SortColumn::Description);
        assert!(state.sort_ascending, "Description defaults to ascending (A-Z)");

        state.toggle_sort(SortColumn::Amount);
        assert_eq!(state.sort_column, SortColumn::Amount);
        assert!(!state.sort_ascending, "Amount defaults to descending (largest first)");
    }

    // --- sort_flows ---

    #[test]
    fn sort_flows_by_date() {
        let mut flows = vec![
            flow("cat-1", NaiveDate::from_ymd_opt(2024, 3, 1).unwrap(), 10.0),
            flow("cat-1", NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(), 20.0),
            flow("cat-1", NaiveDate::from_ymd_opt(2024, 2, 1).unwrap(), 30.0),
        ];

        sort_flows(&mut flows, SortColumn::Date, true);
        assert_eq!(flows.iter().map(|f| f.date.month()).collect::<Vec<_>>(), vec![1, 2, 3]);

        sort_flows(&mut flows, SortColumn::Date, false);
        assert_eq!(flows.iter().map(|f| f.date.month()).collect::<Vec<_>>(), vec![3, 2, 1]);
    }

    #[test]
    fn sort_flows_by_amount() {
        let date = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
        let mut flows = vec![
            flow("cat-1", date, 30.0),
            flow("cat-1", date, 10.0),
            flow("cat-1", date, 20.0),
        ];

        sort_flows(&mut flows, SortColumn::Amount, true);
        assert_eq!(flows.iter().map(|f| f.amount).collect::<Vec<_>>(), vec![10.0, 20.0, 30.0]);

        sort_flows(&mut flows, SortColumn::Amount, false);
        assert_eq!(flows.iter().map(|f| f.amount).collect::<Vec<_>>(), vec![30.0, 20.0, 10.0]);
    }

    #[test]
    fn sort_flows_by_description_is_case_insensitive() {
        let date = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
        let mut flows = vec![
            flow_with_description("cat-1", date, "Banana"),
            flow_with_description("cat-1", date, "apple"),
            flow_with_description("cat-1", date, "cherry"),
        ];

        sort_flows(&mut flows, SortColumn::Description, true);
        assert_eq!(
            flows.iter().map(|f| f.description.as_str()).collect::<Vec<_>>(),
            vec!["apple", "Banana", "cherry"],
            "case-insensitive ascending should put 'apple' before 'Banana', unlike a raw byte-order sort"
        );
    }

    fn flow_with_description(category_id: &str, date: NaiveDate, description: &str) -> Flow {
        Flow {
            description: description.to_string(),
            ..flow(category_id, date, 0.0)
        }
    }
} 