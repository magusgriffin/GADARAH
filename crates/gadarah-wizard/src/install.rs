//! Installation driver — stubbed to run a deterministic, cancellable
//! progress machine the wizard can animate against.
//!
//! V1 drives progress from a timer so the UI flow and the Binder animation
//! can be exercised end-to-end without requiring a real MSI payload. A later
//! revision replaces `simulate_tick` with an `msiexec /i ... /passive`
//! invocation that reports progress through `/LV` log parsing.

use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct InstallState {
    pub started_at: Option<Instant>,
    pub finished: bool,
    pub error: Option<String>,
    /// [0.0, 1.0]
    pub progress: f32,
    /// Current step label shown under the progress bar.
    pub current_step: String,
    pub log: Vec<String>,
}

impl Default for InstallState {
    fn default() -> Self {
        Self {
            started_at: None,
            finished: false,
            error: None,
            progress: 0.0,
            current_step: "Not started".to_string(),
            log: Vec::new(),
        }
    }
}

/// Total simulated install time.
const TOTAL_DURATION_SECS: f32 = 9.0;

const STEPS: &[(&str, f32)] = &[
    ("Preparing destination", 0.08),
    ("Writing GADARAH GUI binary", 0.28),
    ("Writing CLI daemon", 0.42),
    ("Seeding config/firms profiles", 0.58),
    ("Registering Start Menu shortcut", 0.72),
    ("Writing uninstaller metadata", 0.88),
    ("Finalising", 1.00),
];

impl InstallState {
    pub fn start(&mut self) {
        if self.started_at.is_some() {
            return;
        }
        self.started_at = Some(Instant::now());
        self.log
            .push("[wizard] installation sequence started".to_string());
    }

    pub fn tick(&mut self) {
        let Some(started) = self.started_at else {
            return;
        };
        if self.finished {
            return;
        }
        let elapsed = started.elapsed().as_secs_f32();
        let p = (elapsed / TOTAL_DURATION_SECS).clamp(0.0, 1.0);
        self.progress = p;
        let step = STEPS
            .iter()
            .find(|(_, threshold)| p <= *threshold)
            .map(|(name, _)| *name)
            .unwrap_or("Finalising");
        if step != self.current_step {
            self.current_step = step.to_string();
            self.log.push(format!("[wizard] {}", step));
        }
        if p >= 1.0 {
            self.finished = true;
            self.log
                .push("[wizard] installation complete.".to_string());
        }
    }

    pub fn eta(&self) -> Option<Duration> {
        let started = self.started_at?;
        if self.progress <= 0.01 {
            return None;
        }
        let elapsed = started.elapsed().as_secs_f32();
        let remaining = (elapsed / self.progress) - elapsed;
        Some(Duration::from_secs_f32(remaining.max(0.0)))
    }
}
