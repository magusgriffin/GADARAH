//! UI module for GADARAH GUI - contains all tab panels and components

pub mod dashboard;
pub mod price_chart;
pub mod performance;
pub mod backtest;
pub mod payout;
pub mod config_tab;
pub mod logs;

pub use dashboard::DashboardPanel;
pub use price_chart::PriceChartPanel;
pub use performance::PerformancePanel;
pub use backtest::BacktestPanel;
pub use payout::PayoutPanel;
pub use config_tab::ConfigPanel;
pub use logs::LogsPanel;
