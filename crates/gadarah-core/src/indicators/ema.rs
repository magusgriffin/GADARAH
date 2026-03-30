use rust_decimal::Decimal;
use rust_decimal_macros::dec;

/// Exponential Moving Average — streaming, single-value-at-a-time.
///
/// Seeds with an SMA over the first `period` values, then applies the
/// standard EMA formula: EMA_t = alpha * value + (1 - alpha) * EMA_{t-1}.
#[derive(Debug, Clone)]
pub struct EMA {
    period: usize,
    alpha: Decimal,
    value: Option<Decimal>,
    count: usize,
    sum: Decimal, // accumulates first `period` values for SMA seed
}

impl EMA {
    pub fn new(period: usize) -> Self {
        let alpha = dec!(2) / (Decimal::from(period) + dec!(1));
        Self {
            period,
            alpha,
            value: None,
            count: 0,
            sum: Decimal::ZERO,
        }
    }

    /// Feed one close price. Returns the EMA value once the warmup period
    /// is satisfied (i.e., after `period` values have been fed).
    pub fn update(&mut self, close: Decimal) -> Option<Decimal> {
        self.count += 1;
        match self.value {
            None if self.count < self.period => {
                self.sum += close;
                None
            }
            None => {
                // Exactly `period` values received -> seed with SMA
                self.sum += close;
                let sma = self.sum / Decimal::from(self.period);
                self.value = Some(sma);
                Some(sma)
            }
            Some(prev) => {
                let ema = self.alpha * close + (dec!(1) - self.alpha) * prev;
                self.value = Some(ema);
                Some(ema)
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
        self.count = 0;
        self.sum = Decimal::ZERO;
    }
}
