//! Volatility-adjusted SL / TP multipliers.
//!
//! Heads used to carry fixed `sl_atr_mult` / `tp_atr_mult` constants.  Now they
//! carry a base multiplier and a slope: the final multiplier tilts up/down
//! with the current ATR percentile so stops widen in quiet markets and tighten
//! in noisy ones.
//!
//! formula:  mult = base + slope * (percentile - 0.5)
//! clamped to `[base * 0.5, base * 2.0]` so a misconfigured slope cannot
//! explode the SL into oblivion.

use std::collections::VecDeque;

use rust_decimal::Decimal;
use rust_decimal_macros::dec;

#[derive(Debug, Clone)]
pub struct VolAdjustedStops {
    base_sl_mult: Decimal,
    sl_slope: Decimal,
    base_tp_mult: Decimal,
    tp_slope: Decimal,
    lookback: usize,
    atr_window: VecDeque<Decimal>,
}

impl VolAdjustedStops {
    pub fn new(base_sl_mult: Decimal, base_tp_mult: Decimal) -> Self {
        Self {
            base_sl_mult,
            sl_slope: dec!(0.4),
            base_tp_mult,
            tp_slope: dec!(0.6),
            lookback: 60,
            atr_window: VecDeque::with_capacity(60),
        }
    }

    pub fn with_slope(mut self, sl_slope: Decimal, tp_slope: Decimal) -> Self {
        self.sl_slope = sl_slope;
        self.tp_slope = tp_slope;
        self
    }

    pub fn with_lookback(mut self, lookback: usize) -> Self {
        self.lookback = lookback;
        self
    }

    /// Feed the latest ATR reading.  Returns the new percentile of `atr`
    /// within the rolling window, in [0, 1].
    pub fn update(&mut self, atr: Decimal) -> Decimal {
        self.atr_window.push_back(atr);
        if self.atr_window.len() > self.lookback {
            self.atr_window.pop_front();
        }
        self.percentile(atr)
    }

    /// Percentile of `value` within the tracked window.  0.5 during warmup
    /// (i.e. treat as median → no tilt).
    pub fn percentile(&self, value: Decimal) -> Decimal {
        if self.atr_window.len() < 10 {
            return dec!(0.5);
        }
        let below = self.atr_window.iter().filter(|&&x| x < value).count();
        Decimal::from(below) / Decimal::from(self.atr_window.len())
    }

    fn clamp(&self, base: Decimal, candidate: Decimal) -> Decimal {
        let lo = base / dec!(2);
        let hi = base * dec!(2);
        candidate.max(lo).min(hi)
    }

    pub fn sl_mult(&self, percentile: Decimal) -> Decimal {
        let raw = self.base_sl_mult + self.sl_slope * (percentile - dec!(0.5));
        self.clamp(self.base_sl_mult, raw)
    }

    pub fn tp_mult(&self, percentile: Decimal) -> Decimal {
        let raw = self.base_tp_mult + self.tp_slope * (percentile - dec!(0.5));
        self.clamp(self.base_tp_mult, raw)
    }

    /// Convenience: compute both multipliers at once.
    pub fn stops(&self, percentile: Decimal) -> (Decimal, Decimal) {
        (self.sl_mult(percentile), self.tp_mult(percentile))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warmup_percentile_returns_half() {
        let mut v = VolAdjustedStops::new(dec!(1.5), dec!(3.0));
        // Under 10 samples
        for _ in 0..5 {
            let p = v.update(dec!(0.0010));
            assert_eq!(p, dec!(0.5));
        }
    }

    #[test]
    fn high_atr_widens_stops() {
        let mut v = VolAdjustedStops::new(dec!(1.5), dec!(3.0));
        for _ in 0..15 {
            v.update(dec!(0.0005));
        }
        let p = v.update(dec!(0.0020)); // much higher than window
        assert!(p > dec!(0.5));
        assert!(v.sl_mult(p) > dec!(1.5));
    }

    #[test]
    fn low_atr_tightens_stops() {
        let mut v = VolAdjustedStops::new(dec!(1.5), dec!(3.0));
        for _ in 0..15 {
            v.update(dec!(0.0020));
        }
        let p = v.update(dec!(0.0001)); // much lower than window
        assert!(p < dec!(0.5));
        assert!(v.sl_mult(p) < dec!(1.5));
    }

    #[test]
    fn multipliers_never_exceed_hard_bounds() {
        let v = VolAdjustedStops::new(dec!(1.5), dec!(3.0));
        // Extreme percentile cannot push stops past 2x base.
        let sl = v.sl_mult(dec!(10));
        assert!(sl <= dec!(3.0));
        let tp = v.tp_mult(dec!(-10));
        assert!(tp >= dec!(1.5));
    }
}
