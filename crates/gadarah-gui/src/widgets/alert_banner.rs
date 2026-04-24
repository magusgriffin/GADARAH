//! Inline alert banner — top-of-content strip that surfaces the newest
//! un-dismissed `Alert` from `SharedState.alerts`. Severity-coloured,
//! dismissible, auto-expires after 30 s.
//!
//! The Alerts feed used to live only in the Logs tab, which meant critical
//! events (broker desync, vol halt, kill-switch trip) were invisible unless
//! the user was already looking there. This banner fixes that without
//! overriding the existing Logs panel.

use eframe::egui;
use egui::RichText;

use crate::state::{AlertSeverity, AppState};
use crate::theme;

const AUTO_EXPIRE_SECS: i64 = 30;

/// Render the alert banner as a TopBottomPanel if an un-dismissed, unexpired
/// alert is present. Otherwise a zero-height no-op.
pub fn show(ctx: &egui::Context, state: &AppState) {
    let now = chrono::Utc::now().timestamp();

    // Find the newest un-dismissed + unexpired alert, capture the index so
    // the Dismiss button can clear it.
    let (idx, severity, title, body) = {
        let g = state.lock().unwrap();
        let hit = g
            .alerts
            .iter()
            .enumerate()
            .rev()
            .find(|(_, a)| !a.dismissed && now - a.timestamp < AUTO_EXPIRE_SECS);
        match hit {
            Some((i, a)) => (i, a.severity, a.title.clone(), a.body.clone()),
            None => return,
        }
    };

    let (bg, fg, rim) = match severity {
        AlertSeverity::Info => (
            egui::Color32::from_rgb(16, 34, 58),
            theme::TEXT,
            theme::BLUE,
        ),
        AlertSeverity::Warning => (
            egui::Color32::from_rgb(58, 44, 16),
            theme::TEXT,
            theme::YELLOW,
        ),
        AlertSeverity::Danger => (
            egui::Color32::from_rgb(58, 16, 16),
            theme::TEXT,
            theme::RED,
        ),
    };
    let icon = match severity {
        AlertSeverity::Info => "●",
        AlertSeverity::Warning => "▲",
        AlertSeverity::Danger => "✖",
    };

    let mut dismiss = false;
    egui::TopBottomPanel::top("alert-banner")
        .exact_height(32.0)
        .frame(
            egui::Frame::new()
                .fill(bg)
                .stroke(egui::Stroke::new(1.0, rim))
                .inner_margin(egui::Margin::symmetric(12, 6)),
        )
        .show(ctx, |ui| {
            ui.horizontal_centered(|ui| {
                ui.label(RichText::new(icon).color(rim).size(14.0).strong());
                ui.add_space(6.0);
                ui.label(
                    RichText::new(&title)
                        .color(fg)
                        .size(12.5)
                        .strong(),
                );
                if !body.is_empty() {
                    ui.label(
                        RichText::new(format!("— {body}"))
                            .color(fg)
                            .size(12.0),
                    );
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .small_button(RichText::new("Dismiss").color(theme::MUTED).size(11.0))
                        .clicked()
                    {
                        dismiss = true;
                    }
                });
            });
        });

    if dismiss {
        state.lock().unwrap().dismiss_alert(idx);
    }
}
