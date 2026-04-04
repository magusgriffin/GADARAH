//! VolProfileHead — Volume Profile rejection at Value Area edges.
//!
//! Maintains a rolling volume-at-price histogram over the last `lookback`
//! bars (default 24 × M15 = 6 hours).  From the histogram it derives:
//!
//!   - POC (Point of Control): price bucket with the highest cumulative volume.
//!   - Value Area: the contiguous set of price buckets that contain 70% of
//!     total volume, expanded outward from the POC.
//!   - VAH (Value Area High): top of the value area.
//!   - VAL (Value Area Low):  bottom of the value area.
//!
//! Entry:
//!   BUY at VAL:  bar.low ≤ VAL and bar closes ≥ VAL (rejection bounce up).
//!   SELL at VAH: bar.high ≥ VAH and bar closes ≤ VAH (rejection bounce down).
//!
//! SL: ATR × sl_atr_mult beyond the VAL/VAH level.
//! TP: POC (middle of value area), clipped to min_rr.
//!
//! Allowed regimes: WeakTrendUp, WeakTrendDown, RangingTight, Transitioning.

use std::collections::VecDeque;

use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::heads::Head;
use crate::indicators::ATR;
use crate::types::{
    Bar, Direction, HeadId, Regime9, RegimeSignal9, Session, SessionProfile, SignalKind,
    TradeSignal, Timeframe,
};

const BUCKETS: usize = 100;

#[derive(Debug, Clone)]
pub struct VolProfileConfig {
    /// Number of bars in the rolling profile window (default 24 = 6 h on M15).
    pub lookback: usize,
    pub sl_atr_mult: Decimal,
    pub min_rr: Decimal,
    pub base_confidence: Decimal,
    /// Cool-down in bars after each signal.
    pub cool_bars: i64,
    pub symbol: String,
}

impl Default for VolProfileConfig {
    fn default() -> Self {
        Self {
            lookback: 24,
            sl_atr_mult: dec!(0.8),
            min_rr: dec!(1.5),
            base_confidence: dec!(0.62),
            cool_bars: 4,
            symbol: "EURUSD".to_string(),
        }
    }
}

#[derive(Debug, Clone)]
struct ProfileLevels {
    poc: Decimal,
    vah: Decimal,
    val: Decimal,
}

#[derive(Debug, Clone)]
pub struct VolProfileHead {
    config: VolProfileConfig,
    atr: ATR,
    history: VecDeque<Bar>,
    last_signal_bar: i64,
    bar_count: i64,
}

impl VolProfileHead {
    pub fn new(config: VolProfileConfig) -> Self {
        Self {
            config,
            atr: ATR::new(14),
            history: VecDeque::new(),
            last_signal_bar: i64::MIN,
            bar_count: 0,
        }
    }

    /// Build the volume profile from the current history window and return
    /// POC, VAH, VAL.  Returns None if the window has no volume or is too small.
    fn compute_profile(&self) -> Option<ProfileLevels> {
        if self.history.len() < 5 {
            return None;
        }

        let price_min = self
            .history
            .iter()
            .map(|b| b.low)
            .min()
            .unwrap_or(Decimal::ZERO);
        let price_max = self
            .history
            .iter()
            .map(|b| b.high)
            .max()
            .unwrap_or(Decimal::ZERO);

        let range = price_max - price_min;
        if range <= Decimal::ZERO {
            return None;
        }

        // Build bucket array
        let bucket_size = range / Decimal::from(BUCKETS);
        let mut buckets = vec![0u64; BUCKETS];
        let mut total_vol = 0u64;

        for bar in &self.history {
            if bar.volume == 0 {
                continue;
            }
            // Distribute bar's volume uniformly across buckets it spans
            let lo_idx = ((bar.low - price_min) / bucket_size)
                .to_u64_saturating()
                .min(BUCKETS as u64 - 1) as usize;
            let hi_idx = ((bar.high - price_min) / bucket_size)
                .to_u64_saturating()
                .min(BUCKETS as u64 - 1) as usize;
            let span = (hi_idx - lo_idx + 1) as u64;
            let vol_per_bucket = bar.volume / span.max(1);
            for idx in lo_idx..=hi_idx {
                buckets[idx] = buckets[idx].saturating_add(vol_per_bucket);
            }
            total_vol += bar.volume;
        }

        if total_vol == 0 {
            return None;
        }

        // POC = bucket with most volume
        let poc_idx = buckets
            .iter()
            .enumerate()
            .max_by_key(|(_, &v)| v)
            .map(|(i, _)| i)
            .unwrap_or(BUCKETS / 2);

        // Expand value area outward from POC until 70% of volume included
        let target_vol = (total_vol as f64 * 0.70) as u64;
        let mut va_low = poc_idx;
        let mut va_high = poc_idx;
        let mut va_vol = buckets[poc_idx];

        while va_vol < target_vol {
            let can_expand_down = va_low > 0;
            let can_expand_up = va_high < BUCKETS - 1;
            if !can_expand_down && !can_expand_up {
                break;
            }
            let down_vol = if can_expand_down { buckets[va_low - 1] } else { 0 };
            let up_vol = if can_expand_up { buckets[va_high + 1] } else { 0 };
            if up_vol >= down_vol && can_expand_up {
                va_high += 1;
                va_vol += buckets[va_high];
            } else if can_expand_down {
                va_low -= 1;
                va_vol += buckets[va_low];
            } else {
                va_high += 1;
                va_vol += buckets[va_high];
            }
        }

        let bucket_to_price = |idx: usize| -> Decimal {
            price_min + bucket_size * Decimal::from(idx)
        };

        Some(ProfileLevels {
            poc: bucket_to_price(poc_idx) + bucket_size / dec!(2),
            val: bucket_to_price(va_low),
            vah: bucket_to_price(va_high) + bucket_size,
        })
    }
}

trait ToU64Saturating {
    fn to_u64_saturating(self) -> u64;
}

impl ToU64Saturating for Decimal {
    fn to_u64_saturating(self) -> u64 {
        if self < Decimal::ZERO {
            0
        } else {
            self.to_string()
                .split('.')
                .next()
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0)
        }
    }
}

impl Head for VolProfileHead {
    fn id(&self) -> HeadId {
        HeadId::VolProfile
    }

    fn evaluate(
        &mut self,
        bar: &Bar,
        session: &SessionProfile,
        regime: &RegimeSignal9,
    ) -> Vec<TradeSignal> {
        if bar.timeframe != Timeframe::M15 {
            return vec![];
        }

        let atr_val = self.atr.update(bar);

        self.history.push_back(bar.clone());
        if self.history.len() > self.config.lookback {
            self.history.pop_front();
        }

        self.bar_count += 1;

        let Some(atr) = atr_val else {
            return vec![];
        };

        if session.session == Session::Dead {
            return vec![];
        }

        if self.bar_count.saturating_sub(self.last_signal_bar) < self.config.cool_bars {
            return vec![];
        }

        let Some(levels) = self.compute_profile() else {
            return vec![];
        };

        let val = levels.val;
        let vah = levels.vah;
        let poc = levels.poc;

        // BUY at VAL: price tests VAL from above and closes back above it
        if bar.low <= val && bar.close >= val {
            let entry = bar.close;
            let sl = val - atr * self.config.sl_atr_mult;
            let risk = entry - sl;
            if risk > Decimal::ZERO {
                let tp = poc; // TP at POC
                let rr = if risk > Decimal::ZERO {
                    (tp - entry).abs() / risk
                } else {
                    Decimal::ZERO
                };
                if tp > entry && rr >= self.config.min_rr {
                    self.last_signal_bar = self.bar_count;
                    return vec![TradeSignal {
                        symbol: self.config.symbol.clone(),
                        direction: Direction::Buy,
                        kind: SignalKind::Open,
                        entry: dec!(0),
                        stop_loss: sl,
                        take_profit: tp,
                        take_profit2: Some(vah),
                        head: HeadId::VolProfile,
                        head_confidence: self.config.base_confidence,
                        regime: regime.regime,
                        session: session.session,
                        pyramid_level: 0,
                        comment: format!(
                            "VolProfile BUY VAL={:.5} POC={:.5} VAH={:.5}",
                            val, poc, vah
                        ),
                        generated_at: bar.timestamp,
                    }];
                }
            }
        }

        // SELL at VAH: price tests VAH from below and closes back below it
        if bar.high >= vah && bar.close <= vah {
            let entry = bar.close;
            let sl = vah + atr * self.config.sl_atr_mult;
            let risk = sl - entry;
            if risk > Decimal::ZERO {
                let tp = poc;
                let rr = if risk > Decimal::ZERO {
                    (entry - tp).abs() / risk
                } else {
                    Decimal::ZERO
                };
                if tp < entry && rr >= self.config.min_rr {
                    self.last_signal_bar = self.bar_count;
                    return vec![TradeSignal {
                        symbol: self.config.symbol.clone(),
                        direction: Direction::Sell,
                        kind: SignalKind::Open,
                        entry: dec!(0),
                        stop_loss: sl,
                        take_profit: tp,
                        take_profit2: Some(val),
                        head: HeadId::VolProfile,
                        head_confidence: self.config.base_confidence,
                        regime: regime.regime,
                        session: session.session,
                        pyramid_level: 0,
                        comment: format!(
                            "VolProfile SELL VAH={:.5} POC={:.5} VAL={:.5}",
                            vah, poc, val
                        ),
                        generated_at: bar.timestamp,
                    }];
                }
            }
        }

        vec![]
    }

    fn reset(&mut self) {
        self.atr.reset();
        self.history.clear();
        self.last_signal_bar = i64::MIN;
        self.bar_count = 0;
    }

    fn warmup_bars(&self) -> usize {
        self.config.lookback + 14
    }

    fn regime_allowed(&self, regime: &RegimeSignal9) -> bool {
        matches!(
            regime.regime,
            Regime9::WeakTrendUp
                | Regime9::WeakTrendDown
                | Regime9::RangingTight
                | Regime9::Transitioning
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn regime(r: Regime9) -> RegimeSignal9 {
        RegimeSignal9 {
            regime: r,
            confidence: dec!(0.7),
            adx: dec!(20),
            hurst: dec!(0.55),
            atr_ratio: dec!(1.0),
            bb_width_pctile: dec!(0.4),
            choppiness_index: dec!(55),
            computed_at: 0,
        }
    }

    fn bar(ts: i64, close: Decimal, high: Decimal, low: Decimal, vol: u64) -> Bar {
        Bar {
            timestamp: ts,
            open: close,
            high,
            low,
            close,
            volume: vol,
            timeframe: Timeframe::M15,
        }
    }

    #[test]
    fn no_signal_during_warmup() {
        let mut head = VolProfileHead::new(VolProfileConfig::default());
        let sess = SessionProfile::from_utc_hour(10);
        let reg = regime(Regime9::RangingTight);
        for i in 0..15 {
            let c = dec!(1.1000);
            let sigs = head.evaluate(
                &bar(i * 900, c, c + dec!(0.0010), c - dec!(0.0010), 500),
                &sess,
                &reg,
            );
            assert!(sigs.is_empty());
        }
    }

    #[test]
    fn regime_allowed_check() {
        let head = VolProfileHead::new(VolProfileConfig::default());
        assert!(head.regime_allowed(&regime(Regime9::RangingTight)));
        assert!(head.regime_allowed(&regime(Regime9::WeakTrendUp)));
        assert!(head.regime_allowed(&regime(Regime9::Transitioning)));
        assert!(!head.regime_allowed(&regime(Regime9::StrongTrendUp)));
        assert!(!head.regime_allowed(&regime(Regime9::Choppy)));
    }

    #[test]
    fn ignores_non_m15() {
        let mut head = VolProfileHead::new(VolProfileConfig::default());
        let sess = SessionProfile::from_utc_hour(10);
        let reg = regime(Regime9::RangingTight);
        let b = Bar {
            timestamp: 0,
            open: dec!(1.1),
            high: dec!(1.11),
            low: dec!(1.09),
            close: dec!(1.1),
            volume: 500,
            timeframe: Timeframe::M1,
        };
        assert!(head.evaluate(&b, &sess, &reg).is_empty());
    }
}
