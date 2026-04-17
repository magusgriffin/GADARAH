use rust_decimal::Decimal;
use rust_decimal_macros::dec;

/// Relative Strength Index — streaming, Wilder smoothing.
///
/// Seeds avg_gain and avg_loss with SMA of the first `period` up/down moves,
/// then applies Wilder smoothing: avg = (prev × (n-1) + current) / n.
/// RSI = 100 − 100 / (1 + avg_gain/avg_loss).
#[derive(Debug, Clone)]
pub struct RSI {
    period: usize,
    avg_gain: Option<Decimal>,
    avg_loss: Option<Decimal>,
    prev_close: Option<Decimal>,
    count: usize,
    gain_sum: Decimal,
    loss_sum: Decimal,
    value: Option<Decimal>,
}

impl RSI {
    pub fn new(period: usize) -> Self {
        assert!(period >= 2, "RSI period must be >= 2");
        Self {
            period,
            avg_gain: None,
            avg_loss: None,
            prev_close: None,
            count: 0,
            gain_sum: Decimal::ZERO,
            loss_sum: Decimal::ZERO,
            value: None,
        }
    }

    /// Feed one close price. Returns the RSI value once warmup is complete.
    pub fn update(&mut self, close: Decimal) -> Option<Decimal> {
        let Some(prev) = self.prev_close else {
            self.prev_close = Some(close);
            return None;
        };

        let change = close - prev;
        self.prev_close = Some(close);

        let gain = if change > Decimal::ZERO {
            change
        } else {
            Decimal::ZERO
        };
        let loss = if change < Decimal::ZERO {
            -change
        } else {
            Decimal::ZERO
        };

        if self.avg_gain.is_none() {
            // Seeding phase: accumulate gains/losses for initial SMA
            self.count += 1;
            self.gain_sum += gain;
            self.loss_sum += loss;
            if self.count == self.period {
                let n = Decimal::from(self.period);
                let ag = self.gain_sum / n;
                let al = self.loss_sum / n;
                self.avg_gain = Some(ag);
                self.avg_loss = Some(al);
                self.value = Some(rsi_from_avgs(ag, al));
            }
            return self.value;
        }

        // Wilder smoothing
        let n = Decimal::from(self.period);
        let ag = self.avg_gain.unwrap();
        let al = self.avg_loss.unwrap();
        let new_ag = (ag * (n - dec!(1)) + gain) / n;
        let new_al = (al * (n - dec!(1)) + loss) / n;
        self.avg_gain = Some(new_ag);
        self.avg_loss = Some(new_al);
        self.value = Some(rsi_from_avgs(new_ag, new_al));
        self.value
    }

    pub fn value(&self) -> Option<Decimal> {
        self.value
    }

    pub fn is_ready(&self) -> bool {
        self.value.is_some()
    }

    pub fn reset(&mut self) {
        self.avg_gain = None;
        self.avg_loss = None;
        self.prev_close = None;
        self.count = 0;
        self.gain_sum = Decimal::ZERO;
        self.loss_sum = Decimal::ZERO;
        self.value = None;
    }
}

fn rsi_from_avgs(avg_gain: Decimal, avg_loss: Decimal) -> Decimal {
    if avg_loss.is_zero() {
        return dec!(100);
    }
    let rs = avg_gain / avg_loss;
    dec!(100) - dec!(100) / (dec!(1) + rs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rsi_seeds_and_updates() {
        let mut rsi = RSI::new(3);
        // Feed 3 price changes to seed (needs 4 closes = 3 diffs)
        assert!(rsi.update(dec!(100)).is_none());
        assert!(rsi.update(dec!(102)).is_none()); // +2
        assert!(rsi.update(dec!(101)).is_none()); // -1
        let v = rsi.update(dec!(103)); // +2 → count==3, seed
        assert!(v.is_some());
        let rsi_val = v.unwrap();
        assert!(
            rsi_val > dec!(50),
            "RSI should be >50 with more gains than losses"
        );
    }

    #[test]
    fn rsi_all_gains_returns_100() {
        let mut rsi = RSI::new(3);
        rsi.update(dec!(100));
        rsi.update(dec!(101));
        rsi.update(dec!(102));
        let v = rsi.update(dec!(103)).unwrap();
        assert_eq!(v, dec!(100));
    }

    #[test]
    fn rsi_all_losses_returns_zero() {
        let mut rsi = RSI::new(3);
        rsi.update(dec!(100));
        rsi.update(dec!(99));
        rsi.update(dec!(98));
        let v = rsi.update(dec!(97)).unwrap();
        assert_eq!(v, dec!(0));
    }

    #[test]
    fn reset_clears_state() {
        let mut rsi = RSI::new(3);
        for i in 0..10 {
            rsi.update(Decimal::from(100 + i));
        }
        assert!(rsi.is_ready());
        rsi.reset();
        assert!(!rsi.is_ready());
    }
}
