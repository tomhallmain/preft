use eframe::egui;
use chrono::{Local, Datelike};
use crate::models::{Flow, Category};

pub struct Dashboard {
    // Add any state needed for the dashboard here
}

impl Dashboard {
    pub fn new() -> Self {
        Self {
            // Initialize any state here
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui, flows: &[Flow], categories: &[Category]) {
        ui.heading("Financial Dashboard");
        ui.separator();

        // Financial Summary
        ui.heading("Financial Summary");
        let current_year = Local::now().year();
        let mut total_income = 0.0;
        let mut total_expenses = 0.0;

        for flow in flows {
            if flow.date.year() == current_year {
                if flow.amount > 0.0 {
                    total_income += flow.amount;
                } else {
                    total_expenses += flow.amount.abs();
                }
            }
        }

        let net_total = total_income - total_expenses;

        egui::Grid::new("financial_summary_grid")
            .striped(true)
            .show(ui, |ui| {
                ui.label("Total Income:");
                ui.label(format!("${:.2}", total_income));
                ui.end_row();

                ui.label("Total Expenses:");
                ui.label(format!("${:.2}", total_expenses));
                ui.end_row();

                ui.label("Net Total:");
                ui.label(format!("${:.2}", net_total));
                ui.end_row();
            });

        ui.separator();

        // Recent Transactions
        ui.heading("Recent Transactions");
        let mut recent_flows: Vec<_> = flows.iter().collect();
        recent_flows.sort_by(|a, b| b.date.cmp(&a.date));
        recent_flows.truncate(5); // Show only the 5 most recent transactions

        egui::Grid::new("recent_transactions_grid")
            .striped(true)
            .show(ui, |ui| {
                ui.label("Date");
                ui.label("Category");
                ui.label("Amount");
                ui.label("Description");
                ui.end_row();

                for flow in recent_flows {
                    let category_name = categories
                        .iter()
                        .find(|c| c.id == flow.category_id)
                        .map(|c| c.name.clone())
                        .unwrap_or_else(|| "Unknown".to_string());

                    ui.label(flow.date.format("%Y-%m-%d").to_string());
                    ui.label(category_name);
                    ui.label(format!("${:.2}", flow.amount));
                    ui.label(&flow.description);
                    ui.end_row();
                }
            });

        ui.separator();

        // Category Breakdown
        ui.heading("Category Breakdown");
        let mut category_totals: Vec<(String, f64)> = Vec::new();
        for category in categories {
            let total: f64 = flows
                .iter()
                .filter(|f| f.category_id == category.id)
                .map(|f| f.amount)
                .sum();
            category_totals.push((category.name.clone(), total));
        }
        category_totals.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

        egui::Grid::new("category_breakdown_grid")
            .striped(true)
            .show(ui, |ui| {
                ui.label("Category");
                ui.label("Total");
                ui.end_row();

                for (name, total) in category_totals {
                    ui.label(name);
                    ui.label(format!("${:.2}", total));
                    ui.end_row();
                }
            });
    }
} 