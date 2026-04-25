//! GADARAH GUI — Main application entry point
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use eframe::egui;
use egui::RichText;

use gadarah_gui::{
    config::GadarahConfig,
    first_run,
    notifications::NotificationSettings,
    oracle::{auto_prompt, OracleConfig, OracleHandle, OracleReply},
    state::{AppState, ConnectionStatus, LogLevel, SharedState},
    theme,
    ui::{
        welcome::{WelcomeOutcome, WelcomeOverlay},
        BacktestPanel, ConfigPanel, DashboardPanel, LogsPanel, OraclePanel, PayoutPanel,
        PerformancePanel, PriceChartPanel, SessionsPanel, TradingPanel,
    },
    update_check,
    widgets::{
        alert_banner, demo_banner,
        mascot::{self, MascotMood, MascotState, MascotSubsystem},
    },
};

/// Tab index of the Config tab. Kept as a named constant so the Config
/// nudge, preflight "Fix" links, and dashboard CTA all jump to the same
/// place if the tab order ever shifts.
const CONFIG_TAB_IDX: usize = 8;
/// Tab index of the Oracle tab — used by the "Open in Oracle" affordance
/// on auto-prompt follow-up alerts in the banner.
const ORACLE_TAB_IDX: usize = 7;

struct GadarahApp {
    state: AppState,
    selected_tab: usize,
    backtest_panel: BacktestPanel,
    config_panel: ConfigPanel,
    performance_panel: PerformancePanel,
    payout_panel: PayoutPanel,
    oracle_panel: OraclePanel,
    trading_panel: TradingPanel,
    oracle_cfg: OracleConfig,
    oracle: OracleHandle,
    mascot: MascotState,
    welcome: WelcomeOverlay,
    /// Tracks the previous `selected_tab` so we can fire mascot coaching
    /// exactly once per tab transition.
    prev_tab: usize,
    /// Confirmation modal for toggling into live trading.
    pending_live_confirm: bool,
    /// Auto-prompt watcher — diffs state against last frame and fires the
    /// Oracle when an alert ≥ Warning surfaces or a regime flips on a
    /// symbol with an open position. Runs on the UI loop; debounced
    /// per-trigger-key.
    auto_prompt: auto_prompt::AutoPromptWatcher,
}

impl GadarahApp {
    fn new(cc: &eframe::CreationContext<'_>, state: AppState) -> Self {
        theme::setup(&cc.egui_ctx);
        let oracle_cfg = OracleConfig::load();
        let oracle = OracleHandle::spawn(oracle_cfg.clone());
        // Kick off an initial status ping so the integrations panel shows
        // something useful on first paint.
        let _ = oracle.tx.send(gadarah_gui::oracle::OracleRequest::Ping);
        let mut app = Self {
            state,
            selected_tab: 0,
            backtest_panel: BacktestPanel::new(),
            config_panel: ConfigPanel::new(),
            performance_panel: PerformancePanel::default(),
            payout_panel: PayoutPanel::default(),
            oracle_panel: OraclePanel::default(),
            trading_panel: TradingPanel::default(),
            oracle_cfg,
            oracle,
            mascot: MascotState::default(),
            welcome: WelcomeOverlay::default(),
            prev_tab: 0,
            pending_live_confirm: false,
            auto_prompt: auto_prompt::AutoPromptWatcher::default(),
        };
        app.initialize_demo_data();
        // First launch → show welcome overlay. Persisted in
        // `$CONFIG/gadarah/welcome_seen.flag` once the user dismisses it.
        if first_run::is_first_run() {
            app.state.lock().unwrap().show_welcome_overlay = true;
        }
        // Load saved notification preferences (OS toasts + webhook). Default
        // is "Warning+" toasts on, webhook off — safe for a fresh install.
        {
            let mut g = app.state.lock().unwrap();
            g.notification_settings = NotificationSettings::load();
        }
        // Background update check: hits the GitHub Releases API, pushes an
        // Info alert with an "Update Now" button into `state.alerts` if a
        // newer version is available. Spawned once per launch; cached
        // server-side at most once per 6 h.
        update_check::spawn(app.state.clone());
        app
    }

    /// Drain any oracle replies, route into panel + mascot. Auto-prompt
    /// replies (tag `auto_prompt::AUTO_TAG`) bypass the panel and become
    /// follow-up alerts in the alert feed instead.
    fn pump_oracle(&mut self) {
        for reply in self.oracle.drain() {
            match reply {
                OracleReply::Ready(advice) => {
                    if advice.tag() == auto_prompt::AUTO_TAG {
                        // Auto-prompt reply: build a follow-up alert and
                        // push into state. Mascot stays quiet for this
                        // path — the alert banner is the surface.
                        if let Some(title) = self.auto_prompt.take_pending_title() {
                            let alert = auto_prompt::build_follow_up_alert(
                                title,
                                advice.body().to_string(),
                            );
                            self.state.lock().unwrap().push_alert(alert);
                        }
                        continue;
                    }
                    self.oracle_panel.record_advice(&advice);
                    self.mascot
                        .set_mood(MascotSubsystem::Oracle, MascotMood::Watchful);
                    self.mascot.bubble = Some((
                        MascotSubsystem::Oracle,
                        advice.body().chars().take(140).collect(),
                        mascot::BubbleTone::Divination,
                    ));
                }
                OracleReply::Error(msg) => {
                    self.oracle_panel.record_error(msg);
                    self.mascot
                        .set_mood(MascotSubsystem::Oracle, MascotMood::Warning);
                }
                OracleReply::Status { ollama_alive } => {
                    self.oracle_panel.record_status(ollama_alive);
                    self.mascot.set_mood(
                        MascotSubsystem::Oracle,
                        if ollama_alive {
                            MascotMood::Calm
                        } else {
                            MascotMood::Dim
                        },
                    );
                }
            }
        }
    }

    /// Per-frame: diff state and fire any new auto-prompt triggers.
    fn pump_auto_prompt(&mut self) {
        self.auto_prompt.tick(&self.state, &self.oracle.tx);
    }

    /// Derive per-subsystem mascot moods from the current shared state.
    fn refresh_mascot_moods(&mut self) {
        let g = self.state.lock().unwrap();
        // Herald (feed)
        let feed_mood = if g.stale_ms > 2000 {
            MascotMood::Alarmed
        } else if g.stale_ms > 500 {
            MascotMood::Warning
        } else if matches!(g.connection_status, ConnectionStatus::Disconnected) {
            MascotMood::Dim
        } else {
            MascotMood::Calm
        };
        // Warden (risk)
        let warden_mood = if g.kill_switch_active {
            MascotMood::Alarmed
        } else {
            MascotMood::Calm
        };
        // Reckoner (challenge clock)
        let dd_pct_f: f32 = g.max_drawdown_pct.to_string().parse().unwrap_or(0.0);
        let reckoner_mood = if dd_pct_f > 8.0 {
            MascotMood::Alarmed
        } else if dd_pct_f > 4.0 {
            MascotMood::Warning
        } else {
            MascotMood::Watchful
        };
        drop(g);

        self.mascot.set_mood(MascotSubsystem::MarketFeed, feed_mood);
        self.mascot.set_mood(MascotSubsystem::RiskGate, warden_mood);
        self.mascot
            .set_mood(MascotSubsystem::ChallengeClock, reckoner_mood);
        // Oracle mood is driven by pump_oracle; Chronicler stays calm v1.
    }

    /// Drain mascot-coaching events queued by the Trading panel (launch /
    /// stop / preflight-complete transitions) and render them as speech.
    fn pump_trading_mascot_events(&mut self) {
        use gadarah_gui::ui::trading::MascotEvent;
        use gadarah_gui::widgets::mascot::BubbleTone;
        if let Some(evt) = self.trading_panel.take_mascot_event() {
            let (head, tone, text): (MascotSubsystem, BubbleTone, &str) = match evt {
                MascotEvent::LaunchAcknowledged(mode) => (
                    MascotSubsystem::Chronicler,
                    BubbleTone::Rite,
                    match mode {
                        gadarah_gui::ui::trading::TradingMode::DryRun => {
                            "The dry run begins. No orders will ship."
                        }
                        gadarah_gui::ui::trading::TradingMode::DemoExecute => {
                            "Demo orders now flow. Watch the daemon log."
                        }
                        gadarah_gui::ui::trading::TradingMode::LiveExecute => {
                            "Live trading engaged. Every order is real."
                        }
                    },
                ),
                MascotEvent::StoppedByUser => (
                    MascotSubsystem::Chronicler,
                    BubbleTone::Chronicle,
                    "Daemon stopped. Positions remain; the risk gate is quiet.",
                ),
                MascotEvent::PreflightComplete => (
                    MascotSubsystem::Chronicler,
                    BubbleTone::Rite,
                    "Preflight passes. LIVE mode is now eligible to start.",
                ),
            };
            self.mascot.bubble = Some((head, text.to_string(), tone));
        }
    }

    /// Detect a tab transition and push a coaching speech bubble. Runs once
    /// per frame; cheap when nothing changes.
    fn pump_tab_navigation(&mut self) {
        use gadarah_gui::widgets::mascot::BubbleTone;
        if self.selected_tab == self.prev_tab {
            return;
        }
        self.prev_tab = self.selected_tab;
        let (head, tone, text) = match self.selected_tab {
            0 => (
                MascotSubsystem::Chronicler,
                BubbleTone::Chronicle,
                "Dashboard. Equity, positions, and the day's P&L.",
            ),
            1 => (
                MascotSubsystem::RiskGate,
                BubbleTone::Rite,
                "Preflight the gates before starting a run.",
            ),
            2 => (
                MascotSubsystem::MarketFeed,
                BubbleTone::Chronicle,
                "Session overlap. Size aligns with liquidity here.",
            ),
            3 => (
                MascotSubsystem::MarketFeed,
                BubbleTone::Chronicle,
                "Price tape. Bars are what the heads see.",
            ),
            4 => (
                MascotSubsystem::Chronicler,
                BubbleTone::Chronicle,
                "Performance. R-multiples and drawdown speak here.",
            ),
            5 => (
                MascotSubsystem::Chronicler,
                BubbleTone::Rite,
                "Backtest. Validate an edge before you risk it.",
            ),
            6 => (
                MascotSubsystem::ChallengeClock,
                BubbleTone::Rite,
                "Payout geometry. Where the challenge math lives.",
            ),
            7 => (
                MascotSubsystem::Oracle,
                BubbleTone::Divination,
                "Oracle. It advises only — never places orders.",
            ),
            8 => (
                MascotSubsystem::Chronicler,
                BubbleTone::Rite,
                "Config. Broker setup, risk limits, firm profile.",
            ),
            9 => (
                MascotSubsystem::Chronicler,
                BubbleTone::Chronicle,
                "Logs. Scroll for gate rejections and compliance trips.",
            ),
            _ => return,
        };
        self.mascot.bubble = Some((head, text.to_string(), tone));
    }

    fn initialize_demo_data(&mut self) {
        let mut state = self.state.lock().unwrap();

        let config_path = PathBuf::from("config/gadarah.toml");
        if let Ok(config) = GadarahConfig::load(&config_path) {
            state.config = config;
            state.add_log(
                LogLevel::Info,
                "Configuration loaded from config/gadarah.toml",
            );
        } else {
            state.add_log(LogLevel::Warn, "Using default configuration");
        }

        if let Ok(entries) = std::fs::read_dir("config/firms") {
            for entry in entries.flatten() {
                if let Some(name) = entry.path().file_stem() {
                    state
                        .available_firms
                        .push(name.to_string_lossy().to_string());
                }
            }
            state.available_firms.sort();
        }

        state.add_log(
            LogLevel::Info,
            "GADARAH started — waiting for broker connection",
        );
        state.add_log(
            LogLevel::Info,
            "Use --state-file <path> to bridge live data from the CLI",
        );
        state.add_log(
            LogLevel::Info,
            "Go to the Config tab to configure your trading parameters",
        );
    }
}

impl eframe::App for GadarahApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint_after(std::time::Duration::from_millis(500));

        // ── Background pumps ─────────────────────────────────────────────────
        self.pump_oracle();
        self.pump_auto_prompt();
        self.refresh_mascot_moods();
        self.pump_trading_mascot_events();
        self.pump_tab_navigation();

        // ── First-run welcome overlay (blocks interaction until dismissed) ──
        if self.state.lock().unwrap().show_welcome_overlay {
            match self.welcome.show(ctx) {
                WelcomeOutcome::None => {}
                WelcomeOutcome::GoToConfig => {
                    first_run::mark_seen();
                    self.state.lock().unwrap().show_welcome_overlay = false;
                    self.selected_tab = CONFIG_TAB_IDX;
                }
                WelcomeOutcome::Dismiss => {
                    first_run::mark_seen();
                    self.state.lock().unwrap().show_welcome_overlay = false;
                }
            }
        }

        // ── Demo/Live banner (sits above everything else) ────────────────────
        let banner_status = self.state.lock().unwrap().connection_status;
        demo_banner::show(ctx, banner_status);

        // ── Alert banner (newest un-dismissed, auto-expires at 30s) ─────────
        if let Some(tab_idx) = alert_banner::show(ctx, &self.state) {
            // Banner returned a tab-switch request (e.g. "Open in Oracle"
            // on an auto-prompt follow-up). Honour it before tab content
            // renders this frame so the user lands on the right place.
            self.selected_tab = tab_idx;
            // Clamp via our named const to catch tab-order drift early.
            debug_assert!(tab_idx == ORACLE_TAB_IDX);
        }

        // ── Live-trading confirmation ───────────────────────────────────────
        // Fires on the first `ConnectedLive` tick of a session, AND whenever
        // the Trading tab has queued a LIVE start. Both paths flow through
        // the same modal so the user sees exactly one "are you sure" prompt.
        if matches!(banner_status, ConnectionStatus::ConnectedLive)
            && !self.pending_live_confirm
            && !self.state.lock().unwrap().live_acknowledged
        {
            self.pending_live_confirm = true;
        }
        if self.trading_panel.is_awaiting_live_confirmation() && !self.pending_live_confirm {
            self.pending_live_confirm = true;
        }
        if self.pending_live_confirm {
            show_live_confirm(ctx, self);
        }

        // ── Top bar ──────────────────────────────────────────────────────────
        egui::TopBottomPanel::top("top_bar")
            .exact_height(52.0)
            .frame(
                egui::Frame::new()
                    .fill(theme::CARD)
                    .stroke(egui::Stroke::new(1.0, theme::BORDER))
                    .inner_margin(egui::Margin {
                        left: 16,
                        right: 16,
                        top: 0,
                        bottom: 0,
                    }),
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
                            ConnectionStatus::ConnectedLive => (
                                "  LIVE  ",
                                egui::Color32::from_rgb(10, 38, 20),
                                theme::GREEN,
                            ),
                            ConnectionStatus::ConnectedDemo => (
                                "  DEMO  ",
                                egui::Color32::from_rgb(40, 35, 5),
                                theme::YELLOW,
                            ),
                            ConnectionStatus::Connecting => (
                                " CONNECTING",
                                egui::Color32::from_rgb(15, 25, 45),
                                theme::BLUE,
                            ),
                            ConnectionStatus::Disconnected => (
                                " NOT CONNECTED ",
                                egui::Color32::from_rgb(40, 10, 10),
                                theme::RED,
                            ),
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
                    .inner_margin(egui::Margin {
                        left: 16,
                        right: 16,
                        top: 0,
                        bottom: 0,
                    }),
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
                        if i > 0 {
                            ui.separator();
                        }
                        ui.label(RichText::new(item).size(11.0).color(theme::DIM));
                    }
                });
            });

        // ── Main panel ───────────────────────────────────────────────────────
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(theme::BG)
                    .inner_margin(egui::Margin::same(0i8)),
            )
            .show(ctx, |ui| {
                // Tab bar
                egui::Frame::new()
                    .fill(theme::CARD)
                    .stroke(egui::Stroke::new(1.0, theme::BORDER))
                    .inner_margin(egui::Margin {
                        left: 16,
                        right: 16,
                        top: 0,
                        bottom: 0,
                    })
                    .show(ui, |ui| {
                        ui.set_height(44.0);
                        ui.horizontal_centered(|ui| {
                            let tabs = [
                                "Dashboard",
                                "Trading",
                                "Sessions",
                                "Chart",
                                "Performance",
                                "Backtest",
                                "Payout",
                                "Oracle",
                                "Config",
                                "Logs",
                            ];
                            let (needs_setup, is_config_idx_first_run) = {
                                let g = self.state.lock().unwrap();
                                let needs = matches!(
                                    g.connection_status,
                                    ConnectionStatus::Disconnected
                                ) || g.selected_firm.is_none();
                                (needs, needs)
                            };
                            for (i, label) in tabs.iter().enumerate() {
                                let selected = self.selected_tab == i;
                                let is_config = *label == "Config";
                                let nudge = is_config && is_config_idx_first_run;
                                let display: String = if nudge {
                                    format!("▶ {label}")
                                } else {
                                    label.to_string()
                                };
                                let fg = if selected {
                                    theme::ACCENT
                                } else if nudge {
                                    theme::FORGE_GOLD
                                } else {
                                    theme::MUTED
                                };
                                let btn = ui.add(
                                    egui::Button::new(
                                        RichText::new(display).size(13.5).color(fg),
                                    )
                                    .fill(if selected {
                                        egui::Color32::from_rgb(18, 28, 38)
                                    } else {
                                        egui::Color32::TRANSPARENT
                                    })
                                    .stroke(if selected {
                                        egui::Stroke::new(1.0, theme::ACCENT.linear_multiply(0.5))
                                    } else {
                                        egui::Stroke::NONE
                                    })
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
                            let _ = needs_setup; // reserved for future dashboard hook
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
                                let time_secs = ui.ctx().input(|i| i.time) as f32;
                                match self.selected_tab {
                                    0 => {
                                        mascot::show(ui, &self.mascot, time_secs);
                                        ui.add_space(12.0);
                                        let needs_setup = {
                                            let g = state.lock().unwrap();
                                            matches!(
                                                g.connection_status,
                                                ConnectionStatus::Disconnected
                                            ) || g.selected_firm.is_none()
                                        };
                                        if needs_setup {
                                            let mut jump_to_config = false;
                                            theme::card().show(ui, |ui| {
                                                ui.label(
                                                    RichText::new("Finish setup first")
                                                        .color(theme::FORGE_GOLD)
                                                        .strong()
                                                        .size(15.0),
                                                );
                                                ui.add_space(4.0);
                                                ui.label(
                                                    RichText::new(
                                                        "Connect a broker and pick a firm profile in \
                                                         Config before you can start the trading \
                                                         daemon.",
                                                    )
                                                    .color(theme::TEXT)
                                                    .size(12.5),
                                                );
                                                ui.add_space(8.0);
                                                if ui
                                                    .add(
                                                        egui::Button::new(
                                                            RichText::new("Open Config")
                                                                .color(egui::Color32::WHITE)
                                                                .strong(),
                                                        )
                                                        .fill(theme::FORGE_GOLD_DIM),
                                                    )
                                                    .clicked()
                                                {
                                                    jump_to_config = true;
                                                }
                                            });
                                            if jump_to_config {
                                                self.selected_tab = CONFIG_TAB_IDX;
                                            }
                                            ui.add_space(12.0);
                                        }
                                        DashboardPanel::show(ui, &state);
                                    }
                                    1 => {
                                        self.trading_panel.show(ui, &state);
                                        if let Some(idx) = self.trading_panel.request_tab.take() {
                                            self.selected_tab = idx;
                                        }
                                    }
                                    2 => SessionsPanel::show(ui, &state),
                                    3 => PriceChartPanel::show(ui, &state),
                                    4 => self.performance_panel.show(ui, &state),
                                    5 => self.backtest_panel.show(ui, &state),
                                    6 => self.payout_panel.show(ui, &state),
                                    7 => self.oracle_panel.show(
                                        ui,
                                        &mut self.oracle_cfg,
                                        &state,
                                        Some(&self.oracle.tx),
                                    ),
                                    8 => self.config_panel.show(ui, &state),
                                    9 => LogsPanel::show(ui, &state),
                                    _ => {}
                                }
                            });
                    });
            });
    }
}

fn show_live_confirm(ctx: &egui::Context, app: &mut GadarahApp) {
    let mut open = true;
    let resp = egui::Window::new("")
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .frame(
            egui::Frame::new()
                .fill(egui::Color32::from_rgb(36, 12, 10))
                .stroke(egui::Stroke::new(2.0, theme::FORGE_CRIMSON))
                .corner_radius(10u8)
                .inner_margin(egui::Margin::same(24)),
        )
        .open(&mut open)
        .show(ctx, |ui| {
            ui.set_max_width(480.0);
            ui.label(
                RichText::new("LIVE TRADING DETECTED")
                    .size(18.0)
                    .color(theme::FORGE_CRIMSON)
                    .strong(),
            );
            ui.add_space(10.0);
            ui.label(
                RichText::new(
                    "The broker connection is reporting a LIVE account. Real money is at risk on every order this session.",
                )
                .color(theme::TEXT)
                .size(13.0),
            );
            ui.add_space(8.0);
            ui.label(
                RichText::new(
                    "Verify your firm profile, daily DD limits, and kill-switch settings before proceeding. The Oracle cannot place trades; every order goes through the risk gate.",
                )
                .color(theme::MUTED)
                .size(12.0),
            );
            ui.add_space(16.0);
            ui.horizontal(|ui| {
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new("I understand — proceed").color(egui::Color32::WHITE),
                        )
                        .fill(theme::FORGE_GOLD_DIM),
                    )
                    .clicked()
                {
                    let mut g = app.state.lock().unwrap();
                    g.live_acknowledged = true;
                    g.add_log(LogLevel::Warn, "User acknowledged live trading mode");
                    drop(g);
                    app.pending_live_confirm = false;
                    // If the Trading tab has a queued LIVE start, kick it off now.
                    if app.trading_panel.is_awaiting_live_confirmation() {
                        app.trading_panel.resume_after_confirm(&app.state);
                    }
                }
                ui.add_space(8.0);
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new("Cancel").color(theme::TEXT),
                        )
                        .fill(egui::Color32::from_rgb(24, 28, 36)),
                    )
                    .clicked()
                {
                    app.pending_live_confirm = false;
                    if app.trading_panel.is_awaiting_live_confirmation() {
                        app.trading_panel.cancel_live_launch();
                    }
                    app.state.lock().unwrap().add_log(
                        LogLevel::Warn,
                        "Live-trading launch cancelled by user.",
                    );
                }
            });
        });
    let _ = resp;
    if !open {
        // User clicked the close-x — treat as implicit acknowledgment so the
        // app is usable, but record that they did not formally confirm.
        let mut g = app.state.lock().unwrap();
        g.live_acknowledged = true;
        g.add_log(
            LogLevel::Warn,
            "Live-mode confirmation dismissed without explicit consent",
        );
        app.pending_live_confirm = false;
    }
}

fn main() -> eframe::Result<()> {
    // Hydrate the process env from .env.* next to the binary (install dir).
    // `gadarah auth` writes these files and the CLI reads them via
    // std::env::var; without this hook the GUI can't see the credentials
    // when a user launches via Start Menu / Launch GADARAH button.
    // `from_filename_override` is a silent no-op when a file is missing.
    for candidate in [".env", ".env.demo", ".env.live"] {
        let _ = dotenvy::from_filename_override(candidate);
    }

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
        v[key]
            .as_str()
            .and_then(|s| Decimal::from_str(s).ok())
            .unwrap_or(Decimal::ZERO)
    };

    s.balance = dec("balance");
    s.equity = dec("equity");
    s.free_margin = dec("free_margin");
    s.daily_pnl = dec("daily_pnl");

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
                p[key]
                    .as_str()
                    .and_then(|s| Decimal::from_str(s).ok())
                    .unwrap_or(Decimal::ZERO)
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

    // Update chart symbol
    if let Some(sym) = v["symbol"].as_str() {
        s.chart_symbol = sym.to_string();
    }

    // Update price bars for the chart
    if let Some(bars) = v["price_bars"].as_array() {
        s.price_bars.clear();
        for b in bars {
            s.price_bars.push(gadarah_gui::state::PriceBar {
                timestamp: b["timestamp"].as_i64().unwrap_or(0),
                open: b["open"].as_f64().unwrap_or(0.0),
                high: b["high"].as_f64().unwrap_or(0.0),
                low: b["low"].as_f64().unwrap_or(0.0),
                close: b["close"].as_f64().unwrap_or(0.0),
                volume: b["volume"].as_u64().unwrap_or(0),
            });
        }
    }

    // Update equity curve
    if let Some(eq_arr) = v["equity_curve"].as_array() {
        s.equity_curve.clear();
        for e in eq_arr {
            let eq = e["equity"]
                .as_str()
                .and_then(|s| Decimal::from_str(s).ok())
                .unwrap_or(Decimal::ZERO);
            s.equity_curve.push(gadarah_gui::state::EquityPoint {
                timestamp: e["timestamp"].as_i64().unwrap_or(0),
                equity: eq,
                balance: s.balance,
            });
        }
    }
}
