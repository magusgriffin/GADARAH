//! Execution Engine with Smart Order Execution
//!
//! Handles spread-adjusted R:R gating, spread spikes, stale price detection,
//! volatility halts, and retry logic for order execution.

use crate::types::RiskDecision;
use gadarah_core::{Direction, TradeSignal};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use tracing::{debug, warn};

/// Configuration for the execution engine
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionConfig {
    /// Maximum spread as ratio of ATR (reject if spread > this * ATR)
    pub max_spread_atr_ratio: Decimal,
    /// Maximum retries on temporary failures
    pub max_retries: u8,
    /// Delay between retries in milliseconds
    pub retry_delay_ms: u64,
    /// Stale price threshold, legacy seconds path used only by the bar-cadence
    /// backtest. Live loop should use `stale_price_threshold_ms`.
    pub stale_price_threshold: i64,
    /// Stale-price HARD threshold in milliseconds (live path). New feeds
    /// must arrive within this window or new entries are blocked.
    pub stale_price_threshold_ms: i64,
    /// Stale-price WARN threshold in milliseconds. Does not block; exposed
    /// so the GUI can flash amber when latency is creeping up.
    pub stale_warning_ms: i64,
    /// Slippage budget in pips
    pub slippage_budget_pips: Decimal,
    /// Minimum spread-adjusted R:R ratio
    pub min_rr_after_spread: Decimal,
    /// Reject if spread > typical * this multiplier
    pub spread_spike_mult: Decimal,
    /// Minimum ATR samples before the statistical volatility halt engages.
    pub vol_halt_min_samples: usize,
    /// Standard-deviation multiple above the ATR mean that trips the
    /// statistical volatility halt. 3.0 ≈ 99.7% tail under normal assumption.
    pub vol_halt_sigma_mult: Decimal,
    /// Cooldown in milliseconds after any vol halt trips before trading can
    /// resume automatically.
    pub vol_halt_cooldown_ms: i64,
}

impl Default for ExecutionConfig {
    fn default() -> Self {
        Self {
            max_spread_atr_ratio: dec!(0.30),
            max_retries: 3,
            retry_delay_ms: 500,
            stale_price_threshold: 2,
            stale_price_threshold_ms: 500,
            stale_warning_ms: 200,
            slippage_budget_pips: dec!(1.0),
            min_rr_after_spread: dec!(1.2),
            spread_spike_mult: dec!(2.5),
            vol_halt_min_samples: 30,
            vol_halt_sigma_mult: dec!(3.0),
            vol_halt_cooldown_ms: 60_000,
        }
    }
}

// ---------------------------------------------------------------------------
// Volatility halt tracker (A9)
// ---------------------------------------------------------------------------

/// Reason a volatility halt fired. Surfaced to the caller so audit logs and
/// GUI banners can distinguish a spread spike from a statistical ATR blowout.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VolHaltReason {
    /// Current spread blew past `max_spread_atr_ratio * atr`.
    SpreadAtrRatio { spread: Decimal, atr: Decimal },
    /// ATR crossed `mean + sigma_mult * stddev`.
    AtrSigma {
        atr: Decimal,
        mean: Decimal,
        stddev: Decimal,
    },
}

#[derive(Debug, Clone, Default)]
pub struct VolHaltTracker {
    atr_samples: VecDeque<Decimal>,
    activated_ms: Option<i64>,
    reason: Option<VolHaltReason>,
}

impl VolHaltTracker {
    const MAX_SAMPLES: usize = 200;

    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_atr(&mut self, atr: Decimal) {
        if atr.is_sign_negative() || atr.is_zero() {
            return;
        }
        self.atr_samples.push_back(atr);
        if self.atr_samples.len() > Self::MAX_SAMPLES {
            self.atr_samples.pop_front();
        }
    }

    pub fn is_active(&self, now_ms: i64, cooldown_ms: i64) -> bool {
        match self.activated_ms {
            Some(when) => now_ms - when < cooldown_ms,
            None => false,
        }
    }

    pub fn reason(&self) -> Option<&VolHaltReason> {
        self.reason.as_ref()
    }

    /// Test the current tick against both halt conditions. Returns `Some`
    /// with the trigger when a halt fires; arms the tracker for the
    /// cooldown period.
    pub fn check(
        &mut self,
        current_spread: Decimal,
        current_atr: Decimal,
        now_ms: i64,
        config: &ExecutionConfig,
    ) -> Option<VolHaltReason> {
        if self.is_active(now_ms, config.vol_halt_cooldown_ms) {
            return self.reason.clone();
        }

        // Trigger 1: spread > max_spread_atr_ratio * atr
        if !current_atr.is_zero() && current_spread > config.max_spread_atr_ratio * current_atr {
            let reason = VolHaltReason::SpreadAtrRatio {
                spread: current_spread,
                atr: current_atr,
            };
            self.arm(reason.clone(), now_ms);
            return Some(reason);
        }

        // Trigger 2: statistical ATR blowout
        if self.atr_samples.len() >= config.vol_halt_min_samples {
            let (mean, stddev) = mean_stddev(&self.atr_samples);
            let threshold = mean + config.vol_halt_sigma_mult * stddev;
            if current_atr > threshold {
                let reason = VolHaltReason::AtrSigma {
                    atr: current_atr,
                    mean,
                    stddev,
                };
                self.arm(reason.clone(), now_ms);
                return Some(reason);
            }
        }

        None
    }

    fn arm(&mut self, reason: VolHaltReason, now_ms: i64) {
        self.activated_ms = Some(now_ms);
        self.reason = Some(reason.clone());
        warn!(
            activated_at_ms = now_ms,
            reason = ?reason,
            "volatility halt armed",
        );
    }
}

fn mean_stddev(samples: &VecDeque<Decimal>) -> (Decimal, Decimal) {
    let n = samples.len();
    if n == 0 {
        return (Decimal::ZERO, Decimal::ZERO);
    }
    let count = Decimal::from(n as i64);
    let sum: Decimal = samples.iter().copied().sum();
    let mean = sum / count;
    let variance_sum: Decimal = samples
        .iter()
        .map(|x| {
            let d = *x - mean;
            d * d
        })
        .sum();
    // Population stddev (we're estimating for this window, not a sample of the population).
    let variance = variance_sum / count;
    // rust_decimal has no sqrt; fall back to f64 for the last step. Precision
    // here is fine since downstream compares via >, not equality.
    let stddev_f64 = variance.to_f64_retain().sqrt();
    let stddev = Decimal::try_from(stddev_f64).unwrap_or(Decimal::ZERO);
    (mean, stddev)
}

trait DecimalF64Convert {
    fn to_f64_retain(&self) -> f64;
}

impl DecimalF64Convert for Decimal {
    fn to_f64_retain(&self) -> f64 {
        use rust_decimal::prelude::ToPrimitive;
        self.to_f64().unwrap_or(0.0)
    }
}

/// Spread tracker for monitoring spread spikes
#[derive(Debug, Clone)]
pub struct SpreadTracker {
    history: VecDeque<SpreadSample>,
    session_typical: Decimal,
}

#[derive(Debug, Clone)]
struct SpreadSample {
    spread_pips: Decimal,
    timestamp: i64,
}

impl SpreadTracker {
    pub fn new(typical_spread: Decimal) -> Self {
        Self {
            history: VecDeque::with_capacity(100),
            session_typical: typical_spread,
        }
    }

    /// Record a new spread observation
    pub fn record(&mut self, spread_pips: Decimal, timestamp: i64) {
        self.history.push_back(SpreadSample {
            spread_pips,
            timestamp,
        });
        if self.history.len() > 50 {
            self.history.pop_front();
        }
    }

    /// Get current spread
    pub fn current(&self) -> Decimal {
        self.history
            .back()
            .map(|s| s.spread_pips)
            .unwrap_or(self.session_typical)
    }

    /// Get typical spread for the session
    pub fn session_typical(&self) -> Decimal {
        if self.history.len() < 5 {
            return self.session_typical;
        }
        // Use median of recent spreads
        let mut spreads: Vec<Decimal> = self.history.iter().map(|s| s.spread_pips).collect();
        spreads.sort();
        spreads[spreads.len() / 2]
    }

    /// Check if current spread is a spike
    pub fn is_spike(&self) -> bool {
        let current = self.current();
        current > self.session_typical * dec!(2.0)
    }

    /// Get last update timestamp
    pub fn last_update(&self) -> i64 {
        self.history.back().map(|s| s.timestamp).unwrap_or(0)
    }
}

/// Fill record for logging
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FillRecord {
    pub order_id: i64,
    pub symbol: String,
    pub direction: Direction,
    pub requested_price: Decimal,
    pub fill_price: Decimal,
    pub slippage_pips: Decimal,
    pub filled_at: i64,
    pub retries: u8,
}

/// Execution result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExecutionResult {
    /// Order was filled successfully
    Filled(FillRecord),
    /// Order was deferred for retry
    Deferred { reason: String, retry_at: i64 },
    /// Order was rejected
    Rejected { reason: String },
    /// Order execution failed
    Failed { error: String },
}

/// Execution engine
pub struct ExecutionEngine {
    config: ExecutionConfig,
    spread_tracker: SpreadTracker,
    fill_log: VecDeque<FillRecord>,
    last_tick_time: i64,
    /// Wall-clock timestamp (unix ms) of the last tick we saw. Used for
    /// sub-second staleness detection on the live path. Zero means no tick
    /// has been ingested yet.
    last_tick_ms: i64,
    vol_halt: VolHaltTracker,
}

impl ExecutionEngine {
    pub fn new(config: ExecutionConfig, typical_spread: Decimal) -> Self {
        Self {
            config,
            spread_tracker: SpreadTracker::new(typical_spread),
            fill_log: VecDeque::with_capacity(1000),
            last_tick_time: 0,
            last_tick_ms: 0,
            vol_halt: VolHaltTracker::new(),
        }
    }

    /// Update spread from market data. `timestamp` is unix seconds (matches
    /// broker Tick feed). The engine also stamps a wall-clock ms value so
    /// sub-second staleness can be checked on the live path.
    pub fn update_spread(&mut self, spread_pips: Decimal, timestamp: i64) {
        self.spread_tracker.record(spread_pips, timestamp);
        self.last_tick_time = timestamp;
        self.last_tick_ms = chrono::Utc::now().timestamp_millis();
    }

    /// Feed a fresh ATR reading into the volatility-halt tracker. Callers
    /// compute ATR (from the regime classifier or a dedicated tracker) and
    /// hand it in; the engine does not compute it itself.
    pub fn record_atr(&mut self, atr: Decimal) {
        self.vol_halt.record_atr(atr);
    }

    /// Run both volatility-halt triggers against the current tick. Returns
    /// `Some(reason)` when a halt fires; the caller rejects the signal and
    /// logs the reason. Once armed, the tracker re-returns the same reason
    /// for `vol_halt_cooldown_ms` before re-evaluating.
    pub fn check_vol_halt(&mut self, current_atr: Decimal, now_ms: i64) -> Option<VolHaltReason> {
        let spread = self.spread_tracker.current();
        self.vol_halt.check(spread, current_atr, now_ms, &self.config)
    }

    /// True when the vol-halt cooldown is still holding us down from a
    /// previous trigger.
    pub fn vol_halt_active(&self, now_ms: i64) -> bool {
        self.vol_halt
            .is_active(now_ms, self.config.vol_halt_cooldown_ms)
    }

    pub fn vol_halt_reason(&self) -> Option<&VolHaltReason> {
        self.vol_halt.reason()
    }

    /// Get current spread
    pub fn current_spread(&self) -> Decimal {
        self.spread_tracker.current()
    }

    /// True when the current spread is a spike relative to session typical.
    pub fn is_spread_spike(&self) -> bool {
        self.spread_tracker.is_spike()
    }

    /// Read-only view of the config (lets the gate reach thresholds without
    /// duplicating them).
    pub fn config(&self) -> &ExecutionConfig {
        &self.config
    }

    /// Check if prices are stale (legacy seconds-resolution path, backtest).
    pub fn is_stale(&self, current_time: i64) -> bool {
        if self.last_tick_time == 0 {
            return false; // No data yet
        }
        let max_seconds = (self.config.stale_price_threshold_ms / 1000).max(1);
        current_time - self.last_tick_time > max_seconds.max(self.config.stale_price_threshold)
    }

    /// Milliseconds since the last tick arrived (wall-clock). `0` when no tick
    /// has been seen yet.
    pub fn stale_ms(&self) -> i64 {
        if self.last_tick_ms == 0 {
            return 0;
        }
        (chrono::Utc::now().timestamp_millis() - self.last_tick_ms).max(0)
    }

    /// True when the feed has been silent longer than the hard threshold. The
    /// live loop must reject new orders when this is true.
    pub fn is_stale_ms(&self) -> bool {
        if self.last_tick_ms == 0 {
            return false;
        }
        self.stale_ms() > self.config.stale_price_threshold_ms
    }

    /// True when the feed has been silent longer than the warning threshold
    /// but not long enough to block. Pure signal for the GUI.
    pub fn is_warning_latency(&self) -> bool {
        if self.last_tick_ms == 0 {
            return false;
        }
        let elapsed = self.stale_ms();
        elapsed > self.config.stale_warning_ms && elapsed <= self.config.stale_price_threshold_ms
    }

    /// Calculate spread-adjusted R:R for a signal
    pub fn adjusted_rr(&self, signal: &TradeSignal) -> Option<Decimal> {
        let spread = self.spread_tracker.current();

        let entry = signal.entry;
        let tp = signal.take_profit;
        let sl = signal.stop_loss;

        // Net distances after spread cost
        let tp_distance = (tp - entry).abs() - spread;
        let sl_distance = (sl - entry).abs() + spread;

        if sl_distance.is_zero() {
            return None;
        }

        Some(tp_distance / sl_distance)
    }

    /// Execute a risk decision with spread-adjusted gating
    pub fn execute(&mut self, decision: RiskDecision, current_time: i64) -> ExecutionResult {
        let RiskDecision::Execute {
            signal,
            risk_pct: _,
            lots,
            is_pyramid: _,
            witness: _,
        } = decision
        else {
            return ExecutionResult::Rejected {
                reason: "Risk decision was rejected".into(),
            };
        };

        // Gate 1: Spread spike check
        if self.spread_tracker.is_spike() {
            return ExecutionResult::Deferred {
                reason: "Spread spike detected".into(),
                retry_at: current_time + 30_000,
            };
        }

        // Gate 2: Price freshness
        if self.is_stale(current_time) {
            return ExecutionResult::Rejected {
                reason: "Stale price data".into(),
            };
        }

        // Gate 3: Spread-adjusted R:R
        let adjusted_rr = match self.adjusted_rr(&signal) {
            Some(rr) => rr,
            None => {
                return ExecutionResult::Rejected {
                    reason: "Could not calculate R:R".into(),
                };
            }
        };

        if adjusted_rr < self.config.min_rr_after_spread {
            return ExecutionResult::Rejected {
                reason: format!(
                    "Spread-adjusted R:R too low: {:.2} < {:.2}",
                    adjusted_rr, self.config.min_rr_after_spread
                ),
            };
        }

        // Gate 4: Max spread ATR ratio (if ATR available)
        // Note: In production, would check ATR from market data

        // Execute the order (in production, send to broker)
        let fill_price = signal.entry; // Simplified - would get actual fill price
        let slippage = self.config.slippage_budget_pips; // Estimate

        let fill = FillRecord {
            order_id: rand::random::<i64>(),
            symbol: signal.symbol.clone(),
            direction: signal.direction,
            requested_price: signal.entry,
            fill_price,
            slippage_pips: slippage,
            filled_at: current_time,
            retries: 0,
        };

        self.fill_log.push_back(fill.clone());
        if self.fill_log.len() > 1000 {
            self.fill_log.pop_front();
        }

        debug!(
            "Order filled: {:?} {} {} lots at {} (R:R={:.2})",
            signal.direction, signal.symbol, lots, fill_price, adjusted_rr
        );

        ExecutionResult::Filled(fill)
    }

    /// Get recent fill statistics
    pub fn fill_stats(&self) -> FillStats {
        if self.fill_log.is_empty() {
            return FillStats::default();
        }

        let total = self.fill_log.len();
        let avg_slippage: Decimal = self
            .fill_log
            .iter()
            .map(|f| f.slippage_pips)
            .sum::<Decimal>()
            / Decimal::from(total);

        FillStats {
            total_fills: total,
            avg_slippage_pips: avg_slippage,
            median_slippage_pips: median_slippage_last(&self.fill_log, 50),
        }
    }

    /// Rolling median slippage over the last `n` fills, in pips. Median is
    /// preferred over mean for sizing because a single catastrophic fill
    /// would otherwise drag the estimate and cause chronic underfills on
    /// every subsequent order. Returns the static-default when there are
    /// fewer than 5 recent fills — too noisy below that.
    pub fn rolling_slippage_pips(&self, n: usize, default_pips: Decimal) -> Decimal {
        if self.fill_log.len() < 5 {
            return default_pips;
        }
        median_slippage_last(&self.fill_log, n)
    }

    /// Record an externally-executed fill so live and backtest paths can share
    /// the same slippage/fill telemetry.
    pub fn record_fill(&mut self, fill: FillRecord) {
        self.fill_log.push_back(fill);
        if self.fill_log.len() > 1000 {
            self.fill_log.pop_front();
        }
    }
}

/// Fill statistics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FillStats {
    pub total_fills: usize,
    pub avg_slippage_pips: Decimal,
    /// Median slippage over the last 50 fills. Less sensitive to outlier
    /// flash-crash fills than `avg_slippage_pips`, preferred for sizing.
    #[serde(default)]
    pub median_slippage_pips: Decimal,
}

fn median_slippage_last(log: &VecDeque<FillRecord>, n: usize) -> Decimal {
    let take = log.len().min(n);
    if take == 0 {
        return Decimal::ZERO;
    }
    let mut recent: Vec<Decimal> = log
        .iter()
        .rev()
        .take(take)
        .map(|f| f.slippage_pips)
        .collect();
    recent.sort();
    let mid = recent.len() / 2;
    if recent.len() % 2 == 0 {
        (recent[mid - 1] + recent[mid]) / dec!(2)
    } else {
        recent[mid]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spread_tracker() {
        let mut tracker = SpreadTracker::new(dec!(1.0));

        tracker.record(dec!(0.8), 1000);
        tracker.record(dec!(1.0), 1001);
        tracker.record(dec!(1.2), 1002);

        assert_eq!(tracker.current(), dec!(1.2));
        assert!(!tracker.is_spike());

        tracker.record(dec!(5.0), 1003); // Spike
        assert!(tracker.is_spike());
    }

    #[test]
    fn stale_ms_reports_zero_before_any_tick() {
        let engine = ExecutionEngine::new(ExecutionConfig::default(), dec!(0.0001));
        assert_eq!(engine.stale_ms(), 0);
        assert!(!engine.is_stale_ms());
    }

    #[test]
    fn is_stale_ms_tracks_wall_clock_after_tick() {
        let mut engine = ExecutionEngine::new(ExecutionConfig::default(), dec!(0.0001));
        engine.update_spread(dec!(1.0), 1_000_000);
        // Force last_tick_ms into the past to simulate a stale feed without sleeping.
        engine.last_tick_ms -= 1_000;
        assert!(engine.is_stale_ms());
    }

    #[test]
    fn warning_latency_fires_between_warn_and_hard_thresholds() {
        let mut cfg = ExecutionConfig::default();
        cfg.stale_warning_ms = 200;
        cfg.stale_price_threshold_ms = 500;
        let mut engine = ExecutionEngine::new(cfg, dec!(0.0001));
        engine.update_spread(dec!(1.0), 1_000_000);
        // Simulate 300ms elapsed since last tick.
        engine.last_tick_ms -= 300;
        assert!(engine.is_warning_latency());
        assert!(!engine.is_stale_ms());
    }

    #[test]
    fn vol_halt_fires_on_spread_atr_ratio() {
        let mut engine = ExecutionEngine::new(ExecutionConfig::default(), dec!(0.0001));
        // max_spread_atr_ratio default 0.30. If atr = 1.0, threshold = 0.30.
        engine.update_spread(dec!(0.5), 1_000);
        let fired = engine.check_vol_halt(dec!(1.0), 1_000);
        assert!(matches!(fired, Some(VolHaltReason::SpreadAtrRatio { .. })));
    }

    #[test]
    fn vol_halt_fires_on_atr_sigma() {
        let mut engine = ExecutionEngine::new(ExecutionConfig::default(), dec!(0.0001));
        // Feed 40 tight-ATR samples so mean+3σ is low.
        for _ in 0..40 {
            engine.record_atr(dec!(0.0010));
        }
        // Spread must stay under max_spread_atr_ratio * current_atr = 0.30 * 0.10 = 0.03
        // so only the sigma branch can fire.
        engine.update_spread(dec!(0.01), 1_000);
        let fired = engine.check_vol_halt(dec!(0.1000), 1_000);
        assert!(matches!(fired, Some(VolHaltReason::AtrSigma { .. })));
    }

    #[test]
    fn vol_halt_cooldown_holds_for_window() {
        let mut engine = ExecutionEngine::new(ExecutionConfig::default(), dec!(0.0001));
        engine.update_spread(dec!(0.5), 1_000);
        engine.check_vol_halt(dec!(1.0), 1_000); // arm
        // Still armed 30s later
        assert!(engine.vol_halt_active(31_000));
        // Past the 60s cooldown
        assert!(!engine.vol_halt_active(62_000));
    }

    #[test]
    fn test_adjusted_rr() {
        let config = ExecutionConfig::default();
        let mut engine = ExecutionEngine::new(config, dec!(0.0001));

        // Record some spreads
        engine.update_spread(dec!(0.0001), 1000);

        // Create a signal with 2:1 R:R
        let signal = TradeSignal {
            symbol: "EURUSD".to_string(),
            direction: Direction::Buy,
            kind: gadarah_core::SignalKind::Open,
            entry: dec!(1.1000),
            stop_loss: dec!(1.0950),   // 50 pips risk
            take_profit: dec!(1.1100), // 100 pips reward = 2:1
            take_profit2: None,
            head: gadarah_core::HeadId::Momentum,
            head_confidence: dec!(0.8),
            regime: gadarah_core::Regime9::StrongTrendUp,
            session: gadarah_core::Session::London,
            pyramid_level: 0,
            comment: "test".to_string(),
            generated_at: 1000,
        };

        let rr = engine.adjusted_rr(&signal);
        assert!(rr.is_some());

        // With 1 pip spread: reward = 100-1=99, risk = 50+1=51, rr = 99/51 = 1.94
        let rr = rr.unwrap();
        assert!(rr > dec!(1.5));
    }
}
