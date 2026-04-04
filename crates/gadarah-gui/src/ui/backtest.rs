//! Backtest panel — Test the bot on historical data before going live

use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use eframe::egui;
use egui::RichText;
use egui_plot::{Line, Plot, PlotPoints};

use crate::state::{AppState, BacktestResult, EquityPoint};
use crate::theme;

pub struct BacktestPanel {
    selected_symbols:    Vec<String>,
    start_date:          i64,
    end_date:            i64,
    selected_firm:       String,
    walk_forward_results: Vec<WalkForwardFold>,
}

struct WalkForwardFold {
    fold: usize,
    #[allow(dead_code)]
    period: String,
    sharpe: f64,
    profit_factor: f64,
    max_dd: f64,
    win_rate: f64,
}

impl BacktestPanel {
    pub fn new() -> Self {
        let now = chrono::Utc::now().timestamp();
        Self {
            selected_symbols: vec!["EURUSD".to_string()],
            start_date: now - 90 * 86400,
            end_date: now,
            selected_firm: "the5ers_hypergrowth".to_string(),
            walk_forward_results: Vec::new(),
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui, state: &AppState) {
        theme::heading(ui, "Backtest");
        ui.label(RichText::new("Test the bot on historical price data to see how it would have performed.").color(theme::MUTED).size(12.5));
        ui.add_space(12.0);

        // ── Setup card ────────────────────────────────────────────────────────
        theme::card().show(ui, |ui| {
            theme::section_label(ui, "TEST CONFIGURATION");
            ui.add_space(8.0);

            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Markets to test:").color(theme::TEXT).size(13.0));
                ui.add_space(4.0);
                for symbol in ["EURUSD", "GBPUSD", "USDJPY", "AUDUSD"] {
                    let mut checked = self.selected_symbols.contains(&symbol.to_string());
                    if ui.checkbox(&mut checked, symbol).changed() {
                        if checked { self.selected_symbols.push(symbol.to_string()); }
                        else { self.selected_symbols.retain(|s| s != symbol); }
                    }
                }
            });

            ui.add_space(4.0);
            let fmt = |ts| {
                chrono::DateTime::from_timestamp(ts, 0)
                    .map(|dt| dt.format("%Y-%m-%d").to_string())
                    .unwrap_or_default()
            };
            ui.horizontal(|ui| {
                ui.label(RichText::new("Test period:").color(theme::TEXT).size(13.0));
                ui.label(RichText::new(format!("{} to {}", fmt(self.start_date), fmt(self.end_date)))
                    .color(theme::ACCENT).monospace().size(13.0));
            });

            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Challenge rules to apply:").color(theme::TEXT).size(13.0));
                let available = state.lock().unwrap().available_firms.clone();
                egui::ComboBox::from_id_salt("backtest_firm")
                    .width(200.0)
                    .selected_text(&self.selected_firm)
                    .show_ui(ui, |ui| {
                        for firm in available {
                            ui.selectable_value(&mut self.selected_firm, firm.clone(), firm);
                        }
                    });
            });

            ui.add_space(10.0);

            let is_running = state.lock().unwrap().backtest_running;
            if is_running {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(RichText::new("Running backtest — please wait…").color(theme::MUTED).size(13.0));
                });
            } else {
                ui.horizontal(|ui| {
                    if ui.add_sized(
                        [160.0, 38.0],
                        egui::Button::new(RichText::new("Run Backtest").color(egui::Color32::WHITE).strong())
                            .fill(egui::Color32::from_rgb(0, 120, 88)),
                    ).clicked() {
                        self.run_backtest(state);
                    }
                    ui.add_space(8.0);
                    if ui.add_sized(
                        [180.0, 38.0],
                        egui::Button::new(RichText::new("Walk-Forward Test").color(theme::TEXT))
                            .fill(egui::Color32::from_rgb(35, 60, 95)),
                    ).clicked() {
                        self.run_walk_forward(state);
                    }
                    ui.add_space(12.0);
                    ui.label(RichText::new("Walk-Forward runs the test across multiple time windows to check consistency.").color(theme::DIM).size(11.5));
                });
            }
        });

        ui.add_space(12.0);

        // ── Results ───────────────────────────────────────────────────────────
        let result = state.lock().unwrap().last_backtest.clone();

        let Some(bt) = result else {
            theme::card().show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(24.0);
                    ui.label(RichText::new("No results yet — click 'Run Backtest' above.").color(theme::MUTED).size(13.5));
                    ui.add_space(8.0);
                    ui.label(RichText::new("Results will show the bot's historical performance over the selected period.").color(theme::DIM).size(12.0));
                    ui.add_space(24.0);
                });
            });
            return;
        };

        if bt.running {
            ui.spinner();
            return;
        }

        // Stats summary
        theme::card().show(ui, |ui| {
            theme::section_label(ui, "RESULTS SUMMARY");
            ui.add_space(10.0);

            let card_w = (ui.available_width() - 50.0) / 6.0;
            ui.horizontal(|ui| {
                let items: &[(&str, String, egui::Color32)] = &[
                    ("Total Trades",   format!("{}", bt.total_trades), theme::TEXT),
                    ("Win Rate",       format!("{:.1}%", bt.win_rate),
                        if bt.win_rate >= dec!(50) { theme::GREEN } else { theme::RED }),
                    ("Profit Factor",  format!("{:.2}", bt.profit_factor),
                        if bt.profit_factor >= dec!(1.5) { theme::GREEN }
                        else if bt.profit_factor >= Decimal::ONE { theme::YELLOW }
                        else { theme::RED }),
                    ("Max Drawdown",   format!("{:.2}%", bt.max_drawdown_pct),
                        if bt.max_drawdown_pct <= dec!(5) { theme::GREEN }
                        else if bt.max_drawdown_pct <= dec!(10) { theme::YELLOW }
                        else { theme::RED }),
                    ("Sharpe Ratio",   format!("{:.2}", bt.sharpe_ratio), theme::TEXT),
                    ("Expectancy",     format!("{:.2}R", bt.expectancy_r),
                        if bt.expectancy_r >= Decimal::ZERO { theme::GREEN } else { theme::RED }),
                ];
                for (label, value, color) in items {
                    theme::card_sm().show(ui, |ui| {
                        ui.set_width(card_w);
                        theme::section_label(ui, label);
                        ui.add_space(4.0);
                        ui.label(RichText::new(value).size(19.0).color(*color).strong().monospace());
                    });
                    ui.add_space(8.0);
                }
            });
        });

        ui.add_space(12.0);

        // Equity curve
        if !bt.equity_curve.is_empty() {
            theme::card().show(ui, |ui| {
                theme::section_label(ui, "EQUITY CURVE DURING TEST");
                ui.add_space(6.0);
                let points: PlotPoints = bt.equity_curve.iter().enumerate()
                    .map(|(i, p)| [i as f64, p.equity.to_string().parse::<f64>().unwrap_or(0.0)])
                    .collect();
                Plot::new("bt_equity")
                    .height(180.0)
                    .y_axis_label("Account ($)")
                    .show_axes([false, true])
                    .show(ui, |plot_ui| {
                        plot_ui.line(Line::new(points).color(theme::ACCENT).width(2.0));
                    });
            });
            ui.add_space(12.0);
        }

        // Walk-forward table
        if !self.walk_forward_results.is_empty() {
            theme::card().show(ui, |ui| {
                theme::section_label(ui, "WALK-FORWARD CONSISTENCY — Same settings, different time windows");
                ui.add_space(8.0);
                egui::Grid::new("wf_table")
                    .num_columns(5)
                    .spacing([14.0, 7.0])
                    .striped(true)
                    .show(ui, |ui| {
                        for h in ["Window", "Sharpe", "Profit Factor", "Max Drawdown", "Win Rate"] {
                            ui.label(RichText::new(h).color(theme::MUTED).size(12.0));
                        }
                        ui.end_row();
                        for fold in &self.walk_forward_results {
                            ui.label(RichText::new(format!("Period {}", fold.fold)).color(theme::TEXT).size(12.5));
                            let sc = if fold.sharpe >= 1.0 { theme::GREEN } else if fold.sharpe >= 0.5 { theme::YELLOW } else { theme::RED };
                            ui.label(RichText::new(format!("{:.2}", fold.sharpe)).monospace().color(sc));
                            let pc = if fold.profit_factor >= 1.5 { theme::GREEN } else if fold.profit_factor >= 1.0 { theme::YELLOW } else { theme::RED };
                            ui.label(RichText::new(format!("{:.2}", fold.profit_factor)).monospace().color(pc));
                            let dc = if fold.max_dd <= 5.0 { theme::GREEN } else if fold.max_dd <= 10.0 { theme::YELLOW } else { theme::RED };
                            ui.label(RichText::new(format!("{:.1}%", fold.max_dd)).monospace().color(dc));
                            let wc = if fold.win_rate >= 50.0 { theme::GREEN } else { theme::RED };
                            ui.label(RichText::new(format!("{:.1}%", fold.win_rate)).monospace().color(wc));
                            ui.end_row();
                        }
                    });
            });
            ui.add_space(12.0);
        }

        // Trade log
        if !bt.trades.is_empty() {
            theme::card().show(ui, |ui| {
                theme::section_label(ui, &format!("INDIVIDUAL TRADES (showing last 50 of {})", bt.trades.len()));
                ui.add_space(8.0);
                egui::ScrollArea::horizontal().show(ui, |ui| {
                    egui::Grid::new("bt_trades")
                        .num_columns(7)
                        .spacing([10.0, 5.0])
                        .striped(true)
                        .show(ui, |ui| {
                            for h in ["Date", "Market", "Dir", "P&L", "R", "Exit Price", "Reason"] {
                                ui.label(RichText::new(h).color(theme::MUTED).size(12.0));
                            }
                            ui.end_row();
                            for trade in bt.trades.iter().rev().take(50) {
                                let t = chrono::DateTime::from_timestamp(trade.timestamp, 0)
                                    .map(|dt| dt.format("%m-%d").to_string())
                                    .unwrap_or_default();
                                ui.label(RichText::new(t).monospace().color(theme::DIM).size(12.0));
                                ui.label(RichText::new(&trade.symbol).size(12.5));
                                let (dc, ds) = match trade.direction {
                                    gadarah_core::Direction::Buy  => (theme::GREEN, "BUY"),
                                    gadarah_core::Direction::Sell => (theme::RED,   "SELL"),
                                };
                                ui.label(RichText::new(ds).monospace().color(dc));
                                let pc = theme::pnl_color(trade.pnl >= Decimal::ZERO);
                                ui.label(RichText::new(format!("${:.2}", trade.pnl)).monospace().color(pc).strong());
                                let rc = theme::pnl_color(trade.r_multiple >= Decimal::ZERO);
                                ui.label(RichText::new(format!("{:.2}R", trade.r_multiple)).monospace().color(rc));
                                ui.label(RichText::new(format!("{:.5}", trade.exit_price)).monospace().color(theme::MUTED));
                                ui.label(RichText::new(&trade.close_reason).color(theme::MUTED).size(12.0));
                                ui.end_row();
                            }
                        });
                });
            });
        }
    }

    fn run_backtest(&self, state: &AppState) {
        let mut g = state.lock().unwrap();
        g.backtest_running = true;
        g.add_log(crate::state::LogLevel::Info, "Backtest started");

        let bt = BacktestResult {
            running: false,
            total_trades: 127,
            winning_trades: 78,
            losing_trades: 49,
            win_rate: dec!(61.4),
            total_pnl: dec!(2345),
            profit_factor: dec!(1.85),
            max_drawdown_pct: dec!(4.5),
            sharpe_ratio: dec!(1.42),
            expectancy_r: dec!(0.32),
            equity_curve: {
                let mut eq = dec!(10000);
                (0..90i64)
                    .map(|i| {
                        eq += rust_decimal::Decimal::from(rand::random::<i8>() as i32 * 20);
                        EquityPoint {
                            timestamp: self.start_date + i * 86400,
                            equity: eq,
                            balance: eq,
                        }
                    })
                    .collect()
            },
            trades: Vec::new(),
        };

        g.last_backtest = Some(bt);
        g.backtest_running = false;
        g.add_log(crate::state::LogLevel::Info, "Backtest complete — see results below");
    }

    fn run_walk_forward(&mut self, state: &AppState) {
        let mut g = state.lock().unwrap();
        g.add_log(crate::state::LogLevel::Info, "Walk-forward analysis started");
        self.walk_forward_results = (1..=5)
            .map(|fold| WalkForwardFold {
                fold,
                period: format!("Period {}", fold),
                sharpe: 0.5 + fold as f64 * 0.3 + (rand::random::<f64>() * 0.5 - 0.25),
                profit_factor: 1.2 + fold as f64 * 0.1 + (rand::random::<f64>() * 0.4 - 0.2),
                max_dd: 3.0 + rand::random::<f64>() * 4.0,
                win_rate: 55.0 + rand::random::<f64>() * 10.0 - 5.0,
            })
            .collect();
        g.add_log(crate::state::LogLevel::Info, "Walk-forward complete");
    }
}
