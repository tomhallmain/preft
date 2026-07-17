use chrono::{Datelike, NaiveDate};
use crate::models::{Flow, Category};

pub fn calculate_tracking_ratio(flows: &[Flow], category: &Category) -> Option<f64> {
    calculate_tracking_ratio_as_of(flows, category, chrono::Local::now().naive_local().date())
}

/// Core of `calculate_tracking_ratio`, parameterized on "today" so it can be
/// tested deterministically instead of depending on the wall clock. Visible
/// within the crate (not just this module) so callers like `Dashboard` and
/// `CategoryFlowsState` can compute a tracking ratio consistent with an
/// explicit `as_of` date of their own, rather than re-reading the wall clock
/// independently.
pub(crate) fn calculate_tracking_ratio_as_of(flows: &[Flow], category: &Category, as_of: NaiveDate) -> Option<f64> {
    let current_year = as_of.year();

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
    let current_day = as_of.ordinal() as f64;
    let days_in_year = if NaiveDate::from_ymd_opt(current_year, 12, 31).unwrap().leap_year() {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Category, FlowType, TaxDeductionInfo};
    use std::collections::HashMap;

    fn category() -> Category {
        Category {
            id: "cat-1".to_string(),
            name: "Test Category".to_string(),
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
    fn no_flows_for_category_returns_none() {
        let cat = category();
        let as_of = NaiveDate::from_ymd_opt(2024, 7, 1).unwrap();
        assert_eq!(calculate_tracking_ratio_as_of(&[], &cat, as_of), None);
    }

    #[test]
    fn no_data_either_year_returns_none() {
        let cat = category();
        let as_of = NaiveDate::from_ymd_opt(2024, 7, 1).unwrap();
        let flows = vec![flow(&cat.id, NaiveDate::from_ymd_opt(2022, 1, 1).unwrap(), 100.0)];
        assert_eq!(calculate_tracking_ratio_as_of(&flows, &cat, as_of), None);
    }

    #[test]
    fn no_last_year_data_but_this_year_data_returns_9999() {
        let cat = category();
        let as_of = NaiveDate::from_ymd_opt(2024, 7, 1).unwrap();
        let flows = vec![flow(&cat.id, NaiveDate::from_ymd_opt(2024, 3, 1).unwrap(), 50.0)];
        assert_eq!(calculate_tracking_ratio_as_of(&flows, &cat, as_of), Some(9999.0));
    }

    #[test]
    fn on_pace_ratio_is_one_in_leap_year() {
        // 2024 is a leap year (366 days). July 1 is ordinal day 183 -> exactly
        // half the year has passed (183 / 366 = 0.5), so a clean fraction of
        // last year's total lets us assert an exact expected ratio.
        let cat = category();
        let as_of = NaiveDate::from_ymd_opt(2024, 7, 1).unwrap();
        assert_eq!(as_of.ordinal(), 183);

        let flows = vec![
            flow(&cat.id, NaiveDate::from_ymd_opt(2023, 6, 1).unwrap(), 1000.0), // last year total
            flow(&cat.id, NaiveDate::from_ymd_opt(2024, 6, 1).unwrap(), 500.0),  // this year total
        ];
        let ratio = calculate_tracking_ratio_as_of(&flows, &cat, as_of).unwrap();
        assert!((ratio - 1.0).abs() < 1e-9, "expected ratio ~1.0, got {}", ratio);
    }

    #[test]
    fn on_pace_ratio_is_one_in_non_leap_year() {
        // 2023 is not a leap year (365 days). March 14 is ordinal day 73 ->
        // 73 / 365 = 0.2 exactly, another clean fraction to assert against.
        let cat = category();
        let as_of = NaiveDate::from_ymd_opt(2023, 3, 14).unwrap();
        assert_eq!(as_of.ordinal(), 73);

        let flows = vec![
            flow(&cat.id, NaiveDate::from_ymd_opt(2022, 6, 1).unwrap(), 1000.0), // last year total
            flow(&cat.id, NaiveDate::from_ymd_opt(2023, 2, 1).unwrap(), 200.0),  // this year total
        ];
        let ratio = calculate_tracking_ratio_as_of(&flows, &cat, as_of).unwrap();
        assert!((ratio - 1.0).abs() < 1e-9, "expected ratio ~1.0, got {}", ratio);
    }

    #[test]
    fn behind_pace_gives_ratio_below_one() {
        let cat = category();
        let as_of = NaiveDate::from_ymd_opt(2024, 7, 1).unwrap(); // year_progress = 0.5
        let flows = vec![
            flow(&cat.id, NaiveDate::from_ymd_opt(2023, 6, 1).unwrap(), 1_000_000.0),
            flow(&cat.id, NaiveDate::from_ymd_opt(2024, 6, 1).unwrap(), 1.0),
        ];
        let ratio = calculate_tracking_ratio_as_of(&flows, &cat, as_of).unwrap();
        assert!(ratio < 1.0, "expected ratio below 1.0, got {}", ratio);
    }

    #[test]
    fn ratio_clamps_at_9999() {
        let cat = category();
        // Regardless of where in the year `as_of` falls, a this-year total this
        // far above last year's total will always produce a ratio > 9999.
        let as_of = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap();
        let flows = vec![
            flow(&cat.id, NaiveDate::from_ymd_opt(2023, 6, 1).unwrap(), 1.0),
            flow(&cat.id, NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(), 1_000_000.0),
        ];
        assert_eq!(calculate_tracking_ratio_as_of(&flows, &cat, as_of), Some(9999.0));
    }

    #[test]
    fn flows_from_other_categories_are_ignored() {
        let cat = category();
        let as_of = NaiveDate::from_ymd_opt(2024, 7, 1).unwrap();
        let flows = vec![flow("other-category", NaiveDate::from_ymd_opt(2024, 3, 1).unwrap(), 500.0)];
        assert_eq!(calculate_tracking_ratio_as_of(&flows, &cat, as_of), None);
    }
}
