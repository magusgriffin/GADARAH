use std::collections::VecDeque;

use rust_decimal::Decimal;

/// BB Width Percentile Tracker — streaming.
///
/// Maintains a rolling window of the last `window` Bollinger Band width values
/// and computes the percentile rank of the current width within that history.
///
/// A low percentile (e.g., < 0.20) indicates a volatility squeeze.
/// A high percentile (e.g., > 0.80) indicates volatility expansion.
#[derive(Debug, Clone)]
pub struct BBWidthPercentile {
    window: usize,
    history: VecDeque<Decimal>,
}

impl BBWidthPercentile {
    pub fn new(window: usize) -> Self {
        Self {
            window,
            history: VecDeque::with_capacity(window + 1),
        }
    }

    /// Feed one BB width value. Returns the percentile rank [0, 1] of this
    /// width within the rolling history window.
    pub fn update(&mut self, bb_width: Decimal) -> Decimal {
        self.history.push_back(bb_width);
        if self.history.len() > self.window {
            self.history.pop_front();
        }

        let below = self.history.iter().filter(|w| **w <= bb_width).count();
        Decimal::from(below) / Decimal::from(self.history.len())
    }

    pub fn reset(&mut self) {
        self.history.clear();
    }
}
