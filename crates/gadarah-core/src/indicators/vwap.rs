use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::types::Bar;

/// Volume-Weighted Average Price — streaming, session-resetting.
///
/// Accumulates `typical_price * volume` and `volume` within a session.
/// When `session_changed` is true, accumulators reset and a new session begins.
#[derive(Debug, Clone)]
pub struct VWAP {
    cum_price_vol: Decimal,
    cum_volume: Decimal,
    /// Fallback: cumulative typical-price average when volume data is absent.
    cum_typical: Decimal,
    bar_count: u64,
}

impl VWAP {
    pub fn new() -> Self {
        Self {
            cum_price_vol: Decimal::ZERO,
            cum_volume: Decimal::ZERO,
            cum_typical: Decimal::ZERO,
            bar_count: 0,
        }
    }

    /// Feed one bar. If `session_changed` is true, the running totals reset
    /// before this bar is processed.
    ///
    /// Returns the current VWAP value. When volume data is absent (all zeros),
    /// falls back to the cumulative average of typical price, which still
    /// provides a meaningful directional filter.
    pub fn update(&mut self, bar: &Bar, session_changed: bool) -> Decimal {
        if session_changed {
            self.cum_price_vol = Decimal::ZERO;
            self.cum_volume = Decimal::ZERO;
            self.cum_typical = Decimal::ZERO;
            self.bar_count = 0;
        }

        let typical = (bar.high + bar.low + bar.close) / dec!(3);
        let vol = Decimal::from(bar.volume);
        self.cum_price_vol += typical * vol;
        self.cum_volume += vol;
        self.cum_typical += typical;
        self.bar_count += 1;

        if !self.cum_volume.is_zero() {
            self.cum_price_vol / self.cum_volume
        } else {
            // No volume data: use cumulative average of typical price
            self.cum_typical / Decimal::from(self.bar_count)
        }
    }

    pub fn reset(&mut self) {
        self.cum_price_vol = Decimal::ZERO;
        self.cum_volume = Decimal::ZERO;
        self.cum_typical = Decimal::ZERO;
        self.bar_count = 0;
    }
}

impl Default for VWAP {
    fn default() -> Self {
        Self::new()
    }
}
