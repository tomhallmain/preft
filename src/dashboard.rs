use eframe::egui;
use chrono::{Local, Datelike};
use crate::models::{Flow, Category, FlowType};
use crate::utils;
use log::warn;

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

    fn update_tracking_ratios(&mut self, flows: &[Flow], categories: &[Category]) {
        if !self.needs_update {
            return;
        }

        self.tracking_ratios.clear();
        for category in categories {
            if let Some(ratio) = utils::calculate_tracking_ratio(flows, category) {
                self.tracking_ratios.push((category.name.clone(), ratio));
            }
        }
        // Sort by tracking ratio (lowest first)
        self.tracking_ratios.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
        
        self.needs_update = false;
    }

    fn update_financial_summary(&mut self, flows: &[Flow], categories: &[Category]) {
        if !self.needs_update && self.financial_summary.is_some() {
            return;
        }

        let current_year = Local::now().year();
        let mut total_income = 0.0;
        let mut total_expenses = 0.0;

        for flow in flows {
            if flow.date.year() == current_year {
                // Find the category for this flow
                if let Some(category) = categories.iter().find(|c| c.id == flow.category_id) {
                    match category.flow_type {
                        FlowType::Income => total_income += flow.amount,
                        FlowType::Expense => total_expenses += flow.amount,
                    }
                } else {
                    warn!("Flow {} (date: {}) has no matching category (category_id: {})", 
                        flow.id, flow.date, flow.category_id);
                }
            }
        }

        let net_total = total_income - total_expenses;
        self.financial_summary = Some((total_income, total_expenses, net_total));
    }

    pub fn show(&mut self, ui: &mut egui::Ui, flows: &[Flow], categories: &[Category]) {
        // Update tracking ratios if needed
        self.update_tracking_ratios(flows, categories);
        self.update_financial_summary(flows, categories);

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
        ui.heading("Category Tracking");
        egui::ScrollArea::vertical()
            .id_source("dashboard_category_tracking")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Grid::new("category_tracking_grid")
                    .striped(true)
                    .show(ui, |ui| {
                        ui.label("Category");
                        ui.label("Tracking Ratio");
                        ui.end_row();

                        for (name, ratio) in &self.tracking_ratios {
                            ui.label(name);
                            let ratio_text = format!("{:.2}", ratio);
                            let color = if *ratio >= 1.0 {
                                egui::Color32::GREEN  // On track or ahead
                            } else {
                                egui::Color32::RED  // Behind
                            };
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(egui::RichText::new(ratio_text).color(color));
                            });
                            ui.end_row();
                        }
                    });
            });

        ui.separator();

        // Recent Transactions
        ui.heading("Recent Transactions");
        let mut recent_flows: Vec<_> = flows.iter().collect();
        recent_flows.sort_by(|a, b| b.date.cmp(&a.date));
        recent_flows.truncate(5); // Show only the 5 most recent transactions

        egui::ScrollArea::vertical()
            .id_source("dashboard_recent_transactions")
            .auto_shrink([false, false])
            .show(ui, |ui| {
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

        egui::ScrollArea::vertical()
            .id_source("dashboard_category_breakdown")
            .auto_shrink([false, false])
            .show(ui, |ui| {
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
            });
    }
} 