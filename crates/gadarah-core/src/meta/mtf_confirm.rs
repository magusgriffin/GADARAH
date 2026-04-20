//! Multi-timeframe confirmation helper.
//!
//! Every intraday head consults the HTF bias via this helper.  The result is
//! either a boosted / preserved confidence multiplier, or an explicit reject
//! for counter-trend signals when the head has not opted into counter-trend.

use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::htf_bias::HtfBias;
use crate::types::Direction;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MtfDecision {
    /// Signal passes; multiply head confidence by `mult`.
    Allow { mult: Decimal },
    /// Counter-trend signal and head did not opt into counter-trend trading.
    CounterTrendBlocked,
}

#[derive(Debug, Clone)]
pub struct MtfConfirm {
    /// If false (default), counter-trend signals are blocked outright.
    allow_counter_trend: bool,
    /// Minimum multiplier required to pass when counter-trend is allowed.
    min_accept_mult: Decimal,
}

impl MtfConfirm {
    pub fn new() -> Self {
        Self {
            allow_counter_trend: false,
            min_accept_mult: dec!(0.70),
        }
    }

    pub fn allow_counter_trend(mut self, allow: bool) -> Self {
        self.allow_counter_trend = allow;
        self
    }

    pub fn with_min_accept_mult(mut self, m: Decimal) -> Self {
        self.min_accept_mult = m;
        self
    }

    /// Evaluate a signal's direction against the current HTF bias.
    pub fn check(&self, bias: HtfBias, direction: Direction) -> MtfDecision {
        let mult = bias.confidence_multiplier(direction);
        if bias.supports(direction) {
            return MtfDecision::Allow { mult };
        }
        if self.allow_counter_trend && mult >= self.min_accept_mult {
            return MtfDecision::Allow { mult };
        }
        MtfDecision::CounterTrendBlocked
    }
}

impl Default for MtfConfirm {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aligned_signal_gets_boost() {
        let m = MtfConfirm::new();
        assert_eq!(
            m.check(HtfBias::Bullish, Direction::Buy),
            MtfDecision::Allow { mult: dec!(1.15) }
        );
    }

    #[test]
    fn neutral_bias_preserves_confidence() {
        let m = MtfConfirm::new();
        assert_eq!(
            m.check(HtfBias::Neutral, Direction::Sell),
            MtfDecision::Allow { mult: dec!(1.0) }
        );
    }

    #[test]
    fn counter_trend_blocked_by_default() {
        let m = MtfConfirm::new();
        assert_eq!(
            m.check(HtfBias::Bullish, Direction::Sell),
            MtfDecision::CounterTrendBlocked
        );
    }

    #[test]
    fn counter_trend_allowed_when_opted_in() {
        let m = MtfConfirm::new().allow_counter_trend(true);
        assert_eq!(
            m.check(HtfBias::Bullish, Direction::Sell),
            MtfDecision::Allow { mult: dec!(0.70) }
        );
    }
}
