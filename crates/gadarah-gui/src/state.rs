//! Application state management for GADARAH GUI

use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use gadarah_core::{Direction, HeadId, RegimeSignal9};

use crate::config::{FirmConfig, GadarahConfig};

/// RFC-4180 field escape: wrap in quotes (with interior `"` doubled) iff the
/// field contains a comma, quote, or line break; otherwise emit unchanged.
fn csv_escape(field: &str) -> String {
    let needs_quote = field.contains(['"', ',', '\n', '\r']);
    if needs_quote {
        let escaped = field.replace('"', "\"\"");
        format!("\"{}\"", escaped)
    } else {
        field.to_string()
    }
}

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

/// Alert / toast shown in the status header and the alerts feed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alert {
    pub timestamp: i64,
    pub severity: AlertSeverity,
    pub title: String,
    pub body: String,
    pub dismissed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AlertSeverity {
    Info,
    Warning,
    Danger,
}

/// Snapshot of the daily-PnL engine's state, rendered as a coloured pill.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DayStateView {
    Normal,
    Cruising,
    Protecting,
    Stopped,
}

impl DayStateView {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Normal => "NORMAL",
            Self::Cruising => "CRUISING",
            Self::Protecting => "PROTECTING",
            Self::Stopped => "STOPPED",
        }
    }
}

/// One cell of a symbol×symbol correlation grid.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorrelationCell {
    pub symbol_a: String,
    pub symbol_b: String,
    pub correlation: f64,
    pub sample_size: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CorrelationMatrix {
    pub updated_at: i64,
    pub cells: Vec<CorrelationCell>,
}

/// One entry in the trade journal — richer than `TradeRecord` so the user
/// can post-hoc review every decision path.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalEntry {
    pub trade_id: u64,
    pub opened_at: i64,
    pub closed_at: i64,
    pub symbol: String,
    pub head: HeadId,
    pub direction: Direction,
    pub regime: String,
    pub session: String,
    pub entry_price: Decimal,
    pub exit_price: Decimal,
    pub lots: Decimal,
    pub pnl: Decimal,
    pub r_multiple: Decimal,
    pub slippage_pips: Decimal,
    pub entry_reason: String,
    pub exit_reason: String,
    pub posterior_p: Option<f64>,
    pub user_tag: Option<String>,
    pub user_note: Option<String>,
}

/// Gate-rejection event captured for debugging and audit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateReject {
    pub timestamp: i64,
    pub symbol: String,
    pub head: HeadId,
    pub reason: String,
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

/// Broker socket / auth status.
///
/// This is deliberately *not* a liveness signal for the market feed — a
/// healthy `ConnectedLive` can still be stale if ticks stop flowing.  Pair
/// with `SharedState::stale_ms` (freshness of the last received tick) via
/// `SharedState::feed_healthy()` when rendering connection indicators.
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

    // Operational surfacing (Workstream C2)
    pub alerts: VecDeque<Alert>,
    /// Milliseconds since the last market tick — feed freshness, independent
    /// of socket state.  `feed_healthy()` combines this with
    /// `connection_status`.
    pub stale_ms: u64,
    pub correlation_matrix: Option<CorrelationMatrix>,
    pub trade_journal: VecDeque<JournalEntry>,
    pub daily_state: DayStateView,
    pub gate_rejections: VecDeque<GateReject>,
    /// True once the user has confirmed awareness of live trading for this
    /// session. Cleared when the app restarts — the confirmation modal must
    /// fire again every run.
    pub live_acknowledged: bool,
}

const MAX_ALERTS: usize = 100;
const MAX_JOURNAL: usize = 1000;
const MAX_GATE_REJECTIONS: usize = 50;

impl Default for SharedState {
    fn default() -> Self {
        Self {
            // Account
            balance: dec!(10000.0),
            equity: dec!(10000.0),
            free_margin: dec!(10000.0),
            daily_pnl: Decimal::ZERO,
            daily_pnl_pct: Decimal::ZERO,
            total_pnl: Decimal::ZERO,
            total_pnl_pct: Decimal::ZERO,
            starting_balance: dec!(10000.0),
            // Positions
            positions: Vec::new(),
            // Regime tracking
            regime_by_symbol: std::collections::HashMap::new(),
            active_heads: Vec::new(),
            // Kill switch
            kill_switch_active: false,
            kill_switch_reason: None,
            kill_switch_cooldown: None,
            // Configuration
            config: GadarahConfig::default(),
            selected_firm: None,
            firm_config: None,
            available_firms: Vec::new(),
            // Connection
            connection_status: ConnectionStatus::Disconnected,
            // Logs
            logs: VecDeque::new(),
            log_filter: LogLevel::Info,
            // Backtest
            last_backtest: None,
            backtest_running: false,
            // Performance history
            equity_curve: Vec::new(),
            trade_history: Vec::new(),
            // Price chart
            price_bars: Vec::new(),
            chart_symbol: String::new(),
            trade_markers: Vec::new(),
            // Stats
            total_trades: 0,
            win_rate: Decimal::ZERO,
            profit_factor: Decimal::ZERO,
            max_drawdown_pct: Decimal::ZERO,
            sharpe_ratio: Decimal::ZERO,
            expectancy_r: Decimal::ZERO,
            // Operational surfacing (Workstream C2)
            alerts: VecDeque::new(),
            stale_ms: 0,
            correlation_matrix: None,
            trade_journal: VecDeque::new(),
            daily_state: DayStateView::Normal,
            gate_rejections: VecDeque::new(),
            live_acknowledged: false,
        }
    }
}

// ── Logs ──────────────────────────────────────────────────────────────────────
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
}

// ── Performance history & price chart ─────────────────────────────────────────
impl SharedState {
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
}

// ── Operational surfacing (C2): alerts, journal, gate rejections ──────────────
impl SharedState {
    /// True only when the broker is connected *and* the feed is fresh.  Use
    /// this as the single source of truth for the top-bar health indicator;
    /// `stale_ms` and `connection_status` alone each miss half the failure
    /// modes (a live socket with no ticks, or a reconnect mid-flight).
    pub fn feed_healthy(&self) -> bool {
        matches!(
            self.connection_status,
            ConnectionStatus::ConnectedDemo | ConnectionStatus::ConnectedLive
        ) && self.stale_ms < 500
    }

    /// Push an alert. Older alerts are dropped once the cap is reached.
    pub fn push_alert(&mut self, alert: Alert) {
        self.alerts.push_back(alert);
        while self.alerts.len() > MAX_ALERTS {
            self.alerts.pop_front();
        }
    }

    /// Mark an alert as dismissed (index into the current alerts deque).
    pub fn dismiss_alert(&mut self, index: usize) {
        if let Some(alert) = self.alerts.get_mut(index) {
            alert.dismissed = true;
        }
    }

    /// Count of unread (non-dismissed) alerts.
    pub fn unread_alerts(&self) -> usize {
        self.alerts.iter().filter(|a| !a.dismissed).count()
    }

    /// Push a journal entry, capped.
    pub fn push_journal(&mut self, entry: JournalEntry) {
        self.trade_journal.push_back(entry);
        while self.trade_journal.len() > MAX_JOURNAL {
            self.trade_journal.pop_front();
        }
    }

    /// Push a gate-rejection event, capped.
    pub fn push_gate_rejection(&mut self, reject: GateReject) {
        self.gate_rejections.push_back(reject);
        while self.gate_rejections.len() > MAX_GATE_REJECTIONS {
            self.gate_rejections.pop_front();
        }
    }

    /// Export the trade journal as a CSV string (header + rows).
    ///
    /// Fields follow RFC-4180: any value containing `,`, `"`, `\n`, or `\r` is
    /// wrapped in quotes with interior quotes doubled.  Importers round-trip
    /// the original text — notes like `"hit 1,234 level"` survive intact.
    pub fn journal_csv(&self) -> String {
        let mut out = String::from(
            "trade_id,opened_at,closed_at,symbol,head,direction,regime,session,\
             entry,exit,lots,pnl,r_mult,slippage_pips,entry_reason,exit_reason,\
             posterior_p,tag,note\n",
        );
        for e in &self.trade_journal {
            let dir = match e.direction {
                Direction::Buy => "BUY",
                Direction::Sell => "SELL",
            };
            let posterior = e
                .posterior_p
                .map(|p| format!("{:.4}", p))
                .unwrap_or_default();
            let row: [String; 19] = [
                e.trade_id.to_string(),
                e.opened_at.to_string(),
                e.closed_at.to_string(),
                csv_escape(&e.symbol),
                csv_escape(&format!("{:?}", e.head)),
                dir.to_string(),
                csv_escape(&e.regime),
                csv_escape(&e.session),
                e.entry_price.to_string(),
                e.exit_price.to_string(),
                e.lots.to_string(),
                e.pnl.to_string(),
                e.r_multiple.to_string(),
                e.slippage_pips.to_string(),
                csv_escape(&e.entry_reason),
                csv_escape(&e.exit_reason),
                posterior,
                csv_escape(e.user_tag.as_deref().unwrap_or("")),
                csv_escape(e.user_note.as_deref().unwrap_or("")),
            ];
            out.push_str(&row.join(","));
            out.push('\n');
        }
        out
    }
}

// ── Aggregate stats derived from trade + equity history ───────────────────────
impl SharedState {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn alert(severity: AlertSeverity, body: &str) -> Alert {
        Alert {
            timestamp: 0,
            severity,
            title: "t".into(),
            body: body.into(),
            dismissed: false,
        }
    }

    #[test]
    fn alerts_capped_at_max() {
        let mut s = SharedState::default();
        for i in 0..(MAX_ALERTS + 5) {
            s.push_alert(alert(AlertSeverity::Info, &i.to_string()));
        }
        assert_eq!(s.alerts.len(), MAX_ALERTS);
        // Earliest entries dropped
        assert_eq!(s.alerts.front().unwrap().body, "5");
    }

    #[test]
    fn dismiss_alert_sets_flag() {
        let mut s = SharedState::default();
        s.push_alert(alert(AlertSeverity::Danger, "x"));
        s.dismiss_alert(0);
        assert!(s.alerts[0].dismissed);
        assert_eq!(s.unread_alerts(), 0);
    }

    #[test]
    fn gate_rejections_capped() {
        let mut s = SharedState::default();
        for i in 0..(MAX_GATE_REJECTIONS + 3) {
            s.push_gate_rejection(GateReject {
                timestamp: i as i64,
                symbol: "EURUSD".into(),
                head: HeadId::Momentum,
                reason: "r".into(),
            });
        }
        assert_eq!(s.gate_rejections.len(), MAX_GATE_REJECTIONS);
    }

    #[test]
    fn journal_csv_emits_header_and_rows() {
        let mut s = SharedState::default();
        s.push_journal(JournalEntry {
            trade_id: 1,
            opened_at: 100,
            closed_at: 200,
            symbol: "EURUSD".into(),
            head: HeadId::Momentum,
            direction: Direction::Buy,
            regime: "StrongTrendUp".into(),
            session: "London".into(),
            entry_price: dec!(1.1000),
            exit_price: dec!(1.1050),
            lots: dec!(0.10),
            pnl: dec!(50),
            r_multiple: dec!(1.5),
            slippage_pips: dec!(0.2),
            entry_reason: "test".into(),
            exit_reason: "tp".into(),
            posterior_p: Some(0.65),
            user_tag: None,
            user_note: None,
        });
        let csv = s.journal_csv();
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].starts_with("trade_id,"));
        assert!(lines[1].contains("EURUSD"));
        assert!(lines[1].contains("BUY"));
    }

    #[test]
    fn day_state_labels_are_human_readable() {
        assert_eq!(DayStateView::Normal.label(), "NORMAL");
        assert_eq!(DayStateView::Stopped.label(), "STOPPED");
    }

    #[test]
    fn feed_healthy_requires_both_connection_and_freshness() {
        let mut s = SharedState::default();
        // Default: Disconnected → unhealthy regardless of stale_ms.
        assert!(!s.feed_healthy());
        s.stale_ms = 0;
        assert!(!s.feed_healthy());
        // Connected + fresh → healthy.
        s.connection_status = ConnectionStatus::ConnectedLive;
        s.stale_ms = 100;
        assert!(s.feed_healthy());
        // Connected but stale → unhealthy.
        s.stale_ms = 1500;
        assert!(!s.feed_healthy());
        // Reconnecting → unhealthy even with a stale reading of 0.
        s.connection_status = ConnectionStatus::Connecting;
        s.stale_ms = 0;
        assert!(!s.feed_healthy());
    }

    #[test]
    fn csv_escape_handles_commas_quotes_and_newlines() {
        assert_eq!(csv_escape("plain"), "plain");
        assert_eq!(csv_escape("has,comma"), "\"has,comma\"");
        assert_eq!(csv_escape("has\"quote"), "\"has\"\"quote\"");
        assert_eq!(csv_escape("line1\nline2"), "\"line1\nline2\"");
    }

    #[test]
    fn journal_csv_preserves_commas_in_notes() {
        let mut s = SharedState::default();
        s.push_journal(JournalEntry {
            trade_id: 7,
            opened_at: 0,
            closed_at: 0,
            symbol: "EURUSD".into(),
            head: HeadId::Momentum,
            direction: Direction::Sell,
            regime: "ChoppyHiVol".into(),
            session: "NY".into(),
            entry_price: dec!(1.1000),
            exit_price: dec!(1.0900),
            lots: dec!(0.05),
            pnl: dec!(-50),
            r_multiple: dec!(-1),
            slippage_pips: dec!(0),
            entry_reason: "broke 1,234 level".into(),
            exit_reason: "stop".into(),
            posterior_p: None,
            user_tag: None,
            user_note: Some("line1\nline2, with comma".into()),
        });
        let csv = s.journal_csv();
        // The note with a newline stays one CSV row (newline is inside quotes).
        assert!(csv.contains("\"broke 1,234 level\""));
        assert!(csv.contains("\"line1\nline2, with comma\""));
    }
}
