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
use crate::update_check;

const AUTO_EXPIRE_SECS: i64 = 30;

/// Render the alert banner as a TopBottomPanel if an un-dismissed, unexpired
/// alert is present. Otherwise a zero-height no-op.
pub fn show(ctx: &egui::Context, state: &AppState) {
    let now = chrono::Utc::now().timestamp();

    // Find the newest un-dismissed alert. Update-prompt alerts ignore the
    // 30 s expiry — they should stay until the user dismisses or applies
    // them. Other alerts auto-expire.
    let (idx, severity, title, body, action_url, action_update_wizard) = {
        let g = state.lock().unwrap();
        let hit = g.alerts.iter().enumerate().rev().find(|(_, a)| {
            if a.dismissed {
                return false;
            }
            if a.action_update_wizard || a.action_url.is_some() {
                return true;
            }
            now - a.timestamp < AUTO_EXPIRE_SECS
        });
        match hit {
            Some((i, a)) => (
                i,
                a.severity,
                a.title.clone(),
                a.body.clone(),
                a.action_url.clone(),
                a.action_update_wizard,
            ),
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
    let mut update_now = false;
    let mut open_url_now = false;
    let banner_height = if action_update_wizard || action_url.is_some() {
        38.0
    } else {
        32.0
    };
    egui::TopBottomPanel::top("alert-banner")
        .exact_height(banner_height)
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
                    if action_update_wizard {
                        ui.add_space(6.0);
                        if ui
                            .button(
                                RichText::new("Update Now")
                                    .color(egui::Color32::WHITE)
                                    .size(11.5)
                                    .strong(),
                            )
                            .clicked()
                        {
                            update_now = true;
                        }
                    }
                    if action_url.is_some() && !action_update_wizard {
                        ui.add_space(6.0);
                        if ui
                            .button(
                                RichText::new("Open")
                                    .color(egui::Color32::WHITE)
                                    .size(11.5)
                                    .strong(),
                            )
                            .clicked()
                        {
                            open_url_now = true;
                        }
                    }
                });
            });
        });

    if update_now {
        match update_check::launch_update_wizard(action_url.as_deref()) {
            Ok(()) => {
                tracing::info!("update wizard launched");
                if let Ok(mut g) = state.lock() {
                    g.dismiss_alert(idx);
                }
            }
            Err(e) => tracing::warn!(error = %e, "update wizard launch failed"),
        }
    } else if open_url_now {
        if let Some(url) = action_url.as_deref() {
            match update_check::open_action_url(url) {
                Ok(()) => {
                    if let Ok(mut g) = state.lock() {
                        g.dismiss_alert(idx);
                    }
                }
                Err(e) => tracing::warn!(error = %e, "open url failed"),
            }
        }
    }

    if dismiss {
        state.lock().unwrap().dismiss_alert(idx);
    }
}
