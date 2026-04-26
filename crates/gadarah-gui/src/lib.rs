//! GADARAH GUI - Algorithmic Forex Trading Bot Desktop Interface
//!
//! A native desktop GUI built with egui/eframe for monitoring and controlling
//! the GADARAH algorithmic trading system.

pub mod config;
pub mod first_run;
pub mod notifications;
pub mod oracle;
pub mod single_instance;
pub mod state;
pub mod theme;
#[cfg(windows)]
pub mod tray;
pub mod ui;
pub mod update_check;
pub mod widgets;

pub use config::GadarahConfig;
pub use state::{
    AppState, LogEntry, LogLevel, Position, PriceBar, SharedState, TradeMarker, TradeMarkerKind,
};
