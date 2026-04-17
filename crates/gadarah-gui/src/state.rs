//! Application state management for GADARAH GUI

use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use gadarah_core::{Direction, HeadId, RegimeSignal9};

use crate::config::{FirmConfig, GadarahConfig};

/// Maximum number of log entries to keep in memory
const MAX_LOG_ENTRIES: usize = 1000;

/// Maximum number of equity curve points to keep
const MAX_EQUITY_POINTS: usize = 5000;

/// Maximum number of price bars to keep for the chart
const MAX_PRICE_BARS: usize = 500;

/// OHLC price bar for the chart display
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceBar {
    pub timestamp: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: u64,
}

/// Trade marker to overlay on the price chart
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeMarker {
    pub timestamp: i64,
    pub price: f64,
    pub direction: Direction,
    pub kind: TradeMarkerKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TradeMarkerKind {
    Entry,
    TakeProfit,
    StopLoss,
}

/// Active trading position
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position {
    pub id: u64,
    pub symbol: String,
    pub direction: Direction,
    pub lots: Decimal,
    pub entry_price: Decimal,
    pub current_price: Decimal,
    pub unrealized_pnl: Decimal,
    pub stop_loss: Option<Decimal>,
    pub take_profit: Option<Decimal>,
    pub opened_at: i64,
}

/// Trade history record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeRecord {
    pub id: u64,
    pub timestamp: i64,
    pub symbol: String,
    pub head: HeadId,
    pub direction: Direction,
    pub entry_price: Decimal,
    pub exit_price: Decimal,
    pub lots: Decimal,
    pub pnl: Decimal,
    pub r_multiple: Decimal,
    pub close_reason: String,
}

/// Equity curve point
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EquityPoint {
    pub timestamp: i64,
    pub equity: Decimal,
    pub balance: Decimal,
}

/// Backtest result summary
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BacktestResult {
    pub running: bool,
    pub total_trades: u32,
    pub winning_trades: u32,
    pub losing_trades: u32,
    pub win_rate: Decimal,
    pub total_pnl: Decimal,
    pub profit_factor: Decimal,
    pub max_drawdown_pct: Decimal,
    pub sharpe_ratio: Decimal,
    pub expectancy_r: Decimal,
    pub equity_curve: Vec<EquityPoint>,
    pub trades: Vec<TradeRecord>,
}

/// Log entry with level
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub timestamp: i64,
    pub level: LogLevel,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            LogLevel::Trace => "TRACE",
            LogLevel::Debug => "DEBUG",
            LogLevel::Info => "INFO",
            LogLevel::Warn => "WARN",
            LogLevel::Error => "ERROR",
        }
    }
}

/// Broker connection status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConnectionStatus {
    Disconnected,
    Connecting,
    ConnectedDemo,
    ConnectedLive,
}

/// Shared application state
#[derive(Debug, Clone)]
pub struct SharedState {
    // Account
    pub balance: Decimal,
    pub equity: Decimal,
    pub free_margin: Decimal,
    pub daily_pnl: Decimal,
    pub daily_pnl_pct: Decimal,
    pub total_pnl: Decimal,
    pub total_pnl_pct: Decimal,
    pub starting_balance: Decimal,

    // Positions
    pub positions: Vec<Position>,

    // Regime tracking per symbol
    pub regime_by_symbol: std::collections::HashMap<String, RegimeSignal9>,
    pub active_heads: Vec<HeadId>,

    // Kill switch
    pub kill_switch_active: bool,
    pub kill_switch_reason: Option<String>,
    pub kill_switch_cooldown: Option<i64>,

    // Configuration
    pub config: GadarahConfig,
    pub selected_firm: Option<String>,
    pub firm_config: Option<FirmConfig>,
    pub available_firms: Vec<String>,

    // Connection
    pub connection_status: ConnectionStatus,

    // Logs
    pub logs: VecDeque<LogEntry>,
    pub log_filter: LogLevel,

    // Backtest
    pub last_backtest: Option<BacktestResult>,
    pub backtest_running: bool,

    // Performance history
    pub equity_curve: Vec<EquityPoint>,
    pub trade_history: Vec<TradeRecord>,

    // Price chart
    pub price_bars: Vec<PriceBar>,
    pub chart_symbol: String,
    pub trade_markers: Vec<TradeMarker>,

    // Stats
    pub total_trades: u32,
    pub win_rate: Decimal,
    pub profit_factor: Decimal,
    pub max_drawdown_pct: Decimal,
    pub sharpe_ratio: Decimal,
    pub expectancy_r: Decimal,
}

impl Default for SharedState {
    fn default() -> Self {
        Self {
            balance: dec!(10000.0),
            equity: dec!(10000.0),
            free_margin: dec!(10000.0),
            daily_pnl: Decimal::ZERO,
            daily_pnl_pct: Decimal::ZERO,
            total_pnl: Decimal::ZERO,
            total_pnl_pct: Decimal::ZERO,
            starting_balance: dec!(10000.0),
            positions: Vec::new(),
            regime_by_symbol: std::collections::HashMap::new(),
            active_heads: Vec::new(),
            kill_switch_active: false,
            kill_switch_reason: None,
            kill_switch_cooldown: None,
            config: GadarahConfig::default(),
            selected_firm: None,
            firm_config: None,
            available_firms: Vec::new(),
            connection_status: ConnectionStatus::Disconnected,
            logs: VecDeque::new(),
            log_filter: LogLevel::Info,
            last_backtest: None,
            backtest_running: false,
            equity_curve: Vec::new(),
            trade_history: Vec::new(),
            price_bars: Vec::new(),
            chart_symbol: String::new(),
            trade_markers: Vec::new(),
            total_trades: 0,
            win_rate: Decimal::ZERO,
            profit_factor: Decimal::ZERO,
            max_drawdown_pct: Decimal::ZERO,
            sharpe_ratio: Decimal::ZERO,
            expectancy_r: Decimal::ZERO,
        }
    }
}

impl SharedState {
    /// Add a log entry
    pub fn add_log(&mut self, level: LogLevel, message: impl Into<String>) {
        let entry = LogEntry {
            timestamp: chrono::Utc::now().timestamp(),
            level,
            message: message.into(),
        };
        self.logs.push_back(entry);
        while self.logs.len() > MAX_LOG_ENTRIES {
            self.logs.pop_front();
        }
    }

    /// Get filtered logs
    pub fn get_filtered_logs(&self) -> Vec<&LogEntry> {
        self.logs
            .iter()
            .filter(|l| l.level as u8 >= self.log_filter as u8)
            .collect()
    }

    /// Update equity curve point
    pub fn add_equity_point(&mut self, timestamp: i64, equity: Decimal) {
        self.equity_curve.push(EquityPoint {
            timestamp,
            equity,
            balance: self.balance,
        });
        while self.equity_curve.len() > MAX_EQUITY_POINTS {
            self.equity_curve.remove(0);
        }
    }

    /// Add a price bar, keeping the buffer bounded
    pub fn add_price_bar(&mut self, bar: PriceBar) {
        self.price_bars.push(bar);
        while self.price_bars.len() > MAX_PRICE_BARS {
            self.price_bars.remove(0);
        }
    }

    /// Update stats from trade history
    pub fn update_stats(&mut self) {
        if self.trade_history.is_empty() {
            return;
        }

        self.total_trades = self.trade_history.len() as u32;

        let wins: u32 = self
            .trade_history
            .iter()
            .filter(|t| t.pnl > Decimal::ZERO)
            .count() as u32;
        let _losses: u32 = self
            .trade_history
            .iter()
            .filter(|t| t.pnl < Decimal::ZERO)
            .count() as u32;

        self.win_rate = if self.total_trades > 0 {
            Decimal::from(wins) / Decimal::from(self.total_trades) * dec!(100)
        } else {
            Decimal::ZERO
        };

        let total_wins: Decimal = self
            .trade_history
            .iter()
            .filter(|t| t.pnl > Decimal::ZERO)
            .map(|t| t.pnl)
            .sum();
        let total_losses: Decimal = self
            .trade_history
            .iter()
            .filter(|t| t.pnl < Decimal::ZERO)
            .map(|t| t.pnl.abs())
            .sum();

        self.profit_factor = if total_losses > Decimal::ZERO {
            total_wins / total_losses
        } else if total_wins > Decimal::ZERO {
            dec!(999.99)
        } else {
            Decimal::ZERO
        };

        // Calculate expectancy in R
        let avg_r: Decimal = if !self.trade_history.is_empty() {
            self.trade_history
                .iter()
                .map(|t| t.r_multiple)
                .sum::<Decimal>()
                / Decimal::from(self.total_trades)
        } else {
            Decimal::ZERO
        };
        self.expectancy_r = avg_r;

        // Calculate max drawdown
        self.max_drawdown_pct = self.calculate_max_drawdown();

        // Sharpe (simplified)
        self.sharpe_ratio = self.calculate_sharpe();
    }

    fn calculate_max_drawdown(&self) -> Decimal {
        if self.equity_curve.is_empty() {
            return Decimal::ZERO;
        }

        let mut peak = self.equity_curve[0].equity;
        let mut max_dd = Decimal::ZERO;

        for point in &self.equity_curve {
            if point.equity > peak {
                peak = point.equity;
            }
            let dd = (peak - point.equity) / peak * dec!(100);
            if dd > max_dd {
                max_dd = dd;
            }
        }

        max_dd
    }

    fn calculate_sharpe(&self) -> Decimal {
        if self.equity_curve.len() < 2 {
            return Decimal::ZERO;
        }

        let returns: Vec<f64> = self
            .equity_curve
            .windows(2)
            .filter_map(|w| {
                let prev = w[0].equity.to_string().parse::<f64>().ok()?;
                let curr = w[1].equity.to_string().parse::<f64>().ok()?;
                if prev == 0.0 {
                    None
                } else {
                    Some((curr - prev) / prev)
                }
            })
            .collect();

        if returns.is_empty() {
            return Decimal::ZERO;
        }

        let n = returns.len() as f64;
        let mean: f64 = returns.iter().sum::<f64>() / n;
        let variance: f64 = returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / n;
        let std_dev = variance.sqrt();

        if std_dev == 0.0 {
            return Decimal::ZERO;
        }

        // Annualized Sharpe (assuming daily data)
        let sharpe = (mean / std_dev) * 252.0_f64.sqrt();
        // Convert to Decimal
        Decimal::try_from(sharpe).unwrap_or(Decimal::ZERO)
    }
}

/// Thread-safe shared state wrapper
pub type AppState = Arc<Mutex<SharedState>>;

pub fn create_app_state() -> AppState {
    Arc::new(Mutex::new(SharedState::default()))
}
