//! ScalpM5Head — M5 EMA-crossover scalper anchored to VWAP.
//!
//! Fires on M5 bars during London (07:00–12:00 UTC) and Overlap (12:00–16:00 UTC).
//! VWAP resets at 00:00 UTC each day.
//!
//! Entry conditions:
//!   BUY:  EMA(10) crosses above EMA(21) and bar close is within
//!         vwap_band_mult × ATR below VWAP (not extended too far from value).
//!   SELL: EMA(10) crosses below EMA(21) and bar close is within
//!         vwap_band_mult × ATR above VWAP.
//!
//! SL: ATR × sl_atr_mult from entry.
//! TP: SL × tp_r_mult.

use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::heads::Head;
use crate::indicators::{ATR, EMA, VWAP};
use crate::types::{
    Bar, Direction, HeadId, Regime9, RegimeSignal9, Session, SessionProfile, SignalKind, Timeframe,
    TradeSignal,
};

#[derive(Debug, Clone)]
pub struct ScalpM5Config {
    /// Fast EMA period (default 10).
    pub fast_period: usize,
    /// Slow EMA period (default 21).
    pub slow_period: usize,
    /// Price must be within vwap_band_mult×ATR of VWAP.
    pub vwap_band_mult: Decimal,
    /// SL = ATR × sl_atr_mult.
    pub sl_atr_mult: Decimal,
    /// TP = SL × tp_r_mult.
    pub tp_r_mult: Decimal,
    pub min_rr: Decimal,
    pub base_confidence: Decimal,
    /// Cool-down in M5 bars after a signal.
    pub cool_bars: i64,
    pub symbol: String,
}

impl Default for ScalpM5Config {
    fn default() -> Self {
        Self {
            fast_period: 10,
            slow_period: 21,
            vwap_band_mult: dec!(2.0),
            sl_atr_mult: dec!(1.3),
            tp_r_mult: dec!(2.0),
            min_rr: dec!(1.5),
            base_confidence: dec!(0.62),
            cool_bars: 6, // 30 minutes
            symbol: "EURUSD".to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ScalpM5Head {
    config: ScalpM5Config,
    ema_fast: EMA,
    ema_slow: EMA,
    atr: ATR,
    vwap: VWAP,
    prev_fast_above: Option<bool>,
    last_signal_bar: i64,
    bar_count: i64,
    current_day: i64,
}

impl ScalpM5Head {
    pub fn new(config: ScalpM5Config) -> Self {
        let f = config.fast_period;
        let s = config.slow_period;
        Self {
            config,
            ema_fast: EMA::new(f),
            ema_slow: EMA::new(s),
            atr: ATR::new(14),
            vwap: VWAP::new(),
            prev_fast_above: None,
            last_signal_bar: i64::MIN,
            bar_count: 0,
            current_day: -1,
        }
    }
}

impl Head for ScalpM5Head {
    fn id(&self) -> HeadId {
        HeadId::ScalpM5
    }

    fn evaluate(
        &mut self,
        bar: &Bar,
        session: &SessionProfile,
        regime: &RegimeSignal9,
    ) -> Vec<TradeSignal> {
        // Only M5 bars
        if bar.timeframe != Timeframe::M5 {
            return vec![];
        }

        // Only London + Overlap sessions
        if !matches!(session.session, Session::London | Session::Overlap) {
            return vec![];
        }

        // VWAP daily reset
        let day = bar.timestamp / 86_400;
        if day != self.current_day {
            self.vwap.reset();
            self.current_day = day;
        }

        let ef_opt = self.ema_fast.update(bar.close);
        let es_opt = self.ema_slow.update(bar.close);
        let atr_val = self.atr.update(bar);
        // VWAP resets happen manually at day change above; pass false here
        let vwap = self.vwap.update(bar, false);

        self.bar_count += 1;

        // Compute fast_above from references before consuming the Options
        let fast_above_now = match (&ef_opt, &es_opt) {
            (Some(f), Some(s)) => Some(f > s),
            _ => None,
        };
        let prev_above = std::mem::replace(&mut self.prev_fast_above, fast_above_now);

        let (Some(ef), Some(es), Some(atr)) = (ef_opt, es_opt, atr_val) else {
            return vec![];
        };

        let fast_above = ef > es;

        let Some(prev) = prev_above else {
            return vec![];
        };

        // Cool-down gate
        if self.bar_count - self.last_signal_bar < self.config.cool_bars {
            return vec![];
        }

        let band = atr * self.config.vwap_band_mult;
        let price_near_vwap = bar.close >= vwap - band && bar.close <= vwap + band;

        if !price_near_vwap {
            return vec![];
        }

        // BUY: EMA cross up
        if !prev && fast_above {
            let entry = bar.close;
            let sl = entry - atr * self.config.sl_atr_mult;
            let risk = entry - sl;
            if risk <= Decimal::ZERO {
                return vec![];
            }
            let tp = entry + risk * self.config.tp_r_mult;
            let rr = (tp - entry) / risk;
            if rr < self.config.min_rr {
                return vec![];
            }
            self.last_signal_bar = self.bar_count;
            return vec![TradeSignal {
                symbol: self.config.symbol.clone(),
                direction: Direction::Buy,
                kind: SignalKind::Open,
                entry: dec!(0),
                stop_loss: sl,
                take_profit: tp,
                take_profit2: None,
                head: HeadId::ScalpM5,
                head_confidence: self.config.base_confidence,
                regime: regime.regime,
                session: session.session,
                pyramid_level: 0,
                comment: format!(
                    "ScalpM5 BUY EMA{}×{} VWAP={:.5}",
                    self.config.fast_period, self.config.slow_period, vwap
                ),
                generated_at: bar.timestamp,
            }];
        }

        // SELL: EMA cross down
        if prev && !fast_above {
            let entry = bar.close;
            let sl = entry + atr * self.config.sl_atr_mult;
            let risk = sl - entry;
            if risk <= Decimal::ZERO {
                return vec![];
            }
            let tp = entry - risk * self.config.tp_r_mult;
            let rr = (entry - tp) / risk;
            if rr < self.config.min_rr {
                return vec![];
            }
            self.last_signal_bar = self.bar_count;
            return vec![TradeSignal {
                symbol: self.config.symbol.clone(),
                direction: Direction::Sell,
                kind: SignalKind::Open,
                entry: dec!(0),
                stop_loss: sl,
                take_profit: tp,
                take_profit2: None,
                head: HeadId::ScalpM5,
                head_confidence: self.config.base_confidence,
                regime: regime.regime,
                session: session.session,
                pyramid_level: 0,
                comment: format!(
                    "ScalpM5 SELL EMA{}×{} VWAP={:.5}",
                    self.config.fast_period, self.config.slow_period, vwap
                ),
                generated_at: bar.timestamp,
            }];
        }

        vec![]
    }

    fn reset(&mut self) {
        self.ema_fast.reset();
        self.ema_slow.reset();
        self.atr.reset();
        self.vwap.reset();
        self.prev_fast_above = None;
        self.last_signal_bar = i64::MIN;
        self.bar_count = 0;
        self.current_day = -1;
    }

    fn warmup_bars(&self) -> usize {
        self.config.slow_period * 2
    }

    fn regime_allowed(&self, regime: &RegimeSignal9) -> bool {
        // Session-gated internally; best in trending + transitioning
        !matches!(regime.regime, Regime9::Choppy | Regime9::RangingTight)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn london_session() -> SessionProfile {
        SessionProfile::from_utc_hour(9)
    }

    fn regime_trend() -> RegimeSignal9 {
        RegimeSignal9 {
            regime: Regime9::StrongTrendUp,
            confidence: dec!(0.8),
            adx: dec!(30),
            hurst: dec!(0.7),
            atr_ratio: dec!(1.0),
            bb_width_pctile: dec!(0.6),
            choppiness_index: dec!(35),
            computed_at: 0,
        }
    }

    fn m5_bar(ts: i64, close: Decimal) -> Bar {
        Bar {
            timestamp: ts,
            open: close,
            high: close + dec!(0.0005),
            low: close - dec!(0.0005),
            close,
            volume: 300,
            timeframe: Timeframe::M5,
        }
    }

    #[test]
    fn ignores_m15_bars() {
        let mut head = ScalpM5Head::new(ScalpM5Config::default());
        let sess = london_session();
        let reg = regime_trend();
        let bar = Bar {
            timestamp: 9 * 3600,
            open: dec!(1.1000),
            high: dec!(1.1010),
            low: dec!(1.0990),
            close: dec!(1.1000),
            volume: 500,
            timeframe: Timeframe::M15,
        };
        assert!(head.evaluate(&bar, &sess, &reg).is_empty());
    }

    #[test]
    fn ignores_asian_session() {
        let mut head = ScalpM5Head::new(ScalpM5Config::default());
        let sess = SessionProfile::from_utc_hour(3); // Asian
        let reg = regime_trend();
        for i in 0..30 {
            let sigs = head.evaluate(&m5_bar(3 * 3600 + i * 300, dec!(1.1000)), &sess, &reg);
            assert!(sigs.is_empty());
        }
    }

    #[test]
    fn regime_blocked_in_choppy() {
        let head = ScalpM5Head::new(ScalpM5Config::default());
        let reg = RegimeSignal9 {
            regime: Regime9::Choppy,
            confidence: dec!(0.5),
            adx: dec!(10),
            hurst: dec!(0.5),
            atr_ratio: dec!(1.0),
            bb_width_pctile: dec!(0.3),
            choppiness_index: dec!(70),
            computed_at: 0,
        };
        assert!(!head.regime_allowed(&reg));
    }
}
