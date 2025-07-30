use eframe::egui;
use chrono::{Local, Datelike};
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
        if !self.needs_update && self.financial_summary.is_some() {
            return;
        }

        let current_year = Local::now().year();
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
        if !self.needs_update && !self.tracking_ratios.is_empty() {
            return;
        }

        self.tracking_ratios.clear();
        for category in categories {
            if let Some(ratio) = utils::calculate_tracking_ratio(flows, category) {
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