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
}

impl ReportGenerator {
    pub fn new(flows: Vec<Flow>) -> Self {
        Self { flows }
    }

    pub fn generate_report(&self, request: &ReportRequest) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        // Filter flows based on time period
        let filtered_flows = self.filter_flows_by_period(&request.time_period);
        
        // Filter flows based on selection if specified
        let filtered_flows = if !request.selected_flows.is_empty() {
            filtered_flows.into_iter()
                .filter(|flow| request.selected_flows.contains(&flow.id))
                .collect()
        } else {
            filtered_flows
        };

        // Group flows if specified
        let grouped_flows = if let Some(group_by) = &request.group_by {
            self.group_flows(&filtered_flows, group_by)
        } else {
            vec![(None, filtered_flows)]
        };

        // Generate PDF
        self.generate_pdf(&grouped_flows, request)
    }

    fn filter_flows_by_period(&self, period: &TimePeriod) -> Vec<Flow> {
        let (start_date, end_date) = match period {
            TimePeriod::LastYear => {
                let now = chrono::Local::now().naive_local();
                let last_year = now.year() - 1;
                (
                    NaiveDate::from_ymd_opt(last_year, 1, 1).unwrap(),
                    NaiveDate::from_ymd_opt(last_year, 12, 31).unwrap()
                )
            },
            TimePeriod::ThisYear => {
                let now = chrono::Local::now().naive_local();
                (
                    NaiveDate::from_ymd_opt(now.year(), 1, 1).unwrap(),
                    NaiveDate::from_ymd_opt(now.year(), 12, 31).unwrap()
                )
            },
            TimePeriod::Custom(start, end) => (*start, *end),
        };

        self.flows.iter()
            .filter(|flow| flow.date >= start_date && flow.date <= end_date)
            .cloned()
            .collect()
    }

    fn group_flows(&self, flows: &[Flow], field_name: &str) -> Vec<(Option<String>, Vec<Flow>)> {
        let mut groups: HashMap<String, Vec<Flow>> = HashMap::new();
        
        for flow in flows {
            if let Some(value) = flow.custom_fields.get(field_name) {
                groups.entry(value.clone())
                    .or_default()
                    .push(flow.clone());
            }
        }

        let mut result: Vec<(Option<String>, Vec<Flow>)> = groups.into_iter()
            .map(|(k, v)| (Some(k), v))
            .collect();
        
        // Sort by group name
        result.sort_by(|a, b| a.0.cmp(&b.0));
        
        result
    }

    fn generate_pdf(&self, grouped_flows: &[(Option<String>, Vec<Flow>)], request: &ReportRequest) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        // Create a new PDF document
        let (doc, page, layer) = PdfDocument::new(
            "Financial Report",
            Mm(210.0), // A4 width
            Mm(297.0), // A4 height
            "Layer 1"
        );

        let mut layer = doc.get_page(page).get_layer(layer);

        // Load fonts
        let title_font = self.load_font(&doc, &request.font_settings.title_font)?;
        let subtitle_font = self.load_font(&doc, &request.font_settings.subtitle_font)?;
        let header_font = self.load_font(&doc, &request.font_settings.header_font)?;
        let body_font = self.load_font(&doc, &request.font_settings.body_font)?;

        // Add title
        layer.set_font(&title_font, 24.0);
        layer.set_text_cursor(Mm(20.0), Mm(270.0));
        layer.write_text(&request.title, &title_font);

        // Add subtitle if not empty
        if !request.subtitle.is_empty() {
            layer.set_font(&subtitle_font, 16.0);
            layer.set_text_cursor(Mm(20.0), Mm(260.0));
            layer.write_text(&request.subtitle, &subtitle_font);
        }

        // Add time period
        let period_text = match &request.time_period {
            TimePeriod::LastYear => {
                let now = chrono::Local::now().naive_local();
                let last_year = now.year() - 1;
                format!("Period: January 1, {} - December 31, {}", last_year, last_year)
            },
            TimePeriod::ThisYear => {
                let now = chrono::Local::now().naive_local();
                format!("Period: January 1, {} - December 31, {}", now.year(), now.year())
            },
            TimePeriod::Custom(start, end) => {
                format!("Period: {} - {}", start.format("%B %d, %Y"), end.format("%B %d, %Y"))
            },
        };
        layer.set_font(&header_font, 12.0);
        layer.set_text_cursor(Mm(20.0), Mm(250.0));
        layer.write_text(&period_text, &header_font);

        // Add a line separator
        layer.set_outline_color(Color::Rgb(Rgb::new(0.0, 0.0, 0.0, None)));
        layer.set_outline_thickness(0.5);
        layer.add_line_break();
        layer.add_line_break();

        // Add flows
        let mut y_pos = Mm(240.0);
        for (group_name, flows) in grouped_flows {
            // Add group header if grouping is enabled
            if let Some(name) = group_name {
                layer.set_font(&header_font, 14.0);
                layer.set_text_cursor(Mm(20.0), y_pos);
                layer.write_text(name, &header_font);
                y_pos -= Mm(5.0);
            }

            // Add flows table
            layer.set_font(&body_font, 10.0);
            for flow in flows {
                if y_pos < Mm(20.0) {
                    // Add new page if we're running out of space
                    let (new_page, new_layer) = doc.add_page(Mm(210.0), Mm(297.0), "Layer 1");
                    y_pos = Mm(270.0);
                    layer = doc.get_page(new_page).get_layer(new_layer);
                }

                // Date
                layer.set_text_cursor(Mm(20.0), y_pos);
                layer.write_text(&flow.date.format("%Y-%m-%d").to_string(), &body_font);

                // Amount
                layer.set_text_cursor(Mm(60.0), y_pos);
                layer.write_text(&format!("${:.2}", flow.amount), &body_font);

                // Description
                layer.set_text_cursor(Mm(100.0), y_pos);
                layer.write_text(&flow.description, &body_font);

                y_pos -= Mm(5.0);
            }

            // Add group total if grouping is enabled
            if group_name.is_some() {
                let total: f64 = flows.iter().map(|f| f.amount).sum();
                layer.set_font(&header_font, 10.0);
                layer.set_text_cursor(Mm(20.0), y_pos);
                layer.write_text("Total:", &header_font);
                layer.set_text_cursor(Mm(60.0), y_pos);
                layer.write_text(&format!("${:.2}", total), &header_font);
                y_pos -= Mm(10.0);
            }
        }

        // Add grand total
        let grand_total: f64 = grouped_flows.iter()
            .flat_map(|(_, flows)| flows.iter().map(|f| f.amount))
            .sum();
        layer.set_font(&header_font, 12.0);
        layer.set_text_cursor(Mm(20.0), y_pos);
        layer.write_text("Grand Total:", &header_font);
        layer.set_text_cursor(Mm(60.0), y_pos);
        layer.write_text(&format!("${:.2}", grand_total), &header_font);

        // Save the PDF to a buffer
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