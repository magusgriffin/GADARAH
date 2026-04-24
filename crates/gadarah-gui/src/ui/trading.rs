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

        let (connected, status, selected_firm) = {
            let g = app_state.lock().unwrap();
            (
                matches!(
                    g.connection_status,
                    ConnectionStatus::ConnectedDemo | ConnectionStatus::ConnectedLive
                ),
                g.connection_status,
                g.selected_firm.clone(),
            )
        };

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

        // ── Guardrails ────────────────────────────────────────────────────────
        if !connected {
            theme::card().show(ui, |ui| {
                ui.label(
                    RichText::new("Not connected to a broker")
                        .color(theme::RED)
                        .strong()
                        .size(14.0),
                );
                ui.add_space(4.0);
                ui.label(
                    RichText::new(
                        "Open the Config tab and finish Broker Setup before starting \
                         trading.",
                    )
                    .color(theme::MUTED)
                    .size(12.0),
                );
            });
            return;
        }
        if selected_firm.is_none() {
            theme::card().show(ui, |ui| {
                ui.label(
                    RichText::new("No firm challenge selected")
                        .color(theme::RED)
                        .strong()
                        .size(14.0),
                );
                ui.add_space(4.0);
                ui.label(
                    RichText::new(
                        "Pick a firm profile in the Config tab so the daemon knows which \
                         rule set to enforce.",
                    )
                    .color(theme::MUTED)
                    .size(12.0),
                );
            });
            return;
        }

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
                        !running && !transitioning,
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
        self.tail.clear();
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
        self.status = TradingStatus::Stopped;
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
            let mut guard = self.child.lock().unwrap();
            if let Some(child) = guard.as_mut() {
                if let Ok(Some(status)) = child.try_wait() {
                    let _ = guard.take();
                    self.status = TradingStatus::Stopped;
                    app_state.lock().unwrap().add_log(
                        LogLevel::Warn,
                        format!("gadarah daemon exited (status {})", status),
                    );
                }
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
