//! Mascot layer — a mechanical-dragon watcher that observes the app's
//! subsystems and speaks in Warhammer-gothic cadence.
//!
//! Each head corresponds to one app-layer concern (NOT a strategy head, which
//! is the separate `HeadId` enum in `gadarah-core`). The painter renders a
//! compact hydra-style cluster; each head can pulse, go dim, or emit a
//! speech bubble based on a `MascotMood`.

use eframe::egui::{self, Color32};

pub mod bubble;
pub mod painter;
pub mod voice;

pub use bubble::{speech_bubble, BubbleTone};
pub use voice::{phrase_for, VoiceContext};

/// Five app-layer subsystems the mascot watches. Each gets one head.
///
/// Named to read as ranks in a dark-fantasy order, so mascot speech can
/// address them in-character (e.g. "The Chronicler has recorded…").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MascotSubsystem {
    /// Market feed / tick freshness / broker socket.
    MarketFeed,
    /// Pre-trade gate / kill switch / risk ledger.
    RiskGate,
    /// Profit target, DD limits, days remaining.
    ChallengeClock,
    /// The DeepSeek / remote-LLM oracle.
    Oracle,
    /// Trade journal, logs, performance ledger.
    Chronicler,
}

impl MascotSubsystem {
    pub const ALL: [Self; 5] = [
        Self::MarketFeed,
        Self::RiskGate,
        Self::ChallengeClock,
        Self::Oracle,
        Self::Chronicler,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::MarketFeed => "The Herald",
            Self::RiskGate => "The Warden",
            Self::ChallengeClock => "The Reckoner",
            Self::Oracle => "The Oracle",
            Self::Chronicler => "The Chronicler",
        }
    }

    /// Single glyph drawn on the head face.
    pub fn glyph(self) -> &'static str {
        match self {
            Self::MarketFeed => "♆",
            Self::RiskGate => "†",
            Self::ChallengeClock => "⌛",
            Self::Oracle => "✴",
            Self::Chronicler => "§",
        }
    }

    /// Per-head accent color.
    pub fn color(self) -> Color32 {
        match self {
            Self::MarketFeed => Color32::from_rgb(120, 180, 255),
            Self::RiskGate => Color32::from_rgb(230, 80, 80),
            Self::ChallengeClock => Color32::from_rgb(230, 180, 70),
            Self::Oracle => Color32::from_rgb(170, 120, 230),
            Self::Chronicler => Color32::from_rgb(120, 220, 170),
        }
    }
}

/// Mood per head. Drives pulse amplitude and bubble tone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MascotMood {
    Calm,
    Watchful,
    Warning,
    Alarmed,
    Dim,
}

impl MascotMood {
    /// Base → (pulse amp, tint multiplier).
    pub fn amp_tint(self) -> (f32, f32) {
        match self {
            Self::Calm => (0.05, 1.00),
            Self::Watchful => (0.12, 1.05),
            Self::Warning => (0.22, 1.15),
            Self::Alarmed => (0.35, 1.25),
            Self::Dim => (0.00, 0.55),
        }
    }
}

/// Everything the painter needs — shielded from the rest of the app.
#[derive(Debug, Clone)]
pub struct MascotState {
    pub moods: [(MascotSubsystem, MascotMood); 5],
    /// Optional speech bubble pointing at one head.
    pub bubble: Option<(MascotSubsystem, String, BubbleTone)>,
}

impl Default for MascotState {
    fn default() -> Self {
        Self {
            moods: MascotSubsystem::ALL.map(|s| (s, MascotMood::Calm)),
            bubble: None,
        }
    }
}

impl MascotState {
    pub fn mood_of(&self, head: MascotSubsystem) -> MascotMood {
        self.moods
            .iter()
            .find(|(s, _)| *s == head)
            .map(|(_, m)| *m)
            .unwrap_or(MascotMood::Calm)
    }

    pub fn set_mood(&mut self, head: MascotSubsystem, mood: MascotMood) {
        for (s, m) in self.moods.iter_mut() {
            if *s == head {
                *m = mood;
                return;
            }
        }
    }
}

/// Top-level entry: paint the dragon and any active speech bubble.
pub fn show(ui: &mut egui::Ui, state: &MascotState, time_secs: f32) {
    painter::paint(ui, state, time_secs);
}
