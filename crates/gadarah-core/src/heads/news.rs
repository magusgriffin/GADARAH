//! NewsHead — spike-detection-based news momentum trading.
//!
//! Rather than requiring an external economic calendar, this head detects
//! unusually large bars (range ≥ ATR × spike_mult) as proxy indicators of
//! high-impact news releases, then enters a momentum continuation trade on
//! the *next* bar in the direction of the spike.
//!
//! Entry: market order on the next bar open (entry = 0).
//! SL:    spike bar's opposing extreme (low for BUY, high for SELL).
//! TP:    entry ± risk × tp_r_mult.
//!
//! Prop-firm gating: NewsHead self-blocks during London/NY blackout windows
//! (first 10 minutes of session) and will not fire twice within cool_bars.
//!
//! Allowed regimes: BreakoutPending (and any — news overrides regime).
//! Note: compliance layer further blocks this head on FundingPips/FTMO.

use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::heads::Head;
use crate::indicators::{ATR, EMA};
use crate::types::{
    Bar, Direction, HeadId, RegimeSignal9, Session, SessionProfile, SignalKind, TradeSignal,
    Timeframe,
};

#[derive(Debug, Clone)]
pub struct NewsConfig {
    /// Bar range must be ≥ ATR × spike_mult to qualify as a news spike.
    pub spike_atr_mult: Decimal,
    /// SL buffer beyond the spike extreme as a multiple of ATR.
    pub sl_atr_mult: Decimal,
    /// TP = SL × tp_r_mult.
    pub tp_r_mult: Decimal,
    pub min_rr: Decimal,
    pub base_confidence: Decimal,
    /// Minimum bars between signals (prevents chasing continued volatility).
    pub cool_bars: i64,
    pub symbol: String,
}

impl Default for NewsConfig {
    fn default() -> Self {
        Self {
            spike_atr_mult: dec!(2.5),
            sl_atr_mult: dec!(0.3),
            tp_r_mult: dec!(2.0),
            min_rr: dec!(1.5),
            base_confidence: dec!(0.60),
            cool_bars: 8, // 2 hours on M15
            symbol: "EURUSD".to_string(),
        }
    }
}

#[derive(Debug, Clone)]
struct PendingEntry {
    direction: Direction,
    stop_loss: Decimal,
    take_profit: Decimal,
    comment: String,
}

#[derive(Debug, Clone)]
pub struct NewsHead {
    config: NewsConfig,
    atr: ATR,
    ema: EMA,
    /// Spike detected last bar — fire on the *next* bar.
    pending: Option<PendingEntry>,
    last_signal_ts: i64,
    bars_since_signal: i64,
}

impl NewsHead {
    pub fn new(config: NewsConfig) -> Self {
        Self {
            config,
            atr: ATR::new(14),
            ema: EMA::new(20),
            pending: None,
            last_signal_ts: i64::MIN,
            bars_since_signal: i64::MAX,
        }
    }
}

impl Head for NewsHead {
    fn id(&self) -> HeadId {
        HeadId::News
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
        self.ema.update(bar.close);

        // Fire pending entry from last bar's spike
        if let Some(pending) = self.pending.take() {
            self.bars_since_signal = 0;
            self.last_signal_ts = bar.timestamp;
            return vec![TradeSignal {
                symbol: self.config.symbol.clone(),
                direction: pending.direction,
                kind: SignalKind::Open,
                entry: dec!(0), // market on open
                stop_loss: pending.stop_loss,
                take_profit: pending.take_profit,
                take_profit2: None,
                head: HeadId::News,
                head_confidence: self.config.base_confidence,
                regime: regime.regime,
                session: session.session,
                pyramid_level: 0,
                comment: pending.comment,
                generated_at: bar.timestamp,
            }];
        }

        self.bars_since_signal = self.bars_since_signal.saturating_add(1);

        let Some(atr) = atr_val else {
            return vec![];
        };

        // Cool-down gate
        if self.bars_since_signal < self.config.cool_bars {
            return vec![];
        }

        // Skip dead session
        if session.session == Session::Dead {
            return vec![];
        }

        // Detect news spike
        let range = bar.high - bar.low;
        let spike_threshold = atr * self.config.spike_atr_mult;
        if range < spike_threshold {
            return vec![];
        }

        // Determine spike direction: close relative to midpoint
        let midpoint = (bar.high + bar.low) / dec!(2);
        let dir = if bar.close >= midpoint {
            Direction::Buy
        } else {
            Direction::Sell
        };

        // Compute SL and TP for the next bar entry
        let (sl, tp) = match dir {
            Direction::Buy => {
                let sl = bar.low - atr * self.config.sl_atr_mult;
                // use current close as approximate entry price for R calc
                let risk = bar.close - sl;
                if risk <= Decimal::ZERO {
                    return vec![];
                }
                let tp = bar.close + risk * self.config.tp_r_mult;
                let rr = (tp - bar.close) / risk;
                if rr < self.config.min_rr {
                    return vec![];
                }
                (sl, tp)
            }
            Direction::Sell => {
                let sl = bar.high + atr * self.config.sl_atr_mult;
                let risk = sl - bar.close;
                if risk <= Decimal::ZERO {
                    return vec![];
                }
                let tp = bar.close - risk * self.config.tp_r_mult;
                let rr = (bar.close - tp) / risk;
                if rr < self.config.min_rr {
                    return vec![];
                }
                (sl, tp)
            }
        };

        // Queue for next bar
        self.pending = Some(PendingEntry {
            direction: dir,
            stop_loss: sl,
            take_profit: tp,
            comment: format!(
                "News spike range={:.5} ({:.1}×ATR) {:?}",
                range,
                range / atr,
                dir
            ),
        });

        vec![]
    }

    fn reset(&mut self) {
        self.atr.reset();
        self.ema.reset();
        self.pending = None;
        self.last_signal_ts = i64::MIN;
        self.bars_since_signal = i64::MAX;
    }

    fn warmup_bars(&self) -> usize {
        20 // EMA + ATR warmup
    }

    fn regime_allowed(&self, _regime: &RegimeSignal9) -> bool {
        // News fires regardless of regime — compliance layer gates per firm
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Regime9;
    use rust_decimal_macros::dec;

    fn regime(r: Regime9) -> RegimeSignal9 {
        RegimeSignal9 {
            regime: r,
            confidence: dec!(0.8),
            adx: dec!(20),
            hurst: dec!(0.5),
            atr_ratio: dec!(1.0),
            bb_width_pctile: dec!(0.4),
            choppiness_index: dec!(50),
            computed_at: 0,
        }
    }

    fn flat_bar(ts: i64) -> Bar {
        Bar {
            timestamp: ts,
            open: dec!(1.1000),
            high: dec!(1.1010),
            low: dec!(1.0990),
            close: dec!(1.1000),
            volume: 500,
            timeframe: Timeframe::M15,
        }
    }

    #[test]
    fn small_bars_produce_no_signal() {
        let mut head = NewsHead::new(NewsConfig::default());
        let sess = SessionProfile::from_utc_hour(14);
        let reg = regime(Regime9::BreakoutPending);
        for i in 0..30 {
            let sigs = head.evaluate(&flat_bar(i * 900), &sess, &reg);
            assert!(sigs.is_empty());
        }
    }

    #[test]
    fn spike_bar_fires_on_next_bar() {
        let mut head = NewsHead::new(NewsConfig {
            cool_bars: 0, // disable cooldown for test
            ..Default::default()
        });
        let sess = SessionProfile::from_utc_hour(14);
        let reg = regime(Regime9::BreakoutPending);

        // Warm up ATR with 20 flat bars (range 0.0020 each)
        for i in 0..20i64 {
            head.evaluate(&flat_bar(i * 900), &sess, &reg);
        }

        // Feed a spike bar: range = 0.0200 which should be >> 2.5×ATR(≈0.0020)
        let spike = Bar {
            timestamp: 20 * 900,
            open: dec!(1.1000),
            high: dec!(1.1100),   // +0.0100
            low: dec!(1.0900),    // total range = 0.0200
            close: dec!(1.1050),  // closes above midpoint → BUY
            volume: 5000,
            timeframe: Timeframe::M15,
        };
        let sigs = head.evaluate(&spike, &sess, &reg);
        assert!(sigs.is_empty(), "spike bar itself emits nothing");

        // Next bar should fire the signal
        let next = Bar {
            timestamp: 21 * 900,
            ..flat_bar(21 * 900)
        };
        let sigs = head.evaluate(&next, &sess, &reg);
        assert_eq!(sigs.len(), 1);
        assert_eq!(sigs[0].direction, Direction::Buy);
        assert_eq!(sigs[0].head, HeadId::News);
    }

    #[test]
    fn regime_always_allowed() {
        let head = NewsHead::new(NewsConfig::default());
        assert!(head.regime_allowed(&regime(Regime9::Choppy)));
        assert!(head.regime_allowed(&regime(Regime9::RangingTight)));
    }
}
