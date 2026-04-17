//! Dashboard — Live trading overview designed for clarity at a glance

use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use eframe::egui;
use egui::RichText;
use gadarah_core::{Direction, HeadId, Regime9, RegimeSignal9};

use crate::state::{AppState, LogLevel, Position};
use crate::theme;

pub struct DashboardPanel;

impl DashboardPanel {
    pub fn show(ui: &mut egui::Ui, app_state: &AppState) {
        let (
            balance,
            equity,
            daily_pnl,
            daily_pnl_pct,
            total_pnl,
            total_pnl_pct,
            kill_switch_active,
            kill_switch_reason,
            kill_switch_cooldown,
            positions,
            regime_map,
            active_heads,
            selected_firm,
            daily_dd_limit,
            total_dd_limit,
        ) = {
            let g = app_state.lock().unwrap();
            let daily_dd_limit: f32 = g
                .config
                .kill_switch
                .daily_dd_trigger_pct
                .to_string()
                .parse()
                .unwrap_or(5.0);
            let total_dd_limit: f32 = g
                .config
                .kill_switch
                .total_dd_trigger_pct
                .to_string()
                .parse()
                .unwrap_or(10.0);
            (
                g.balance,
                g.equity,
                g.daily_pnl,
                g.daily_pnl_pct,
                g.total_pnl,
                g.total_pnl_pct,
                g.kill_switch_active,
                g.kill_switch_reason.clone(),
                g.kill_switch_cooldown,
                g.positions.clone(),
                g.regime_by_symbol.clone(),
                g.active_heads.clone(),
                g.selected_firm.clone(),
                daily_dd_limit,
                total_dd_limit,
            )
        };

        // ── Kill switch banner (if active) ────────────────────────────────────
        if kill_switch_active {
            theme::danger_card().show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("TRADING IS HALTED")
                            .size(15.0)
                            .color(theme::RED)
                            .strong(),
                    );
                    ui.add_space(8.0);
                    if let Some(reason) = &kill_switch_reason {
                        ui.label(RichText::new(format!("Reason: {}", reason)).color(theme::ORANGE));
                    }
                    if let Some(cooldown) = kill_switch_cooldown {
                        let remaining = (cooldown - chrono::Utc::now().timestamp()).max(0);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(
                                RichText::new(format!("Cooldown: {}s remaining", remaining))
                                    .color(theme::RED)
                                    .monospace(),
                            );
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new("Resume Trading").color(theme::TEXT),
                                    )
                                    .fill(egui::Color32::from_rgb(80, 20, 20)),
                                )
                                .clicked()
                            {
                                let mut g = app_state.lock().unwrap();
                                g.kill_switch_active = false;
                                g.kill_switch_reason = None;
                                g.kill_switch_cooldown = None;
                                g.add_log(LogLevel::Info, "Kill switch cleared — trading resumed");
                            }
                        });
                    }
                });
                ui.label(
                    RichText::new(
                        "The bot will not open or modify any trades until this is cleared.",
                    )
                    .color(theme::MUTED)
                    .size(12.0),
                );
            });
            ui.add_space(12.0);
        }

        // ── Firm header ─────────────────────────────────────────────────────
        if let Some(firm) = &selected_firm {
            ui.horizontal(|ui| {
                ui.label(RichText::new(firm).size(14.0).color(theme::MUTED).strong());
            });
            ui.add_space(4.0);
        }

        // ── Row 1: Account + P&L + Risk cards ────────────────────────────────
        let card_width = (ui.available_width() - 24.0) / 3.0;

        ui.horizontal(|ui| {
            // Account Value card
            theme::card().show(ui, |ui| {
                ui.set_width(card_width);
                theme::section_label(ui, "ACCOUNT VALUE");
                ui.add_space(6.0);
                theme::big_stat(
                    ui,
                    &format!("${:.2}", balance),
                    "Starting balance",
                    theme::TEXT,
                );
                ui.add_space(4.0);
                let eq_color = if equity >= balance {
                    theme::GREEN
                } else {
                    theme::RED
                };
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Equity:").color(theme::MUTED).size(12.0));
                    ui.label(
                        RichText::new(format!("${:.2}", equity))
                            .color(eq_color)
                            .size(13.0)
                            .strong()
                            .monospace(),
                    );
                });
            });

            ui.add_space(12.0);

            // Daily P&L card
            let dp_pos = daily_pnl >= Decimal::ZERO;
            let card = if dp_pos {
                theme::ok_card()
            } else {
                theme::warn_card()
            };
            card.show(ui, |ui| {
                ui.set_width(card_width);
                theme::section_label(ui, "TODAY'S PROFIT / LOSS");
                ui.add_space(6.0);
                theme::big_stat(
                    ui,
                    &format!("${:.2}", daily_pnl),
                    &format!("{:+.2}% from start of day", daily_pnl_pct),
                    theme::pnl_color(dp_pos),
                );
                ui.add_space(4.0);
                let tp_pos = total_pnl >= Decimal::ZERO;
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Total P&L:").color(theme::MUTED).size(12.0));
                    ui.label(
                        RichText::new(format!("${:.2} ({:+.2}%)", total_pnl, total_pnl_pct))
                            .color(theme::pnl_color(tp_pos))
                            .size(12.0)
                            .monospace(),
                    );
                });
            });

            ui.add_space(12.0);

            // Risk gauges card
            theme::card().show(ui, |ui| {
                ui.set_width(card_width);
                theme::section_label(ui, "RISK LIMITS");
                ui.add_space(6.0);

                // Compute used percentages from balance
                let daily_used = if daily_pnl < Decimal::ZERO {
                    (daily_pnl.abs() / balance * dec!(100))
                        .to_string()
                        .parse::<f32>()
                        .unwrap_or(0.0)
                } else {
                    0.0
                };
                let total_used = if total_pnl < Decimal::ZERO {
                    (total_pnl.abs() / balance * dec!(100))
                        .to_string()
                        .parse::<f32>()
                        .unwrap_or(0.0)
                } else {
                    0.0
                };

                theme::dd_bar(ui, "Daily Loss Limit", daily_used, daily_dd_limit);
                ui.add_space(4.0);
                theme::dd_bar(ui, "Total Loss Limit", total_used, total_dd_limit);
                ui.add_space(4.0);

                let daily_remaining = daily_dd_limit - daily_used;
                let color = if daily_remaining > daily_dd_limit * 0.4 {
                    theme::GREEN
                } else if daily_remaining > daily_dd_limit * 0.15 {
                    theme::YELLOW
                } else {
                    theme::RED
                };
                let balance_f32: f32 = balance.to_string().parse().unwrap_or(0.0);
                let buffer_dollars = balance_f32 * daily_remaining / 100.0;
                ui.label(
                    RichText::new(format!(
                        "You can lose ${:.0} more today before the bot stops automatically.",
                        buffer_dollars
                    ))
                    .color(color)
                    .size(11.5),
                );
            });
        });

        ui.add_space(12.0);

        // ── Row 2: Positions + Market Conditions ──────────────────────────────
        ui.horizontal(|ui| {
            let left_width = ui.available_width() * 0.60 - 6.0;
            let right_width = ui.available_width() - left_width - 12.0;

            // Open Positions card
            theme::card().show(ui, |ui| {
                ui.set_width(left_width);
                ui.horizontal(|ui| {
                    theme::section_label(ui, "OPEN TRADES");
                    ui.add_space(6.0);
                    if positions.is_empty() {
                        theme::pill(ui, " No open trades ", egui::Color32::from_rgb(20, 26, 35), theme::MUTED);
                    } else {
                        theme::pill(
                            ui,
                            &format!(" {} open ", positions.len()),
                            egui::Color32::from_rgb(10, 30, 45),
                            theme::BLUE,
                        );
                    }
                });

                ui.add_space(8.0);

                if positions.is_empty() {
                    theme::empty_state(ui, "📡", "No Open Trades", "The bot is monitoring the market — trades will appear here when positions are opened.");
                } else {
                    egui::Grid::new("pos_grid")
                        .num_columns(8)
                        .spacing([10.0, 6.0])
                        .striped(true)
                        .show(ui, |ui| {
                            for h in ["Market", "Direction", "Size", "Entry", "Current", "P&L", "Stop Loss", "Age"] {
                                ui.label(RichText::new(h).color(theme::MUTED).size(11.5));
                            }
                            ui.end_row();
                            for pos in &positions {
                                render_position(ui, pos);
                            }
                        });
                }
            });

            ui.add_space(12.0);

            // Market Conditions card
            theme::card().show(ui, |ui| {
                ui.set_width(right_width);
                theme::section_label(ui, "MARKET CONDITIONS");
                ui.add_space(8.0);

                if regime_map.is_empty() {
                    theme::empty_state(ui, "📊", "No Market Data", "Waiting for price feed to classify market conditions.");
                } else {
                    let mut entries: Vec<_> = regime_map.iter().collect();
                    entries.sort_by_key(|(k, _)| k.as_str());
                    for (symbol, regime) in entries {
                        regime_card(ui, symbol, regime);
                        ui.add_space(8.0);
                    }
                }

                ui.separator();
                ui.add_space(4.0);

                // Active heads
                theme::section_label(ui, "ACTIVE STRATEGIES");
                ui.add_space(4.0);
                if active_heads.is_empty() {
                    ui.label(RichText::new("Waiting for suitable market conditions to activate strategies.").color(theme::MUTED).size(12.0));
                } else {
                    ui.horizontal_wrapped(|ui| {
                        for head in &active_heads {
                            theme::pill(
                                ui,
                                &format!(" {} ", head_display(*head)),
                                egui::Color32::from_rgb(15, 30, 45),
                                theme::BLUE,
                            );
                        }
                    });
                }
            });
        });

        ui.add_space(12.0);

        // ── Kill switch row ───────────────────────────────────────────────────
        if !kill_switch_active {
            theme::card_sm().show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new("Emergency Stop").size(13.5).color(theme::TEXT).strong());
                        ui.label(RichText::new("Immediately stops all trading and prevents new trades from opening.").color(theme::MUTED).size(12.0));
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add_sized(
                            [200.0, 42.0],
                            egui::Button::new(
                                RichText::new("STOP ALL TRADING").size(13.5).color(egui::Color32::WHITE).strong(),
                            )
                            .fill(theme::RED),
                        ).clicked() {
                            let mut g = app_state.lock().unwrap();
                            g.kill_switch_active = true;
                            g.kill_switch_reason = Some("Manual stop by user".to_string());
                            g.kill_switch_cooldown = Some(chrono::Utc::now().timestamp() + 1800);
                            g.add_log(LogLevel::Warn, "TRADING STOPPED — Kill switch activated manually");
                        }
                    });
                });
            });
        }
    }
}

fn render_position(ui: &mut egui::Ui, pos: &Position) {
    ui.label(RichText::new(&pos.symbol).strong().size(13.0));

    let (dir_color, dir_label) = match pos.direction {
        Direction::Buy => (theme::GREEN, "BUY"),
        Direction::Sell => (theme::RED, "SELL"),
    };
    theme::pill(ui, dir_label, egui::Color32::TRANSPARENT, dir_color);

    ui.label(
        RichText::new(format!("{:.2} lots", pos.lots))
            .monospace()
            .color(theme::TEXT)
            .size(12.5),
    );
    ui.label(
        RichText::new(format!("{:.5}", pos.entry_price))
            .monospace()
            .color(theme::MUTED)
            .size(12.0),
    );
    ui.label(
        RichText::new(format!("{:.5}", pos.current_price))
            .monospace()
            .color(theme::TEXT)
            .size(12.0),
    );

    let pnl_pos = pos.unrealized_pnl >= Decimal::ZERO;
    ui.label(
        RichText::new(format!("${:.2}", pos.unrealized_pnl))
            .monospace()
            .color(theme::pnl_color(pnl_pos))
            .size(12.5)
            .strong(),
    );

    match pos.stop_loss {
        Some(sl) => ui.label(
            RichText::new(format!("{:.5}", sl))
                .monospace()
                .color(theme::RED)
                .size(12.0),
        ),
        None => ui.label(RichText::new("None").color(theme::DIM).size(12.0)),
    };

    let age = chrono::Utc::now().timestamp() - pos.opened_at;
    let age_str = if age < 3600 {
        format!("{}m", age / 60)
    } else {
        format!("{}h {}m", age / 3600, (age % 3600) / 60)
    };
    ui.label(RichText::new(age_str).color(theme::MUTED).size(12.0));
    ui.end_row();
}

fn regime_card(ui: &mut egui::Ui, symbol: &str, regime: &RegimeSignal9) {
    let (plain_name, color) = match regime.regime {
        Regime9::StrongTrendUp => ("Strong Uptrend", theme::GREEN),
        Regime9::StrongTrendDown => ("Strong Downtrend", theme::RED),
        Regime9::WeakTrendUp => ("Mild Uptrend", egui::Color32::from_rgb(100, 210, 120)),
        Regime9::WeakTrendDown => ("Mild Downtrend", egui::Color32::from_rgb(230, 110, 100)),
        Regime9::RangingTight => ("Tight Range", theme::BLUE),
        Regime9::RangingWide => ("Wide Range", egui::Color32::from_rgb(120, 170, 255)),
        Regime9::Choppy => ("Choppy / Unclear", theme::YELLOW),
        Regime9::BreakoutPending => ("Breakout Coming", theme::ORANGE),
        Regime9::Transitioning => ("Transitioning", theme::MUTED),
    };

    let conf_pct = (regime.confidence * dec!(100))
        .to_string()
        .parse::<f32>()
        .unwrap_or(0.0);

    egui::Frame::new()
        .fill(egui::Color32::from_rgb(14, 20, 28))
        .stroke(egui::Stroke::new(1.0, color.linear_multiply(0.25)))
        .corner_radius(8u8)
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(symbol).strong().size(13.5).color(theme::TEXT));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new(format!("{:.0}% conf.", conf_pct))
                            .color(theme::MUTED)
                            .size(11.0),
                    );
                });
            });
            ui.label(RichText::new(plain_name).color(color).size(13.0).strong());
        });
}

fn head_display(head: HeadId) -> &'static str {
    match head {
        HeadId::Momentum => "Momentum",
        HeadId::AsianRange => "Asian Range",
        HeadId::Breakout => "Breakout",
        HeadId::Trend => "Trend Follow",
        HeadId::Grid => "Grid",
        HeadId::Smc => "Smart Money",
        HeadId::News => "News Spike",
        HeadId::ScalpM1 => "Scalp M1",
        HeadId::ScalpM5 => "Scalp M5",
        HeadId::VolProfile => "Volume Profile",
    }
}
