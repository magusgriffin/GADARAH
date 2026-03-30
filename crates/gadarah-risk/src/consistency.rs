use std::collections::VecDeque;

use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// ConsistencyTracker
// ---------------------------------------------------------------------------

const MAX_DAILY_HISTORY: usize = 30;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsistencyTracker {
    daily_pnl_history: VecDeque<(i64, Decimal)>,
    pub streak_losing_days: u8,
    pub total_profitable_days: u32,
    pub total_trading_days: u32,
    pub max_single_day_pct: Decimal,
}

impl ConsistencyTracker {
    pub fn new() -> Self {
        Self {
            daily_pnl_history: VecDeque::with_capacity(MAX_DAILY_HISTORY + 1),
            streak_losing_days: 0,
            total_profitable_days: 0,
            total_trading_days: 0,
            max_single_day_pct: Decimal::ZERO,
        }
    }

    /// Record the end-of-day P&L. Maintains a rolling 30-day window.
    pub fn record_day(&mut self, timestamp: i64, pnl: Decimal) {
        self.daily_pnl_history.push_back((timestamp, pnl));
        if self.daily_pnl_history.len() > MAX_DAILY_HISTORY {
            self.daily_pnl_history.pop_front();
        }
        self.total_trading_days += 1;
        if pnl > Decimal::ZERO {
            self.total_profitable_days += 1;
            self.streak_losing_days = 0;
        } else {
            self.streak_losing_days += 1;
        }
        if pnl > self.max_single_day_pct {
            self.max_single_day_pct = pnl;
        }
    }

    /// Whether trading should be paused due to consecutive losing days (>= 3).
    pub fn is_paused_for_consistency(&self) -> bool {
        self.streak_losing_days >= 3
    }

    /// Fraction of trading days that were profitable.
    pub fn profitable_day_rate(&self) -> Decimal {
        if self.total_trading_days == 0 {
            return dec!(0);
        }
        Decimal::from(self.total_profitable_days) / Decimal::from(self.total_trading_days)
    }

    /// Access the rolling daily P&L history.
    pub fn daily_pnl_history(&self) -> &VecDeque<(i64, Decimal)> {
        &self.daily_pnl_history
    }
}

impl Default for ConsistencyTracker {
    fn default() -> Self {
        Self::new()
    }
}
