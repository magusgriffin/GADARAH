//! Permanent top-of-window stripe that makes demo vs. live visually
//! unmistakable. Yellow when paper, green when real money, gray when no
//! broker link. Renders as its own `TopBottomPanel` above every other panel
//! so it is impossible to miss.

use eframe::egui::{self, Color32, Stroke};

use crate::state::ConnectionStatus;
use crate::theme;

/// Height in logical px. Exposed so the caller can reserve layout space if it
/// ever needs to — today nothing does.
pub const BANNER_HEIGHT: f32 = 22.0;

/// Render the banner. Must be called before the main `top_bar` panel so it
/// sits at the very top of the window.
pub fn show(ctx: &egui::Context, status: ConnectionStatus) {
    let (label, fg, bg) = match status {
        ConnectionStatus::ConnectedLive => (
            "  LIVE — REAL MONEY  ",
            Color32::from_rgb(240, 250, 240),
            Color32::from_rgb(18, 78, 36),
        ),
        ConnectionStatus::ConnectedDemo => (
            "  DEMO ACCOUNT — NO REAL MONEY AT RISK  ",
            Color32::from_rgb(32, 22, 4),
            theme::YELLOW,
        ),
        ConnectionStatus::Connecting => (
            "  CONNECTING…  ",
            Color32::from_rgb(230, 237, 244),
            Color32::from_rgb(20, 40, 72),
        ),
        ConnectionStatus::Disconnected => (
            "  NOT CONNECTED  ",
            Color32::from_rgb(214, 222, 234),
            Color32::from_rgb(48, 52, 60),
        ),
    };

    egui::TopBottomPanel::top("gadarah_demo_banner")
        .exact_height(BANNER_HEIGHT)
        .frame(
            egui::Frame::new()
                .fill(bg)
                .stroke(Stroke::new(1.0, bg.linear_multiply(0.7))),
        )
        .show(ctx, |ui| {
            ui.set_height(BANNER_HEIGHT);
            ui.horizontal_centered(|ui| {
                ui.with_layout(
                    egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
                    |ui| {
                        ui.label(
                            egui::RichText::new(label)
                                .color(fg)
                                .size(11.0)
                                .strong()
                                .monospace(),
                        );
                    },
                );
            });
        });
}
