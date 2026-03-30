use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::types::Bar;

/// Wilder-smoothed running average, used internally by ADX for +DM, -DM, and DX.
///
/// Operates the same as a Wilder EMA: seed with SMA over the first `period`
/// values, then smooth via: value = (prev * (n-1) + new) / n.
#[derive(Debug, Clone)]
pub struct WilderSmooth {
    period: usize,
    value: Option<Decimal>,
    count: usize,
    sum: Decimal,
}

impl WilderSmooth {
    pub fn new(period: usize) -> Self {
        Self {
            period,
            value: None,
            count: 0,
            sum: Decimal::ZERO,
        }
    }

    pub fn update(&mut self, val: Decimal) -> Option<Decimal> {
        self.count += 1;
        match self.value {
            None if self.count < self.period => {
                self.sum += val;
                None
            }
            None => {
                self.sum += val;
                let avg = self.sum / Decimal::from(self.period);
                self.value = Some(avg);
                Some(avg)
            }
            Some(prev) => {
                let n = Decimal::from(self.period);
                let smoothed = (prev * (n - dec!(1)) + val) / n;
                self.value = Some(smoothed);
                Some(smoothed)
            }
        }
    }

    pub fn value(&self) -> Option<Decimal> {
        self.value
    }

    pub fn reset(&mut self) {
        self.value = None;
        self.count = 0;
        self.sum = Decimal::ZERO;
    }
}

/// Average Directional Index (ADX) — streaming, one bar at a time.
///
/// Internally tracks +DI, -DI via `WilderSmooth`, computes DX, then
/// smooths DX via another `WilderSmooth` to produce ADX.
#[derive(Debug, Clone)]
pub struct ADX {
    period: usize,
    plus_di: WilderSmooth,
    minus_di: WilderSmooth,
    adx_smooth: WilderSmooth,
    prev_bar: Option<(Decimal, Decimal, Decimal)>, // (high, low, close)
    count: usize,
}

impl ADX {
    pub fn new(period: usize) -> Self {
        Self {
            period,
            plus_di: WilderSmooth::new(period),
            minus_di: WilderSmooth::new(period),
            adx_smooth: WilderSmooth::new(period),
            prev_bar: None,
            count: 0,
        }
    }

    /// Feed one bar. Returns the ADX value once enough data has accumulated
    /// (requires roughly 2*period bars for the double-smoothing).
    pub fn update(&mut self, bar: &Bar) -> Option<Decimal> {
        if let Some((ph, pl, _pc)) = self.prev_bar {
            let up_move = bar.high - ph;
            let down_move = pl - bar.low;

            let plus_dm = if up_move > down_move && up_move > Decimal::ZERO {
                up_move
            } else {
                Decimal::ZERO
            };
            let minus_dm = if down_move > up_move && down_move > Decimal::ZERO {
                down_move
            } else {
                Decimal::ZERO
            };

            self.plus_di.update(plus_dm);
            self.minus_di.update(minus_dm);

            self.count += 1;
            if self.count >= self.period {
                let pdi = self.plus_di.value().unwrap_or(Decimal::ZERO);
                let mdi = self.minus_di.value().unwrap_or(Decimal::ZERO);
                let sum = pdi + mdi;
                let dx = if sum.is_zero() {
                    Decimal::ZERO
                } else {
                    (pdi - mdi).abs() / sum * dec!(100)
                };
                self.adx_smooth.update(dx);
                self.prev_bar = Some((bar.high, bar.low, bar.close));
                return self.adx_smooth.value();
            }
        }
        self.prev_bar = Some((bar.high, bar.low, bar.close));
        None
    }

    pub fn value(&self) -> Option<Decimal> {
        self.adx_smooth.value()
    }

    pub fn is_ready(&self) -> bool {
        self.adx_smooth.value().is_some()
    }

    pub fn reset(&mut self) {
        self.plus_di.reset();
        self.minus_di.reset();
        self.adx_smooth.reset();
        self.prev_bar = None;
        self.count = 0;
    }
}
