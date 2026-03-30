use std::collections::VecDeque;

use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::decimal_math::{decimal_ln, decimal_sqrt};

/// Hurst Exponent via rescaled range (R/S) analysis — streaming.
///
/// Maintains a rolling window of `period` close prices. Once full, computes
/// the R/S statistic on the price differences and derives H = ln(R/S) / ln(n).
///
/// H > 0.5 => trending (persistent), H < 0.5 => mean-reverting, H ~ 0.5 => random walk.
#[derive(Debug, Clone)]
pub struct HurstExponent {
    period: usize,
    prices: VecDeque<Decimal>,
}

impl HurstExponent {
    pub fn new(period: usize) -> Self {
        Self {
            period,
            prices: VecDeque::with_capacity(period + 1),
        }
    }

    /// Feed one close price. Returns the Hurst exponent (clamped to [0, 1])
    /// once the rolling window is full.
    pub fn update(&mut self, close: Decimal) -> Option<Decimal> {
        self.prices.push_back(close);
        if self.prices.len() > self.period {
            self.prices.pop_front();
        }
        if self.prices.len() < self.period {
            return None;
        }

        // Compute first-differences (returns)
        let returns: Vec<Decimal> = self
            .prices
            .iter()
            .zip(self.prices.iter().skip(1))
            .map(|(a, b)| *b - *a)
            .collect();

        let n = returns.len();
        if n == 0 {
            return Some(dec!(0.5));
        }

        let mean = returns.iter().copied().sum::<Decimal>() / Decimal::from(n);
        let deviations: Vec<Decimal> = returns.iter().map(|r| *r - mean).collect();

        // Cumulative deviations
        let mut cumdev = Vec::with_capacity(n);
        let mut running = Decimal::ZERO;
        for d in &deviations {
            running += *d;
            cumdev.push(running);
        }

        let range = cumdev.iter().copied().fold(Decimal::MIN, Decimal::max)
            - cumdev.iter().copied().fold(Decimal::MAX, Decimal::min);

        let variance = deviations.iter().map(|d| *d * *d).sum::<Decimal>() / Decimal::from(n);
        let std_dev = decimal_sqrt(variance);

        if std_dev.is_zero() {
            return Some(dec!(0.5));
        }

        let rs = range / std_dev;
        if rs <= Decimal::ZERO {
            return Some(dec!(0.5));
        }

        let ln_n = decimal_ln(Decimal::from(n));
        if ln_n.is_zero() {
            return Some(dec!(0.5));
        }

        let h = decimal_ln(rs) / ln_n;
        Some(h.max(dec!(0)).min(dec!(1)))
    }

    pub fn reset(&mut self) {
        self.prices.clear();
    }
}
