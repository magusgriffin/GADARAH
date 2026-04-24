//! Trading control panel — start/stop the live-trading daemon from the GUI.
//!
//! Spawns the sibling `gadarah.exe` binary with the right flags for the
//! selected firm and mode (dry-run, demo execute, live execute). Tails its
//! stdout into a log window and flips `ConnectionStatus` to track child
//! lifecycle. The live-execute path runs through the existing live-trading
//! confirmation modal in `main.rs`.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const DRY_RUN_UNLOCK_SECS: u64 = 30;

use eframe::egui;
use egui::RichText;

use crate::state::{AppState, ConnectionStatus, LogLevel};
use crate::theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TradingMode {
    /// `gadarah live` with no `--execute` and no `--live`. Just simulates.
    DryRun,
    /// `gadarah live --execute` against the demo server.
    DemoExecute,
    /// `gadarah live --execute --live` against the real server. Guarded by
    /// the existing `pending_live_confirm` modal.
    LiveExecute,
}

impl TradingMode {
    fn label(self) -> &'static str {
        match self {
            Self::DryRun => "Dry Run",
            Self::DemoExecute => "Demo (real demo-server orders)",
            Self::LiveExecute => "LIVE (real-money orders)",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TradingStatus {
    Stopped,
    Starting,
    Running,
    Stopping,
}

/// Mirror of the selected mode kept on `SharedState` so the demo banner and
/// live-confirm modal can see it from outside this panel.
pub struct TradingPanel {
    mode: TradingMode,
    status: TradingStatus,
    child: Arc<Mutex<Option<Child>>>,
    log_rx: Option<Receiver<String>>,
    tail: VecDeque<String>,
    /// Set true when the user has asked to start in Live mode; main.rs's
    /// live-confirm modal will flip this back and we'll proceed.
    pending_live_launch: bool,
    /// When the current DryRun child started; used to qualify it for the
    /// preflight unlock (≥30 s runtime before it counts).
    current_run_started_at: Option<Instant>,
    /// True once at least one DryRun session has been started AND run for
    /// DRY_RUN_UNLOCK_SECS this app session. Required to unlock Live mode.
    dry_run_completed_this_session: bool,
    /// Tab index a row's "Fix it" button wants main.rs to navigate to. main.rs
    /// reads and clears per frame.
    pub request_tab: Option<usize>,
    /// One-shot mascot coaching events produced by the Trading flow. main.rs
    /// drains these into the mascot bubble.
    pending_mascot_event: Option<MascotEvent>,
}

/// Coaching events the Trading panel wants the mascot to react to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MascotEvent {
    PreflightComplete,
    LaunchAcknowledged(TradingMode),
    StoppedByUser,
}

/// Computed once per frame and shared across preflight rows + button gating.
struct PreflightState {
    connection_status: ConnectionStatus,
    connected: bool,
    firm_selected: bool,
    risk_acknowledged: bool,
    kill_switch_configured: bool,
    dry_run_completed: bool,
}

impl PreflightState {
    /// Return true when every gate required for `mode` is green. Dry Run
    /// unlocks first (broker + firm), Demo adds risk + kill-switch, Live
    /// additionally requires a completed dry-run this session.
    fn gates_for(&self, mode: TradingMode) -> bool {
        let base = self.connected && self.firm_selected;
        match mode {
            TradingMode::DryRun => base,
            TradingMode::DemoExecute => {
                base && self.risk_acknowledged && self.kill_switch_configured
            }
            TradingMode::LiveExecute => {
                base
                    && self.risk_acknowledged
                    && self.kill_switch_configured
                    && self.dry_run_completed
                    && matches!(self.connection_status, ConnectionStatus::ConnectedLive)
            }
        }
    }
}

impl Default for TradingPanel {
    fn default() -> Self {
        Self {
            mode: TradingMode::DryRun,
            status: TradingStatus::Stopped,
            child: Arc::new(Mutex::new(None)),
            log_rx: None,
            tail: VecDeque::with_capacity(300),
            pending_live_launch: false,
            current_run_started_at: None,
            dry_run_completed_this_session: false,
            request_tab: None,
            pending_mascot_event: None,
        }
    }
}

impl TradingPanel {
    /// Return true when a live run has been requested and the GUI's
    /// live-confirm modal should fire. Cleared once `resume_after_confirm`
    /// or `cancel_live_launch` is called.
    pub fn is_awaiting_live_confirmation(&self) -> bool {
        self.pending_live_launch
    }

    /// User approved the live run — kick off the process.
    pub fn resume_after_confirm(&mut self, app_state: &AppState) {
        self.pending_live_launch = false;
        self.spawn_child(app_state, TradingMode::LiveExecute);
    }

    /// User cancelled — revert mode to demo so the banner doesn't lie.
    pub fn cancel_live_launch(&mut self) {
        self.pending_live_launch = false;
        self.mode = TradingMode::DemoExecute;
    }

    pub fn show(&mut self, ui: &mut egui::Ui, app_state: &AppState) {
        self.pump_events(app_state);

        let preflight = self.preflight_snapshot(app_state);
        let status = preflight.connection_status;

        theme::heading(ui, "Trading");
        ui.label(
            RichText::new(
                "Start and stop the GADARAH live-trading daemon. Dry-run is safe to \
                 leave enabled; Demo and Live send real orders to your broker.",
            )
            .color(theme::MUTED)
            .size(12.5),
        );
        ui.add_space(14.0);

        // ── Preflight readiness checklist ────────────────────────────────────
        self.show_preflight_card(ui, &preflight);
        ui.add_space(12.0);

        // ── Mode selector ────────────────────────────────────────────────────
        theme::card().show(ui, |ui| {
            theme::section_label(ui, "TRADING MODE");
            ui.add_space(6.0);
            let is_running = !matches!(self.status, TradingStatus::Stopped);
            ui.add_enabled_ui(!is_running, |ui| {
                for mode in [
                    TradingMode::DryRun,
                    TradingMode::DemoExecute,
                    TradingMode::LiveExecute,
                ] {
                    let color = match mode {
                        TradingMode::DryRun => theme::MUTED,
                        TradingMode::DemoExecute => theme::YELLOW,
                        TradingMode::LiveExecute => theme::RED,
                    };
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut self.mode, mode, "");
                        ui.label(RichText::new(mode.label()).color(color).strong());
                    });
                }
            });
            // Warn if mode = Live but connected only to a demo account.
            if matches!(self.mode, TradingMode::LiveExecute)
                && matches!(status, ConnectionStatus::ConnectedDemo)
            {
                ui.add_space(6.0);
                ui.label(
                    RichText::new(
                        "⚠ Selected Live mode but the connected account is DEMO. Switch \
                         firm/account in Config → Broker Setup first.",
                    )
                    .color(theme::YELLOW)
                    .size(12.0),
                );
            }
        });

        ui.add_space(12.0);

        // ── Start / Stop controls ────────────────────────────────────────────
        theme::card().show(ui, |ui| {
            theme::section_label(ui, "CONTROLS");
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let running = matches!(self.status, TradingStatus::Running);
                let transitioning = matches!(
                    self.status,
                    TradingStatus::Starting | TradingStatus::Stopping
                );
                let preflight_ok = preflight.gates_for(self.mode);
                let start_label = match self.mode {
                    TradingMode::DryRun => "Start Dry Run",
                    TradingMode::DemoExecute => "Start Demo Trading",
                    TradingMode::LiveExecute => "Start LIVE Trading",
                };
                let start_fill = match self.mode {
                    TradingMode::DryRun => egui::Color32::from_rgb(40, 70, 100),
                    TradingMode::DemoExecute => egui::Color32::from_rgb(95, 75, 20),
                    TradingMode::LiveExecute => egui::Color32::from_rgb(120, 40, 40),
                };
                if ui
                    .add_enabled(
                        !running && !transitioning && preflight_ok,
                        egui::Button::new(
                            RichText::new(start_label)
                                .color(egui::Color32::WHITE)
                                .strong()
                                .size(14.0),
                        )
                        .fill(start_fill)
                        .min_size(egui::vec2(200.0, 42.0)),
                    )
                    .clicked()
                {
                    self.handle_start(app_state);
                }
                ui.add_space(8.0);
                if ui
                    .add_enabled(
                        running || transitioning,
                        egui::Button::new(
                            RichText::new("Stop")
                                .color(egui::Color32::WHITE)
                                .strong()
                                .size(14.0),
                        )
                        .fill(egui::Color32::from_rgb(50, 50, 55))
                        .min_size(egui::vec2(120.0, 42.0)),
                    )
                    .clicked()
                {
                    self.stop_child(app_state);
                }
                ui.add_space(14.0);
                let (dot, label, color) = match self.status {
                    TradingStatus::Stopped => ("○", "Stopped", theme::MUTED),
                    TradingStatus::Starting => ("◌", "Starting…", theme::YELLOW),
                    TradingStatus::Running => ("●", "Running", theme::GREEN),
                    TradingStatus::Stopping => ("◌", "Stopping…", theme::YELLOW),
                };
                ui.label(RichText::new(dot).color(color).size(16.0));
                ui.label(RichText::new(label).color(color).strong());
            });
        });

        ui.add_space(12.0);

        // ── Live child stdout tail ────────────────────────────────────────────
        theme::card().show(ui, |ui| {
            theme::section_label(ui, "DAEMON OUTPUT");
            ui.add_space(6.0);
            if self.tail.is_empty() {
                ui.label(
                    RichText::new("No output yet.")
                        .color(theme::MUTED)
                        .size(12.0),
                );
            } else {
                egui::ScrollArea::vertical()
                    .auto_shrink([false; 2])
                    .stick_to_bottom(true)
                    .max_height(260.0)
                    .show(ui, |ui| {
                        for line in &self.tail {
                            ui.label(
                                RichText::new(line)
                                    .monospace()
                                    .size(11.5)
                                    .color(theme::TEXT),
                            );
                        }
                    });
            }
        });
    }

    /// Snapshot of every gate the preflight card evaluates, so the UI can
    /// render row-by-row statuses without taking and releasing the state lock
    /// for each read.
    fn preflight_snapshot(&self, app_state: &AppState) -> PreflightState {
        let g = app_state.lock().unwrap();
        let connection_status = g.connection_status;
        let connected = matches!(
            connection_status,
            ConnectionStatus::ConnectedDemo | ConnectionStatus::ConnectedLive
        );
        let firm_selected = g.selected_firm.is_some();
        let risk_ack = g.risk_acknowledged;
        let ks_configured = g.config.kill_switch.daily_dd_trigger_pct
            > rust_decimal::Decimal::ZERO
            && g.config.kill_switch.total_dd_trigger_pct > rust_decimal::Decimal::ZERO;
        let dry_run_done = self.dry_run_completed_this_session;
        PreflightState {
            connection_status,
            connected,
            firm_selected,
            risk_acknowledged: risk_ack,
            kill_switch_configured: ks_configured,
            dry_run_completed: dry_run_done,
        }
    }

    fn show_preflight_card(&mut self, ui: &mut egui::Ui, p: &PreflightState) {
        theme::card().show(ui, |ui| {
            theme::section_label(ui, "PREFLIGHT");
            ui.label(
                RichText::new("Gates unlock in order: Dry Run → Demo → LIVE.")
                    .color(theme::MUTED)
                    .size(11.5),
            );
            ui.add_space(8.0);
            self.row(ui, p.connected, "Broker connected", "Open Config → Broker Setup", 8);
            self.row(
                ui,
                p.firm_selected,
                "Firm profile selected",
                "Pick a challenge in Config",
                8,
            );
            self.row(
                ui,
                p.risk_acknowledged,
                "Risk limits acknowledged",
                "Review + acknowledge in Config → Auto-Stop Rules",
                8,
            );
            self.row(
                ui,
                p.kill_switch_configured,
                "Kill switch configured",
                "Set DD triggers in Config",
                8,
            );
            self.row(
                ui,
                p.dry_run_completed,
                "Dry run completed this session (≥30 s)",
                "Start a Dry Run below to unlock LIVE",
                0,
            );

            // Summary of what each mode needs.
            ui.add_space(10.0);
            ui.horizontal_wrapped(|ui| {
                let needed = |label: &str, ok: bool| -> RichText {
                    RichText::new(format!(
                        "{} {label}",
                        if ok { "✓" } else { "○" }
                    ))
                    .color(if ok { theme::GREEN } else { theme::MUTED })
                    .size(11.5)
                };
                ui.label(needed("Dry Run ready", p.gates_for(TradingMode::DryRun)));
                ui.label(RichText::new("·").color(theme::MUTED));
                ui.label(needed("Demo ready", p.gates_for(TradingMode::DemoExecute)));
                ui.label(RichText::new("·").color(theme::MUTED));
                ui.label(needed("Live ready", p.gates_for(TradingMode::LiveExecute)));
            });
        });
    }

    /// One row of the preflight card. `fix_tab_idx` is the main-tab index to
    /// jump to via `request_tab` when the row is red and the user clicks "Fix".
    fn row(
        &mut self,
        ui: &mut egui::Ui,
        ok: bool,
        label: &str,
        fix_hint: &str,
        fix_tab_idx: usize,
    ) {
        ui.horizontal(|ui| {
            let (dot, color) = if ok {
                ("✓", theme::GREEN)
            } else {
                ("✗", theme::RED)
            };
            ui.label(RichText::new(dot).color(color).size(14.0).strong());
            ui.add_space(6.0);
            ui.label(RichText::new(label).color(theme::TEXT).size(13.0));
            if !ok {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .small_button(RichText::new("Fix").color(theme::FORGE_GOLD).size(11.0))
                        .clicked()
                    {
                        self.request_tab = Some(fix_tab_idx);
                    }
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(fix_hint)
                            .color(theme::MUTED)
                            .size(11.0)
                            .italics(),
                    );
                });
            }
        });
    }

    /// Drain the single pending mascot event produced by start/stop actions.
    pub fn take_mascot_event(&mut self) -> Option<MascotEvent> {
        self.pending_mascot_event.take()
    }

    fn handle_start(&mut self, app_state: &AppState) {
        match self.mode {
            TradingMode::LiveExecute => {
                // Defer the actual spawn until the user clears the live-
                // confirmation modal (handled in main.rs).
                self.pending_live_launch = true;
                app_state
                    .lock()
                    .unwrap()
                    .add_log(LogLevel::Warn, "LIVE trading requested — awaiting confirmation.");
            }
            mode => self.spawn_child(app_state, mode),
        }
    }

    fn spawn_child(&mut self, app_state: &AppState, mode: TradingMode) {
        self.status = TradingStatus::Starting;
        let (exe, firm_path, state_file_path) = {
            let g = app_state.lock().unwrap();
            let selected = match g.selected_firm.clone() {
                Some(s) => s,
                None => {
                    drop(g);
                    self.status = TradingStatus::Stopped;
                    app_state
                        .lock()
                        .unwrap()
                        .add_log(LogLevel::Error, "No firm selected.");
                    return;
                }
            };
            let exe = gui_sibling_exe("gadarah");
            let firm_path = PathBuf::from(format!("config/firms/{selected}.toml"));
            let state_file_path = PathBuf::from("state.json");
            (exe, firm_path, state_file_path)
        };

        let mut cmd = Command::new(&exe);
        cmd.arg("live")
            .arg("--firm")
            .arg(&firm_path)
            .arg("--gui-state-file")
            .arg(&state_file_path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if !matches!(mode, TradingMode::DryRun) {
            cmd.arg("--execute");
        }
        if matches!(mode, TradingMode::LiveExecute) {
            cmd.arg("--live");
        }

        let child = match cmd.spawn() {
            Ok(c) => c,
            Err(err) => {
                self.status = TradingStatus::Stopped;
                app_state
                    .lock()
                    .unwrap()
                    .add_log(LogLevel::Error, format!("Spawn {} failed: {err}", exe.display()));
                return;
            }
        };

        let (tx, rx) = channel();
        self.log_rx = Some(rx);

        let mut child = child;
        if let Some(out) = child.stdout.take() {
            let tx = tx.clone();
            thread::spawn(move || {
                for line in BufReader::new(out).lines().map_while(Result::ok) {
                    let _ = tx.send(line);
                }
            });
        }
        if let Some(err_pipe) = child.stderr.take() {
            let tx_err = tx;
            thread::spawn(move || {
                for line in BufReader::new(err_pipe).lines().map_while(Result::ok) {
                    let _ = tx_err.send(format!("! {line}"));
                }
            });
        }
        *self.child.lock().unwrap() = Some(child);
        self.status = TradingStatus::Running;
        self.current_run_started_at = Some(Instant::now());
        self.tail.clear();
        self.pending_mascot_event = Some(MascotEvent::LaunchAcknowledged(mode));
        app_state.lock().unwrap().add_log(
            LogLevel::Info,
            format!(
                "Started gadarah daemon in {} mode (firm={}, state={})",
                mode.label(),
                firm_path.display(),
                state_file_path.display(),
            ),
        );
    }

    fn stop_child(&mut self, app_state: &AppState) {
        self.status = TradingStatus::Stopping;
        let mut guard = self.child.lock().unwrap();
        if let Some(mut child) = guard.take() {
            // `kill()` is SIGKILL on Unix / TerminateProcess on Windows.
            // Clean ctrl-break on Windows requires the job-objects API; for
            // v2.1.3 we accept the abrupt terminate since the daemon
            // flushes on every fill anyway.
            let _ = child.kill();
            let _ = child.wait();
        }
        drop(guard);
        self.mark_run_ended(app_state, true);
    }

    fn mark_run_ended(&mut self, app_state: &AppState, by_user: bool) {
        // If the ended run was a sufficiently-long DryRun, unlock Live.
        if matches!(self.mode, TradingMode::DryRun) {
            if let Some(started) = self.current_run_started_at {
                if started.elapsed() >= Duration::from_secs(DRY_RUN_UNLOCK_SECS)
                    && !self.dry_run_completed_this_session
                {
                    self.dry_run_completed_this_session = true;
                    app_state.lock().unwrap().add_log(
                        LogLevel::Info,
                        format!(
                            "Dry-run unlock satisfied ({} s) — LIVE mode available \
                             once other gates are green.",
                            DRY_RUN_UNLOCK_SECS
                        ),
                    );
                }
            }
        }
        self.current_run_started_at = None;
        self.status = TradingStatus::Stopped;
        if by_user {
            self.pending_mascot_event = Some(MascotEvent::StoppedByUser);
        }
        app_state
            .lock()
            .unwrap()
            .add_log(LogLevel::Info, "Stopped gadarah daemon.");
    }

    fn pump_events(&mut self, app_state: &AppState) {
        if let Some(rx) = self.log_rx.as_ref() {
            loop {
                match rx.try_recv() {
                    Ok(line) => {
                        if self.tail.len() >= 300 {
                            self.tail.pop_front();
                        }
                        self.tail.push_back(line);
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        self.log_rx = None;
                        break;
                    }
                }
            }
        }
        // Did the child exit on its own?
        if matches!(self.status, TradingStatus::Running) {
            let exited_status = {
                let mut guard = self.child.lock().unwrap();
                match guard.as_mut().and_then(|c| c.try_wait().ok()).flatten() {
                    Some(s) => {
                        let _ = guard.take();
                        Some(s)
                    }
                    None => None,
                }
            };
            if let Some(status) = exited_status {
                app_state.lock().unwrap().add_log(
                    LogLevel::Warn,
                    format!("gadarah daemon exited (status {})", status),
                );
                self.mark_run_ended(app_state, false);
            }
        }
    }
}

/// Absolute path to a sibling executable next to `gadarah-gui.exe`. Falls
/// back to bare name so PATH resolution still works when launched from a
/// dev workspace.
fn gui_sibling_exe(name: &str) -> PathBuf {
    let exe_name = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    };
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join(&exe_name)))
        .unwrap_or_else(|| PathBuf::from(&exe_name))
}
