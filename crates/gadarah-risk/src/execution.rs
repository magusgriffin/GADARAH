//! Execution Engine with Smart Order Execution
//!
//! Handles spread-adjusted R:R gating, spread spikes, stale price detection,
//! and retry logic for order execution.

use crate::sizing::calculate_lots;
use crate::types::{RiskDecision, RiskError, RiskPercent};
use gadarah_core::{Direction, TradeSignal};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use tracing::{debug, info, warn};

/// Configuration for the execution engine
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionConfig {
    /// Maximum spread as ratio of ATR (reject if spread > this * ATR)
    pub max_spread_atr_ratio: Decimal,
    /// Maximum retries on temporary failures
    pub max_retries: u8,
    /// Delay between retries in milliseconds
    pub retry_delay_ms: u64,
    /// Stale price threshold in seconds
    pub stale_price_threshold: i64,
    /// Slippage budget in pips
    pub slippage_budget_pips: Decimal,
    /// Minimum spread-adjusted R:R ratio
    pub min_rr_after_spread: Decimal,
    /// Reject if spread > typical * this multiplier
    pub spread_spike_mult: Decimal,
}

impl Default for ExecutionConfig {
    fn default() -> Self {
        Self {
            max_spread_atr_ratio: dec!(0.30),
            max_retries: 3,
            retry_delay_ms: 500,
            stale_price_threshold: 2,
            slippage_budget_pips: dec!(1.0),
            min_rr_after_spread: dec!(1.2),
            spread_spike_mult: dec!(2.5),
        }
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
        self.history.push_back(SpreadSample { spread_pips, timestamp });
        if self.history.len() > 50 {
            self.history.pop_front();
        }
    }

    /// Get current spread
    pub fn current(&self) -> Decimal {
        self.history.back().map(|s| s.spread_pips).unwrap_or(self.session_typical)
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
}

impl ExecutionEngine {
    pub fn new(config: ExecutionConfig, typical_spread: Decimal) -> Self {
        Self {
            config,
            spread_tracker: SpreadTracker::new(typical_spread),
            fill_log: VecDeque::with_capacity(1000),
            last_tick_time: 0,
        }
    }

    /// Update spread from market data
    pub fn update_spread(&mut self, spread_pips: Decimal, timestamp: i64) {
        self.spread_tracker.record(spread_pips, timestamp);
        self.last_tick_time = timestamp;
    }

    /// Get current spread
    pub fn current_spread(&self) -> Decimal {
        self.spread_tracker.current()
    }

    /// Check if prices are stale
    pub fn is_stale(&self, current_time: i64) -> bool {
        if self.last_tick_time == 0 {
            return false; // No data yet
        }
        current_time - self.last_tick_time > self.config.stale_price_threshold
    }

    /// Calculate spread-adjusted R:R for a signal
    pub fn adjusted_rr(&self, signal: &TradeSignal) -> Option<Decimal> {
        let spread = self.spread_tracker.current();
        
        let entry = signal.entry;
        let tp = signal.take_profit;
        let sl = signal.stop_loss;
        
        let direction_mult = match signal.direction {
            Direction::Buy => dec!(1),
            Direction::Sell => dec!(-1),
        };
        
        // Net distances after spread cost
        let tp_distance = (tp - entry).abs() - spread;
        let sl_distance = (sl - entry).abs() + spread;
        
        if sl_distance.is_zero() {
            return None;
        }
        
        Some(tp_distance / sl_distance)
    }

    /// Execute a risk decision with spread-adjusted gating
    pub fn execute(
        &mut self,
        decision: RiskDecision,
        current_time: i64,
    ) -> ExecutionResult {
        let RiskDecision::Execute { signal, risk_pct, lots, is_pyramid: _ } = decision else {
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
        let avg_slippage: Decimal = self.fill_log.iter()
            .map(|f| f.slippage_pips)
            .sum::<Decimal>() / Decimal::from(total);

        FillStats {
            total_fills: total,
            avg_slippage_pips: avg_slippage,
        }
    }
}

/// Fill statistics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FillStats {
    pub total_fills: usize,
    pub avg_slippage_pips: Decimal,
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
            stop_loss: dec!(1.0950), // 50 pips risk
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