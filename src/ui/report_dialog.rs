use eframe::egui;
use chrono::Datelike;
use std::fs::File;
use std::io::Write;

use crate::app::PreftApp;
use crate::models::Flow;
use crate::reporting::{FontVariant, ReportCategoryInfo, ReportGenerator, TimePeriod};
use std::collections::HashMap;

/// The "Custom" range is seeded with Jan 1 -> today the first time it's
/// selected; the date pickers shown below the combo box let it be narrowed
/// from there. This request (including any custom range) lives only on
/// `PreftApp` in memory -- it's never written to `UserSettings`/the database,
/// so it does not persist across app restarts.
///
/// Every widget below reads/writes `app.report_request` directly rather than
/// a local clone. This function runs every frame the dialog is open (not
/// once), so a local clone that's only written back to `app.report_request`
/// when the dialog closes loses any edit made on a frame where the dialog
/// stays open -- which is exactly what used to make the "Custom" selection
/// (and every other field: title, subtitle, fonts, group-by) flash and then
/// revert, since egui repaints continuously while a combo-box popup is open.
pub fn show_report_dialog(ctx: &egui::Context, app: &mut PreftApp) {
    // The report covers flows across all categories, not just whichever one
    // happens to be selected in the sidebar -- so "Group By" needs to offer
    // every category's custom field names, not just the selected category's.
    // (Previously this used `app.get_selected_category()`, which meant the
    // dropdown was often empty, e.g. whenever the dashboard was open instead
    // of a specific category.)
    let mut field_names: Vec<String> = app.categories.iter()
        .flat_map(|c| c.fields.iter().map(|f| f.name.clone()))
        .collect();
    field_names.sort();
    field_names.dedup();

    let flows: Vec<Flow> = app.flows.clone();
    let categories: HashMap<String, ReportCategoryInfo> = app.categories.iter()
        .map(|cat| (cat.id.clone(), ReportCategoryInfo {
            name: cat.name.clone(),
            flow_type: cat.flow_type.clone(),
            fields: cat.fields.clone(),
        }))
        .collect();
    let mut should_close = false;
    let mut pdf_data = None;
    let mut show_window = true;

    egui::Window::new("Generate Report")
        .open(&mut show_window)
        .show(ctx, |ui| {
            ui.heading("Report Settings");

            show_time_period_selection(ui, &mut app.report_request.time_period);

            // Group by selection
            show_group_by_selection(ui, &mut app.report_request.group_by, &field_names);

            // Title and subtitle
            ui.horizontal(|ui| {
                ui.label("Title:");
                ui.text_edit_singleline(&mut app.report_request.title);
            });
            ui.horizontal(|ui| {
                ui.label("Subtitle:");
                ui.text_edit_singleline(&mut app.report_request.subtitle);
            });

            // Font settings
            ui.separator();
            ui.heading("Font Settings");

            show_font_selection(ui, "title_font", "Title Font:", &mut app.report_request.font_settings.title_font);
            show_font_selection(ui, "subtitle_font", "Subtitle Font:", &mut app.report_request.font_settings.subtitle_font);
            show_font_selection(ui, "header_font", "Header Font:", &mut app.report_request.font_settings.header_font);
            show_font_selection(ui, "body_font", "Body Font:", &mut app.report_request.font_settings.body_font);

            // Generate button
            if ui.button("Generate Report").clicked() {
                let generator = ReportGenerator::new(flows.clone(), categories.clone());
                if let Ok(data) = generator.generate_report(&app.report_request) {
                    pdf_data = Some(data);
                    should_close = true;
                }
            }
        });

    if should_close || !show_window {
        if let Some(data) = pdf_data {
            // Save the PDF file
            if let Some(path) = rfd::FileDialog::new()
                .set_title("Save Report")
                .set_file_name("financial_report.pdf")
                .save_file() {
                if let Ok(mut file) = File::create(path) {
                    if let Err(e) = file.write_all(&data) {
                        log::error!("Failed to save PDF: {}", e);
                    }
                }
            }
        }
        app.show_report_dialog = false;
    }
}

fn show_time_period_selection(ui: &mut egui::Ui, time_period: &mut TimePeriod) {
    ui.horizontal(|ui| {
        ui.label("Time Period:");
        egui::ComboBox::from_id_source("time_period")
            .selected_text(time_period_label(time_period))
            .show_ui(ui, |ui| {
                ui.selectable_value(time_period, TimePeriod::LastYear, "Last Year");
                ui.selectable_value(time_period, TimePeriod::ThisYear, "This Year");

                let is_custom = matches!(time_period, TimePeriod::Custom(_, _));
                if ui.selectable_label(is_custom, "Custom").clicked() && !is_custom {
                    let today = chrono::Local::now().date_naive();
                    *time_period = TimePeriod::Custom(
                        today.with_month(1).unwrap().with_day(1).unwrap(),
                        today,
                    );
                }
            });
    });

    // Only shown (and only narrowable) once "Custom" is actually selected;
    // re-selecting "Custom" above does *not* reset an already-narrowed range,
    // unlike the old behavior this replaces.
    if let TimePeriod::Custom(start, end) = time_period {
        ui.horizontal(|ui| {
            ui.label("From:");
            ui.add(egui_extras::DatePickerButton::new(start).id_source("report_custom_start"));
            ui.label("To:");
            ui.add(egui_extras::DatePickerButton::new(end).id_source("report_custom_end"));
        });
    }
}

fn time_period_label(time_period: &TimePeriod) -> String {
    match time_period {
        TimePeriod::LastYear => "Last Year".to_string(),
        TimePeriod::ThisYear => "This Year".to_string(),
        TimePeriod::Custom(start, end) => format!(
            "Custom: {} to {}",
            start.format("%b %d, %Y"),
            end.format("%b %d, %Y")
        ),
    }
}

fn show_group_by_selection(ui: &mut egui::Ui, group_by: &mut Option<String>, field_names: &[String]) {
    ui.horizontal(|ui| {
        ui.label("Group By:");
        egui::ComboBox::from_id_source("group_by")
            .selected_text(group_by.as_deref().unwrap_or("None"))
            .show_ui(ui, |ui| {
                ui.selectable_value(group_by, None, "None");
                for name in field_names {
                    ui.selectable_value(group_by, Some(name.clone()), name);
                }
            });
    });
}

fn show_font_selection(ui: &mut egui::Ui, id_source: &str, label: &str, selected: &mut FontVariant) {
    ui.horizontal(|ui| {
        ui.label(label);
        egui::ComboBox::from_id_source(id_source)
            .selected_text(selected.get_display_name())
            .show_ui(ui, |ui| {
                for variant in [
                    FontVariant::RobotoRegular,
                    FontVariant::RobotoBold,
                    FontVariant::RobotoItalic,
                    FontVariant::RobotoBoldItalic,
                    FontVariant::TimesRegular,
                    FontVariant::TimesBold,
                    FontVariant::TimesItalic,
                    FontVariant::TimesBoldItalic,
                ] {
                    ui.selectable_value(selected, variant, variant.get_display_name());
                }
            });
    });
}
