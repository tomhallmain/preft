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

impl Default for TimePeriod {
    fn default() -> Self {
        let now = chrono::Local::now().naive_local();
        let month = now.month();
        
        // If we're significantly after tax time (after April), default to this year
        // Otherwise default to last year
        if month > 4 {
            TimePeriod::ThisYear
        } else {
            TimePeriod::LastYear
        }
    }
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
                let now = chrono::Local::now();
                let start = now.date_naive().with_month(1).unwrap().with_day(1).unwrap();
                let end = start.with_year(start.year() - 1).unwrap();
                format!("Time Period: {} to {}", end.format("%B %d, %Y"), start.format("%B %d, %Y"))
            },
            TimePeriod::ThisYear => {
                let now = chrono::Local::now();
                let start = now.date_naive().with_month(1).unwrap().with_day(1).unwrap();
                format!("Time Period: {} to {}", start.format("%B %d, %Y"), now.format("%B %d, %Y"))
            },
            TimePeriod::Custom(start, end) => {
                format!("Time Period: {} to {}", start.format("%B %d, %Y"), end.format("%B %d, %Y"))
            },
        };
        current_layer.use_text(&time_period_text, 12.0, Mm(20.0), Mm(230.0), &subtitle_font);

        // Group flows by category
        let mut category_flows: HashMap<String, Vec<&Flow>> = HashMap::new();
        for flow in &self.flows {
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
                let mut grouped_flows: HashMap<String, Vec<&Flow>> = HashMap::new();
                for flow in flows {
                    if let Some(value) = flow.custom_fields.get(group_by) {
                        grouped_flows.entry(value.clone())
                            .or_default()
                            .push(flow);
                    }
                }

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