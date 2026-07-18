use chrono::{NaiveDate, Datelike};
use std::collections::HashMap;
use crate::models::{CategoryField, FieldType, Flow, FlowType};
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

/// Per-category info a report needs: display name and flow type (for netting
/// the overall total), plus the category's custom field definitions (for
/// rendering per-flow custom field columns, formatted according to each
/// field's type).
#[derive(Debug, Clone)]
pub struct ReportCategoryInfo {
    pub name: String,
    pub flow_type: FlowType,
    pub fields: Vec<CategoryField>,
}

/// Nets per-category totals into a single overall total: Income category
/// totals add, Expense category totals subtract. Flow amounts are stored as
/// unsigned magnitudes (sign comes from the category's `FlowType`, the same
/// convention `Dashboard::update_financial_summary_as_of` uses), so a plain
/// sum of all category totals would overstate the result by counting
/// expenses as if they were income. A category id with no matching entry in
/// `categories` (e.g. the category was deleted after flows referencing it
/// were saved) contributes nothing, consistent with how the dashboard skips
/// such flows rather than guessing a sign for them.
fn net_total(category_totals: &HashMap<String, f64>, categories: &HashMap<String, ReportCategoryInfo>) -> f64 {
    category_totals.iter()
        .map(|(category_id, total)| match categories.get(category_id) {
            Some(info) if info.flow_type == FlowType::Income => *total,
            Some(info) if info.flow_type == FlowType::Expense => -*total,
            _ => 0.0,
        })
        .sum()
}

/// Custom fields to show as report columns for a category: all of them,
/// except the one currently selected as "Group By" (already shown as each
/// group's section header, so repeating it per row would be redundant).
fn visible_custom_fields<'a>(category_fields: &'a [CategoryField], group_by: &Option<String>) -> Vec<&'a CategoryField> {
    category_fields.iter()
        .filter(|f| group_by.as_deref() != Some(f.name.as_str()))
        .collect()
}

/// Formats a flow's custom field value for display, applying the same
/// per-type formatting used elsewhere in the app (currency symbols,
/// Yes/No for booleans, etc.) rather than printing the raw stored string.
fn format_field_value(field: &CategoryField, flow: &Flow) -> String {
    let Some(value) = flow.custom_fields.get(&field.name) else {
        return String::new();
    };
    match field.field_type {
        FieldType::Boolean => {
            if value.parse::<bool>().unwrap_or(false) { "Yes".to_string() } else { "No".to_string() }
        },
        FieldType::Currency => {
            match value.replace(['$', ','], "").parse::<f64>() {
                Ok(num) => format!("${:.2}", num),
                Err(_) => value.clone(),
            }
        },
        FieldType::Integer => value.parse::<i64>().map(|n| n.to_string()).unwrap_or_else(|_| value.clone()),
        FieldType::Float => value.parse::<f64>().map(|n| format!("{:.2}", n)).unwrap_or_else(|_| value.clone()),
        _ => value.clone(),
    }
}

/// Greedily wraps `text` into lines of at most `max_chars_per_line`
/// (Unicode scalar count, not accounting for variable glyph widths -- an
/// approximation, since exact text-width measurement isn't readily
/// available from `printpdf` for externally-loaded fonts). A word longer
/// than the limit gets its own line rather than being split mid-word.
/// Always returns at least one (possibly empty) line.
fn wrap_text(text: &str, max_chars_per_line: usize) -> Vec<String> {
    let max_chars_per_line = max_chars_per_line.max(1);
    let mut lines = Vec::new();
    let mut current_line = String::new();

    for word in text.split_whitespace() {
        let candidate_len = if current_line.is_empty() {
            word.chars().count()
        } else {
            current_line.chars().count() + 1 + word.chars().count()
        };

        if current_line.is_empty() || candidate_len <= max_chars_per_line {
            if !current_line.is_empty() {
                current_line.push(' ');
            }
            current_line.push_str(word);
        } else {
            lines.push(std::mem::take(&mut current_line));
            current_line.push_str(word);
        }
    }

    if !current_line.is_empty() || lines.is_empty() {
        lines.push(current_line);
    }
    lines
}

/// Rough character budget for a column of the given width, at the given
/// font size, using an approximate average glyph width (~half the font
/// size, a common heuristic for proportional fonts) rather than exact font
/// metrics.
fn max_chars_for_width(width_mm: f64, font_size_pt: f64) -> usize {
    const PT_TO_MM: f64 = 0.3528;
    let avg_char_width_mm = (font_size_pt * PT_TO_MM * 0.5).max(0.1);
    ((width_mm / avg_char_width_mm).floor() as usize).max(4)
}

/// Body text shrinks as more custom-field columns need to fit alongside
/// Date/Amount/Description, since `printpdf` lays out text at fixed
/// coordinates with no automatic column sizing.
fn body_font_size_for_extra_columns(extra_column_count: usize) -> f64 {
    match extra_column_count {
        0 => 12.0,
        1..=2 => 10.0,
        3..=4 => 8.5,
        _ => 7.0,
    }
}

/// X positions (mm from the page's left edge) and widths for the flow table's
/// columns: fixed-width Date/Amount, then Description and any extra custom
/// field columns sharing the remaining page width equally.
struct ColumnLayout {
    date_x: f64,
    amount_x: f64,
    description_x: f64,
    column_width: f64,
    extra_field_x: Vec<f64>,
}

fn compute_column_layout(extra_field_count: usize) -> ColumnLayout {
    const PAGE_WIDTH_MM: f64 = 210.0;
    const LEFT_MARGIN_MM: f64 = 20.0;
    const RIGHT_MARGIN_MM: f64 = 10.0;
    const DATE_WIDTH_MM: f64 = 35.0;
    const AMOUNT_WIDTH_MM: f64 = 25.0;
    const MIN_REMAINING_WIDTH_MM: f64 = 20.0;

    let date_x = LEFT_MARGIN_MM;
    let amount_x = date_x + DATE_WIDTH_MM;
    let description_x = amount_x + AMOUNT_WIDTH_MM;
    let remaining_width = (PAGE_WIDTH_MM - RIGHT_MARGIN_MM - description_x).max(MIN_REMAINING_WIDTH_MM);
    let column_count = (extra_field_count + 1) as f64; // Description + extras
    let column_width = remaining_width / column_count;

    let extra_field_x = (0..extra_field_count)
        .map(|i| description_x + column_width * (i as f64 + 1.0))
        .collect();

    ColumnLayout {
        date_x,
        amount_x,
        description_x,
        column_width,
        extra_field_x,
    }
}

/// Draws one flow's row: Date and Amount (always one line), then Description
/// and each visible custom field, word-wrapped to fit their column width.
/// Advances `y_pos` past however many wrapped lines the tallest column in
/// this row needed.
fn render_flow_row(
    layer: &PdfLayerReference,
    flow: &Flow,
    visible_fields: &[&CategoryField],
    layout: &ColumnLayout,
    body_size: f64,
    body_font: &IndirectFontRef,
    y_pos: &mut Mm,
) {
    const PT_TO_MM: f64 = 0.3528;
    let line_height = body_size * PT_TO_MM * 1.3;
    let max_chars = max_chars_for_width(layout.column_width, body_size);

    let description_lines = wrap_text(&flow.description, max_chars);
    let field_lines: Vec<Vec<String>> = visible_fields.iter()
        .map(|field| wrap_text(&format_field_value(field, flow), max_chars))
        .collect();

    let row_line_count = std::iter::once(description_lines.len())
        .chain(field_lines.iter().map(|lines| lines.len()))
        .max()
        .unwrap_or(1)
        .max(1);

    layer.use_text(&flow.date.format("%B %d, %Y").to_string(), body_size, Mm(layout.date_x), *y_pos, body_font);
    layer.use_text(&format!("${:.2}", flow.amount), body_size, Mm(layout.amount_x), *y_pos, body_font);

    let mut line_y = *y_pos;
    for line in &description_lines {
        layer.use_text(line, body_size, Mm(layout.description_x), line_y, body_font);
        line_y -= Mm(line_height);
    }

    for (field_idx, lines) in field_lines.iter().enumerate() {
        let mut line_y = *y_pos;
        for line in lines {
            layer.use_text(line, body_size, Mm(layout.extra_field_x[field_idx]), line_y, body_font);
            line_y -= Mm(line_height);
        }
    }

    *y_pos -= Mm(line_height * row_line_count as f64 + 2.0);
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
    categories: HashMap<String, ReportCategoryInfo>, // category_id -> info
    title_font: Option<IndirectFontRef>,
    subtitle_font: Option<IndirectFontRef>,
    header_font: Option<IndirectFontRef>,
    body_font: Option<IndirectFontRef>,
}

impl ReportGenerator {
    pub fn new(flows: Vec<Flow>, categories: HashMap<String, ReportCategoryInfo>) -> Self {
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
                .map(|info| info.name.as_str())
                .unwrap_or(category_id);
            layer.use_text(&format!("Category: {}", category_name), 16.0, Mm(20.0), y_pos, &header_font);
            y_pos -= Mm(15.0);

            // Custom fields (other than the active group-by field, which is
            // already shown as each group's section header) get their own
            // column, sharing the remaining page width with Description.
            let category_fields = self.categories.get(category_id)
                .map(|info| info.fields.as_slice())
                .unwrap_or(&[]);
            let visible_fields = visible_custom_fields(category_fields, &request.group_by);
            let layout = compute_column_layout(visible_fields.len());
            let body_size = body_font_size_for_extra_columns(visible_fields.len());
            let header_size = (body_size + 1.0).min(12.0);

            // Add table headers
            layer.use_text("Date", header_size, Mm(layout.date_x), y_pos, &header_font);
            layer.use_text("Amount", header_size, Mm(layout.amount_x), y_pos, &header_font);
            layer.use_text("Description", header_size, Mm(layout.description_x), y_pos, &header_font);
            for (field, x) in visible_fields.iter().zip(&layout.extra_field_x) {
                layer.use_text(&field.display_name(), header_size, Mm(*x), y_pos, &header_font);
            }
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
                        render_flow_row(&layer, flow, &visible_fields, &layout, body_size, &body_font, &mut y_pos);
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
                    render_flow_row(&layer, flow, &visible_fields, &layout, body_size, &body_font, &mut y_pos);
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
        let overall_total = net_total(&category_totals, &self.categories);

        for (category_id, total) in &category_totals {
            let category_name = self.categories.get(category_id)
                .map(|info| info.name.as_str())
                .unwrap_or(category_id);

            layer.use_text(&format!("{}: ${:.2}", category_name, total),
                14.0, Mm(20.0), y_pos, &body_font);
            y_pos -= Mm(15.0);
        }

        // Add overall total (net: income minus expenses, not a sum of all
        // flow magnitudes -- flow amounts are unsigned, so Expense category
        // totals must be subtracted rather than added)
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

    fn category_info(name: &str, flow_type: FlowType) -> ReportCategoryInfo {
        ReportCategoryInfo { name: name.to_string(), flow_type, fields: Vec::new() }
    }

    // --- net_total ---

    #[test]
    fn net_total_subtracts_expenses_from_income() {
        let mut category_totals = HashMap::new();
        category_totals.insert("salary".to_string(), 5000.0);
        category_totals.insert("rent".to_string(), 2000.0);

        let mut categories = HashMap::new();
        categories.insert("salary".to_string(), category_info("Salary", FlowType::Income));
        categories.insert("rent".to_string(), category_info("Rent", FlowType::Expense));

        assert_eq!(net_total(&category_totals, &categories), 3000.0);
    }

    #[test]
    fn net_total_all_income_sums_directly() {
        let mut category_totals = HashMap::new();
        category_totals.insert("salary".to_string(), 1000.0);
        category_totals.insert("bonus".to_string(), 500.0);

        let mut categories = HashMap::new();
        categories.insert("salary".to_string(), category_info("Salary", FlowType::Income));
        categories.insert("bonus".to_string(), category_info("Bonus", FlowType::Income));

        assert_eq!(net_total(&category_totals, &categories), 1500.0);
    }

    #[test]
    fn net_total_all_expenses_is_negative() {
        let mut category_totals = HashMap::new();
        category_totals.insert("rent".to_string(), 2000.0);

        let mut categories = HashMap::new();
        categories.insert("rent".to_string(), category_info("Rent", FlowType::Expense));

        assert_eq!(net_total(&category_totals, &categories), -2000.0);
    }

    #[test]
    fn net_total_excludes_categories_with_no_matching_entry() {
        let mut category_totals = HashMap::new();
        category_totals.insert("salary".to_string(), 1000.0);
        category_totals.insert("deleted-category".to_string(), 9999.0);

        let mut categories = HashMap::new();
        categories.insert("salary".to_string(), category_info("Salary", FlowType::Income));
        // "deleted-category" intentionally has no entry in `categories`.

        assert_eq!(
            net_total(&category_totals, &categories),
            1000.0,
            "a category with no matching entry should contribute nothing, not be guessed as income"
        );
    }

    #[test]
    fn net_total_empty_input_is_zero() {
        assert_eq!(net_total(&HashMap::new(), &HashMap::new()), 0.0);
    }

    // --- visible_custom_fields ---

    fn text_field(name: &str) -> CategoryField {
        CategoryField { name: name.to_string(), field_type: FieldType::Text, required: false, default_value: None }
    }

    #[test]
    fn visible_custom_fields_returns_all_when_not_grouping() {
        let fields = vec![text_field("recipient"), text_field("notes")];
        let visible = visible_custom_fields(&fields, &None);
        assert_eq!(visible.len(), 2);
    }

    #[test]
    fn visible_custom_fields_excludes_the_active_group_by_field() {
        let fields = vec![text_field("recipient"), text_field("notes")];
        let visible = visible_custom_fields(&fields, &Some("recipient".to_string()));
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].name, "notes");
    }

    #[test]
    fn visible_custom_fields_unaffected_by_unrelated_group_by_value() {
        // Grouping by a field name that isn't one of this category's fields
        // (e.g. another category's field) shouldn't hide anything here.
        let fields = vec![text_field("recipient")];
        let visible = visible_custom_fields(&fields, &Some("some_other_field".to_string()));
        assert_eq!(visible.len(), 1);
    }

    // --- format_field_value ---

    fn flow_with_custom_field(name: &str, value: &str) -> Flow {
        let mut custom_fields = HashMap::new();
        custom_fields.insert(name.to_string(), value.to_string());
        flow("f", NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(), custom_fields)
    }

    #[test]
    fn format_field_value_missing_value_is_empty_string() {
        let field = text_field("recipient");
        let f = flow("f", NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(), HashMap::new());
        assert_eq!(format_field_value(&field, &f), "");
    }

    #[test]
    fn format_field_value_text_passes_through_unchanged() {
        let field = text_field("recipient");
        let f = flow_with_custom_field("recipient", "Goodwill");
        assert_eq!(format_field_value(&field, &f), "Goodwill");
    }

    #[test]
    fn format_field_value_boolean_renders_yes_no() {
        let field = CategoryField { name: "covered".to_string(), field_type: FieldType::Boolean, required: false, default_value: None };
        assert_eq!(format_field_value(&field, &flow_with_custom_field("covered", "true")), "Yes");
        assert_eq!(format_field_value(&field, &flow_with_custom_field("covered", "false")), "No");
    }

    #[test]
    fn format_field_value_currency_normalizes_symbols() {
        let field = CategoryField { name: "cost".to_string(), field_type: FieldType::Currency, required: false, default_value: None };
        assert_eq!(format_field_value(&field, &flow_with_custom_field("cost", "$1,234.5")), "$1234.50");
    }

    #[test]
    fn format_field_value_invalid_number_falls_back_to_raw_value() {
        let field = CategoryField { name: "count".to_string(), field_type: FieldType::Integer, required: false, default_value: None };
        assert_eq!(format_field_value(&field, &flow_with_custom_field("count", "not-a-number")), "not-a-number");
    }

    // --- wrap_text ---

    #[test]
    fn wrap_text_fits_within_char_budget() {
        let lines = wrap_text("Hello world this is a test", 10);
        for line in &lines {
            assert!(line.chars().count() <= 10, "line {:?} exceeds the budget", line);
        }
        // Rejoining should reproduce the original words in order.
        assert_eq!(lines.join(" "), "Hello world this is a test");
    }

    #[test]
    fn wrap_text_empty_string_yields_one_empty_line() {
        assert_eq!(wrap_text("", 10), vec!["".to_string()]);
    }

    #[test]
    fn wrap_text_word_longer_than_budget_gets_its_own_line() {
        let lines = wrap_text("supercalifragilisticexpialidocious short", 10);
        assert_eq!(lines[0], "supercalifragilisticexpialidocious");
        assert_eq!(lines[1], "short");
    }

    #[test]
    fn wrap_text_short_text_is_a_single_line() {
        assert_eq!(wrap_text("short", 50), vec!["short".to_string()]);
    }

    // --- max_chars_for_width / body_font_size_for_extra_columns ---

    #[test]
    fn max_chars_for_width_increases_with_more_room() {
        let narrow = max_chars_for_width(20.0, 10.0);
        let wide = max_chars_for_width(60.0, 10.0);
        assert!(wide > narrow);
    }

    #[test]
    fn max_chars_for_width_has_a_floor() {
        assert!(max_chars_for_width(0.1, 12.0) >= 4);
    }

    #[test]
    fn body_font_size_shrinks_as_extra_columns_grow() {
        let zero = body_font_size_for_extra_columns(0);
        let two = body_font_size_for_extra_columns(2);
        let five = body_font_size_for_extra_columns(5);
        assert!(zero > two);
        assert!(two > five);
    }

    // --- compute_column_layout ---

    #[test]
    fn compute_column_layout_with_no_extra_fields_gives_description_the_full_remainder() {
        let layout = compute_column_layout(0);
        assert!(layout.extra_field_x.is_empty());
        assert!(layout.amount_x > layout.date_x);
        assert!(layout.description_x > layout.amount_x);
        assert!(layout.column_width > 0.0);
    }

    #[test]
    fn compute_column_layout_extra_columns_are_ordered_after_description() {
        let layout = compute_column_layout(2);
        assert_eq!(layout.extra_field_x.len(), 2);
        assert!(layout.extra_field_x[0] > layout.description_x);
        assert!(layout.extra_field_x[1] > layout.extra_field_x[0]);
    }

    #[test]
    fn compute_column_layout_column_width_shrinks_as_extra_fields_increase() {
        let no_extras = compute_column_layout(0);
        let with_extras = compute_column_layout(3);
        assert!(with_extras.column_width < no_extras.column_width);
    }
} 