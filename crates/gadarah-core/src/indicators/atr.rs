use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::types::Bar;

/// Average True Range with Wilder smoothing — streaming, one bar at a time.
///
/// Seeds with the arithmetic mean of the first `period` true-range values,
/// then applies Wilder smoothing: ATR_t = (ATR_{t-1} * (n-1) + TR_t) / n.
#[derive(Debug, Clone)]
pub struct ATR {
    period: usize,
    value: Option<Decimal>,
    prev_close: Option<Decimal>,
    count: usize,
    tr_sum: Decimal,
}

impl ATR {
    pub fn new(period: usize) -> Self {
        Self {
            period,
            value: None,
            prev_close: None,
            count: 0,
            tr_sum: Decimal::ZERO,
        }
    }

    /// Feed one bar. Returns the ATR value once enough bars have been processed.
    pub fn update(&mut self, bar: &Bar) -> Option<Decimal> {
        let tr = match self.prev_close {
            None => bar.high - bar.low,
            Some(pc) => (bar.high - bar.low)
                .max((bar.high - pc).abs())
                .max((bar.low - pc).abs()),
        };
        self.prev_close = Some(bar.close);
        self.count += 1;

        match self.value {
            None if self.count < self.period => {
                self.tr_sum += tr;
                None
            }
            None => {
                // Exactly `period` bars -> seed ATR with SMA of true ranges
                self.tr_sum += tr;
                let atr = self.tr_sum / Decimal::from(self.period);
                self.value = Some(atr);
                Some(atr)
            }
            Some(prev) => {
                // Wilder smoothing
                let n = Decimal::from(self.period);
                let atr = (prev * (n - dec!(1)) + tr) / n;
                self.value = Some(atr);
                Some(atr)
            }
        }
    }

    pub fn value(&self) -> Option<Decimal> {
        self.value
    }

    pub fn period(&self) -> usize {
        self.period
    }

    pub fn is_ready(&self) -> bool {
        self.value.is_some()
    }

    pub fn reset(&mut self) {
        self.value = None;
        self.prev_close = None;
        self.count = 0;
        self.tr_sum = Decimal::ZERO;
    }
}
