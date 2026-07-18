use chrono::{NaiveDate, Datelike};
use std::collections::HashMap;
use crate::models::{CategoryField, FieldType, Flow, FlowType};
use printpdf::*;
use printpdf::indices::{PdfPageIndex, PdfLayerIndex};
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

/// Label used to group flows that have no value set for the field being
/// grouped by (either the field is missing entirely, or its value is empty/
/// whitespace-only), so they still appear -- and count toward totals --
/// instead of silently vanishing from the report.
const UNSET_GROUP_LABEL: &str = "(Value not set)";

/// Groups flows by the value of a custom field. Every flow passed in ends up
/// in exactly one group -- flows with no meaningful value for the field are
/// grouped together under `UNSET_GROUP_LABEL` rather than dropped, so the
/// displayed group totals always sum to the category total shown on the
/// same page.
fn group_flows_by_field<'a>(flows: &[&'a Flow], field_name: &str) -> HashMap<String, Vec<&'a Flow>> {
    let mut grouped: HashMap<String, Vec<&'a Flow>> = HashMap::new();
    for flow in flows {
        let key = match flow.custom_fields.get(field_name) {
            Some(value) if !value.trim().is_empty() => value.clone(),
            _ => UNSET_GROUP_LABEL.to_string(),
        };
        grouped.entry(key).or_default().push(flow);
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

/// Formats a dollar amount, using parentheses for negatives (accounting
/// style, e.g. `"($100.00)"`) instead of a leading minus sign (`"$-100.00"`).
fn format_currency(amount: f64) -> String {
    if amount < 0.0 {
        format!("(${:.2})", -amount)
    } else {
        format!("${:.2}", amount)
    }
}

/// Reverses the sign a category's raw (unsigned) total displays as in the
/// report summary: Expense totals show as positive (their raw magnitude),
/// Income totals show as negative. This is the opposite of standard
/// accounting sign convention, deliberately: since the primary use case for
/// this app is expense tracking, a net loss reading as a plain positive
/// number (rather than a negative one) is the more useful default -- see the
/// note printed alongside the summary table.
fn summary_display_value(raw_total: f64, flow_type: &FlowType) -> f64 {
    match flow_type {
        FlowType::Income => -raw_total,
        FlowType::Expense => raw_total,
    }
}

/// Custom fields to show as report columns for a category: all of them,
/// except the one currently selected as "Group By" (already shown as each
/// group's section header, so repeating it per row would be redundant).
fn visible_custom_fields<'a>(category_fields: &'a [CategoryField], group_by: &Option<String>) -> Vec<&'a CategoryField> {
    category_fields.iter()
        .filter(|f| group_by.as_deref() != Some(f.name.as_str()))
        .collect()
}

/// Whether the report's selected "Group By" field name is actually one of
/// this category's own fields. A single "Group By" selection is shared
/// across the whole report (it can span many categories with different
/// field sets), so this decides, per category, whether to group at all --
/// otherwise `group_flows_by_field` would silently drop every flow that
/// doesn't have the field, which is *every* flow for a category that never
/// had it, making that category's section show a total with zero detail
/// rows to back it up. Categories where this is false render their flows
/// normally (ungrouped), the same as when no "Group By" is selected.
fn group_by_applies_to_category(group_by: &Option<String>, category_fields: &[CategoryField]) -> bool {
    match group_by {
        Some(name) => category_fields.iter().any(|f| &f.name == name),
        None => false,
    }
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
                Ok(num) => format_currency(num),
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
/// coordinates with no automatic column sizing. Grouped sections shrink a
/// bit further still (regardless of extra-column count), since group
/// headers and group totals add extra vertical density on top of the rows
/// themselves.
fn body_font_size_for_extra_columns(extra_column_count: usize, is_grouped: bool) -> f64 {
    let base: f64 = match extra_column_count {
        0 => 12.0,
        1..=2 => 10.0,
        3..=4 => 8.5,
        _ => 7.0,
    };
    if is_grouped {
        (base - 1.5).max(6.0)
    } else {
        base
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

/// Line height in mm at the given font size, and the per-line character
/// budget for the flow table's shared column width. Shared by `row_height_mm`
/// (checking page space before drawing a row) and `render_flow_row` (actually
/// drawing it), so both agree on how a row will be laid out.
fn row_wrap_metrics(layout: &ColumnLayout, body_size: f64) -> (f64, usize) {
    const PT_TO_MM: f64 = 0.3528;
    let line_height = body_size * PT_TO_MM * 1.3;
    let max_chars = max_chars_for_width(layout.column_width, body_size);
    (line_height, max_chars)
}

/// Height in mm a flow's row will need once word-wrapped -- how much
/// vertical space to check for (via `ensure_page_space`) before drawing it.
fn row_height_mm(flow: &Flow, visible_fields: &[&CategoryField], layout: &ColumnLayout, body_size: f64) -> f64 {
    let (line_height, max_chars) = row_wrap_metrics(layout, body_size);

    let description_line_count = wrap_text(&flow.description, max_chars).len();
    let field_line_count = visible_fields.iter()
        .map(|field| wrap_text(&format_field_value(field, flow), max_chars).len())
        .max()
        .unwrap_or(1);

    let row_line_count = description_line_count.max(field_line_count).max(1);
    line_height * row_line_count as f64 + 2.0
}

/// Ensures at least `needed_height_mm` of vertical space remains below
/// `y_pos` on the current page before the caller draws something that
/// shouldn't be split across a page break (a table row, a header line, a
/// total line). If there isn't enough room, starts a new page, updates
/// `current_page`/`current_layer`, and resets `y_pos` near the top. Always
/// returns the layer content should now be drawn on -- the current page's if
/// nothing changed, or the freshly-started one's if it did.
fn ensure_page_space(
    doc: &PdfDocumentReference,
    current_page: &mut PdfPageIndex,
    current_layer: &mut PdfLayerIndex,
    y_pos: &mut Mm,
    needed_height_mm: f64,
) -> PdfLayerReference {
    const BOTTOM_MARGIN_MM: f64 = 25.0;
    const TOP_OF_NEW_PAGE_MM: f64 = 270.0;

    if y_pos.0 - needed_height_mm < BOTTOM_MARGIN_MM {
        let (page, layer) = doc.add_page(Mm(210.0), Mm(297.0), "Layer 1");
        *current_page = page;
        *current_layer = layer;
        *y_pos = Mm(TOP_OF_NEW_PAGE_MM);
    }

    doc.get_page(*current_page).get_layer(*current_layer)
}

/// Draws one flow's row: Date and Amount (always one line), then Description
/// and each visible custom field, word-wrapped to fit their column width.
/// Advances `y_pos` past however many wrapped lines the tallest column in
/// this row needed. Caller is responsible for calling `ensure_page_space`
/// (with `row_height_mm`'s result) first, and passing in whatever `layer`
/// that returns.
fn render_flow_row(
    layer: &PdfLayerReference,
    flow: &Flow,
    visible_fields: &[&CategoryField],
    layout: &ColumnLayout,
    body_size: f64,
    body_font: &IndirectFontRef,
    y_pos: &mut Mm,
) {
    let (line_height, max_chars) = row_wrap_metrics(layout, body_size);

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
    layer.use_text(&format_currency(flow.amount), body_size, Mm(layout.amount_x), *y_pos, body_font);

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
            let mut layer = doc.get_page(current_page).get_layer(current_layer);
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
            let is_grouped = group_by_applies_to_category(&request.group_by, category_fields);
            let body_size = body_font_size_for_extra_columns(visible_fields.len(), is_grouped);
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

            // Group flows if requested and this category actually has the
            // field being grouped by -- otherwise render normally below.
            if is_grouped {
                let group_by = request.group_by.as_ref().unwrap();
                let grouped_flows = group_flows_by_field(flows, group_by);

                // Add each group
                for (group_value, group_flows) in &grouped_flows {
                    layer = ensure_page_space(&doc, &mut current_page, &mut current_layer, &mut y_pos, 12.0);
                    layer.use_text(&format!("{}: {}", group_by, group_value), 14.0, Mm(20.0), y_pos, &header_font);
                    y_pos -= Mm(10.0);

                    // Add flows in this group -- checking (and paginating for)
                    // space before each row, since previously nothing did,
                    // and content past the bottom margin is simply invisible.
                    for flow in group_flows {
                        let needed = row_height_mm(flow, &visible_fields, &layout, body_size);
                        layer = ensure_page_space(&doc, &mut current_page, &mut current_layer, &mut y_pos, needed);
                        render_flow_row(&layer, flow, &visible_fields, &layout, body_size, &body_font, &mut y_pos);
                    }

                    // Add group total -- in the same column as individual
                    // flow amounts, not the old hardcoded Mm(80.0), which
                    // landed under Description once column positions became
                    // dynamic (variable custom-field columns).
                    layer = ensure_page_space(&doc, &mut current_page, &mut current_layer, &mut y_pos, 15.0);
                    let group_total: f64 = group_flows.iter().map(|f| f.amount).sum();
                    layer.use_text("Group Total:", 12.0, Mm(20.0), y_pos, &body_font);
                    layer.use_text(&format_currency(group_total), 12.0, Mm(layout.amount_x), y_pos, &body_font);
                    y_pos -= Mm(15.0);
                }
            } else {
                // Add all flows without grouping
                for flow in flows {
                    let needed = row_height_mm(flow, &visible_fields, &layout, body_size);
                    layer = ensure_page_space(&doc, &mut current_page, &mut current_layer, &mut y_pos, needed);
                    render_flow_row(&layer, flow, &visible_fields, &layout, body_size, &body_font, &mut y_pos);
                }
            }

            // Add category total, with a bit of breathing room above it.
            layer = ensure_page_space(&doc, &mut current_page, &mut current_layer, &mut y_pos, 28.0);
            y_pos -= Mm(8.0);
            let category_total: f64 = flows.iter().map(|f| f.amount).sum();
            category_totals.insert(category_id.clone(), category_total);
            layer.use_text(&format!("Category Total: {}", format_currency(category_total)), 14.0, Mm(20.0), y_pos, &header_font);
            y_pos -= Mm(20.0);

            page_count += 1;
        }

        // Add summary page
        let (mut summary_page, mut summary_layer) = doc.add_page(Mm(210.0), Mm(297.0), "Layer 1");
        let mut layer = doc.get_page(summary_page).get_layer(summary_layer);

        // Add summary title
        layer.use_text("Summary", 20.0, Mm(20.0), Mm(250.0), &header_font);

        let mut y_pos = Mm(235.0);

        // Clarifying note: everything below uses a reversed sign convention
        // from standard accounting (see `summary_display_value`).
        const SUMMARY_SIGN_NOTE: &str = "Note: amounts below use a reversed sign convention, since this app is primarily used for expense tracking. Expense totals and a net loss are shown as positive; Income totals and a net gain are shown as negative, in parentheses.";
        let note_size = 9.0;
        let note_lines = wrap_text(SUMMARY_SIGN_NOTE, max_chars_for_width(170.0, note_size));
        layer = ensure_page_space(&doc, &mut summary_page, &mut summary_layer, &mut y_pos, note_lines.len() as f64 * 4.5 + 8.0);
        for line in &note_lines {
            layer.use_text(line, note_size, Mm(20.0), y_pos, &body_font);
            y_pos -= Mm(4.5);
        }
        y_pos -= Mm(8.0);

        // Table header
        const SUMMARY_AMOUNT_X: f64 = 120.0;
        layer = ensure_page_space(&doc, &mut summary_page, &mut summary_layer, &mut y_pos, 13.0);
        layer.use_text("Category", 12.0, Mm(20.0), y_pos, &header_font);
        layer.use_text("Total", 12.0, Mm(SUMMARY_AMOUNT_X), y_pos, &header_font);
        y_pos -= Mm(8.0);
        layer.add_line_break();
        y_pos -= Mm(5.0);

        // Per-category totals (reversed-sign display), plus a running
        // income/expense breakdown for the summary lines below.
        let mut total_income = 0.0;
        let mut total_expense = 0.0;

        for (category_id, raw_total) in &category_totals {
            let info = self.categories.get(category_id);
            let category_name = info.map(|i| i.name.as_str()).unwrap_or(category_id);

            let displayed = match info.map(|i| i.flow_type.clone()) {
                Some(FlowType::Income) => {
                    total_income += *raw_total;
                    summary_display_value(*raw_total, &FlowType::Income)
                }
                Some(FlowType::Expense) => {
                    total_expense += *raw_total;
                    summary_display_value(*raw_total, &FlowType::Expense)
                }
                // Category was deleted after flows referencing it were saved:
                // shown for transparency but excluded from the income/expense
                // breakdown and net total, same as `net_total` already does.
                None => *raw_total,
            };

            layer = ensure_page_space(&doc, &mut summary_page, &mut summary_layer, &mut y_pos, 12.0);
            layer.use_text(category_name, 12.0, Mm(20.0), y_pos, &body_font);
            layer.use_text(&format_currency(displayed), 12.0, Mm(SUMMARY_AMOUNT_X), y_pos, &body_font);
            y_pos -= Mm(12.0);
        }

        // Keep Total Income/Total Expense/Net Total together rather than
        // letting a page break land in the middle of the block.
        layer = ensure_page_space(&doc, &mut summary_page, &mut summary_layer, &mut y_pos, 60.0);
        y_pos -= Mm(6.0);
        layer.add_line_break();
        y_pos -= Mm(10.0);

        layer.use_text("Total Income:", 12.0, Mm(20.0), y_pos, &body_font);
        layer.use_text(&format_currency(total_income), 12.0, Mm(SUMMARY_AMOUNT_X), y_pos, &body_font);
        y_pos -= Mm(12.0);

        layer.use_text("Total Expense:", 12.0, Mm(20.0), y_pos, &body_font);
        layer.use_text(&format_currency(total_expense), 12.0, Mm(SUMMARY_AMOUNT_X), y_pos, &body_font);
        y_pos -= Mm(16.0);

        // Net total, same reversed convention: a net loss (expenses exceeded
        // income) displays as positive, a net gain as negative.
        let overall_total = -net_total(&category_totals, &self.categories);
        layer.use_text("Net Total:", 16.0, Mm(20.0), y_pos, &header_font);
        layer.use_text(&format_currency(overall_total), 16.0, Mm(SUMMARY_AMOUNT_X), y_pos, &header_font);

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
    fn group_flows_by_field_buckets_missing_values_instead_of_dropping_them() {
        let date = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
        let mut fields_a = HashMap::new();
        fields_a.insert("vendor".to_string(), "Acme".to_string());
        let fields_b = HashMap::new(); // no "vendor" field at all

        let flow_a = flow("a", date, fields_a);
        let flow_b = flow("b", date, fields_b);
        let flows: Vec<&Flow> = vec![&flow_a, &flow_b];

        let grouped = group_flows_by_field(&flows, "vendor");

        let total_grouped_flows: usize = grouped.values().map(|v| v.len()).sum();
        assert_eq!(total_grouped_flows, 2, "every flow should end up in some group, so group totals sum to the category total");
        assert_eq!(grouped.get("Acme").unwrap().len(), 1);
        assert_eq!(grouped.get(UNSET_GROUP_LABEL).unwrap().len(), 1);
    }

    #[test]
    fn group_flows_by_field_treats_empty_and_whitespace_values_as_unset() {
        let date = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
        let mut fields_a = HashMap::new();
        fields_a.insert("vendor".to_string(), "".to_string());
        let mut fields_b = HashMap::new();
        fields_b.insert("vendor".to_string(), "   ".to_string());

        let flow_a = flow("a", date, fields_a);
        let flow_b = flow("b", date, fields_b);
        let flows: Vec<&Flow> = vec![&flow_a, &flow_b];

        let grouped = group_flows_by_field(&flows, "vendor");

        assert_eq!(grouped.len(), 1);
        assert_eq!(grouped.get(UNSET_GROUP_LABEL).unwrap().len(), 2);
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

    // --- format_currency ---

    #[test]
    fn format_currency_positive_has_no_parentheses() {
        assert_eq!(format_currency(100.0), "$100.00");
    }

    #[test]
    fn format_currency_negative_uses_parentheses_not_a_minus_sign() {
        assert_eq!(format_currency(-100.0), "($100.00)");
    }

    #[test]
    fn format_currency_zero_has_no_parentheses() {
        assert_eq!(format_currency(0.0), "$0.00");
    }

    // --- summary_display_value ---

    #[test]
    fn summary_display_value_reverses_income_to_negative() {
        // Deliberately the opposite of standard accounting sign convention --
        // see the doc comment on `summary_display_value` for why.
        assert_eq!(summary_display_value(500.0, &FlowType::Income), -500.0);
    }

    #[test]
    fn summary_display_value_reverses_expense_to_positive() {
        assert_eq!(summary_display_value(500.0, &FlowType::Expense), 500.0);
    }

    #[test]
    fn summary_display_value_and_format_currency_together_show_income_in_parens() {
        // The combination is what actually appears on the page: an Income
        // category's raw (positive, unsigned) total should render inside
        // parentheses, not as a plain positive number.
        let displayed = summary_display_value(500.0, &FlowType::Income);
        assert_eq!(format_currency(displayed), "($500.00)");
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

    // --- group_by_applies_to_category ---

    #[test]
    fn group_by_applies_when_category_has_the_field() {
        let fields = vec![text_field("recipient")];
        assert!(group_by_applies_to_category(&Some("recipient".to_string()), &fields));
    }

    #[test]
    fn group_by_does_not_apply_when_category_lacks_the_field() {
        // This is the bug this function fixes: a report spans many
        // categories, and a "Group By" selection picked for one category
        // (e.g. Donations' "recipient") shouldn't cause a category that never
        // had that field (e.g. Salary) to render zero detail rows.
        let fields = vec![text_field("some_other_field")];
        assert!(!group_by_applies_to_category(&Some("recipient".to_string()), &fields));
    }

    #[test]
    fn group_by_does_not_apply_when_nothing_is_selected() {
        let fields = vec![text_field("recipient")];
        assert!(!group_by_applies_to_category(&None, &fields));
    }

    #[test]
    fn group_by_does_not_apply_to_a_category_with_no_fields_at_all() {
        assert!(!group_by_applies_to_category(&Some("recipient".to_string()), &[]));
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
        let zero = body_font_size_for_extra_columns(0, false);
        let two = body_font_size_for_extra_columns(2, false);
        let five = body_font_size_for_extra_columns(5, false);
        assert!(zero > two);
        assert!(two > five);
    }

    #[test]
    fn body_font_size_shrinks_further_when_grouped() {
        for extra_columns in [0, 2, 5] {
            let ungrouped = body_font_size_for_extra_columns(extra_columns, false);
            let grouped = body_font_size_for_extra_columns(extra_columns, true);
            assert!(
                grouped < ungrouped,
                "grouped ({}) should be smaller than ungrouped ({}) at {} extra columns",
                grouped, ungrouped, extra_columns
            );
        }
    }

    #[test]
    fn body_font_size_has_a_floor_even_when_grouped() {
        assert!(body_font_size_for_extra_columns(10, true) >= 6.0);
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

    // --- row_height_mm ---

    fn flow_with_description(description: &str) -> Flow {
        Flow {
            description: description.to_string(),
            ..flow("f", NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(), HashMap::new())
        }
    }

    #[test]
    fn row_height_mm_increases_with_more_wrapped_lines() {
        let layout = compute_column_layout(0);
        let short = flow_with_description("Short");
        let long = flow_with_description(&"word ".repeat(50));

        let short_height = row_height_mm(&short, &[], &layout, 12.0);
        let long_height = row_height_mm(&long, &[], &layout, 12.0);

        assert!(long_height > short_height);
    }

    #[test]
    fn row_height_mm_is_never_less_than_one_line() {
        let layout = compute_column_layout(0);
        let empty = flow_with_description("");

        assert!(row_height_mm(&empty, &[], &layout, 12.0) > 0.0);
    }

    // --- ensure_page_space ---

    #[test]
    fn ensure_page_space_resets_y_pos_when_not_enough_room() {
        let (doc, page1, layer1) = PdfDocument::new("Test", Mm(210.0), Mm(297.0), "Layer 1");
        let mut current_page = page1;
        let mut current_layer = layer1;
        let mut y_pos = Mm(30.0); // close to the bottom margin already

        ensure_page_space(&doc, &mut current_page, &mut current_layer, &mut y_pos, 20.0);

        assert!(y_pos.0 > 100.0, "y_pos should reset near the top of a new page, got {}", y_pos.0);
    }

    #[test]
    fn ensure_page_space_leaves_y_pos_alone_when_room_remains() {
        let (doc, page1, layer1) = PdfDocument::new("Test", Mm(210.0), Mm(297.0), "Layer 1");
        let mut current_page = page1;
        let mut current_layer = layer1;
        let mut y_pos = Mm(200.0);

        ensure_page_space(&doc, &mut current_page, &mut current_layer, &mut y_pos, 20.0);

        assert_eq!(y_pos.0, 200.0, "y_pos shouldn't change when there's already enough room");
    }
} 