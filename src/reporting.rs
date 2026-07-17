use chrono::{NaiveDate, Datelike};
use std::collections::HashMap;
use crate::models::Flow;
use printpdf::*;
use std::io::{Cursor, BufWriter, Write};
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub enum TimePeriod {
    LastYear,
    ThisYear,
    Custom(NaiveDate, NaiveDate),
}

impl TimePeriod {
    /// Core of `Default::default`, parameterized on "today" so it's testable
    /// without depending on the wall clock.
    fn default_for(today: NaiveDate) -> Self {
        // If we're significantly after tax time (after April), default to this year
        // Otherwise default to last year
        if today.month() > 4 {
            TimePeriod::ThisYear
        } else {
            TimePeriod::LastYear
        }
    }

    /// Whether `date` falls within this period, as of `today`. `LastYear` is
    /// the half-open range [Jan 1 last year, Jan 1 this year); `ThisYear` is
    /// [Jan 1 this year, today] inclusive; `Custom` is inclusive on both ends.
    fn contains(&self, date: NaiveDate, today: NaiveDate) -> bool {
        match self {
            TimePeriod::LastYear => {
                let start = today.with_month(1).unwrap().with_day(1).unwrap();
                let end = start.with_year(start.year() - 1).unwrap();
                date >= end && date < start
            },
            TimePeriod::ThisYear => {
                let start = today.with_month(1).unwrap().with_day(1).unwrap();
                date >= start && date <= today
            },
            TimePeriod::Custom(start, end) => {
                date >= *start && date <= *end
            },
        }
    }
}

impl Default for TimePeriod {
    fn default() -> Self {
        Self::default_for(chrono::Local::now().naive_local().date())
    }
}

/// Groups flows by the value of a custom field. Flows that don't have the
/// field set are silently excluded from the result entirely (not grouped
/// under any key) -- this mirrors the report's existing display behavior.
fn group_flows_by_field<'a>(flows: &[&'a Flow], field_name: &str) -> HashMap<String, Vec<&'a Flow>> {
    let mut grouped: HashMap<String, Vec<&'a Flow>> = HashMap::new();
    for flow in flows {
        if let Some(value) = flow.custom_fields.get(field_name) {
            grouped.entry(value.clone()).or_default().push(flow);
        }
    }
    grouped
}

#[derive(Debug, Clone, PartialEq, Copy)]
pub enum FontVariant {
    RobotoRegular,
    RobotoBold,
    RobotoItalic,
    RobotoBoldItalic,
    TimesRegular,
    TimesBold,
    TimesItalic,
    TimesBoldItalic,
}

impl FontVariant {
    pub fn get_font_path(&self) -> &'static str {
        match self {
            FontVariant::RobotoRegular => "assets/fonts/Roboto-Regular.ttf",
            FontVariant::RobotoBold => "assets/fonts/Roboto-Bold.ttf",
            FontVariant::RobotoItalic => "assets/fonts/Roboto-Italic.ttf",
            FontVariant::RobotoBoldItalic => "assets/fonts/Roboto-BoldItalic.ttf",
            _ => "", // Built-in fonts don't need paths
        }
    }

    pub fn get_builtin_font(&self) -> Option<BuiltinFont> {
        match self {
            FontVariant::TimesRegular => Some(BuiltinFont::TimesRoman),
            FontVariant::TimesBold => Some(BuiltinFont::TimesBold),
            FontVariant::TimesItalic => Some(BuiltinFont::TimesItalic),
            FontVariant::TimesBoldItalic => Some(BuiltinFont::TimesBoldItalic),
            _ => None,
        }
    }

    pub fn get_display_name(&self) -> &'static str {
        match self {
            FontVariant::RobotoRegular => "Roboto Regular",
            FontVariant::RobotoBold => "Roboto Bold",
            FontVariant::RobotoItalic => "Roboto Italic",
            FontVariant::RobotoBoldItalic => "Roboto Bold Italic",
            FontVariant::TimesRegular => "Times New Roman Regular",
            FontVariant::TimesBold => "Times New Roman Bold",
            FontVariant::TimesItalic => "Times New Roman Italic",
            FontVariant::TimesBoldItalic => "Times New Roman Bold Italic",
        }
    }
}

#[derive(Debug, Clone)]
pub struct FontSettings {
    pub title_font: FontVariant,
    pub subtitle_font: FontVariant,
    pub header_font: FontVariant,
    pub body_font: FontVariant,
}

impl Default for FontSettings {
    fn default() -> Self {
        Self {
            title_font: FontVariant::RobotoBold,
            subtitle_font: FontVariant::RobotoRegular,
            header_font: FontVariant::RobotoBold,
            body_font: FontVariant::RobotoRegular,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ReportRequest {
    pub time_period: TimePeriod,
    pub selected_flows: Vec<String>, // Flow IDs
    pub group_by: Option<String>, // Field name to group by
    pub title: String,
    pub subtitle: String,
    pub font_settings: FontSettings,
}

impl Default for ReportRequest {
    fn default() -> Self {
        Self {
            time_period: TimePeriod::default(),
            selected_flows: Vec::new(),
            group_by: None,
            title: "Financial Flows Report".to_string(),
            subtitle: String::new(),
            font_settings: FontSettings::default(),
        }
    }
}

pub struct ReportGenerator {
    flows: Vec<Flow>,
    categories: HashMap<String, String>, // category_id -> category_name
    title_font: Option<IndirectFontRef>,
    subtitle_font: Option<IndirectFontRef>,
    header_font: Option<IndirectFontRef>,
    body_font: Option<IndirectFontRef>,
}

impl ReportGenerator {
    pub fn new(flows: Vec<Flow>, categories: HashMap<String, String>) -> Self {
        Self { 
            flows,
            categories,
            title_font: None,
            subtitle_font: None,
            header_font: None,
            body_font: None,
        }
    }

    pub fn generate_report(&self, request: &ReportRequest) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        // Filter flows based on time period
        let today = chrono::Local::now().date_naive();
        let filtered_flows: Vec<&Flow> = self.flows.iter()
            .filter(|flow| request.time_period.contains(flow.date, today))
            .collect();

        // Sort flows by date (TODO: Add support for sorting by amount with higher priority)
        let mut sorted_flows = filtered_flows;
        sorted_flows.sort_by(|a, b| a.date.cmp(&b.date));

        // Create a new document
        let (doc, page1, layer1) = PdfDocument::new("Financial Report", Mm(210.0), Mm(297.0), "Layer 1");
        let current_layer = doc.get_page(page1).get_layer(layer1);

        // Load fonts
        let title_font = self.load_font(&doc, &request.font_settings.title_font)?;
        let subtitle_font = self.load_font(&doc, &request.font_settings.subtitle_font)?;
        let header_font = self.load_font(&doc, &request.font_settings.header_font)?;
        let body_font = self.load_font(&doc, &request.font_settings.body_font)?;

        // Add title
        current_layer.use_text(&request.title, 24.0, Mm(20.0), Mm(250.0), &title_font);
        
        // Add time period subheader
        let time_period_text = match &request.time_period {
            TimePeriod::LastYear => {
                let now = chrono::Local::now().date_naive();
                let start = now.with_month(1).unwrap().with_day(1).unwrap();
                let end = start.with_year(start.year() - 1).unwrap();
                format!("Time Period: {} to {}", end.format("%B %d, %Y"), start.format("%B %d, %Y"))
            },
            TimePeriod::ThisYear => {
                let now = chrono::Local::now().date_naive();
                let start = now.with_month(1).unwrap().with_day(1).unwrap();
                format!("Time Period: {} to {}", start.format("%B %d, %Y"), now.format("%B %d, %Y"))
            },
            TimePeriod::Custom(start, end) => {
                format!("Time Period: {} to {}", start.format("%B %d, %Y"), end.format("%B %d, %Y"))
            },
        };
        current_layer.use_text(&time_period_text, 12.0, Mm(20.0), Mm(230.0), &subtitle_font);

        // Group flows by category
        let mut category_flows: HashMap<String, Vec<&Flow>> = HashMap::new();
        for flow in sorted_flows {
            category_flows.entry(flow.category_id.clone())
                .or_default()
                .push(flow);
        }

        // Create a new page for each category
        let mut current_page = page1;
        let mut current_layer = layer1;
        let mut y_pos = Mm(200.0);
        let mut page_count = 1;

        // Store category totals for later use
        let mut category_totals: HashMap<String, f64> = HashMap::new();

        for (category_id, flows) in &category_flows {
            // If we're not on the first page, create a new page
            if page_count > 1 {
                let (page, layer) = doc.add_page(Mm(210.0), Mm(297.0), "Layer 1");
                current_page = page;
                current_layer = layer;
                y_pos = Mm(250.0);
            }

            // Add category header
            let layer = doc.get_page(current_page).get_layer(current_layer);
            let category_name = self.categories.get(category_id)
                .map(|name| name.as_str())
                .unwrap_or(category_id);
            layer.use_text(&format!("Category: {}", category_name), 16.0, Mm(20.0), y_pos, &header_font);
            y_pos -= Mm(15.0);

            // Add table headers
            layer.use_text("Date", 12.0, Mm(20.0), y_pos, &header_font);
            layer.use_text("Amount", 12.0, Mm(80.0), y_pos, &header_font);
            layer.use_text("Description", 12.0, Mm(140.0), y_pos, &header_font);
            y_pos -= Mm(10.0);

            // Add separator line
            layer.add_line_break();
            y_pos -= Mm(5.0);

            // Group flows if requested
            if let Some(group_by) = &request.group_by {
                let grouped_flows = group_flows_by_field(flows, group_by);

                // Add each group
                for (group_value, group_flows) in &grouped_flows {
                    layer.use_text(&format!("{}: {}", group_by, group_value), 14.0, Mm(20.0), y_pos, &header_font);
                    y_pos -= Mm(10.0);

                    // Add flows in this group
                    for flow in group_flows {
                        layer.use_text(&flow.date.format("%B %d, %Y").to_string(), 12.0, Mm(20.0), y_pos, &body_font);
                        layer.use_text(&format!("${:.2}", flow.amount), 12.0, Mm(80.0), y_pos, &body_font);
                        layer.use_text(&flow.description, 12.0, Mm(140.0), y_pos, &body_font);
                        y_pos -= Mm(8.0);
                    }

                    // Add group total
                    let group_total: f64 = group_flows.iter().map(|f| f.amount).sum();
                    layer.use_text("Group Total:", 12.0, Mm(20.0), y_pos, &body_font);
                    layer.use_text(&format!("${:.2}", group_total), 12.0, Mm(80.0), y_pos, &body_font);
                    y_pos -= Mm(15.0);
                }
            } else {
                // Add all flows without grouping
                for flow in flows {
                    layer.use_text(&flow.date.format("%B %d, %Y").to_string(), 12.0, Mm(20.0), y_pos, &body_font);
                    layer.use_text(&format!("${:.2}", flow.amount), 12.0, Mm(80.0), y_pos, &body_font);
                    layer.use_text(&flow.description, 12.0, Mm(140.0), y_pos, &body_font);
                    y_pos -= Mm(8.0);
                }
            }

            // Add category total
            let category_total: f64 = flows.iter().map(|f| f.amount).sum();
            category_totals.insert(category_id.clone(), category_total);
            layer.use_text(&format!("Category Total: ${:.2}", category_total), 14.0, Mm(20.0), y_pos, &header_font);
            y_pos -= Mm(20.0);

            page_count += 1;
        }

        // Add summary page
        let (summary_page, summary_layer) = doc.add_page(Mm(210.0), Mm(297.0), "Layer 1");
        let layer = doc.get_page(summary_page).get_layer(summary_layer);
        
        // Add summary title
        layer.use_text("Summary", 20.0, Mm(20.0), Mm(250.0), &header_font);
        
        // Add category totals
        let mut y_pos = Mm(220.0);
        let mut overall_total = 0.0;
        
        for (category_id, total) in &category_totals {
            overall_total += total;
            
            let category_name = self.categories.get(category_id)
                .map(|name| name.as_str())
                .unwrap_or(category_id);
            
            layer.use_text(&format!("{}: ${:.2}", category_name, total), 
                14.0, Mm(20.0), y_pos, &body_font);
            y_pos -= Mm(15.0);
        }
        
        // Add overall total
        layer.use_text(&format!("Overall Total: ${:.2}", overall_total), 
            16.0, Mm(20.0), y_pos, &header_font);

        // Save the document
        let mut buffer = Vec::new();
        {
            let mut writer = BufWriter::new(&mut buffer);
            doc.save(&mut writer)?;
        }
        Ok(buffer)
    }

    fn load_font(&self, doc: &PdfDocumentReference, variant: &FontVariant) -> Result<IndirectFontRef, Box<dyn std::error::Error>> {
        if let Some(builtin) = variant.get_builtin_font() {
            Ok(doc.add_builtin_font(builtin)?)
        } else {
            match variant {
                FontVariant::RobotoRegular => {
                    Ok(doc.add_external_font(&include_bytes!("../assets/fonts/Roboto-Regular.ttf")[..])?)
                },
                FontVariant::RobotoBold => {
                    Ok(doc.add_external_font(&include_bytes!("../assets/fonts/Roboto-Bold.ttf")[..])?)
                },
                FontVariant::RobotoItalic => {
                    Ok(doc.add_external_font(&include_bytes!("../assets/fonts/Roboto-Italic.ttf")[..])?)
                },
                FontVariant::RobotoBoldItalic => {
                    Ok(doc.add_external_font(&include_bytes!("../assets/fonts/Roboto-BoldItalic.ttf")[..])?)
                },
                _ => Ok(doc.add_builtin_font(BuiltinFont::TimesRoman)?),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flow(id: &str, date: NaiveDate, custom_fields: HashMap<String, String>) -> Flow {
        Flow {
            id: id.to_string(),
            date,
            amount: 10.0,
            category_id: "cat-1".to_string(),
            description: String::new(),
            linked_flows: Vec::new(),
            custom_fields,
            tax_deductible: None,
        }
    }

    // --- TimePeriod::default_for ---

    #[test]
    fn default_for_is_last_year_on_or_before_april() {
        for month in 1..=4 {
            let today = NaiveDate::from_ymd_opt(2024, month, 15).unwrap();
            assert_eq!(TimePeriod::default_for(today), TimePeriod::LastYear, "month {}", month);
        }
    }

    #[test]
    fn default_for_is_this_year_after_april() {
        for month in 5..=12 {
            let today = NaiveDate::from_ymd_opt(2024, month, 15).unwrap();
            assert_eq!(TimePeriod::default_for(today), TimePeriod::ThisYear, "month {}", month);
        }
    }

    // --- TimePeriod::contains ---

    #[test]
    fn last_year_covers_the_full_previous_calendar_year() {
        let today = NaiveDate::from_ymd_opt(2024, 6, 15).unwrap();
        let period = TimePeriod::LastYear;

        assert!(period.contains(NaiveDate::from_ymd_opt(2023, 1, 1).unwrap(), today));
        assert!(period.contains(NaiveDate::from_ymd_opt(2023, 12, 31).unwrap(), today));
        assert!(!period.contains(NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(), today), "current year's Jan 1 is excluded");
        assert!(!period.contains(NaiveDate::from_ymd_opt(2022, 12, 31).unwrap(), today), "the year before last is excluded");
    }

    #[test]
    fn this_year_covers_january_first_through_today_inclusive() {
        let today = NaiveDate::from_ymd_opt(2024, 6, 15).unwrap();
        let period = TimePeriod::ThisYear;

        assert!(period.contains(NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(), today));
        assert!(period.contains(today, today));
        assert!(!period.contains(NaiveDate::from_ymd_opt(2024, 6, 16).unwrap(), today), "dates after today are excluded");
        assert!(!period.contains(NaiveDate::from_ymd_opt(2023, 12, 31).unwrap(), today), "last year is excluded");
    }

    #[test]
    fn custom_range_is_inclusive_on_both_ends() {
        let today = NaiveDate::from_ymd_opt(2024, 6, 15).unwrap();
        let period = TimePeriod::Custom(
            NaiveDate::from_ymd_opt(2024, 3, 1).unwrap(),
            NaiveDate::from_ymd_opt(2024, 3, 31).unwrap(),
        );

        assert!(period.contains(NaiveDate::from_ymd_opt(2024, 3, 1).unwrap(), today));
        assert!(period.contains(NaiveDate::from_ymd_opt(2024, 3, 31).unwrap(), today));
        assert!(!period.contains(NaiveDate::from_ymd_opt(2024, 2, 29).unwrap(), today));
        assert!(!period.contains(NaiveDate::from_ymd_opt(2024, 4, 1).unwrap(), today));
    }

    // --- group_flows_by_field ---

    #[test]
    fn group_flows_by_field_groups_matching_values_together() {
        let date = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
        let mut fields_a = HashMap::new();
        fields_a.insert("vendor".to_string(), "Acme".to_string());
        let mut fields_b = HashMap::new();
        fields_b.insert("vendor".to_string(), "Acme".to_string());
        let mut fields_c = HashMap::new();
        fields_c.insert("vendor".to_string(), "Other".to_string());

        let flow_a = flow("a", date, fields_a);
        let flow_b = flow("b", date, fields_b);
        let flow_c = flow("c", date, fields_c);
        let flows: Vec<&Flow> = vec![&flow_a, &flow_b, &flow_c];

        let grouped = group_flows_by_field(&flows, "vendor");

        assert_eq!(grouped.len(), 2);
        assert_eq!(grouped.get("Acme").unwrap().len(), 2);
        assert_eq!(grouped.get("Other").unwrap().len(), 1);
    }

    #[test]
    fn group_flows_by_field_excludes_flows_missing_the_field() {
        let date = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
        let mut fields_a = HashMap::new();
        fields_a.insert("vendor".to_string(), "Acme".to_string());
        let fields_b = HashMap::new(); // no "vendor" field at all

        let flow_a = flow("a", date, fields_a);
        let flow_b = flow("b", date, fields_b);
        let flows: Vec<&Flow> = vec![&flow_a, &flow_b];

        let grouped = group_flows_by_field(&flows, "vendor");

        let total_grouped_flows: usize = grouped.values().map(|v| v.len()).sum();
        assert_eq!(total_grouped_flows, 1, "the flow missing the field should not appear in any group");
        assert_eq!(grouped.get("Acme").unwrap().len(), 1);
    }

    #[test]
    fn group_flows_by_field_empty_input_yields_empty_map() {
        let flows: Vec<&Flow> = Vec::new();
        let grouped = group_flows_by_field(&flows, "vendor");
        assert!(grouped.is_empty());
    }
} 