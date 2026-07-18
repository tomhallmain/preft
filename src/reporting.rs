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

/// Orders the keys of `present` (category ids with actual data to show)
/// according to `order` (the category dropdown's order), appending any keys
/// not found in `order` -- e.g. a category deleted after a flow referencing
/// it was saved -- at the end, so nothing is silently dropped from the
/// report just because it's no longer in the ordered list.
fn ordered_category_ids<T>(order: &[String], present: &HashMap<String, T>) -> Vec<String> {
    let mut ids: Vec<String> = order.iter()
        .filter(|id| present.contains_key(*id))
        .cloned()
        .collect();
    for id in present.keys() {
        if !order.contains(id) {
            ids.push(id.clone());
        }
    }
    ids
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

/// Inserts thousands separators into a string of ASCII digits (no sign, no
/// decimal point), e.g. `"1234567"` -> `"1,234,567"`.
fn group_thousands(digits: &str) -> String {
    let mut grouped = String::new();
    for (i, c) in digits.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(c);
    }
    grouped.chars().rev().collect()
}

/// Formats a non-negative amount to two decimal places with thousands
/// separators in the integer part, e.g. `1234567.5` -> `"1,234,567.50"`.
/// Rounds to the nearest cent the same way `{:.2}` would.
fn format_amount_grouped(amount: f64) -> String {
    let cents = (amount * 100.0).round() as i64;
    format!("{}.{:02}", group_thousands(&(cents / 100).to_string()), cents % 100)
}

/// Formats a dollar amount, using parentheses for negatives (accounting
/// style, e.g. `"($1,234.00)"`) instead of a leading minus sign
/// (`"$-1,234.00"`), and thousands separators so large amounts stay
/// readable at a glance.
fn format_currency(amount: f64) -> String {
    if amount < 0.0 {
        format!("(${})", format_amount_grouped(-amount))
    } else {
        format!("${}", format_amount_grouped(amount))
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
        FieldType::Integer => value.parse::<i64>().map(|n| {
            let sign = if n < 0 { "-" } else { "" };
            format!("{}{}", sign, group_thousands(&n.unsigned_abs().to_string()))
        }).unwrap_or_else(|_| value.clone()),
        FieldType::Float => value.parse::<f64>().map(|n| {
            let sign = if n < 0.0 { "-" } else { "" };
            format!("{}{}", sign, format_amount_grouped(n.abs()))
        }).unwrap_or_else(|_| value.clone()),
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

/// Estimated on-page width of `text` at `font_size_pt`, using the same
/// average-glyph-width heuristic as `max_chars_for_width` -- `printpdf`
/// doesn't expose font metrics for externally-loaded fonts, so there's no
/// way to measure exact text width. Amount columns are mostly digits and a
/// handful of narrow, fairly uniform-width symbols (`$`, `.`, `,`, `(`, `)`),
/// so this estimate is close enough to right-align them without the visible
/// jitter it would cause on free-form text like Description.
fn estimate_text_width_mm(text: &str, font_size_pt: f64) -> f64 {
    const PT_TO_MM: f64 = 0.3528;
    let avg_char_width_mm = (font_size_pt * PT_TO_MM * 0.5).max(0.1);
    text.chars().count() as f64 * avg_char_width_mm
}

/// X position (mm from the page's left edge) to draw `text` at so its right
/// edge lands approximately at `right_edge_mm` -- see `estimate_text_width_mm`
/// for why this is an estimate, not an exact measurement.
fn right_align_x(text: &str, right_edge_mm: f64, font_size_pt: f64) -> f64 {
    right_edge_mm - estimate_text_width_mm(text, font_size_pt)
}

/// Like `right_align_x`, but never returns a position left of `left_edge_mm`.
/// The width estimate it's built on is a heuristic (`estimate_text_width_mm`),
/// not an exact measurement -- for an unusually wide value (or just an
/// underestimate), a plain `right_align_x` could push text left into the
/// previous column entirely. This floors it at the column's own left edge
/// instead: for an amount too wide to right-align cleanly, drawing it flush
/// against the column boundary is a better failure mode than overlapping
/// the Date/Category column to its left.
fn right_align_x_clamped(text: &str, right_edge_mm: f64, left_edge_mm: f64, font_size_pt: f64) -> f64 {
    right_align_x(text, right_edge_mm, font_size_pt).max(left_edge_mm)
}

/// X position to center `text` between `left_edge_mm` and `right_edge_mm`,
/// floored at `left_edge_mm` the same way `right_align_x_clamped` is. Used
/// for column *header* labels rather than `right_align_x_clamped`: headers
/// are drawn in the bold header font, and `estimate_text_width_mm`'s
/// character-width heuristic was tuned against amount values (mostly
/// digits/currency symbols in the regular body font, narrower on average
/// than bold word text), so it systematically underestimates a bold header
/// label's actual width. Centering makes that underestimate much less
/// visible -- the error splits across both sides of the column instead of
/// being dumped entirely into the neighboring column the way a hard
/// right-align would.
fn center_align_x(text: &str, left_edge_mm: f64, right_edge_mm: f64, font_size_pt: f64) -> f64 {
    let width = estimate_text_width_mm(text, font_size_pt);
    let center = (left_edge_mm + right_edge_mm) / 2.0;
    (center - width / 2.0).max(left_edge_mm)
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
    /// Right edge to align amount text to within the Amount column (a few mm
    /// before `description_x`, so a right-aligned amount doesn't butt right
    /// up against the Description column).
    amount_right_edge_x: f64,
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
    const AMOUNT_COLUMN_GAP_MM: f64 = 3.0;
    const MIN_REMAINING_WIDTH_MM: f64 = 20.0;

    let date_x = LEFT_MARGIN_MM;
    let amount_x = date_x + DATE_WIDTH_MM;
    let description_x = amount_x + AMOUNT_WIDTH_MM;
    let amount_right_edge_x = description_x - AMOUNT_COLUMN_GAP_MM;
    let remaining_width = (PAGE_WIDTH_MM - RIGHT_MARGIN_MM - description_x).max(MIN_REMAINING_WIDTH_MM);
    let column_count = (extra_field_count + 1) as f64; // Description + extras
    let column_width = remaining_width / column_count;

    let extra_field_x = (0..extra_field_count)
        .map(|i| description_x + column_width * (i as f64 + 1.0))
        .collect();

    ColumnLayout {
        date_x,
        amount_x,
        amount_right_edge_x,
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
/// vertical space to check for (via `PageCursor::ensure_space`) before drawing it.
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

const PAGE_WIDTH_MM: f64 = 210.0;
const PAGE_HEIGHT_MM: f64 = 297.0;
/// Where content starts on a freshly created page. Kept close to the page
/// edge (a ~22mm top margin) with a small gap below the period-label chrome
/// drawn at `HEADER_CHROME_Y_MM`.
const CONTENT_TOP_MM: f64 = 275.0;
/// Content may not be drawn below this y position (an ~18mm bottom margin,
/// leaving room above the page-number chrome at `FOOTER_CHROME_Y_MM`).
const BOTTOM_MARGIN_MM: f64 = 18.0;
const CHROME_FONT_SIZE: f64 = 8.0;
const HEADER_CHROME_Y_MM: f64 = 288.0;
const FOOTER_CHROME_Y_MM: f64 = 12.0;

/// Draws the small-font period label (top margin) and page number (bottom
/// margin) that appear on every page of the report -- the cover page, every
/// category page, and any page created mid-content by pagination.
fn draw_page_chrome(layer: &PdfLayerReference, page_number: usize, time_period_text: &str, chrome_font: &IndirectFontRef) {
    layer.use_text(time_period_text, CHROME_FONT_SIZE, Mm(20.0), Mm(HEADER_CHROME_Y_MM), chrome_font);
    layer.use_text(&format!("Page {}", page_number), CHROME_FONT_SIZE, Mm(20.0), Mm(FOOTER_CHROME_Y_MM), chrome_font);
}

/// Tracks the current page/layer/y-position while rendering the report body,
/// and centralizes page creation so every new page (whether forced, e.g. one
/// category per page, or triggered by running out of room) gets the same
/// period/page-number chrome and content-top position.
struct PageCursor<'a> {
    doc: &'a PdfDocumentReference,
    page: PdfPageIndex,
    layer_idx: PdfLayerIndex,
    y_pos: Mm,
    page_number: usize,
    time_period_text: &'a str,
    chrome_font: &'a IndirectFontRef,
}

impl<'a> PageCursor<'a> {
    fn layer(&self) -> PdfLayerReference {
        self.doc.get_page(self.page).get_layer(self.layer_idx)
    }

    /// Unconditionally starts a fresh page, e.g. so each category begins on
    /// its own page rather than possibly sharing one with the previous
    /// category's tail end.
    fn start_new_page(&mut self) -> PdfLayerReference {
        let (page, layer_idx) = self.doc.add_page(Mm(PAGE_WIDTH_MM), Mm(PAGE_HEIGHT_MM), "Layer 1");
        self.page = page;
        self.layer_idx = layer_idx;
        self.y_pos = Mm(CONTENT_TOP_MM);
        self.page_number += 1;
        let layer = self.layer();
        draw_page_chrome(&layer, self.page_number, self.time_period_text, self.chrome_font);
        layer
    }

    /// Starts a fresh page only if `needed_height_mm` more content wouldn't
    /// fit above the bottom margin; otherwise returns the current layer
    /// unchanged.
    fn ensure_space(&mut self, needed_height_mm: f64) -> PdfLayerReference {
        if self.y_pos.0 - needed_height_mm < BOTTOM_MARGIN_MM {
            self.start_new_page()
        } else {
            self.layer()
        }
    }
}

/// Draws one flow's row: Date and Amount (always one line), then Description
/// and each visible custom field, word-wrapped to fit their column width.
/// Advances `y_pos` past however many wrapped lines the tallest column in
/// this row needed. Caller is responsible for calling `PageCursor::ensure_space`
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
    let amount_text = format_currency(flow.amount);
    layer.use_text(&amount_text, body_size, Mm(right_align_x_clamped(&amount_text, layout.amount_right_edge_x, layout.amount_x, body_size)), *y_pos, body_font);

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
    /// Category ids in the same order they appear in the category selection
    /// dropdown, so the report's category/page order is deterministic
    /// instead of following `categories`' arbitrary `HashMap` iteration
    /// order. Any category id referenced by a flow but missing here (e.g. a
    /// deleted category) is still shown, just appended after the ordered
    /// ones -- see `ordered_category_ids` in `generate_report`.
    category_order: Vec<String>,
    title_font: Option<IndirectFontRef>,
    subtitle_font: Option<IndirectFontRef>,
    header_font: Option<IndirectFontRef>,
    body_font: Option<IndirectFontRef>,
}

impl ReportGenerator {
    pub fn new(flows: Vec<Flow>, categories: HashMap<String, ReportCategoryInfo>, category_order: Vec<String>) -> Self {
        Self {
            flows,
            categories,
            category_order,
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

        // Create a new document -- page1/layer1 becomes the cover page below.
        let (doc, page1, layer1) = PdfDocument::new("Financial Report", Mm(PAGE_WIDTH_MM), Mm(PAGE_HEIGHT_MM), "Layer 1");

        // Load fonts
        let title_font = self.load_font(&doc, &request.font_settings.title_font)?;
        let subtitle_font = self.load_font(&doc, &request.font_settings.subtitle_font)?;
        let header_font = self.load_font(&doc, &request.font_settings.header_font)?;
        let body_font = self.load_font(&doc, &request.font_settings.body_font)?;

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

        // Group flows by category
        let mut category_flows: HashMap<String, Vec<&Flow>> = HashMap::new();
        for flow in sorted_flows {
            category_flows.entry(flow.category_id.clone())
                .or_default()
                .push(flow);
        }

        // Categories are rendered in the same order as the category
        // selection dropdown (`self.category_order`), not `category_flows`'
        // arbitrary `HashMap` iteration order -- otherwise which category
        // shows up first/second/etc. changes randomly between report runs.
        // Computed once, up front, so both the cover page's table of
        // contents and the detail-page loop below agree on the order.
        let category_display_order = ordered_category_ids(&self.category_order, &category_flows);

        // Cover page: title, subtitle, time period, and a mini table of
        // contents -- the categories that appear (in the same order as their
        // detail pages) plus a pointer to the summary at the end. No page
        // numbers here: what page each category lands on isn't known until
        // it's actually rendered below.
        let cover_layer = doc.get_page(page1).get_layer(layer1);
        cover_layer.use_text(&request.title, 26.0, Mm(20.0), Mm(180.0), &title_font);
        if !request.subtitle.is_empty() {
            cover_layer.use_text(&request.subtitle, 16.0, Mm(20.0), Mm(163.0), &subtitle_font);
        }
        cover_layer.use_text(&time_period_text, 14.0, Mm(20.0), Mm(148.0), &subtitle_font);
        // No page-number/period chrome on the cover page itself -- it already
        // shows the period as part of its own content, and numbering starts
        // on the first page after it (see `page_number: 0` below).

        let mut cover_y = Mm(130.0);
        if !category_display_order.is_empty() {
            cover_layer.use_text("Categories in this report:", 13.0, Mm(20.0), cover_y, &header_font);
            cover_y -= Mm(8.0);
            for category_id in &category_display_order {
                let category_name = self.categories.get(category_id)
                    .map(|info| info.name.as_str())
                    .unwrap_or(category_id);
                cover_layer.use_text(&format!("- {}", category_name), 11.0, Mm(24.0), cover_y, &subtitle_font);
                cover_y -= Mm(6.0);
            }
            cover_y -= Mm(6.0);
        }
        cover_layer.use_text("A financial summary appears at the end of this report.", 11.0, Mm(20.0), cover_y, &subtitle_font);

        let mut cursor = PageCursor {
            doc: &doc,
            page: page1,
            layer_idx: layer1,
            y_pos: Mm(CONTENT_TOP_MM),
            // Starts at 0 (not 1) so the first page created after the cover
            // page -- the first category's page -- becomes "Page 1", not
            // "Page 2". The cover page itself is never numbered.
            page_number: 0,
            time_period_text: &time_period_text,
            chrome_font: &body_font,
        };

        // Store category totals for later use
        let mut category_totals: HashMap<String, f64> = HashMap::new();

        for category_id in &category_display_order {
            let flows = &category_flows[category_id];

            // Each category always starts on its own fresh page.
            let mut layer = cursor.start_new_page();

            // Add category header
            let category_name = self.categories.get(category_id)
                .map(|info| info.name.as_str())
                .unwrap_or(category_id);
            layer.use_text(&format!("Category: {}", category_name), 16.0, Mm(20.0), cursor.y_pos, &header_font);
            cursor.y_pos -= Mm(15.0);

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
            layer.use_text("Date", header_size, Mm(layout.date_x), cursor.y_pos, &header_font);
            layer.use_text("Amount", header_size, Mm(center_align_x("Amount", layout.amount_x, layout.amount_right_edge_x, header_size)), cursor.y_pos, &header_font);
            layer.use_text("Description", header_size, Mm(layout.description_x), cursor.y_pos, &header_font);
            for (field, x) in visible_fields.iter().zip(&layout.extra_field_x) {
                layer.use_text(&field.display_name(), header_size, Mm(*x), cursor.y_pos, &header_font);
            }
            cursor.y_pos -= Mm(10.0);

            // Add separator line
            layer.add_line_break();
            cursor.y_pos -= Mm(5.0);

            // Group flows if requested and this category actually has the
            // field being grouped by -- otherwise render normally below.
            if is_grouped {
                let group_by = request.group_by.as_ref().unwrap();
                let grouped_flows = group_flows_by_field(flows, group_by);

                // Add each group
                for (group_value, group_flows) in &grouped_flows {
                    layer = cursor.ensure_space(12.0);
                    layer.use_text(&format!("{}: {}", group_by, group_value), 14.0, Mm(20.0), cursor.y_pos, &header_font);
                    cursor.y_pos -= Mm(10.0);

                    // Add flows in this group -- checking (and paginating for)
                    // space before each row, since previously nothing did,
                    // and content past the bottom margin is simply invisible.
                    for flow in group_flows {
                        let needed = row_height_mm(flow, &visible_fields, &layout, body_size);
                        layer = cursor.ensure_space(needed);
                        render_flow_row(&layer, flow, &visible_fields, &layout, body_size, &body_font, &mut cursor.y_pos);
                    }

                    // Add group total -- in the same column as individual
                    // flow amounts, not the old hardcoded Mm(80.0), which
                    // landed under Description once column positions became
                    // dynamic (variable custom-field columns).
                    layer = cursor.ensure_space(15.0);
                    let group_total: f64 = group_flows.iter().map(|f| f.amount).sum();
                    let group_total_text = format_currency(group_total);
                    layer.use_text("Group Total:", 12.0, Mm(20.0), cursor.y_pos, &body_font);
                    layer.use_text(&group_total_text, 12.0, Mm(right_align_x_clamped(&group_total_text, layout.amount_right_edge_x, layout.amount_x, 12.0)), cursor.y_pos, &body_font);
                    cursor.y_pos -= Mm(15.0);
                }
            } else {
                // Add all flows without grouping
                for flow in flows {
                    let needed = row_height_mm(flow, &visible_fields, &layout, body_size);
                    layer = cursor.ensure_space(needed);
                    render_flow_row(&layer, flow, &visible_fields, &layout, body_size, &body_font, &mut cursor.y_pos);
                }
            }

            // Add category total, with a bit of breathing room above it.
            layer = cursor.ensure_space(28.0);
            cursor.y_pos -= Mm(8.0);
            let category_total: f64 = flows.iter().map(|f| f.amount).sum();
            category_totals.insert(category_id.clone(), category_total);
            let category_total_text = format_currency(category_total);
            layer.use_text("Category Total:", 14.0, Mm(20.0), cursor.y_pos, &header_font);
            layer.use_text(&category_total_text, 14.0, Mm(right_align_x_clamped(&category_total_text, layout.amount_right_edge_x, layout.amount_x, 14.0)), cursor.y_pos, &header_font);
        }

        // Add summary page
        let mut layer = cursor.start_new_page();
        layer.use_text("Summary", 20.0, Mm(20.0), cursor.y_pos, &header_font);
        cursor.y_pos -= Mm(15.0);

        // Clarifying note: everything below uses a reversed sign convention
        // from standard accounting (see `summary_display_value`).
        const SUMMARY_SIGN_NOTE: &str = "Note: amounts below use a reversed sign convention, since this app is primarily used for expense tracking. Expense totals and a net loss are shown as positive; Income totals and a net gain are shown as negative, in parentheses.";
        let note_size = 9.0;
        let note_lines = wrap_text(SUMMARY_SIGN_NOTE, max_chars_for_width(170.0, note_size));
        layer = cursor.ensure_space(note_lines.len() as f64 * 4.5 + 8.0);
        for line in &note_lines {
            layer.use_text(line, note_size, Mm(20.0), cursor.y_pos, &body_font);
            cursor.y_pos -= Mm(4.5);
        }
        cursor.y_pos -= Mm(8.0);

        // Table header
        const SUMMARY_AMOUNT_X: f64 = 120.0;
        const SUMMARY_AMOUNT_RIGHT_EDGE_MM: f64 = 170.0;
        layer = cursor.ensure_space(13.0);
        layer.use_text("Category", 12.0, Mm(20.0), cursor.y_pos, &header_font);
        layer.use_text("Total", 12.0, Mm(center_align_x("Total", SUMMARY_AMOUNT_X, SUMMARY_AMOUNT_RIGHT_EDGE_MM, 12.0)), cursor.y_pos, &header_font);
        cursor.y_pos -= Mm(8.0);
        layer.add_line_break();
        cursor.y_pos -= Mm(5.0);

        // Per-category totals (reversed-sign display), plus a running
        // income/expense breakdown for the summary lines below. Same
        // dropdown-derived ordering as the detail pages, for consistency.
        let mut total_income = 0.0;
        let mut total_expense = 0.0;

        for category_id in ordered_category_ids(&self.category_order, &category_totals) {
            let raw_total = category_totals[&category_id];
            let info = self.categories.get(&category_id);
            let category_name = info.map(|i| i.name.as_str()).unwrap_or(&category_id);

            let displayed = match info.map(|i| i.flow_type.clone()) {
                Some(FlowType::Income) => {
                    total_income += raw_total;
                    summary_display_value(raw_total, &FlowType::Income)
                }
                Some(FlowType::Expense) => {
                    total_expense += raw_total;
                    summary_display_value(raw_total, &FlowType::Expense)
                }
                // Category was deleted after flows referencing it were saved:
                // shown for transparency but excluded from the income/expense
                // breakdown and net total, same as `net_total` already does.
                None => raw_total,
            };

            layer = cursor.ensure_space(12.0);
            layer.use_text(category_name, 12.0, Mm(20.0), cursor.y_pos, &body_font);
            let displayed_text = format_currency(displayed);
            layer.use_text(&displayed_text, 12.0, Mm(right_align_x_clamped(&displayed_text, SUMMARY_AMOUNT_RIGHT_EDGE_MM, SUMMARY_AMOUNT_X, 12.0)), cursor.y_pos, &body_font);
            cursor.y_pos -= Mm(12.0);
        }

        // Keep Total Income/Total Expense/Net Total together rather than
        // letting a page break land in the middle of the block.
        layer = cursor.ensure_space(60.0);
        cursor.y_pos -= Mm(6.0);
        layer.add_line_break();
        cursor.y_pos -= Mm(10.0);

        let total_income_text = format_currency(total_income);
        layer.use_text("Total Income:", 12.0, Mm(20.0), cursor.y_pos, &body_font);
        layer.use_text(&total_income_text, 12.0, Mm(right_align_x_clamped(&total_income_text, SUMMARY_AMOUNT_RIGHT_EDGE_MM, SUMMARY_AMOUNT_X, 12.0)), cursor.y_pos, &body_font);
        cursor.y_pos -= Mm(12.0);

        let total_expense_text = format_currency(total_expense);
        layer.use_text("Total Expense:", 12.0, Mm(20.0), cursor.y_pos, &body_font);
        layer.use_text(&total_expense_text, 12.0, Mm(right_align_x_clamped(&total_expense_text, SUMMARY_AMOUNT_RIGHT_EDGE_MM, SUMMARY_AMOUNT_X, 12.0)), cursor.y_pos, &body_font);
        cursor.y_pos -= Mm(16.0);

        // Net total, same reversed convention: a net loss (expenses exceeded
        // income) displays as positive, a net gain as negative.
        let overall_total = -net_total(&category_totals, &self.categories);
        let overall_total_text = format_currency(overall_total);
        layer.use_text("Net Total:", 16.0, Mm(20.0), cursor.y_pos, &header_font);
        layer.use_text(&overall_total_text, 16.0, Mm(right_align_x_clamped(&overall_total_text, SUMMARY_AMOUNT_RIGHT_EDGE_MM, SUMMARY_AMOUNT_X, 16.0)), cursor.y_pos, &header_font);

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

    #[test]
    fn format_currency_thousands_get_a_separator() {
        assert_eq!(format_currency(1234.5), "$1,234.50");
    }

    #[test]
    fn format_currency_millions_get_two_separators() {
        assert_eq!(format_currency(12345678.9), "$12,345,678.90");
    }

    #[test]
    fn format_currency_negative_thousands_use_parentheses_with_separator() {
        assert_eq!(format_currency(-1234.5), "($1,234.50)");
    }

    #[test]
    fn format_currency_under_a_thousand_has_no_separator() {
        assert_eq!(format_currency(999.99), "$999.99");
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
        assert_eq!(format_field_value(&field, &flow_with_custom_field("cost", "$1,234.5")), "$1,234.50");
    }

    #[test]
    fn format_field_value_invalid_number_falls_back_to_raw_value() {
        let field = CategoryField { name: "count".to_string(), field_type: FieldType::Integer, required: false, default_value: None };
        assert_eq!(format_field_value(&field, &flow_with_custom_field("count", "not-a-number")), "not-a-number");
    }

    #[test]
    fn format_field_value_integer_gets_a_thousands_separator() {
        let field = CategoryField { name: "count".to_string(), field_type: FieldType::Integer, required: false, default_value: None };
        assert_eq!(format_field_value(&field, &flow_with_custom_field("count", "1234567")), "1,234,567");
    }

    #[test]
    fn format_field_value_negative_integer_keeps_the_minus_sign() {
        let field = CategoryField { name: "count".to_string(), field_type: FieldType::Integer, required: false, default_value: None };
        assert_eq!(format_field_value(&field, &flow_with_custom_field("count", "-1234")), "-1,234");
    }

    #[test]
    fn format_field_value_float_gets_a_thousands_separator() {
        let field = CategoryField { name: "amount".to_string(), field_type: FieldType::Float, required: false, default_value: None };
        assert_eq!(format_field_value(&field, &flow_with_custom_field("amount", "1234567.5")), "1,234,567.50");
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

    // --- right_align_x ---

    #[test]
    fn right_align_x_places_longer_text_further_left() {
        let short_x = right_align_x("$5.00", 100.0, 12.0);
        let long_x = right_align_x("$5,000,000.00", 100.0, 12.0);
        assert!(long_x < short_x, "wider text should start further left to keep the same right edge");
    }

    #[test]
    fn right_align_x_estimated_right_edge_matches_the_target() {
        let text = "$1,234.00";
        let right_edge = 100.0;
        let x = right_align_x(text, right_edge, 12.0);
        assert_eq!(x + estimate_text_width_mm(text, 12.0), right_edge);
    }

    // --- right_align_x_clamped ---

    #[test]
    fn right_align_x_clamped_floors_at_the_left_edge_for_text_too_wide_to_fit() {
        // A right edge close to the left edge, with text wide enough that a
        // plain `right_align_x` would push it left of the column entirely.
        let x = right_align_x_clamped("$999,999,999.99", 40.0, 20.0, 12.0);
        assert_eq!(x, 20.0);
    }

    #[test]
    fn right_align_x_clamped_matches_right_align_x_when_text_fits() {
        let text = "$5.00";
        let right_edge = 100.0;
        let left_edge = 20.0;
        assert_eq!(right_align_x_clamped(text, right_edge, left_edge, 12.0), right_align_x(text, right_edge, 12.0));
    }

    // --- center_align_x ---

    #[test]
    fn center_align_x_centers_text_within_the_bounds() {
        let x = center_align_x("Amount", 55.0, 77.0, 12.0);
        let width = estimate_text_width_mm("Amount", 12.0);
        // Left and right gaps around the text should be equal.
        assert!((x - 55.0 - (77.0 - 55.0 - width) / 2.0).abs() < 1e-9);
    }

    #[test]
    fn center_align_x_floors_at_the_left_edge_for_text_too_wide_to_fit() {
        let x = center_align_x("A Very Long Header Label Indeed", 55.0, 77.0, 12.0);
        assert_eq!(x, 55.0);
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
    fn compute_column_layout_amount_right_edge_stays_within_the_amount_column() {
        let layout = compute_column_layout(0);
        assert!(layout.amount_right_edge_x > layout.amount_x, "right edge should be to the right of where the column starts");
        assert!(layout.amount_right_edge_x < layout.description_x, "right edge should leave a gap before the next column");
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

    // --- PageCursor::ensure_space ---

    #[test]
    fn ensure_page_space_resets_y_pos_when_not_enough_room() {
        let (doc, page1, layer1) = PdfDocument::new("Test", Mm(210.0), Mm(297.0), "Layer 1");
        let font = doc.add_builtin_font(BuiltinFont::TimesRoman).unwrap();
        let period = "Time Period: Jan 01, 2024 to Dec 31, 2024";
        let mut cursor = PageCursor {
            doc: &doc,
            page: page1,
            layer_idx: layer1,
            y_pos: Mm(30.0), // close to the bottom margin already
            page_number: 1,
            time_period_text: period,
            chrome_font: &font,
        };

        cursor.ensure_space(20.0);

        assert!(cursor.y_pos.0 > 100.0, "y_pos should reset near the top of a new page, got {}", cursor.y_pos.0);
        assert_eq!(cursor.page_number, 2, "starting a new page should advance the page number");
    }

    #[test]
    fn ensure_page_space_leaves_y_pos_alone_when_room_remains() {
        let (doc, page1, layer1) = PdfDocument::new("Test", Mm(210.0), Mm(297.0), "Layer 1");
        let font = doc.add_builtin_font(BuiltinFont::TimesRoman).unwrap();
        let period = "Time Period: Jan 01, 2024 to Dec 31, 2024";
        let mut cursor = PageCursor {
            doc: &doc,
            page: page1,
            layer_idx: layer1,
            y_pos: Mm(200.0),
            page_number: 1,
            time_period_text: period,
            chrome_font: &font,
        };

        cursor.ensure_space(20.0);

        assert_eq!(cursor.y_pos.0, 200.0, "y_pos shouldn't change when there's already enough room");
        assert_eq!(cursor.page_number, 1, "page number shouldn't advance when there's already enough room");
    }

    #[test]
    fn ordered_category_ids_follows_the_given_order() {
        let mut present: HashMap<String, i32> = HashMap::new();
        present.insert("b".to_string(), 1);
        present.insert("a".to_string(), 2);
        present.insert("c".to_string(), 3);

        let order = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        assert_eq!(ordered_category_ids(&order, &present), vec!["a", "b", "c"]);
    }

    #[test]
    fn ordered_category_ids_appends_ids_missing_from_order() {
        let mut present: HashMap<String, i32> = HashMap::new();
        present.insert("known".to_string(), 1);
        present.insert("orphaned".to_string(), 2);

        let order = vec!["known".to_string()];
        let result = ordered_category_ids(&order, &present);

        assert_eq!(result[0], "known");
        assert!(result.contains(&"orphaned".to_string()), "an id missing from `order` must still be included, not dropped");
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn ordered_category_ids_skips_order_entries_with_no_data() {
        let mut present: HashMap<String, i32> = HashMap::new();
        present.insert("has_data".to_string(), 1);

        let order = vec!["no_data".to_string(), "has_data".to_string()];
        assert_eq!(ordered_category_ids(&order, &present), vec!["has_data"]);
    }
} 