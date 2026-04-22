//! Approximate Cashout Value (ACV) widget.
//!
//! "What you'd walk away with right now." For a challenge phase, that's the
//! progress toward the profit target; for funded accounts it's the running
//! share since the last payout cycle. V1 renders the running total PnL —
//! firm-phase-aware math lands in Workstream D's payout extension.
//!
//! Staleness contract: when `stale_ms > 5000` the widget desaturates and drops
//! the breathing glow, so the user can tell at a glance that the number on
//! screen is older than five seconds.

use eframe::egui::{self, Color32, RichText, Stroke};
use rust_decimal::Decimal;

use crate::theme;
use crate::widgets::ornaments;

/// ms past which the ACV is rendered in a stale visual state.
pub const STALE_MS: u64 = 5_000;

pub struct AcvParams<'a> {
    /// Current equity minus starting balance — signed.
    pub value_usd: Decimal,
    /// Profit target (absolute USD) for the current phase. Used for the
    /// progress hint line. Pass None when no target applies.
    pub target_usd: Option<Decimal>,
    /// Whether this is live or paper money. Only influences the subtitle
    /// wording.
    pub is_live: bool,
    /// Tick freshness in ms. >STALE_MS triggers the stale visual state.
    pub stale_ms: u64,
    /// App-clock seconds, used to drive the breathing glow.
    pub time_secs: f32,
    /// Short currency label, e.g. "USD".
    pub currency: &'a str,
}

pub fn show(ui: &mut egui::Ui, p: &AcvParams<'_>) {
    let stale = p.stale_ms > STALE_MS;
    let base_border = if stale {
        theme::FORGE_GOLD_DIM
    } else {
        theme::breathing_glow(p.time_secs, theme::FORGE_GOLD, 0.35)
    };

    ornaments::etched_frame(base_border).show(ui, |ui| {
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("APPROXIMATE CASHOUT VALUE")
                        .size(10.5)
                        .color(theme::FORGE_GOLD_DIM)
                        .strong(),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if stale {
                        theme::pill(
                            ui,
                            " STALE ",
                            Color32::from_rgb(36, 16, 8),
                            theme::YELLOW,
                        );
                    }
                });
            });

            ui.add_space(6.0);

            let value_color = if stale {
                theme::MUTED
            } else if p.value_usd >= Decimal::ZERO {
                theme::GREEN
            } else {
                theme::RED
            };

            let big = format!("{}{:.2} {}", sign(p.value_usd), p.value_usd.abs(), p.currency);
            ui.label(
                RichText::new(big)
                    .size(30.0)
                    .color(value_color)
                    .monospace()
                    .strong(),
            );

            ui.add_space(4.0);

            let subtitle = if p.is_live {
                "Realizable profit share at this moment"
            } else {
                "Paper account — no real money at risk"
            };
            ui.label(RichText::new(subtitle).size(11.0).color(theme::FORGE_GOLD_DIM));

            if let Some(target) = p.target_usd {
                if target > Decimal::ZERO {
                    ui.add_space(8.0);
                    progress_line(ui, p.value_usd, target, stale);
                }
            }
        });
    });
}

fn sign(v: Decimal) -> &'static str {
    if v >= Decimal::ZERO {
        "+"
    } else {
        "−"
    }
}

fn progress_line(ui: &mut egui::Ui, value: Decimal, target: Decimal, stale: bool) {
    let progress = (value / target).max(Decimal::ZERO).min(Decimal::ONE);
    let pct_f = progress.to_string().parse::<f32>().unwrap_or(0.0);

    let bar_color = if stale {
        theme::DIM
    } else if pct_f >= 1.0 {
        theme::GOLD
    } else if pct_f >= 0.5 {
        theme::GREEN
    } else {
        theme::BLUE
    };

    ui.horizontal(|ui| {
        ui.label(
            RichText::new(format!("Target: ${:.0}", target))
                .size(10.5)
                .color(theme::MUTED)
                .monospace(),
        );
        let (rect, _) = ui.allocate_exact_size(egui::vec2(160.0, 6.0), egui::Sense::hover());
        let painter = ui.painter();
        painter.rect_filled(rect, 2, theme::INPUT_BG);
        let mut fill = rect;
        fill.set_width(rect.width() * pct_f);
        painter.rect_filled(fill, 2, bar_color);
        painter.rect_stroke(
            rect,
            2,
            Stroke::new(0.5, theme::FORGE_GOLD_DIM),
            egui::StrokeKind::Inside,
        );
        ui.label(
            RichText::new(format!("{:.0}%", pct_f * 100.0))
                .size(10.5)
                .color(theme::MUTED)
                .monospace(),
        );
    });
}
