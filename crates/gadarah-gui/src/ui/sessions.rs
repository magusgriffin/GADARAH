//! Current Sessions tab — 24-hour UTC timeline showing the four major forex
//! sessions (Sydney, Tokyo, London, New York), the active-overlap window,
//! and the current time marker. Also surfaces the heads permitted under the
//! current session per `gadarah-core`'s `Session::from_utc_hour`.
//!
//! The tab is pure derivation from the wall clock plus the shared state's
//! head allowlist — no extra persisted state.

use chrono::{Timelike, Utc};
use eframe::egui::{self, Color32, CornerRadius, RichText, Stroke};
use gadarah_core::types::Session as CoreSession;

use crate::state::AppState;
use crate::theme;

pub struct SessionsPanel;

/// One row of the timeline. Start/end are UTC hours in [0, 24). When
/// `end < start` the session wraps midnight.
struct SessionBar {
    label: &'static str,
    short: &'static str,
    start_h: f32,
    end_h: f32,
    color: Color32,
}

const BARS: [SessionBar; 4] = [
    SessionBar {
        label: "Sydney",
        short: "SYD",
        start_h: 22.0,
        end_h: 7.0,
        color: Color32::from_rgb(120, 180, 255),
    },
    SessionBar {
        label: "Tokyo",
        short: "TYO",
        start_h: 0.0,
        end_h: 9.0,
        color: Color32::from_rgb(230, 80, 110),
    },
    SessionBar {
        label: "London",
        short: "LDN",
        start_h: 8.0,
        end_h: 17.0,
        color: Color32::from_rgb(80, 200, 150),
    },
    SessionBar {
        label: "New York",
        short: "NY",
        start_h: 13.0,
        end_h: 22.0,
        color: Color32::from_rgb(230, 180, 70),
    },
];

impl SessionsPanel {
    pub fn show(ui: &mut egui::Ui, state: &AppState) {
        let now = Utc::now();
        let utc_hour = now.hour() as f32 + (now.minute() as f32) / 60.0;
        let utc_hour_int = now.hour() as u8;
        let core_session = CoreSession::from_utc_hour(utc_hour_int);

        // Header band ────────────────────────────────────────────────────────
        theme::card().show(ui, |ui| {
            ui.horizontal(|ui| {
                theme::heading(ui, "Current Sessions");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new(now.format("%Y-%m-%d %H:%M:%S UTC").to_string())
                            .size(12.0)
                            .color(theme::MUTED)
                            .monospace(),
                    );
                });
            });

            ui.add_space(4.0);
            ui.label(
                RichText::new("All times in Coordinated Universal Time. Colored bars show when each major center is open; overlaps are the fat liquidity windows.")
                    .size(11.5)
                    .color(theme::MUTED),
            );
        });

        ui.add_space(12.0);

        // Timeline ────────────────────────────────────────────────────────────
        theme::card().show(ui, |ui| {
            timeline(ui, utc_hour);
        });

        ui.add_space(12.0);

        // Active-session summary ─────────────────────────────────────────────
        let active = active_session_names(utc_hour);
        let g = state.lock().unwrap();
        let head_count = g.active_heads.len();
        let head_names: String = if head_count == 0 {
            "—".to_string()
        } else {
            g.active_heads
                .iter()
                .map(|h| format!("{:?}", h).to_lowercase())
                .collect::<Vec<_>>()
                .join(", ")
        };
        drop(g);

        theme::card().show(ui, |ui| {
            ui.horizontal(|ui| {
                theme::section_label(ui, "ACTIVE NOW");
                ui.add_space(6.0);
                if active.is_empty() {
                    ui.label(
                        RichText::new("DEAD ZONE")
                            .size(14.0)
                            .color(theme::MUTED)
                            .strong(),
                    );
                } else {
                    for name in &active {
                        let color = BARS
                            .iter()
                            .find(|b| b.label == *name)
                            .map(|b| b.color)
                            .unwrap_or(theme::ACCENT);
                        theme::pill(ui, name, color.linear_multiply(0.25), color);
                        ui.add_space(6.0);
                    }
                    if active.len() >= 2 {
                        ui.add_space(6.0);
                        theme::pill(
                            ui,
                            "  OVERLAP  ",
                            Color32::from_rgb(32, 24, 8),
                            theme::GOLD,
                        );
                    }
                }
            });

            ui.add_space(10.0);
            ui.horizontal(|ui| {
                theme::section_label(ui, "CORE CLASSIFICATION");
                ui.add_space(6.0);
                ui.label(
                    RichText::new(format!("{:?}", core_session))
                        .size(13.0)
                        .color(theme::TEXT)
                        .monospace(),
                );
                ui.add_space(12.0);
                theme::section_label(ui, "SIZING MULT");
                ui.add_space(6.0);
                ui.label(
                    RichText::new(format!("{:.2}×", core_session.sizing_multiplier()))
                        .size(13.0)
                        .color(theme::TEXT)
                        .monospace(),
                );
                ui.add_space(12.0);
                theme::section_label(ui, "SLIPPAGE MULT");
                ui.add_space(6.0);
                ui.label(
                    RichText::new(format!("{:.2}×", core_session.slippage_multiplier()))
                        .size(13.0)
                        .color(theme::TEXT)
                        .monospace(),
                );
            });

            ui.add_space(10.0);
            ui.horizontal(|ui| {
                theme::section_label(ui, "ACTIVE HEADS");
                ui.add_space(6.0);
                ui.label(RichText::new(head_names).size(12.5).color(theme::TEXT));
            });
        });
    }
}

fn timeline(ui: &mut egui::Ui, now_hour: f32) {
    let available_w = ui.available_width().max(320.0);
    let label_col = 72.0_f32;
    let bar_area_w = (available_w - label_col - 24.0).max(240.0);
    let row_h = 22.0;
    let row_gap = 8.0;
    let total_h = (row_h + row_gap) * BARS.len() as f32 + 42.0;

    let (rect, _) = ui.allocate_exact_size(egui::vec2(available_w, total_h), egui::Sense::hover());
    let painter = ui.painter();

    let hour_to_x = |h: f32| rect.left() + label_col + (h / 24.0) * bar_area_w;

    // Bars
    for (i, bar) in BARS.iter().enumerate() {
        let y = rect.top() + (row_h + row_gap) * i as f32;

        // Label
        painter.text(
            egui::pos2(rect.left() + label_col - 8.0, y + row_h / 2.0),
            egui::Align2::RIGHT_CENTER,
            bar.label,
            egui::FontId::proportional(12.0),
            theme::MUTED,
        );

        // Background rail
        let rail = egui::Rect::from_min_size(
            egui::pos2(rect.left() + label_col, y),
            egui::vec2(bar_area_w, row_h),
        );
        painter.rect_filled(rail, CornerRadius::same(3), theme::INPUT_BG);

        // Active segments (may wrap midnight → draw two)
        let segments = segments_for(bar.start_h, bar.end_h);
        let active_now = hour_in_range(now_hour, bar.start_h, bar.end_h);
        let fill_color = if active_now {
            bar.color
        } else {
            bar.color.linear_multiply(0.45)
        };

        for (s, e) in segments {
            let seg = egui::Rect::from_min_max(
                egui::pos2(hour_to_x(s), y + 2.0),
                egui::pos2(hour_to_x(e), y + row_h - 2.0),
            );
            painter.rect_filled(seg, CornerRadius::same(2), fill_color);
            if active_now {
                painter.rect_stroke(
                    seg,
                    CornerRadius::same(2),
                    Stroke::new(1.0, Color32::WHITE),
                    egui::StrokeKind::Outside,
                );
            }
            painter.text(
                seg.center(),
                egui::Align2::CENTER_CENTER,
                bar.short,
                egui::FontId::monospace(10.0),
                Color32::from_rgb(18, 14, 8),
            );
        }
    }

    // Hour axis
    let axis_y = rect.top() + (row_h + row_gap) * BARS.len() as f32 + 10.0;
    painter.line_segment(
        [
            egui::pos2(rect.left() + label_col, axis_y),
            egui::pos2(rect.left() + label_col + bar_area_w, axis_y),
        ],
        Stroke::new(1.0, theme::BORDER),
    );
    for h in (0..=24).step_by(3) {
        let x = hour_to_x(h as f32);
        painter.line_segment(
            [egui::pos2(x, axis_y), egui::pos2(x, axis_y + 4.0)],
            Stroke::new(1.0, theme::MUTED),
        );
        painter.text(
            egui::pos2(x, axis_y + 16.0),
            egui::Align2::CENTER_CENTER,
            format!("{:02}", h),
            egui::FontId::monospace(10.0),
            theme::MUTED,
        );
    }

    // Now marker
    let x_now = hour_to_x(now_hour);
    painter.line_segment(
        [
            egui::pos2(x_now, rect.top()),
            egui::pos2(x_now, axis_y),
        ],
        Stroke::new(1.5, theme::ACCENT),
    );
    painter.circle_filled(egui::pos2(x_now, rect.top()), 3.5, theme::ACCENT);
}

fn segments_for(start: f32, end: f32) -> Vec<(f32, f32)> {
    if end >= start {
        vec![(start, end)]
    } else {
        vec![(start, 24.0), (0.0, end)]
    }
}

fn hour_in_range(h: f32, start: f32, end: f32) -> bool {
    if end >= start {
        h >= start && h < end
    } else {
        h >= start || h < end
    }
}

fn active_session_names(h: f32) -> Vec<&'static str> {
    BARS.iter()
        .filter(|b| hour_in_range(h, b.start_h, b.end_h))
        .map(|b| b.label)
        .collect()
}
