//! Performance panel — Equity curve, trade statistics, trade history

use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use eframe::egui;
use egui::RichText;
use egui_plot::{Line, Plot, PlotPoints};

use crate::state::{AppState, TradeRecord};
use crate::theme;

pub struct PerformancePanel {
    pub filter_head:   Option<String>,
    pub filter_symbol: Option<String>,
}

impl Default for PerformancePanel {
    fn default() -> Self {
        Self { filter_head: None, filter_symbol: None }
    }
}

impl PerformancePanel {
    pub fn show(&mut self, ui: &mut egui::Ui, state: &AppState) {
        let g = state.lock().unwrap();

        theme::heading(ui, "Performance");
        ui.label(RichText::new("How the bot has performed over its lifetime.").color(theme::MUTED).size(12.5));
        ui.add_space(12.0);

        // ── Stats cards ───────────────────────────────────────────────────────
        let card_w = (ui.available_width() - 60.0) / 6.0;
        ui.horizontal(|ui| {
            let stats = [
                ("Trades",        format!("{}", g.total_trades),            theme::TEXT),
                ("Win Rate",      format!("{:.1}%", g.win_rate),
                    if g.win_rate >= dec!(50) { theme::GREEN } else { theme::RED }),
                ("Profit Factor", format!("{:.2}", g.profit_factor),
                    if g.profit_factor >= dec!(1.5) { theme::GREEN }
                    else if g.profit_factor >= Decimal::ONE { theme::YELLOW }
                    else { theme::RED }),
                ("Max Drawdown",  format!("{:.2}%", g.max_drawdown_pct),
                    if g.max_drawdown_pct <= dec!(5) { theme::GREEN }
                    else if g.max_drawdown_pct <= dec!(10) { theme::YELLOW }
                    else { theme::RED }),
                ("Sharpe Ratio",  format!("{:.2}", g.sharpe_ratio),
                    if g.sharpe_ratio >= dec!(1) { theme::GREEN }
                    else if g.sharpe_ratio >= dec!(0.5) { theme::YELLOW }
                    else { theme::RED }),
                ("Expectancy",    format!("{:.2}R", g.expectancy_r),
                    if g.expectancy_r >= Decimal::ZERO { theme::GREEN } else { theme::RED }),
            ];

            for (label, value, color) in &stats {
                theme::card_sm().show(ui, |ui| {
                    ui.set_width(card_w);
                    theme::section_label(ui, label);
                    ui.add_space(4.0);
                    ui.label(RichText::new(value).size(20.0).color(*color).strong().monospace());
                });
                ui.add_space(10.0);
            }
        });

        ui.add_space(12.0);

        // ── Equity curve ──────────────────────────────────────────────────────
        theme::card().show(ui, |ui| {
            theme::section_label(ui, "EQUITY CURVE — Account value over time");
            ui.add_space(8.0);

            if g.equity_curve.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.add_space(20.0);
                    ui.label(RichText::new("No equity data available yet.").color(theme::MUTED));
                    ui.add_space(20.0);
                });
            } else {
                let points: PlotPoints = g.equity_curve
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
                        plot_ui.line(
                            Line::new(points)
                                .color(theme::ACCENT)
                                .width(2.0),
                        );
                    });
            }
        });

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
                        for h in ["Momentum", "AsianRange", "Breakout", "Trend", "ScalpM1", "ScalpM5"] {
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
                            ui.selectable_value(&mut self.filter_symbol, Some(sym.to_string()), sym);
                        }
                    });
            });

            ui.add_space(8.0);

            let filtered: Vec<&TradeRecord> = g.trade_history.iter()
                .filter(|t| {
                    let head_ok = self.filter_head.as_ref().map_or(true, |h| {
                        let s = match t.head {
                            gadarah_core::HeadId::Momentum   => "Momentum",
                            gadarah_core::HeadId::AsianRange => "AsianRange",
                            gadarah_core::HeadId::Breakout   => "Breakout",
                            gadarah_core::HeadId::Trend      => "Trend",
                            gadarah_core::HeadId::Grid       => "Grid",
                            gadarah_core::HeadId::Smc        => "Smc",
                            gadarah_core::HeadId::News       => "News",
                            gadarah_core::HeadId::ScalpM1    => "ScalpM1",
                            gadarah_core::HeadId::ScalpM5    => "ScalpM5",
                            gadarah_core::HeadId::VolProfile => "VolProfile",
                        };
                        s == h
                    });
                    let sym_ok = self.filter_symbol.as_ref().map_or(true, |s| &t.symbol == s);
                    head_ok && sym_ok
                })
                .collect();

            if filtered.is_empty() {
                ui.label(RichText::new("No trades match the current filter.").color(theme::MUTED).size(12.5));
                return;
            }

            ui.label(RichText::new(format!("Showing {} trades", filtered.len())).color(theme::DIM).size(11.5));
            ui.add_space(4.0);

            egui::ScrollArea::horizontal().show(ui, |ui| {
                egui::Grid::new("trade_log")
                    .num_columns(9)
                    .spacing([10.0, 6.0])
                    .striped(true)
                    .show(ui, |ui| {
                        for h in ["Time", "Market", "Strategy", "Dir", "Entry", "Exit", "P&L", "R", "Result"] {
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
                                gadarah_core::HeadId::Momentum   => "Momentum",
                                gadarah_core::HeadId::AsianRange => "AsianRange",
                                gadarah_core::HeadId::Breakout   => "Breakout",
                                gadarah_core::HeadId::Trend      => "Trend",
                                gadarah_core::HeadId::Grid       => "Grid",
                                gadarah_core::HeadId::Smc        => "SmartMoney",
                                gadarah_core::HeadId::News       => "News",
                                gadarah_core::HeadId::ScalpM1    => "ScalpM1",
                                gadarah_core::HeadId::ScalpM5    => "ScalpM5",
                                gadarah_core::HeadId::VolProfile => "VolProfile",
                            };
                            ui.label(RichText::new(head_s).color(theme::MUTED).size(12.0));

                            let (dc, ds) = match trade.direction {
                                gadarah_core::Direction::Buy  => (theme::GREEN, "BUY"),
                                gadarah_core::Direction::Sell => (theme::RED,   "SELL"),
                            };
                            ui.label(RichText::new(ds).monospace().color(dc).size(12.0).strong());

                            ui.label(RichText::new(format!("{:.5}", trade.entry_price)).monospace().color(theme::MUTED).size(12.0));
                            ui.label(RichText::new(format!("{:.5}", trade.exit_price)).monospace().color(theme::TEXT).size(12.0));

                            let pnl_c = theme::pnl_color(trade.pnl >= Decimal::ZERO);
                            ui.label(RichText::new(format!("${:.2}", trade.pnl)).monospace().color(pnl_c).size(12.5).strong());

                            let r_c = theme::pnl_color(trade.r_multiple >= Decimal::ZERO);
                            ui.label(RichText::new(format!("{:.2}R", trade.r_multiple)).monospace().color(r_c).size(12.0));

                            ui.label(RichText::new(&trade.close_reason).color(theme::MUTED).size(12.0));
                            ui.end_row();
                        }
                    });
            });
        });
    }
}
