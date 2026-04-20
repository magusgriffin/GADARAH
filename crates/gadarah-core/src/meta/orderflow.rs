//! Lightweight order-flow surrogates computed on bar close.
//!
//! We do not have tick-level order book data from the feed yet, so the tracker
//! derives three proxies from OHLCV bars:
//!
//! 1. Bar-body imbalance — signed `(close - open) / range` in [-1, 1].
//! 2. VWAP deviation persistence — number of consecutive bars `close` has stayed
//!    above or below the running session VWAP.
//! 3. Volume-delta surrogate — bar body sign times bar volume, rolling sum.
//!
//! The signal scorer / heads can read [`OrderFlowFeatures`] to tilt their
//! confidence when flow and structure agree.

use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::types::Bar;

#[derive(Debug, Clone, Copy, Default)]
pub struct OrderFlowFeatures {
    pub body_imbalance: Decimal,
    pub vwap_deviation: Decimal,
    pub vwap_streak_bars: i32,
    pub volume_delta: Decimal,
}

#[derive(Debug, Clone)]
pub struct OrderFlowTracker {
    window: usize,
    closes: Vec<Decimal>,
    volumes: Vec<Decimal>,
    cum_vwap_price_vol: Decimal,
    cum_vwap_vol: Decimal,
    vwap_streak: i32,
    last_vwap_sign: i32,
    volume_delta_window: Vec<Decimal>,
}

impl OrderFlowTracker {
    pub fn new(window: usize) -> Self {
        Self {
            window,
            closes: Vec::with_capacity(window),
            volumes: Vec::with_capacity(window),
            cum_vwap_price_vol: Decimal::ZERO,
            cum_vwap_vol: Decimal::ZERO,
            vwap_streak: 0,
            last_vwap_sign: 0,
            volume_delta_window: Vec::with_capacity(window),
        }
    }

    /// Reset the running VWAP — call at the start of a new session.
    pub fn reset_session(&mut self) {
        self.cum_vwap_price_vol = Decimal::ZERO;
        self.cum_vwap_vol = Decimal::ZERO;
        self.vwap_streak = 0;
        self.last_vwap_sign = 0;
    }

    pub fn update(&mut self, bar: &Bar) -> OrderFlowFeatures {
        let range = bar.high - bar.low;
        let body = bar.close - bar.open;
        let body_imbalance = if range.is_zero() {
            Decimal::ZERO
        } else {
            (body / range).max(dec!(-1)).min(dec!(1))
        };

        let typical = (bar.high + bar.low + bar.close) / dec!(3);
        let vol = Decimal::from(bar.volume);
        self.cum_vwap_price_vol += typical * vol;
        self.cum_vwap_vol += vol;

        let vwap = if self.cum_vwap_vol.is_zero() {
            bar.close
        } else {
            self.cum_vwap_price_vol / self.cum_vwap_vol
        };
        let vwap_deviation = bar.close - vwap;
        let sign = match vwap_deviation.cmp(&Decimal::ZERO) {
            std::cmp::Ordering::Greater => 1,
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Equal => 0,
        };
        if sign != 0 && sign == self.last_vwap_sign {
            self.vwap_streak += sign;
        } else {
            self.vwap_streak = sign;
        }
        self.last_vwap_sign = sign;

        let delta = if body > Decimal::ZERO {
            vol
        } else if body < Decimal::ZERO {
            -vol
        } else {
            Decimal::ZERO
        };
        self.volume_delta_window.push(delta);
        if self.volume_delta_window.len() > self.window {
            self.volume_delta_window.remove(0);
        }
        let volume_delta: Decimal = self.volume_delta_window.iter().copied().sum();

        self.closes.push(bar.close);
        self.volumes.push(vol);
        if self.closes.len() > self.window {
            self.closes.remove(0);
            self.volumes.remove(0);
        }

        OrderFlowFeatures {
            body_imbalance,
            vwap_deviation,
            vwap_streak_bars: self.vwap_streak,
            volume_delta,
        }
    }
}

impl Default for OrderFlowTracker {
    fn default() -> Self {
        Self::new(50)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Timeframe;

    fn bar(o: Decimal, h: Decimal, l: Decimal, c: Decimal, v: u64) -> Bar {
        Bar {
            open: o,
            high: h,
            low: l,
            close: c,
            volume: v,
            timestamp: 0,
            timeframe: Timeframe::M5,
        }
    }

    #[test]
    fn bullish_bar_has_positive_imbalance() {
        let mut t = OrderFlowTracker::new(10);
        let f = t.update(&bar(dec!(1.0), dec!(1.2), dec!(0.9), dec!(1.18), 100));
        assert!(f.body_imbalance > Decimal::ZERO);
    }

    #[test]
    fn vwap_streak_counts_consecutive_above_bars() {
        let mut t = OrderFlowTracker::new(10);
        // First bar: close well above typical → above-VWAP
        t.update(&bar(dec!(1.0), dec!(1.1), dec!(0.99), dec!(1.09), 100));
        t.update(&bar(dec!(1.09), dec!(1.12), dec!(1.08), dec!(1.11), 100));
        let f = t.update(&bar(dec!(1.11), dec!(1.14), dec!(1.10), dec!(1.13), 100));
        assert!(f.vwap_streak_bars > 0);
    }

    #[test]
    fn reset_session_clears_vwap() {
        let mut t = OrderFlowTracker::new(10);
        // Build up a multi-bar positive streak.
        for _ in 0..3 {
            t.update(&bar(dec!(1.0), dec!(1.1), dec!(0.99), dec!(1.09), 100));
        }
        assert!(t.last_vwap_sign != 0);
        t.reset_session();
        assert_eq!(t.vwap_streak, 0);
        assert_eq!(t.last_vwap_sign, 0);
    }

    #[test]
    fn volume_delta_sign_tracks_price_direction() {
        let mut t = OrderFlowTracker::new(5);
        t.update(&bar(dec!(1.0), dec!(1.01), dec!(0.99), dec!(1.01), 100));
        let f = t.update(&bar(dec!(1.01), dec!(1.02), dec!(1.0), dec!(1.015), 200));
        assert!(f.volume_delta > Decimal::ZERO);
    }
}
