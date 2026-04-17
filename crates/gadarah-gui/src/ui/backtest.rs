//! Backtest panel — Test the bot on historical data before going live

use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use eframe::egui;
use egui::RichText;
use egui_plot::{Line, Plot, PlotPoints};

use gadarah_backtest::{
    run_replay, run_walk_forward as run_wf, simulate_challenge, ChallengeSimResult, ReplayConfig,
    WalkForwardConfig,
};
use gadarah_core::heads::{
    asian_range::{AsianRangeConfig, AsianRangeHead},
    breakout::{BreakoutConfig, BreakoutHead},
    momentum::{MomentumConfig, MomentumHead},
};
use gadarah_core::Timeframe;
use gadarah_data::{load_all_bars, Database};

use crate::config::FirmConfig;
use crate::state::{AppState, BacktestResult, EquityPoint, LogLevel, TradeRecord};
use crate::theme;
use super::challenge_rules_for;

pub struct BacktestPanel {
    selected_symbols: Vec<String>,
    selected_firm: String,
    walk_forward_results: Vec<WalkForwardFold>,
    db_path: String,
    challenge_result: Option<ChallengeSimResult>,
}

struct WalkForwardFold {
    fold: usize,
    sharpe: f64,
    profit_factor: f64,
    max_dd: f64,
    win_rate: f64,
}

impl BacktestPanel {
    pub fn new() -> Self {
        Self {
            selected_symbols: vec!["EURUSD".to_string()],
            selected_firm: "the5ers_hypergrowth".to_string(),
            walk_forward_results: Vec::new(),
            db_path: "data/gadarah.db".to_string(),
            challenge_result: None,
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui, state: &AppState) {
        theme::heading(ui, "Backtest");
        ui.label(
            RichText::new(
                "Test the bot on historical price data to see how it would have performed.",
            )
            .color(theme::MUTED)
            .size(12.5),
        );
        ui.add_space(12.0);

        // Setup card
        theme::card().show(ui, |ui| {
            theme::section_label(ui, "TEST CONFIGURATION");
            ui.add_space(8.0);

            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Markets to test:").color(theme::TEXT).size(13.0));
                ui.add_space(4.0);
                for symbol in ["EURUSD", "GBPUSD", "USDJPY", "AUDUSD"] {
                    let mut checked = self.selected_symbols.contains(&symbol.to_string());
                    if ui.checkbox(&mut checked, symbol).changed() {
                        if checked {
                            self.selected_symbols.push(symbol.to_string());
                        } else {
                            self.selected_symbols.retain(|s| s != symbol);
                        }
                    }
                }
            });

            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Database:").color(theme::TEXT).size(13.0));
                ui.add(egui::TextEdit::singleline(&mut self.db_path).desired_width(250.0));
            });

            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Challenge rules to apply:")
                        .color(theme::TEXT)
                        .size(13.0),
                );
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
                    ui.label(
                        RichText::new("Running backtest — please wait…")
                            .color(theme::MUTED)
                            .size(13.0),
                    );
                });
            } else {
                ui.horizontal(|ui| {
                    if ui
                        .add_sized(
                            [160.0, 38.0],
                            egui::Button::new(
                                RichText::new("Run Backtest")
                                    .color(egui::Color32::WHITE)
                                    .strong(),
                            )
                            .fill(egui::Color32::from_rgb(0, 120, 88)),
                        )
                        .clicked()
                    {
                        self.run_backtest(state);
                    }
                    ui.add_space(8.0);
                    if ui
                        .add_sized(
                            [180.0, 38.0],
                            egui::Button::new(
                                RichText::new("Walk-Forward Test").color(theme::TEXT),
                            )
                            .fill(egui::Color32::from_rgb(35, 60, 95)),
                        )
                        .clicked()
                    {
                        self.run_walk_forward(state);
                    }
                    ui.add_space(12.0);
                    ui.label(
                        RichText::new(
                            "Walk-Forward runs the test across multiple time windows to check consistency.",
                        )
                        .color(theme::DIM)
                        .size(11.5),
                    );
                });
            }
        });

        ui.add_space(12.0);

        // Results
        let result = state.lock().unwrap().last_backtest.clone();

        let Some(bt) = result else {
            theme::card().show(ui, |ui| {
                theme::empty_state(ui, "🧪", "No Results Yet", "Click 'Run Backtest' above to test the bot on historical data. Make sure you have bars in the database (use 'gadarah fetch').");
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
            ui.horizontal_wrapped(|ui| {
                let items: &[(&str, String, egui::Color32)] = &[
                    ("Trades", format!("{}", bt.total_trades), theme::TEXT),
                    (
                        "Win Rate",
                        format!("{:.1}%", bt.win_rate),
                        if bt.win_rate >= dec!(50) {
                            theme::GREEN
                        } else {
                            theme::RED
                        },
                    ),
                    (
                        "PF",
                        format!("{:.2}", bt.profit_factor),
                        if bt.profit_factor >= dec!(1.5) {
                            theme::GREEN
                        } else if bt.profit_factor >= Decimal::ONE {
                            theme::YELLOW
                        } else {
                            theme::RED
                        },
                    ),
                    (
                        "Max DD",
                        format!("{:.2}%", bt.max_drawdown_pct),
                        if bt.max_drawdown_pct <= dec!(5) {
                            theme::GREEN
                        } else if bt.max_drawdown_pct <= dec!(10) {
                            theme::YELLOW
                        } else {
                            theme::RED
                        },
                    ),
                    ("Sharpe", format!("{:.2}", bt.sharpe_ratio), theme::TEXT),
                    (
                        "Expect.",
                        format!("{:.2}R", bt.expectancy_r),
                        if bt.expectancy_r >= Decimal::ZERO {
                            theme::GREEN
                        } else {
                            theme::RED
                        },
                    ),
                ];
                for (label, value, color) in items {
                    theme::stat_card(ui, label, value, *color, card_w);
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
                let points: PlotPoints = bt
                    .equity_curve
                    .iter()
                    .enumerate()
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

        // Challenge simulation result
        if let Some(ref cr) = self.challenge_result {
            theme::card().show(ui, |ui| {
                let status_color = if cr.passed { theme::GREEN } else { theme::RED };
                let status_label = if cr.passed { "PASSED" } else { "FAILED" };
                theme::section_label(
                    ui,
                    &format!("CHALLENGE SIMULATION — {} — {}", cr.rules.name, status_label),
                );
                ui.add_space(10.0);

                let card_w = (ui.available_width() - 50.0) / 5.0;
                ui.horizontal_wrapped(|ui| {
                    theme::stat_card(
                        ui,
                        "Result",
                        status_label,
                        status_color,
                        card_w,
                    );
                    ui.add_space(8.0);
                    theme::stat_card(
                        ui,
                        "Profit",
                        &format!("{:.2}%", cr.profit_pct),
                        if cr.target_reached { theme::GREEN } else { theme::RED },
                        card_w,
                    );
                    ui.add_space(8.0);
                    theme::stat_card(
                        ui,
                        "Max Daily DD",
                        &format!("{:.2}%", cr.max_daily_dd_pct),
                        if cr.daily_dd_breached { theme::RED } else { theme::GREEN },
                        card_w,
                    );
                    ui.add_space(8.0);
                    theme::stat_card(
                        ui,
                        "Max Total DD",
                        &format!("{:.2}%", cr.max_total_dd_pct),
                        if cr.max_dd_breached { theme::RED } else { theme::GREEN },
                        card_w,
                    );
                    ui.add_space(8.0);
                    let days_str = cr
                        .days_to_target
                        .map(|d| format!("{}", d))
                        .unwrap_or_else(|| "—".to_string());
                    theme::stat_card(ui, "Trading Days", &days_str, theme::TEXT, card_w);
                });

                if let Some(ref reason) = cr.breach_reason {
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new(format!("Breach: {}", reason))
                            .color(theme::RED)
                            .size(12.5),
                    );
                }

                if !cr.consistency_met {
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new("Best Day Rule / consistency check failed")
                            .color(theme::ORANGE)
                            .size(12.0),
                    );
                }
            });
            ui.add_space(12.0);
        }

        // Walk-forward table
        if !self.walk_forward_results.is_empty() {
            theme::card().show(ui, |ui| {
                theme::section_label(
                    ui,
                    "WALK-FORWARD CONSISTENCY — Same settings, different time windows",
                );
                ui.add_space(8.0);
                egui::Grid::new("wf_table")
                    .num_columns(5)
                    .spacing([14.0, 7.0])
                    .striped(true)
                    .show(ui, |ui| {
                        for h in [
                            "Window",
                            "Sharpe",
                            "Profit Factor",
                            "Max Drawdown",
                            "Win Rate",
                        ] {
                            ui.label(RichText::new(h).color(theme::MUTED).size(12.0));
                        }
                        ui.end_row();
                        for fold in &self.walk_forward_results {
                            ui.label(
                                RichText::new(format!("Period {}", fold.fold))
                                    .color(theme::TEXT)
                                    .size(12.5),
                            );
                            let sc = if fold.sharpe >= 1.0 {
                                theme::GREEN
                            } else if fold.sharpe >= 0.5 {
                                theme::YELLOW
                            } else {
                                theme::RED
                            };
                            ui.label(
                                RichText::new(format!("{:.2}", fold.sharpe))
                                    .monospace()
                                    .color(sc),
                            );
                            let pc = if fold.profit_factor >= 1.5 {
                                theme::GREEN
                            } else if fold.profit_factor >= 1.0 {
                                theme::YELLOW
                            } else {
                                theme::RED
                            };
                            ui.label(
                                RichText::new(format!("{:.2}", fold.profit_factor))
                                    .monospace()
                                    .color(pc),
                            );
                            let dc = if fold.max_dd <= 5.0 {
                                theme::GREEN
                            } else if fold.max_dd <= 10.0 {
                                theme::YELLOW
                            } else {
                                theme::RED
                            };
                            ui.label(
                                RichText::new(format!("{:.1}%", fold.max_dd))
                                    .monospace()
                                    .color(dc),
                            );
                            let wc = if fold.win_rate >= 50.0 {
                                theme::GREEN
                            } else {
                                theme::RED
                            };
                            ui.label(
                                RichText::new(format!("{:.1}%", fold.win_rate))
                                    .monospace()
                                    .color(wc),
                            );
                            ui.end_row();
                        }
                    });
            });
            ui.add_space(12.0);
        }

        // Trade log
        if !bt.trades.is_empty() {
            theme::card().show(ui, |ui| {
                theme::section_label(
                    ui,
                    &format!("INDIVIDUAL TRADES (showing last 50 of {})", bt.trades.len()),
                );
                ui.add_space(8.0);
                egui::ScrollArea::horizontal().show(ui, |ui| {
                    egui::Grid::new("bt_trades")
                        .num_columns(6)
                        .spacing([10.0, 5.0])
                        .striped(true)
                        .show(ui, |ui| {
                            for h in ["Date", "Strategy", "P&L", "R", "Result", "Duration"] {
                                ui.label(RichText::new(h).color(theme::MUTED).size(12.0));
                            }
                            ui.end_row();
                            for trade in bt.trades.iter().rev().take(50) {
                                let t = chrono::DateTime::from_timestamp(trade.timestamp, 0)
                                    .map(|dt| dt.format("%m-%d %H:%M").to_string())
                                    .unwrap_or_default();
                                ui.label(RichText::new(t).monospace().color(theme::DIM).size(12.0));
                                ui.label(
                                    RichText::new(&trade.close_reason)
                                        .size(12.5)
                                        .color(theme::MUTED),
                                );
                                let pc = theme::pnl_color(trade.pnl >= Decimal::ZERO);
                                ui.label(
                                    RichText::new(format!("${:.2}", trade.pnl))
                                        .monospace()
                                        .color(pc)
                                        .strong(),
                                );
                                let rc = theme::pnl_color(trade.r_multiple >= Decimal::ZERO);
                                ui.label(
                                    RichText::new(format!("{:.2}R", trade.r_multiple))
                                        .monospace()
                                        .color(rc),
                                );
                                let result_str = if trade.pnl >= Decimal::ZERO {
                                    "Win"
                                } else {
                                    "Loss"
                                };
                                ui.label(RichText::new(result_str).color(pc).size(12.0));
                                // Duration not available in TradeResult, show head instead
                                ui.label(
                                    RichText::new(format!("{:?}", trade.head))
                                        .monospace()
                                        .color(theme::DIM)
                                        .size(12.0),
                                );
                                ui.end_row();
                            }
                        });
                });
            });
        }
    }

    fn run_backtest(&mut self, state: &AppState) {
        let mut g = state.lock().unwrap();
        g.backtest_running = true;
        g.add_log(LogLevel::Info, "Backtest started");

        // Load firm config for the selected firm so DD limits match.
        let firm_config = load_firm_config(&self.selected_firm);
        let (daily_dd, max_dd) = match &firm_config {
            Some(fc) => (fc.firm.daily_dd_limit_pct, fc.firm.max_dd_limit_pct),
            None => (dec!(3.0), dec!(6.0)),
        };

        let db = match Database::open(&self.db_path) {
            Ok(db) => db,
            Err(e) => {
                g.add_log(
                    LogLevel::Error,
                    format!("Failed to open database '{}': {}", self.db_path, e),
                );
                g.backtest_running = false;
                return;
            }
        };

        let mut all_trades = Vec::new();
        let mut all_backtest_trades = Vec::new();
        let mut all_equity: Vec<EquityPoint> = Vec::new();
        let mut combined_stats = None;

        for symbol in &self.selected_symbols {
            let bars = match load_all_bars(db.conn(), symbol, Timeframe::M15) {
                Ok(b) if !b.is_empty() => b,
                Ok(_) => {
                    g.add_log(LogLevel::Warn, format!("No M15 bars found for {}", symbol));
                    continue;
                }
                Err(e) => {
                    g.add_log(
                        LogLevel::Warn,
                        format!("Failed to load bars for {}: {}", symbol, e),
                    );
                    continue;
                }
            };

            g.add_log(
                LogLevel::Info,
                format!("Running backtest on {} ({} bars)", symbol, bars.len()),
            );

            let config = make_replay_config(symbol, dec!(10000), daily_dd, max_dd);
            let mut heads = make_heads(symbol);

            match run_replay(&bars, &mut heads, &config) {
                Ok(result) => {
                    // Convert equity curve
                    for (ts, eq) in &result.equity_curve {
                        all_equity.push(EquityPoint {
                            timestamp: *ts,
                            equity: *eq,
                            balance: *eq,
                        });
                    }

                    // Keep raw TradeResult copies for challenge simulation
                    all_backtest_trades.extend(result.trades.iter().cloned());

                    // Convert trades for UI display
                    for t in &result.trades {
                        let head_name = format!("{:?}", t.head);
                        all_trades.push(TradeRecord {
                            id: all_trades.len() as u64,
                            timestamp: t.closed_at,
                            symbol: symbol.clone(),
                            head: t.head,
                            direction: gadarah_core::Direction::Buy, // TradeResult doesn't carry direction
                            entry_price: Decimal::ZERO,
                            exit_price: Decimal::ZERO,
                            lots: Decimal::ZERO,
                            pnl: t.pnl,
                            r_multiple: t.r_multiple,
                            close_reason: head_name,
                        });
                    }

                    g.add_log(
                        LogLevel::Info,
                        format!(
                            "{}: {} trades, PF={:.2}, WR={:.1}%, DD={:.2}%",
                            symbol,
                            result.trades.len(),
                            result.stats.profit_factor,
                            result.stats.win_rate,
                            result.stats.max_drawdown_pct
                        ),
                    );

                    combined_stats = Some(result.stats);
                }
                Err(e) => {
                    g.add_log(
                        LogLevel::Error,
                        format!("Backtest failed for {}: {}", symbol, e),
                    );
                }
            }
        }

        // Run challenge simulation using the proper ChallengeRules constructor
        self.challenge_result = if let Some(fc) = &firm_config {
            if !all_backtest_trades.is_empty() {
                let rules = challenge_rules_for(fc);
                let result = simulate_challenge(&all_backtest_trades, dec!(10000), &rules);
                let status = if result.passed { "PASSED" } else { "FAILED" };
                g.add_log(
                    LogLevel::Info,
                    format!(
                        "Challenge sim ({}): {} — profit {:.2}%, max daily DD {:.2}%, max total DD {:.2}%",
                        rules.name, status, result.profit_pct, result.max_daily_dd_pct, result.max_total_dd_pct,
                    ),
                );
                if let Some(ref reason) = result.breach_reason {
                    g.add_log(LogLevel::Warn, format!("Breach reason: {}", reason));
                }
                Some(result)
            } else {
                None
            }
        } else {
            g.add_log(
                LogLevel::Warn,
                format!(
                    "No firm config found for '{}' — challenge simulation skipped",
                    self.selected_firm,
                ),
            );
            None
        };

        // Sort equity curve by timestamp
        all_equity.sort_by_key(|e| e.timestamp);

        let stats = combined_stats.unwrap_or_default();

        let bt = BacktestResult {
            running: false,
            total_trades: stats.total_trades as u32,
            winning_trades: stats.winners as u32,
            losing_trades: stats.losers as u32,
            win_rate: stats.win_rate,
            total_pnl: stats.total_pnl,
            profit_factor: stats.profit_factor,
            max_drawdown_pct: stats.max_drawdown_pct,
            sharpe_ratio: stats.sharpe_ratio,
            expectancy_r: stats.expectancy_r,
            equity_curve: all_equity,
            trades: all_trades,
        };

        g.last_backtest = Some(bt);
        g.backtest_running = false;
        g.add_log(LogLevel::Info, "Backtest complete — see results below");
    }

    fn run_walk_forward(&mut self, state: &AppState) {
        let mut g = state.lock().unwrap();
        g.add_log(LogLevel::Info, "Walk-forward analysis started");

        let firm_config = load_firm_config(&self.selected_firm);
        let (daily_dd, max_dd) = match &firm_config {
            Some(fc) => (fc.firm.daily_dd_limit_pct, fc.firm.max_dd_limit_pct),
            None => (dec!(3.0), dec!(6.0)),
        };

        let db = match Database::open(&self.db_path) {
            Ok(db) => db,
            Err(e) => {
                g.add_log(
                    LogLevel::Error,
                    format!("Failed to open database '{}': {}", self.db_path, e),
                );
                return;
            }
        };

        self.walk_forward_results.clear();

        for symbol in &self.selected_symbols {
            let bars = match load_all_bars(db.conn(), symbol, Timeframe::M15) {
                Ok(b) if !b.is_empty() => b,
                Ok(_) => {
                    g.add_log(LogLevel::Warn, format!("No M15 bars for {}", symbol));
                    continue;
                }
                Err(e) => {
                    g.add_log(
                        LogLevel::Warn,
                        format!("Failed to load bars for {}: {}", symbol, e),
                    );
                    continue;
                }
            };

            let config = make_replay_config(symbol, dec!(10000), daily_dd, max_dd);
            let wf_config = WalkForwardConfig {
                num_folds: 5,
                in_sample_ratio: 0.70,
            };

            let sym = symbol.clone();
            let head_factory = move || make_heads(&sym);

            match run_wf(&bars, head_factory, &config, &wf_config) {
                Ok(result) => {
                    for fold in &result.folds {
                        let oos = &fold.out_of_sample_stats;
                        self.walk_forward_results.push(WalkForwardFold {
                            fold: fold.fold_index + 1,
                            sharpe: oos.sharpe_ratio.to_string().parse::<f64>().unwrap_or(0.0),
                            profit_factor: oos
                                .profit_factor
                                .to_string()
                                .parse::<f64>()
                                .unwrap_or(0.0),
                            max_dd: oos
                                .max_drawdown_pct
                                .to_string()
                                .parse::<f64>()
                                .unwrap_or(0.0),
                            win_rate: oos.win_rate.to_string().parse::<f64>().unwrap_or(0.0),
                        });
                    }

                    let pass_str = if result.passed { "PASSED" } else { "FAILED" };
                    g.add_log(
                        LogLevel::Info,
                        format!(
                            "Walk-forward {}: {} — OOS degradation {:.1}%",
                            symbol, pass_str, result.oos_degradation_pct
                        ),
                    );
                }
                Err(e) => {
                    g.add_log(
                        LogLevel::Error,
                        format!("Walk-forward failed for {}: {}", symbol, e),
                    );
                }
            }
        }

        g.add_log(LogLevel::Info, "Walk-forward analysis complete");
    }
}

fn make_replay_config(
    symbol: &str,
    balance: Decimal,
    daily_dd_limit_pct: Decimal,
    max_dd_limit_pct: Decimal,
) -> ReplayConfig {
    ReplayConfig {
        symbol: symbol.to_string(),
        pip_size: if symbol.contains("JPY") {
            dec!(0.01)
        } else {
            dec!(0.0001)
        },
        pip_value_per_lot: dec!(10.0),
        starting_balance: balance,
        risk_pct: dec!(0.74),
        daily_dd_limit_pct,
        max_dd_limit_pct,
        max_positions: 3,
        min_rr: dec!(1.5),
        max_spread_pips: dec!(3.0),
        mock_config: gadarah_broker::MockConfig::default(),
        consecutive_loss_halt: 5,
    }
}

fn load_firm_config(firm_name: &str) -> Option<FirmConfig> {
    let path = std::path::PathBuf::from(format!("config/firms/{}.toml", firm_name));
    FirmConfig::load(&path).ok()
}

fn make_heads(symbol: &str) -> Vec<Box<dyn gadarah_core::Head>> {
    let pip_size = if symbol.contains("JPY") {
        dec!(0.01)
    } else {
        dec!(0.0001)
    };
    vec![
        Box::new(BreakoutHead::new(BreakoutConfig {
            squeeze_pctile: dec!(10.0),
            expansion_pctile: dec!(90.0),
            min_squeeze_bars: 4,
            volume_mult: dec!(1.2),
            tp1_atr_mult: dec!(1.5),
            tp2_atr_mult: dec!(2.5),
            min_rr: dec!(1.5),
            fakeout_bars: 3,
            base_confidence: dec!(0.5),
            symbol: symbol.to_string(),
        })),
        Box::new(MomentumHead::new(MomentumConfig {
            min_rr: dec!(1.5),
            base_confidence: dec!(0.5),
            first_hour_bars: 4,
            min_range_pips: dec!(10.0),
            breakout_buffer_pips: dec!(5.0),
            pip_size,
            symbol: symbol.to_string(),
        })),
        Box::new(AsianRangeHead::new(
            AsianRangeConfig {
                asian_start_utc: 0,
                asian_end_utc: 4,
                entry_window_end: 9,
                min_range_pips: dec!(15.0),
                max_range_pips: dec!(60.0),
                sl_buffer_pips: dec!(5.0),
                tp1_multiplier: dec!(1.5),
                tp2_multiplier: dec!(2.5),
                min_rr: dec!(1.5),
                max_trades_per_day: 3,
                symbol: symbol.to_string(),
                base_confidence: dec!(0.5),
            },
            pip_size,
        )),
    ]
}
