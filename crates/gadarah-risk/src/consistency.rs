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
        if pnl.is_zero() {
            return;
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

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn pauses_after_three_consecutive_losing_days() {
        let mut t = ConsistencyTracker::new();
        t.record_day(1, dec!(-50));
        assert!(!t.is_paused_for_consistency());
        t.record_day(2, dec!(-30));
        assert!(!t.is_paused_for_consistency());
        t.record_day(3, dec!(-10));
        assert!(
            t.is_paused_for_consistency(),
            "must pause after 3 consecutive losing days"
        );
    }

    #[test]
    fn winning_day_resets_losing_streak() {
        let mut t = ConsistencyTracker::new();
        t.record_day(1, dec!(-50));
        t.record_day(2, dec!(-30));
        t.record_day(3, dec!(100)); // winning day resets streak
        assert!(!t.is_paused_for_consistency());
        assert_eq!(t.streak_losing_days, 0);
    }

    #[test]
    fn profitable_day_rate_calculation() {
        let mut t = ConsistencyTracker::new();
        t.record_day(1, dec!(100));
        t.record_day(2, dec!(-50));
        t.record_day(3, dec!(200));
        t.record_day(4, dec!(-10));
        // 2 profitable / 4 total = 0.5
        assert_eq!(t.profitable_day_rate(), dec!(0.5));
    }

    #[test]
    fn flat_days_do_not_count_as_losing_streak() {
        let mut t = ConsistencyTracker::new();
        t.record_day(1, dec!(0));
        t.record_day(2, dec!(0));
        t.record_day(3, dec!(-25));
        assert!(!t.is_paused_for_consistency());
        assert_eq!(t.total_trading_days, 1);
        assert_eq!(t.streak_losing_days, 1);
    }
}
