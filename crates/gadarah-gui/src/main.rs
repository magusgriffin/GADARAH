//! GADARAH GUI — Main application entry point

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use eframe::egui;
use egui::RichText;

use gadarah_gui::{
    config::GadarahConfig,
    state::{
        AppState, ConnectionStatus, EquityPoint, LogLevel, Position, SharedState, TradeRecord,
    },
    theme,
    ui::{BacktestPanel, ConfigPanel, DashboardPanel, LogsPanel, PerformancePanel},
};

struct GadarahApp {
    state: AppState,
    selected_tab: usize,
    backtest_panel: BacktestPanel,
    config_panel: ConfigPanel,
    performance_panel: PerformancePanel,
}

impl GadarahApp {
    fn new(cc: &eframe::CreationContext<'_>, state: AppState) -> Self {
        theme::setup(&cc.egui_ctx);
        let mut app = Self {
            state,
            selected_tab: 0,
            backtest_panel: BacktestPanel::new(),
            config_panel: ConfigPanel::new(),
            performance_panel: PerformancePanel::default(),
        };
        app.initialize_demo_data();
        app
    }

    fn initialize_demo_data(&mut self) {
        use rust_decimal_macros::dec;
        let mut state = self.state.lock().unwrap();

        let config_path = PathBuf::from("config/gadarah.toml");
        if let Ok(config) = GadarahConfig::load(&config_path) {
            state.config = config;
            state.add_log(LogLevel::Info, "Configuration loaded from config/gadarah.toml");
        } else {
            state.add_log(LogLevel::Warn, "Using default configuration");
        }

        if let Ok(entries) = std::fs::read_dir("config/firms") {
            for entry in entries.flatten() {
                if let Some(name) = entry.path().file_stem() {
                    state.available_firms.push(name.to_string_lossy().to_string());
                }
            }
            state.available_firms.sort();
        }

        state.balance          = dec!(10000);
        state.equity           = dec!(10500);
        state.free_margin      = dec!(9500);
        state.daily_pnl        = dec!(500);
        state.daily_pnl_pct    = dec!(5);
        state.total_pnl        = dec!(500);
        state.total_pnl_pct    = dec!(5);
        state.starting_balance = dec!(10000);

        use gadarah_core::{Direction, Regime9};
        state.positions = vec![
            Position {
                id: 1,
                symbol: "EURUSD".to_string(),
                direction: Direction::Buy,
                lots: dec!(0.10),
                entry_price: dec!(1.0850),
                current_price: dec!(1.0865),
                unrealized_pnl: dec!(15),
                stop_loss: Some(dec!(1.0830)),
                take_profit: Some(dec!(1.0900)),
                opened_at: chrono::Utc::now().timestamp() - 3600,
            },
            Position {
                id: 2,
                symbol: "GBPUSD".to_string(),
                direction: Direction::Sell,
                lots: dec!(0.08),
                entry_price: dec!(1.2650),
                current_price: dec!(1.2635),
                unrealized_pnl: dec!(12),
                stop_loss: Some(dec!(1.2680)),
                take_profit: Some(dec!(1.2600)),
                opened_at: chrono::Utc::now().timestamp() - 1800,
            },
        ];

        state.regime_by_symbol.insert(
            "EURUSD".to_string(),
            gadarah_core::RegimeSignal9 {
                regime: Regime9::StrongTrendUp,
                confidence: dec!(0.80),
                adx: dec!(28.5),
                hurst: dec!(0.65),
                atr_ratio: dec!(0.7),
                bb_width_pctile: dec!(0.45),
                choppiness_index: dec!(42),
                computed_at: chrono::Utc::now().timestamp(),
            },
        );
        state.regime_by_symbol.insert(
            "GBPUSD".to_string(),
            gadarah_core::RegimeSignal9 {
                regime: Regime9::WeakTrendUp,
                confidence: dec!(0.55),
                adx: dec!(22),
                hurst: dec!(0.52),
                atr_ratio: dec!(0.6),
                bb_width_pctile: dec!(0.38),
                choppiness_index: dec!(48),
                computed_at: chrono::Utc::now().timestamp(),
            },
        );

        state.active_heads = vec![
            gadarah_core::HeadId::Momentum,
            gadarah_core::HeadId::Breakout,
            gadarah_core::HeadId::Trend,
        ];

        // Demo equity curve (60 bars)
        let base_time = chrono::Utc::now().timestamp() - 86400 * 30;
        let mut running = dec!(10000);
        for i in 0..60i64 {
            let delta = rust_decimal::Decimal::from(rand::random::<i8>() as i32 * 15);
            running += delta;
            state.equity_curve.push(EquityPoint {
                timestamp: base_time + i * 86400,
                equity: running,
                balance: dec!(10000) + rust_decimal::Decimal::from(i * 8),
            });
        }

        // Demo trade history
        state.trade_history = vec![
            TradeRecord {
                id: 1,
                timestamp: chrono::Utc::now().timestamp() - 86400,
                symbol: "EURUSD".to_string(),
                head: gadarah_core::HeadId::Momentum,
                direction: Direction::Buy,
                entry_price: dec!(1.0800),
                exit_price: dec!(1.0850),
                lots: dec!(0.10),
                pnl: dec!(50),
                r_multiple: dec!(1.5),
                close_reason: "Take Profit".to_string(),
            },
            TradeRecord {
                id: 2,
                timestamp: chrono::Utc::now().timestamp() - 86400 * 2,
                symbol: "GBPUSD".to_string(),
                head: gadarah_core::HeadId::Breakout,
                direction: Direction::Sell,
                entry_price: dec!(1.2700),
                exit_price: dec!(1.2650),
                lots: dec!(0.08),
                pnl: dec!(40),
                r_multiple: dec!(1.2),
                close_reason: "Take Profit".to_string(),
            },
            TradeRecord {
                id: 3,
                timestamp: chrono::Utc::now().timestamp() - 86400 * 3,
                symbol: "EURUSD".to_string(),
                head: gadarah_core::HeadId::ScalpM5,
                direction: Direction::Buy,
                entry_price: dec!(1.0820),
                exit_price: dec!(1.0810),
                lots: dec!(0.05),
                pnl: dec!(-5),
                r_multiple: dec!(-0.5),
                close_reason: "Stop Loss".to_string(),
            },
        ];

        state.add_log(LogLevel::Info, "GADARAH started — bot is ready");
        state.add_log(LogLevel::Info, "Go to the Config tab to connect your broker account");
        state.update_stats();
    }
}

impl eframe::App for GadarahApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint_after(std::time::Duration::from_millis(500));

        // ── Top bar ──────────────────────────────────────────────────────────
        egui::TopBottomPanel::top("top_bar")
            .exact_height(52.0)
            .frame(
                egui::Frame::new()
                    .fill(theme::CARD)
                    .stroke(egui::Stroke::new(1.0, theme::BORDER))
                    .inner_margin(egui::Margin { left: 16, right: 16, top: 0, bottom: 0 }),
            )
            .show(ctx, |ui| {
                ui.set_height(52.0);
                ui.horizontal_centered(|ui| {
                    // Logo
                    ui.label(
                        RichText::new("GADARAH")
                            .size(20.0)
                            .strong()
                            .color(theme::ACCENT),
                    );
                    ui.label(
                        RichText::new("Algorithmic Forex Trading")
                            .size(12.0)
                            .color(theme::MUTED),
                    );

                    ui.add_space(20.0);

                    // Connection badge
                    let (conn_text, conn_bg, conn_fg) = {
                        let g = self.state.lock().unwrap();
                        match g.connection_status {
                            ConnectionStatus::ConnectedLive  => ("  LIVE  ",  egui::Color32::from_rgb(10, 38, 20), theme::GREEN),
                            ConnectionStatus::ConnectedDemo  => ("  DEMO  ",  egui::Color32::from_rgb(40, 35, 5),  theme::YELLOW),
                            ConnectionStatus::Connecting     => (" CONNECTING", egui::Color32::from_rgb(15, 25, 45), theme::BLUE),
                            ConnectionStatus::Disconnected   => (" NOT CONNECTED ", egui::Color32::from_rgb(40, 10, 10), theme::RED),
                        }
                    };
                    theme::pill(ui, conn_text, conn_bg, conn_fg);

                    // Kill switch warning banner
                    let ks_active = self.state.lock().unwrap().kill_switch_active;
                    if ks_active {
                        ui.add_space(12.0);
                        theme::pill(
                            ui,
                            "  TRADING HALTED — Kill Switch Active  ",
                            egui::Color32::from_rgb(80, 10, 10),
                            theme::RED,
                        );
                    }

                    // Right-align: last update time
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let now = chrono::Local::now().format("%H:%M:%S").to_string();
                        ui.label(RichText::new(now).color(theme::DIM).size(11.5).monospace());
                        ui.label(RichText::new("Updated ").color(theme::DIM).size(11.5));
                    });
                });
            });

        // ── Status bar ───────────────────────────────────────────────────────
        egui::TopBottomPanel::bottom("status_bar")
            .exact_height(26.0)
            .frame(
                egui::Frame::new()
                    .fill(theme::CARD)
                    .stroke(egui::Stroke::new(1.0, theme::BORDER))
                    .inner_margin(egui::Margin { left: 16, right: 16, top: 0, bottom: 0 }),
            )
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    let g = self.state.lock().unwrap();
                    let items = [
                        format!("Open positions: {}", g.positions.len()),
                        format!("Trades today: {}", g.total_trades),
                        format!("Markets tracked: {}", g.regime_by_symbol.len()),
                    ];
                    for (i, item) in items.iter().enumerate() {
                        if i > 0 { ui.separator(); }
                        ui.label(RichText::new(item).size(11.0).color(theme::DIM));
                    }
                });
            });

        // ── Main panel ───────────────────────────────────────────────────────
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::BG).inner_margin(egui::Margin::same(0i8)))
            .show(ctx, |ui| {
                // Tab bar
                egui::Frame::new()
                    .fill(theme::CARD)
                    .stroke(egui::Stroke::new(1.0, theme::BORDER))
                    .inner_margin(egui::Margin { left: 16, right: 16, top: 0, bottom: 0 })
                    .show(ui, |ui| {
                        ui.set_height(44.0);
                        ui.horizontal_centered(|ui| {
                            let tabs = [
                                "Dashboard",
                                "Performance",
                                "Backtest",
                                "Config",
                                "Logs",
                            ];
                            for (i, label) in tabs.iter().enumerate() {
                                let selected = self.selected_tab == i;
                                let fg = if selected { theme::ACCENT } else { theme::MUTED };
                                let btn = ui.add(
                                    egui::Button::new(
                                        RichText::new(*label).size(13.5).color(fg),
                                    )
                                    .frame(false)
                                    .min_size(egui::vec2(0.0, 44.0)),
                                );
                                if selected {
                                    ui.painter().hline(
                                        btn.rect.x_range(),
                                        btn.rect.bottom() - 1.0,
                                        egui::Stroke::new(2.5, theme::ACCENT),
                                    );
                                }
                                if btn.clicked() {
                                    self.selected_tab = i;
                                }
                                ui.add_space(16.0);
                            }
                        });
                    });

                // Content
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        // Padding around content
                        egui::Frame::new()
                            .inner_margin(egui::Margin::same(16i8))
                            .show(ui, |ui| {
                                let state = self.state.clone();
                                match self.selected_tab {
                                    0 => DashboardPanel::show(ui, &state),
                                    1 => self.performance_panel.show(ui, &state),
                                    2 => self.backtest_panel.show(ui, &state),
                                    3 => self.config_panel.show(ui, &state),
                                    4 => LogsPanel::show(ui, &state),
                                    _ => {}
                                }
                            });
                    });
            });
    }
}

fn main() -> eframe::Result<()> {
    let state = Arc::new(Mutex::new(SharedState::default()));

    // If --state-file <path> is passed, spawn a background thread that re-reads
    // the CLI-written JSON snapshot every second and updates SharedState.
    let args: Vec<String> = std::env::args().collect();
    if let Some(pos) = args.iter().position(|a| a == "--state-file") {
        if let Some(path) = args.get(pos + 1) {
            let path = path.clone();
            let state_bg = Arc::clone(&state);
            std::thread::spawn(move || loop {
                if let Ok(raw) = std::fs::read_to_string(&path) {
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&raw) {
                        let mut s = state_bg.lock().unwrap();
                        apply_state_snapshot(&mut s, &val);
                    }
                }
                std::thread::sleep(std::time::Duration::from_secs(1));
            });
        }
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1400.0, 900.0])
            .with_min_inner_size([1000.0, 640.0])
            .with_title("GADARAH — Algorithmic Forex Trading Bot"),
        ..Default::default()
    };
    eframe::run_native(
        "GADARAH",
        options,
        Box::new(|cc| Ok(Box::new(GadarahApp::new(cc, state)))),
    )
}

/// Apply a JSON state snapshot (written by the CLI live loop) to SharedState.
fn apply_state_snapshot(s: &mut SharedState, v: &serde_json::Value) {
    use rust_decimal::Decimal;
    use std::str::FromStr;

    let dec = |key: &str| -> Decimal {
        v[key].as_str().and_then(|s| Decimal::from_str(s).ok()).unwrap_or(Decimal::ZERO)
    };

    s.balance    = dec("balance");
    s.equity     = dec("equity");
    s.free_margin = dec("free_margin");
    s.daily_pnl  = dec("daily_pnl");

    s.kill_switch_active = v["kill_switch_active"].as_bool().unwrap_or(false);
    if s.kill_switch_active && s.kill_switch_reason.is_none() {
        s.kill_switch_reason = Some("DD threshold breached".to_string());
    } else if !s.kill_switch_active {
        s.kill_switch_reason = None;
    }

    s.connection_status = if v["kill_switch_active"].is_boolean() {
        gadarah_gui::state::ConnectionStatus::ConnectedDemo
    } else {
        gadarah_gui::state::ConnectionStatus::Disconnected
    };

    // Update positions from snapshot
    if let Some(positions) = v["positions"].as_array() {
        s.positions.clear();
        for p in positions {
            use gadarah_core::Direction;
            let direction = match p["direction"].as_str().unwrap_or("Buy") {
                "Sell" => Direction::Sell,
                _ => Direction::Buy,
            };
            let dec_field = |key: &str| -> Decimal {
                p[key].as_str().and_then(|s| Decimal::from_str(s).ok()).unwrap_or(Decimal::ZERO)
            };
            s.positions.push(gadarah_gui::state::Position {
                id: p["position_id"].as_u64().unwrap_or(0),
                symbol: p["symbol"].as_str().unwrap_or("").to_string(),
                direction,
                lots: dec_field("lots"),
                entry_price: dec_field("entry"),
                current_price: dec_field("entry"),
                unrealized_pnl: Decimal::ZERO,
                stop_loss: Some(dec_field("sl")),
                take_profit: Some(dec_field("tp")),
                opened_at: p["opened_at"].as_i64().unwrap_or(0),
            });
        }
    }
}
