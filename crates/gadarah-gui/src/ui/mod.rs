//! UI module for GADARAH GUI - contains all tab panels and components

pub mod dashboard;
pub mod performance;
pub mod backtest;
pub mod config_tab;
pub mod logs;

pub use dashboard::DashboardPanel;
pub use performance::PerformancePanel;
pub use backtest::BacktestPanel;
pub use config_tab::ConfigPanel;
pub use logs::LogsPanel;
