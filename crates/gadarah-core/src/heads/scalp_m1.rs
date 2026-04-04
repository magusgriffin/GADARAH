//! ScalpM1Head — intraday M1 scalper using VWAP deviation + RSI mean-reversion.
//!
//! Only fires on M1 bars during the Overlap session (12:00–16:00 UTC).
//! VWAP resets at 00:00 UTC each day.
//!
//! Entry conditions:
//!   BUY:  RSI(14) crosses above 35 from below while price is within
//!         vwap_band_mult × ATR below VWAP (mean-reversion dip).
//!   SELL: RSI(14) crosses below 65 from above while price is within
//!         vwap_band_mult × ATR above VWAP.
//!
//! SL: ATR × sl_atr_mult from entry.
//! TP: entry ± SL_distance × tp_r_mult.
//!
//! Maximum one active signal per session; cool-down of cool_minutes after close.

use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::heads::Head;
use crate::indicators::{RSI, ATR, VWAP};
use crate::types::{
    Bar, Direction, HeadId, RegimeSignal9, Session, SessionProfile, SignalKind, TradeSignal,
    Timeframe,
};

#[derive(Debug, Clone)]
pub struct ScalpM1Config {
    /// RSI period (default 14).
    pub rsi_period: usize,
    /// RSI oversold threshold for BUY (default 35).
    pub rsi_oversold: Decimal,
    /// RSI overbought threshold for SELL (default 65).
    pub rsi_overbought: Decimal,
    /// Price must be within vwap_band_mult×ATR of VWAP to qualify.
    pub vwap_band_mult: Decimal,
    /// SL distance = ATR × sl_atr_mult.
    pub sl_atr_mult: Decimal,
    /// TP = SL × tp_r_mult.
    pub tp_r_mult: Decimal,
    pub min_rr: Decimal,
    pub base_confidence: Decimal,
    /// Cool-down in M1 bars after each signal.
    pub cool_bars: i64,
    pub symbol: String,
}

impl Default for ScalpM1Config {
    fn default() -> Self {
        Self {
            rsi_period: 14,
            rsi_oversold: dec!(35),
            rsi_overbought: dec!(65),
            vwap_band_mult: dec!(1.5),
            sl_atr_mult: dec!(1.2),
            tp_r_mult: dec!(1.5),
            min_rr: dec!(1.3),
            base_confidence: dec!(0.60),
            cool_bars: 15, // 15 M1 bars = 15 minutes
            symbol: "EURUSD".to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ScalpM1Head {
    config: ScalpM1Config,
    rsi: RSI,
    atr: ATR,
    vwap: VWAP,
    prev_rsi: Option<Decimal>,
    last_signal_bar: i64, // bar counter
    bar_count: i64,
    current_day: i64, // unix day number for VWAP reset
}

impl ScalpM1Head {
    pub fn new(config: ScalpM1Config) -> Self {
        let rsi_p = config.rsi_period;
        Self {
            config,
            rsi: RSI::new(rsi_p),
            atr: ATR::new(14),
            vwap: VWAP::new(),
            prev_rsi: None,
            last_signal_bar: i64::MIN,
            bar_count: 0,
            current_day: -1,
        }
    }
}

impl Head for ScalpM1Head {
    fn id(&self) -> HeadId {
        HeadId::ScalpM1
    }

    fn evaluate(
        &mut self,
        bar: &Bar,
        session: &SessionProfile,
        regime: &RegimeSignal9,
    ) -> Vec<TradeSignal> {
        // Only process M1 bars
        if bar.timeframe != Timeframe::M1 {
            return vec![];
        }

        // Only trade Overlap session
        if session.session != Session::Overlap {
            return vec![];
        }

        // Reset VWAP at start of each new day
        let day = bar.timestamp / 86_400;
        if day != self.current_day {
            self.vwap.reset();
            self.current_day = day;
        }

        let rsi_val = self.rsi.update(bar.close);
        let atr_val = self.atr.update(bar);
        // VWAP resets happen manually at day change above; pass false here
        let vwap = self.vwap.update(bar, false);

        self.bar_count += 1;

        let (Some(rsi), Some(atr)) = (rsi_val, atr_val) else {
            self.prev_rsi = rsi_val;
            return vec![];
        };

        let prev_rsi = self.prev_rsi.replace(rsi);

        // Cool-down gate
        if self.bar_count - self.last_signal_bar < self.config.cool_bars {
            return vec![];
        }

        let Some(prev) = prev_rsi else {
            return vec![];
        };

        let band = atr * self.config.vwap_band_mult;

        // BUY: RSI crosses up through oversold while price is near/below VWAP
        if prev < self.config.rsi_oversold
            && rsi >= self.config.rsi_oversold
            && bar.close <= vwap + band
            && bar.close >= vwap - band
        {
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
                head: HeadId::ScalpM1,
                head_confidence: self.config.base_confidence,
                regime: regime.regime,
                session: session.session,
                pyramid_level: 0,
                comment: format!("ScalpM1 BUY RSI={:.1} VWAP={:.5}", rsi, vwap),
                generated_at: bar.timestamp,
            }];
        }

        // SELL: RSI crosses down through overbought while price is near/above VWAP
        if prev > self.config.rsi_overbought
            && rsi <= self.config.rsi_overbought
            && bar.close >= vwap - band
            && bar.close <= vwap + band
        {
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
                head: HeadId::ScalpM1,
                head_confidence: self.config.base_confidence,
                regime: regime.regime,
                session: session.session,
                pyramid_level: 0,
                comment: format!("ScalpM1 SELL RSI={:.1} VWAP={:.5}", rsi, vwap),
                generated_at: bar.timestamp,
            }];
        }

        vec![]
    }

    fn reset(&mut self) {
        self.rsi.reset();
        self.atr.reset();
        self.vwap.reset();
        self.prev_rsi = None;
        self.last_signal_bar = i64::MIN;
        self.bar_count = 0;
        self.current_day = -1;
    }

    fn warmup_bars(&self) -> usize {
        self.config.rsi_period * 2
    }

    fn regime_allowed(&self, _regime: &RegimeSignal9) -> bool {
        // Session-gated internally; accepts any regime during Overlap
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Regime9;
    use rust_decimal_macros::dec;

    fn overlap_session() -> SessionProfile {
        SessionProfile::from_utc_hour(14) // 14:00 UTC = Overlap
    }

    fn regime_any() -> RegimeSignal9 {
        RegimeSignal9 {
            regime: Regime9::StrongTrendUp,
            confidence: dec!(0.8),
            adx: dec!(30),
            hurst: dec!(0.6),
            atr_ratio: dec!(1.0),
            bb_width_pctile: dec!(0.5),
            choppiness_index: dec!(40),
            computed_at: 0,
        }
    }

    fn m1_bar(ts: i64, close: Decimal) -> Bar {
        Bar {
            timestamp: ts,
            open: close,
            high: close + dec!(0.0003),
            low: close - dec!(0.0003),
            close,
            volume: 200,
            timeframe: Timeframe::M1,
        }
    }

    #[test]
    fn ignores_non_m1_bars() {
        let mut head = ScalpM1Head::new(ScalpM1Config::default());
        let sess = overlap_session();
        let reg = regime_any();
        let bar = Bar {
            timestamp: 14 * 3600,
            open: dec!(1.1000),
            high: dec!(1.1010),
            low: dec!(1.0990),
            close: dec!(1.1000),
            volume: 500,
            timeframe: Timeframe::M15,
        };
        let sigs = head.evaluate(&bar, &sess, &reg);
        assert!(sigs.is_empty());
    }

    #[test]
    fn ignores_non_overlap_session() {
        let mut head = ScalpM1Head::new(ScalpM1Config::default());
        let sess = SessionProfile::from_utc_hour(9); // London, not Overlap
        let reg = regime_any();
        for i in 0..30 {
            let sigs = head.evaluate(
                &m1_bar(9 * 3600 + i * 60, dec!(1.1000)),
                &sess,
                &reg,
            );
            assert!(sigs.is_empty());
        }
    }

    #[test]
    fn regime_always_allowed() {
        let head = ScalpM1Head::new(ScalpM1Config::default());
        let reg = RegimeSignal9 {
            regime: Regime9::Choppy,
            confidence: dec!(0.5),
            adx: dec!(10),
            hurst: dec!(0.5),
            atr_ratio: dec!(1.0),
            bb_width_pctile: dec!(0.5),
            choppiness_index: dec!(70),
            computed_at: 0,
        };
        assert!(head.regime_allowed(&reg));
    }
}
