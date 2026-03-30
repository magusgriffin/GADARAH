use std::collections::VecDeque;

use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::decimal_math::decimal_ln;
use crate::indicators::atr::ATR;
use crate::types::Bar;

/// Choppiness Index — streaming, one bar at a time.
///
/// CI = 100 * ln(sum(ATR_i, n) / (highest_high - lowest_low)) / ln(n)
///
/// High CI (> ~61.8) indicates choppy/ranging market.
/// Low CI (< ~38.2) indicates trending market.
#[derive(Debug, Clone)]
pub struct ChoppinessIndex {
    period: usize,
    atr_vals: VecDeque<Decimal>,
    highs: VecDeque<Decimal>,
    lows: VecDeque<Decimal>,
    atr: ATR,
}

impl ChoppinessIndex {
    pub fn new(period: usize) -> Self {
        Self {
            period,
            atr_vals: VecDeque::with_capacity(period + 1),
            highs: VecDeque::with_capacity(period + 1),
            lows: VecDeque::with_capacity(period + 1),
            atr: ATR::new(1), // We use ATR(1) to get per-bar true range
        }
    }

    /// Feed one bar. Returns the Choppiness Index once the rolling window
    /// of `period` bars (with ATR values) is full.
    pub fn update(&mut self, bar: &Bar) -> Option<Decimal> {
        if let Some(atr_val) = self.atr.update(bar) {
            self.atr_vals.push_back(atr_val);
            self.highs.push_back(bar.high);
            self.lows.push_back(bar.low);

            if self.atr_vals.len() > self.period {
                self.atr_vals.pop_front();
            }
            if self.highs.len() > self.period {
                self.highs.pop_front();
            }
            if self.lows.len() > self.period {
                self.lows.pop_front();
            }

            if self.atr_vals.len() == self.period {
                let atr_sum: Decimal = self.atr_vals.iter().copied().sum();
                let hh = self.highs.iter().copied().fold(Decimal::MIN, Decimal::max);
                let ll = self.lows.iter().copied().fold(Decimal::MAX, Decimal::min);
                let range = hh - ll;

                if range.is_zero() {
                    return Some(dec!(50));
                }

                let ci = dec!(100) * decimal_ln(atr_sum / range)
                    / decimal_ln(Decimal::from(self.period));
                return Some(ci);
            }
        }
        None
    }

    pub fn reset(&mut self) {
        self.atr_vals.clear();
        self.highs.clear();
        self.lows.clear();
        self.atr.reset();
    }
}
