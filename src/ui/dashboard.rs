use eframe::egui;
use chrono::{Local, NaiveDate, Datelike};
use log::{info, warn, error};

use crate::models::{Flow, Category};
use crate::utils;

pub struct Dashboard {
    tracking_ratios: Vec<(String, f64)>,
    needs_update: bool,
    financial_summary: Option<(f64, f64, f64)>, // (income, expenses, net)
}

impl Dashboard {
    pub fn new() -> Self {
        Self {
            tracking_ratios: Vec::new(),
            needs_update: true,
            financial_summary: None,
        }
    }

    pub fn mark_for_update(&mut self) {
        self.needs_update = true;
    }

    fn update_financial_summary(&mut self, flows: &[Flow], categories: &[Category]) {
        self.update_financial_summary_as_of(flows, categories, Local::now().naive_local().date());
    }

    /// Core of `update_financial_summary`, parameterized on "today" so it's
    /// testable without depending on the wall clock.
    fn update_financial_summary_as_of(&mut self, flows: &[Flow], categories: &[Category], as_of: NaiveDate) {
        if !self.needs_update && self.financial_summary.is_some() {
            return;
        }

        let current_year = as_of.year();
        let mut total_income = 0.0;
        let mut total_expenses = 0.0;

        for flow in flows {
            if flow.date.year() == current_year {
                if let Some(category) = categories.iter().find(|c| c.id == flow.category_id) {
                    match category.flow_type {
                        crate::models::FlowType::Income => total_income += flow.amount,
                        crate::models::FlowType::Expense => total_expenses += flow.amount,
                    }
                } else {
                    log::warn!("Flow {} (date: {}) has no matching category (category_id: {})",
                        flow.id, flow.date, flow.category_id);
                }
            }
        }

        let net_total = total_income - total_expenses;
        self.financial_summary = Some((total_income, total_expenses, net_total));
    }

    fn update_tracking_ratios(&mut self, flows: &[Flow], categories: &[Category]) {
        self.update_tracking_ratios_as_of(flows, categories, Local::now().naive_local().date());
    }

    /// Core of `update_tracking_ratios`, parameterized on "today" so it's
    /// testable without depending on the wall clock.
    fn update_tracking_ratios_as_of(&mut self, flows: &[Flow], categories: &[Category], as_of: NaiveDate) {
        if !self.needs_update && !self.tracking_ratios.is_empty() {
            return;
        }

        self.tracking_ratios.clear();
        for category in categories {
            if let Some(ratio) = utils::calculate_tracking_ratio_as_of(flows, category, as_of) {
                self.tracking_ratios.push((category.name.clone(), ratio));
            }
        }
        // Sort by tracking ratio (lowest first)
        self.tracking_ratios.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    }

    pub fn show(&mut self, ui: &mut egui::Ui, flows: &[Flow], categories: &[Category]) {
        // Update financial summary and tracking ratios if needed
        self.update_financial_summary(flows, categories);
        self.update_tracking_ratios(flows, categories);
        
        // Reset the update flag after both functions have run
        self.needs_update = false;

        ui.heading("Financial Dashboard");
        ui.separator();

        // Financial Summary
        ui.heading("Financial Summary");
        if let Some((income, expenses, net)) = self.financial_summary {
            egui::Grid::new("financial_summary_grid")
                .striped(true)
                .show(ui, |ui| {
                    ui.label("Total Income:");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(format!("${:.2}", income));
                    });
                    ui.end_row();

                    ui.label("Total Expenses:");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(format!("${:.2}", expenses));
                    });
                    ui.end_row();

                    ui.label("Net Total:");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let color = if net >= 0.0 {
                            egui::Color32::GREEN
                        } else {
                            egui::Color32::RED
                        };
                        ui.label(egui::RichText::new(format!("${:.2}", net)).color(color));
                    });
                    ui.end_row();
                });
        }

        ui.separator();

        // Category Tracking Ratios
        ui.heading("Category Tracking Ratios");
        egui::Grid::new("tracking_ratios_grid")
            .striped(true)
            .show(ui, |ui| {
                for (category_name, ratio) in &self.tracking_ratios {
                    ui.label(category_name);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let color = if *ratio >= 1.0 {
                            egui::Color32::GREEN
                        } else {
                            egui::Color32::RED
                        };
                        ui.label(egui::RichText::new(format!("{:.2}", ratio)).color(color));
                    });
                    ui.end_row();
                }
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{FlowType, TaxDeductionInfo};
    use std::collections::HashMap;

    fn category(id: &str, flow_type: FlowType) -> Category {
        Category {
            id: id.to_string(),
            name: format!("Category {}", id),
            flow_type,
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
    fn financial_summary_separates_income_and_expenses_by_flow_type() {
        let categories = vec![
            category("income-cat", FlowType::Income),
            category("expense-cat", FlowType::Expense),
        ];
        let as_of = NaiveDate::from_ymd_opt(2024, 6, 15).unwrap();
        let flows = vec![
            flow("income-cat", NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(), 1000.0),
            flow("expense-cat", NaiveDate::from_ymd_opt(2024, 2, 1).unwrap(), 300.0),
        ];

        let mut dashboard = Dashboard::new();
        dashboard.update_financial_summary_as_of(&flows, &categories, as_of);

        assert_eq!(dashboard.financial_summary, Some((1000.0, 300.0, 700.0)));
    }

    #[test]
    fn financial_summary_excludes_flows_from_other_years() {
        let categories = vec![category("income-cat", FlowType::Income)];
        let as_of = NaiveDate::from_ymd_opt(2024, 6, 15).unwrap();
        let flows = vec![
            flow("income-cat", NaiveDate::from_ymd_opt(2023, 12, 31).unwrap(), 1000.0),
            flow("income-cat", NaiveDate::from_ymd_opt(2025, 1, 1).unwrap(), 1000.0),
        ];

        let mut dashboard = Dashboard::new();
        dashboard.update_financial_summary_as_of(&flows, &categories, as_of);

        assert_eq!(dashboard.financial_summary, Some((0.0, 0.0, 0.0)));
    }

    #[test]
    fn financial_summary_skips_flows_with_no_matching_category_instead_of_panicking() {
        let categories: Vec<Category> = Vec::new(); // no categories at all
        let as_of = NaiveDate::from_ymd_opt(2024, 6, 15).unwrap();
        let flows = vec![flow("missing-cat", as_of, 500.0)];

        let mut dashboard = Dashboard::new();
        dashboard.update_financial_summary_as_of(&flows, &categories, as_of);

        assert_eq!(dashboard.financial_summary, Some((0.0, 0.0, 0.0)));
    }

    #[test]
    fn financial_summary_not_recomputed_until_marked_for_update_again() {
        let categories = vec![category("income-cat", FlowType::Income)];
        let as_of = NaiveDate::from_ymd_opt(2024, 6, 15).unwrap();
        let flows = vec![flow("income-cat", as_of, 100.0)];

        let mut dashboard = Dashboard::new(); // needs_update starts true
        dashboard.update_financial_summary_as_of(&flows, &categories, as_of);
        assert_eq!(dashboard.financial_summary, Some((100.0, 0.0, 100.0)));

        // update_financial_summary itself never clears needs_update -- only
        // `show()` does, after both it and update_tracking_ratios have run.
        // Simulate that here rather than going through `show()` (which needs
        // an egui::Ui to call).
        dashboard.needs_update = false;

        let different_flows = vec![flow("income-cat", as_of, 999.0)];
        dashboard.update_financial_summary_as_of(&different_flows, &categories, as_of);
        assert_eq!(
            dashboard.financial_summary,
            Some((100.0, 0.0, 100.0)),
            "should not recompute until marked for update again"
        );

        dashboard.mark_for_update();
        dashboard.update_financial_summary_as_of(&different_flows, &categories, as_of);
        assert_eq!(dashboard.financial_summary, Some((999.0, 0.0, 999.0)));
    }

    #[test]
    fn tracking_ratios_sorted_lowest_first() {
        let as_of = NaiveDate::from_ymd_opt(2024, 7, 1).unwrap(); // year_progress = 0.5 (2024 is a leap year)
        let categories = vec![
            category("ahead", FlowType::Expense),
            category("behind", FlowType::Expense),
        ];
        let flows = vec![
            // "ahead": on pace to double last year's total (ratio 2.0)
            flow("ahead", NaiveDate::from_ymd_opt(2023, 6, 1).unwrap(), 1000.0),
            flow("ahead", NaiveDate::from_ymd_opt(2024, 6, 1).unwrap(), 1000.0),
            // "behind": far behind last year's pace (ratio near 0)
            flow("behind", NaiveDate::from_ymd_opt(2023, 6, 1).unwrap(), 1_000_000.0),
            flow("behind", NaiveDate::from_ymd_opt(2024, 6, 1).unwrap(), 1.0),
        ];

        let mut dashboard = Dashboard::new();
        dashboard.update_tracking_ratios_as_of(&flows, &categories, as_of);

        assert_eq!(dashboard.tracking_ratios.len(), 2);
        assert_eq!(dashboard.tracking_ratios[0].0, "Category behind", "lowest ratio should sort first");
        assert_eq!(dashboard.tracking_ratios[1].0, "Category ahead");
    }

    #[test]
    fn new_dashboard_defaults_to_needs_update_with_no_summary() {
        let dashboard = Dashboard::new();
        assert!(dashboard.needs_update);
        assert_eq!(dashboard.financial_summary, None);
        assert!(dashboard.tracking_ratios.is_empty());
    }
} 