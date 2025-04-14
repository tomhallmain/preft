use chrono::{Local, Datelike};
use crate::models::{Flow, Category};

pub fn calculate_tracking_ratio(flows: &[Flow], category: &Category) -> Option<f64> {
    let current_date = chrono::Local::now();
    let current_year = current_date.year();
    let current_month = current_date.month();
    
    // Get flows for this category
    let category_flows: Vec<_> = flows.iter()
        .filter(|f| f.category_id == category.id)
        .collect();
    
    // Calculate last year's total
    let last_year_total: f64 = category_flows.iter()
        .filter(|f| f.date.year() == current_year - 1)
        .map(|f| f.amount)
        .sum();
    
    // Calculate this year's total
    let this_year_total: f64 = category_flows.iter()
        .filter(|f| f.date.year() == current_year)
        .map(|f| f.amount)
        .sum();
    
    // If there was no data last year, return 9999.0
    if last_year_total == 0.0 {
        if this_year_total == 0.0 {
            return None;
        } else {
            return Some(9999.0);
        }
    }
    
    // Calculate the proportion of the year that has passed
    let current_day = current_date.ordinal() as f64;
    let days_in_year = if chrono::NaiveDate::from_ymd_opt(current_year, 12, 31).unwrap().leap_year() {
        366.0
    } else {
        365.0
    };
    let year_progress = current_day / days_in_year;
    
    // Calculate what proportion of last year's total we should have by now
    let expected_this_year = last_year_total * year_progress;
    
    // Calculate the tracking ratio (actual vs expected)
    let ratio = this_year_total / expected_this_year;
    
    // If ratio exceeds 9999.0, return 9999.0
    if ratio > 9999.0 {
        Some(9999.0)
    } else {
        Some(ratio)
    }
} 