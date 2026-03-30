use std::collections::VecDeque;

use rust_decimal::Decimal;

use crate::decimal_math::decimal_sqrt;

/// Bollinger Bands output values.
#[derive(Debug, Clone)]
pub struct BBValues {
    pub upper: Decimal,
    pub mid: Decimal,
    pub lower: Decimal,
    /// Normalized width: (upper - lower) / mid. Zero if mid is zero.
    pub width: Decimal,
}

/// Bollinger Bands — streaming, one close price at a time.
///
/// Maintains a rolling window of `period` close prices, computes the SMA
/// (middle band) and standard deviation, then derives upper and lower bands
/// at `k` standard deviations.
#[derive(Debug, Clone)]
pub struct BollingerBands {
    period: usize,
    k: Decimal,
    prices: VecDeque<Decimal>,
    cached: Option<BBValues>,
}

impl BollingerBands {
    pub fn new(period: usize, k: Decimal) -> Self {
        Self {
            period,
            k,
            prices: VecDeque::with_capacity(period + 1),
            cached: None,
        }
    }

    /// Feed one close price. Returns a reference to the current BB values
    /// once the rolling window has `period` data points.
    pub fn update(&mut self, close: Decimal) -> Option<&BBValues> {
        self.prices.push_back(close);
        if self.prices.len() > self.period {
            self.prices.pop_front();
        }
        if self.prices.len() < self.period {
            return None;
        }

        let n = Decimal::from(self.period);
        let sum: Decimal = self.prices.iter().copied().sum();
        let mid = sum / n;

        let variance: Decimal = self
            .prices
            .iter()
            .map(|p| (*p - mid) * (*p - mid))
            .sum::<Decimal>()
            / n;

        let std_dev = decimal_sqrt(variance);
        let upper = mid + self.k * std_dev;
        let lower = mid - self.k * std_dev;
        let width = if mid.is_zero() {
            Decimal::ZERO
        } else {
            (upper - lower) / mid
        };

        self.cached = Some(BBValues {
            upper,
            mid,
            lower,
            width,
        });
        self.cached.as_ref()
    }

    pub fn value(&self) -> Option<&BBValues> {
        self.cached.as_ref()
    }

    pub fn is_ready(&self) -> bool {
        self.cached.is_some()
    }

    pub fn reset(&mut self) {
        self.prices.clear();
        self.cached = None;
    }
}
