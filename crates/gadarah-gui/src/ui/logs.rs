//! Logs panel — Scrolling event log with level filter

use eframe::egui;
use egui::RichText;

use crate::state::{AppState, LogEntry, LogLevel};
use crate::theme;

pub struct LogsPanel;

impl LogsPanel {
    pub fn show(ui: &mut egui::Ui, app_state: &AppState) {
        let (logs, current_filter) = {
            let g = app_state.lock().unwrap();
            let logs: Vec<LogEntry> = g.get_filtered_logs().into_iter().cloned().collect();
            (logs, g.log_filter)
        };

        theme::heading(ui, "Event Log");
        ui.label(
            RichText::new(
                "All bot activity is recorded here. Filter by severity to focus on what matters.",
            )
            .color(theme::MUTED)
            .size(12.5),
        );
        ui.add_space(10.0);

        // ── Filter bar ───────────────────────────────────────────────────────
        theme::card_sm().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Show:").color(theme::MUTED).size(12.5));
                let mut new_filter = current_filter;

                let levels: &[(LogLevel, &str, egui::Color32)] = &[
                    (LogLevel::Trace, "Everything", theme::DIM),
                    (LogLevel::Debug, "Debug+", theme::MUTED),
                    (LogLevel::Info, "Info+", theme::TEXT),
                    (LogLevel::Warn, "Warnings+", theme::YELLOW),
                    (LogLevel::Error, "Errors Only", theme::RED),
                ];

                for (level, label, color) in levels {
                    let selected = new_filter == *level;
                    let btn = ui.add(
                        egui::Button::new(RichText::new(*label).color(*color).size(12.5))
                            .fill(if selected {
                                egui::Color32::from_rgb(28, 35, 48)
                            } else {
                                egui::Color32::TRANSPARENT
                            })
                            .stroke(if selected {
                                egui::Stroke::new(1.0, theme::ACCENT)
                            } else {
                                egui::Stroke::new(1.0, egui::Color32::TRANSPARENT)
                            }),
                    );
                    if btn.clicked() {
                        new_filter = *level;
                    }
                }

                if new_filter != current_filter {
                    app_state.lock().unwrap().log_filter = new_filter;
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new(format!("{} entries", logs.len()))
                            .color(theme::DIM)
                            .size(11.5),
                    );
                });
            });
        });

        ui.add_space(8.0);

        // ── Log entries ───────────────────────────────────────────────────────
        theme::card().show(ui, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .stick_to_bottom(true)
                .max_height(ui.available_height() - 20.0)
                .show(ui, |ui| {
                    if logs.is_empty() {
                        theme::empty_state(
                            ui,
                            "📋",
                            "No Log Entries",
                            "No events at this filter level. Try selecting a lower severity.",
                        );
                    } else {
                        for (i, entry) in logs.iter().rev().enumerate() {
                            Self::log_row(ui, entry, i % 2 == 0);
                        }
                    }
                });
        });
    }

    fn log_row(ui: &mut egui::Ui, entry: &LogEntry, odd: bool) {
        let (level_color, level_bg) = match entry.level {
            LogLevel::Trace => (theme::DIM, egui::Color32::TRANSPARENT),
            LogLevel::Debug => (theme::MUTED, egui::Color32::TRANSPARENT),
            LogLevel::Info => (theme::TEXT, egui::Color32::TRANSPARENT),
            LogLevel::Warn => (theme::YELLOW, egui::Color32::from_rgb(30, 24, 5)),
            LogLevel::Error => (theme::RED, egui::Color32::from_rgb(35, 8, 8)),
        };

        let row_bg = if level_bg != egui::Color32::TRANSPARENT {
            level_bg
        } else if odd {
            egui::Color32::from_rgb(16, 22, 30)
        } else {
            egui::Color32::TRANSPARENT
        };

        egui::Frame::new()
            .fill(row_bg)
            .corner_radius(4u8)
            .inner_margin(egui::Margin {
                left: 8,
                right: 8,
                top: 3,
                bottom: 3,
            })
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let time_str = chrono::DateTime::from_timestamp(entry.timestamp, 0)
                        .map(|dt| dt.format("%H:%M:%S").to_string())
                        .unwrap_or_default();
                    ui.add_sized(
                        [60.0, 16.0],
                        egui::Label::new(
                            RichText::new(time_str)
                                .color(theme::DIM)
                                .monospace()
                                .size(11.5),
                        ),
                    );
                    ui.add_sized(
                        [60.0, 16.0],
                        egui::Label::new(
                            RichText::new(entry.level.as_str())
                                .color(level_color)
                                .monospace()
                                .size(11.5)
                                .strong(),
                        ),
                    );
                    ui.label(RichText::new(&entry.message).color(level_color).size(12.5));
                });
            });
    }
}
