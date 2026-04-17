//! Performance panel — Equity curve, trade statistics, trade history

use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use eframe::egui;
use egui::RichText;
use egui_plot::{Bar, BarChart, Line, Plot, PlotPoints};

use crate::state::{AppState, TradeRecord};
use crate::theme;

pub struct PerformancePanel {
    pub filter_head: Option<String>,
    pub filter_symbol: Option<String>,
}

impl Default for PerformancePanel {
    fn default() -> Self {
        Self {
            filter_head: None,
            filter_symbol: None,
        }
    }
}

impl PerformancePanel {
    pub fn show(&mut self, ui: &mut egui::Ui, state: &AppState) {
        let g = state.lock().unwrap();

        theme::heading(ui, "Performance");
        ui.label(
            RichText::new("How the bot has performed over its lifetime.")
                .color(theme::MUTED)
                .size(12.5),
        );
        ui.add_space(12.0);

        // ── Stats cards ───────────────────────────────────────────────────────
        let has_trades = g.total_trades > 0;
        let card_w = (ui.available_width() - 50.0) / 6.0;
        ui.horizontal_wrapped(|ui| {
            let stats: Vec<(&str, String, egui::Color32)> = if has_trades {
                vec![
                    ("Trades", format!("{}", g.total_trades), theme::TEXT),
                    (
                        "Win Rate",
                        format!("{:.1}%", g.win_rate),
                        if g.win_rate >= dec!(50) {
                            theme::GREEN
                        } else {
                            theme::RED
                        },
                    ),
                    (
                        "Profit Factor",
                        format!("{:.2}", g.profit_factor),
                        if g.profit_factor >= dec!(1.5) {
                            theme::GREEN
                        } else if g.profit_factor >= Decimal::ONE {
                            theme::YELLOW
                        } else {
                            theme::RED
                        },
                    ),
                    (
                        "Max DD",
                        format!("{:.2}%", g.max_drawdown_pct),
                        if g.max_drawdown_pct <= dec!(5) {
                            theme::GREEN
                        } else if g.max_drawdown_pct <= dec!(10) {
                            theme::YELLOW
                        } else {
                            theme::RED
                        },
                    ),
                    (
                        "Sharpe",
                        format!("{:.2}", g.sharpe_ratio),
                        if g.sharpe_ratio >= dec!(1) {
                            theme::GREEN
                        } else if g.sharpe_ratio >= dec!(0.5) {
                            theme::YELLOW
                        } else {
                            theme::RED
                        },
                    ),
                    (
                        "Expect.",
                        format!("{:.2}R", g.expectancy_r),
                        if g.expectancy_r >= Decimal::ZERO {
                            theme::GREEN
                        } else {
                            theme::RED
                        },
                    ),
                ]
            } else {
                vec![
                    ("Trades", "0".to_string(), theme::MUTED),
                    ("Win Rate", "--".to_string(), theme::MUTED),
                    ("PF", "--".to_string(), theme::MUTED),
                    ("Max DD", "--".to_string(), theme::MUTED),
                    ("Sharpe", "--".to_string(), theme::MUTED),
                    ("Expect.", "--".to_string(), theme::MUTED),
                ]
            };

            for (label, value, color) in &stats {
                theme::stat_card(ui, label, value, *color, card_w);
                ui.add_space(6.0);
            }
        });

        ui.add_space(12.0);

        // ── Equity curve ──────────────────────────────────────────────────────
        theme::card().show(ui, |ui| {
            theme::section_label(ui, "EQUITY CURVE — Account value over time");
            ui.add_space(8.0);

            if g.equity_curve.is_empty() {
                theme::empty_state(
                    ui,
                    "📈",
                    "No Equity Data",
                    "Equity curve will build as trades are executed.",
                );
            } else {
                let points: PlotPoints = g
                    .equity_curve
                    .iter()
                    .enumerate()
                    .map(|(i, p)| [i as f64, p.equity.to_string().parse::<f64>().unwrap_or(0.0)])
                    .collect();

                Plot::new("equity_curve")
                    .height(200.0)
                    .x_axis_label("Trade #")
                    .y_axis_label("Account Value ($)")
                    .show_axes([false, true])
                    .show(ui, |plot_ui| {
                        plot_ui.line(Line::new(points).color(theme::ACCENT).width(2.0));
                    });
            }
        });

        ui.add_space(12.0);

        // ── Daily P&L history + projection ───────────────────────────────────
        {
            // Build daily PnL from trade history grouped by calendar day
            let mut daily_pnl: std::collections::BTreeMap<i64, f64> =
                std::collections::BTreeMap::new();
            for t in &g.trade_history {
                let day = t.timestamp - (t.timestamp % 86400); // floor to UTC midnight
                let pnl_f: f64 = t.pnl.to_string().parse().unwrap_or(0.0);
                *daily_pnl.entry(day).or_default() += pnl_f;
            }

            // Also derive from equity curve if no trades yet
            if daily_pnl.is_empty() && g.equity_curve.len() >= 2 {
                for pair in g.equity_curve.windows(2) {
                    let day = pair[1].timestamp - (pair[1].timestamp % 86400);
                    let prev: f64 = pair[0].equity.to_string().parse().unwrap_or(0.0);
                    let curr: f64 = pair[1].equity.to_string().parse().unwrap_or(0.0);
                    *daily_pnl.entry(day).or_default() += curr - prev;
                }
            }

            theme::card().show(ui, |ui| {
                theme::section_label(
                    ui,
                    "DAILY P&L — Profit/loss per trading day + 30-day projection",
                );
                ui.add_space(8.0);

                if daily_pnl.is_empty() {
                    theme::empty_state(
                        ui,
                        "📅",
                        "No Daily Data",
                        "Trade history will populate this chart as the bot trades.",
                    );
                } else {
                    let days: Vec<(i64, f64)> = daily_pnl.into_iter().collect();
                    let n = days.len();

                    // Build bar chart for historical daily PnL
                    let pnl_bars: Vec<Bar> = days
                        .iter()
                        .enumerate()
                        .map(|(i, (_day, pnl))| {
                            let color = if *pnl >= 0.0 {
                                theme::GREEN
                            } else {
                                theme::RED
                            };
                            Bar::new(i as f64, *pnl).fill(color).width(0.7)
                        })
                        .collect();

                    // Calculate average daily PnL for projection
                    let total_pnl: f64 = days.iter().map(|(_, p)| p).sum();
                    let avg_daily = total_pnl / n as f64;

                    // Build 30-day projection line (dotted continuation)
                    let mut projection_points: Vec<[f64; 2]> = Vec::new();
                    let mut cumulative = total_pnl;
                    // Start projection from end of actuals
                    projection_points.push([n as f64 - 1.0, cumulative]);
                    for d in 0..30 {
                        cumulative += avg_daily;
                        projection_points.push([(n + d) as f64, cumulative]);
                    }

                    // Build cumulative actual line
                    let mut running = 0.0;
                    let cum_points: Vec<[f64; 2]> = days
                        .iter()
                        .enumerate()
                        .map(|(i, (_, pnl))| {
                            running += pnl;
                            [i as f64, running]
                        })
                        .collect();

                    // Summary stats
                    let winning_days = days.iter().filter(|(_, p)| *p > 0.0).count();
                    let losing_days = days.iter().filter(|(_, p)| *p < 0.0).count();
                    let best_day = days
                        .iter()
                        .map(|(_, p)| *p)
                        .fold(f64::NEG_INFINITY, f64::max);
                    let worst_day = days.iter().map(|(_, p)| *p).fold(f64::INFINITY, f64::min);

                    ui.horizontal(|ui| {
                        for (label, value, color) in [
                            (
                                "Avg Daily",
                                format!("${:.2}", avg_daily),
                                if avg_daily >= 0.0 {
                                    theme::GREEN
                                } else {
                                    theme::RED
                                },
                            ),
                            ("Best Day", format!("${:.2}", best_day), theme::GREEN),
                            ("Worst Day", format!("${:.2}", worst_day), theme::RED),
                            (
                                "Win Days",
                                format!("{}/{}", winning_days, n),
                                if winning_days > losing_days {
                                    theme::GREEN
                                } else {
                                    theme::RED
                                },
                            ),
                            (
                                "30d Projection",
                                format!("${:.0}", avg_daily * 30.0),
                                if avg_daily >= 0.0 {
                                    theme::ACCENT
                                } else {
                                    theme::RED
                                },
                            ),
                        ] {
                            ui.vertical(|ui| {
                                ui.label(
                                    RichText::new(label).size(10.5).color(theme::MUTED).strong(),
                                );
                                ui.label(
                                    RichText::new(value)
                                        .size(13.0)
                                        .color(color)
                                        .monospace()
                                        .strong(),
                                );
                            });
                            ui.add_space(16.0);
                        }
                    });
                    ui.add_space(8.0);

                    // Daily PnL bar chart
                    Plot::new("daily_pnl_bars")
                        .height(180.0)
                        .show_axes([true, true])
                        .y_axis_label("Daily P&L ($)")
                        .label_formatter(move |_name, value| {
                            format!("Day {:.0}: ${:.2}", value.x + 1.0, value.y)
                        })
                        .show(ui, |plot_ui| {
                            plot_ui.bar_chart(BarChart::new(pnl_bars).name("Daily P&L"));
                            // Zero line
                            plot_ui.hline(egui_plot::HLine::new(0.0).color(theme::DIM).width(1.0));
                        });

                    ui.add_space(10.0);
                    theme::section_label(ui, "CUMULATIVE P&L + 30-DAY PROJECTION");
                    ui.add_space(4.0);

                    // Cumulative line + projection
                    Plot::new("daily_pnl_cumulative")
                        .height(160.0)
                        .show_axes([true, true])
                        .y_axis_label("Cumulative ($)")
                        .label_formatter(move |_name, value| {
                            format!("Day {:.0}: ${:.2}", value.x + 1.0, value.y)
                        })
                        .show(ui, |plot_ui| {
                            plot_ui.line(
                                Line::new(PlotPoints::new(cum_points))
                                    .name("Actual")
                                    .color(theme::ACCENT)
                                    .width(2.0),
                            );
                            plot_ui.line(
                                Line::new(PlotPoints::new(projection_points))
                                    .name("30-day Projection")
                                    .color(theme::YELLOW)
                                    .width(1.5)
                                    .style(egui_plot::LineStyle::dashed_dense()),
                            );
                            plot_ui.hline(egui_plot::HLine::new(0.0).color(theme::DIM).width(1.0));
                        });
                }
            });
        }

        ui.add_space(12.0);

        // ── Trade log ─────────────────────────────────────────────────────────
        theme::card().show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                theme::section_label(ui, "TRADE HISTORY");
                ui.add_space(12.0);

                ui.label(RichText::new("Strategy:").color(theme::MUTED).size(12.0));
                egui::ComboBox::from_id_salt("head_filter")
                    .width(130.0)
                    .selected_text(self.filter_head.as_deref().unwrap_or("All"))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.filter_head, None, "All");
                        for h in [
                            "Momentum",
                            "AsianRange",
                            "Breakout",
                            "Trend",
                            "ScalpM1",
                            "ScalpM5",
                        ] {
                            ui.selectable_value(&mut self.filter_head, Some(h.to_string()), h);
                        }
                    });

                ui.label(RichText::new("Market:").color(theme::MUTED).size(12.0));
                egui::ComboBox::from_id_salt("symbol_filter")
                    .width(110.0)
                    .selected_text(self.filter_symbol.as_deref().unwrap_or("All"))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.filter_symbol, None, "All");
                        for sym in ["EURUSD", "GBPUSD", "USDJPY", "AUDUSD"] {
                            ui.selectable_value(
                                &mut self.filter_symbol,
                                Some(sym.to_string()),
                                sym,
                            );
                        }
                    });
            });

            ui.add_space(8.0);

            let filtered: Vec<&TradeRecord> = g
                .trade_history
                .iter()
                .filter(|t| {
                    let head_ok = self.filter_head.as_ref().map_or(true, |h| {
                        let s = match t.head {
                            gadarah_core::HeadId::Momentum => "Momentum",
                            gadarah_core::HeadId::AsianRange => "AsianRange",
                            gadarah_core::HeadId::Breakout => "Breakout",
                            gadarah_core::HeadId::Trend => "Trend",
                            gadarah_core::HeadId::Grid => "Grid",
                            gadarah_core::HeadId::Smc => "Smc",
                            gadarah_core::HeadId::News => "News",
                            gadarah_core::HeadId::ScalpM1 => "ScalpM1",
                            gadarah_core::HeadId::ScalpM5 => "ScalpM5",
                            gadarah_core::HeadId::VolProfile => "VolProfile",
                        };
                        s == h
                    });
                    let sym_ok = self.filter_symbol.as_ref().map_or(true, |s| &t.symbol == s);
                    head_ok && sym_ok
                })
                .collect();

            if filtered.is_empty() {
                theme::empty_state(
                    ui,
                    "🔍",
                    "No Matching Trades",
                    "Try adjusting the strategy or market filter above.",
                );
                return;
            }

            ui.label(
                RichText::new(format!("Showing {} trades", filtered.len()))
                    .color(theme::DIM)
                    .size(11.5),
            );
            ui.add_space(4.0);

            egui::ScrollArea::horizontal().show(ui, |ui| {
                egui::Grid::new("trade_log")
                    .num_columns(9)
                    .spacing([10.0, 6.0])
                    .striped(true)
                    .show(ui, |ui| {
                        for h in [
                            "Time", "Market", "Strategy", "Dir", "Entry", "Exit", "P&L", "R",
                            "Result",
                        ] {
                            ui.label(RichText::new(h).color(theme::MUTED).size(12.0));
                        }
                        ui.end_row();

                        for trade in filtered.iter().rev().take(100) {
                            let time = chrono::DateTime::from_timestamp(trade.timestamp, 0)
                                .map(|dt| dt.format("%m-%d %H:%M").to_string())
                                .unwrap_or_default();
                            ui.label(RichText::new(time).monospace().color(theme::DIM).size(12.0));
                            ui.label(RichText::new(&trade.symbol).strong().size(12.5));

                            let head_s = match trade.head {
                                gadarah_core::HeadId::Momentum => "Momentum",
                                gadarah_core::HeadId::AsianRange => "AsianRange",
                                gadarah_core::HeadId::Breakout => "Breakout",
                                gadarah_core::HeadId::Trend => "Trend",
                                gadarah_core::HeadId::Grid => "Grid",
                                gadarah_core::HeadId::Smc => "SmartMoney",
                                gadarah_core::HeadId::News => "News",
                                gadarah_core::HeadId::ScalpM1 => "ScalpM1",
                                gadarah_core::HeadId::ScalpM5 => "ScalpM5",
                                gadarah_core::HeadId::VolProfile => "VolProfile",
                            };
                            ui.label(RichText::new(head_s).color(theme::MUTED).size(12.0));

                            let (dc, ds) = match trade.direction {
                                gadarah_core::Direction::Buy => (theme::GREEN, "BUY"),
                                gadarah_core::Direction::Sell => (theme::RED, "SELL"),
                            };
                            ui.label(RichText::new(ds).monospace().color(dc).size(12.0).strong());

                            ui.label(
                                RichText::new(format!("{:.5}", trade.entry_price))
                                    .monospace()
                                    .color(theme::MUTED)
                                    .size(12.0),
                            );
                            ui.label(
                                RichText::new(format!("{:.5}", trade.exit_price))
                                    .monospace()
                                    .color(theme::TEXT)
                                    .size(12.0),
                            );

                            let pnl_c = theme::pnl_color(trade.pnl >= Decimal::ZERO);
                            ui.label(
                                RichText::new(format!("${:.2}", trade.pnl))
                                    .monospace()
                                    .color(pnl_c)
                                    .size(12.5)
                                    .strong(),
                            );

                            let r_c = theme::pnl_color(trade.r_multiple >= Decimal::ZERO);
                            ui.label(
                                RichText::new(format!("{:.2}R", trade.r_multiple))
                                    .monospace()
                                    .color(r_c)
                                    .size(12.0),
                            );

                            ui.label(
                                RichText::new(&trade.close_reason)
                                    .color(theme::MUTED)
                                    .size(12.0),
                            );
                            ui.end_row();
                        }
                    });
            });
        });
    }
}
