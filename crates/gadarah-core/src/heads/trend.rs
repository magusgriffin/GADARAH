//! TrendHead — EMA crossover with ADX-filtered pullback entries.
//!
//! Strategy:
//!   1. EMA(21) crosses EMA(50) → establishes trend direction.
//!   2. ADX(14) > 25 confirms the trend is strong enough to trade.
//!   3. Entry on the first pullback where price's low (BUY) or high (SELL)
//!      tags the EMA(21) and the close is back on the trend side.
//!   4. SL: EMA21 − ATR×sl_mult (buy) or EMA21 + ATR×sl_mult (sell).
//!   5. TP: entry + (entry − SL) × tp_r_mult.
//!
//! Allowed regimes: StrongTrendUp, StrongTrendDown, WeakTrendUp, WeakTrendDown.

use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::heads::Head;
use crate::indicators::{ADX, ATR, EMA};
use crate::types::{
    Bar, Direction, HeadId, Regime9, RegimeSignal9, Session, SessionProfile, SignalKind,
    TradeSignal,
};

#[derive(Debug, Clone)]
pub struct TrendConfig {
    /// Fast EMA period (default 21).
    pub fast_period: usize,
    /// Slow EMA period (default 50).
    pub slow_period: usize,
    /// ADX period (default 14).
    pub adx_period: usize,
    /// Minimum ADX value to consider trend strong (default 25).
    pub adx_threshold: Decimal,
    /// SL distance = ATR × sl_atr_mult.
    pub sl_atr_mult: Decimal,
    /// TP distance = SL × tp_r_mult.
    pub tp_r_mult: Decimal,
    pub min_rr: Decimal,
    pub base_confidence: Decimal,
    pub symbol: String,
}

impl Default for TrendConfig {
    fn default() -> Self {
        Self {
            fast_period: 21,
            slow_period: 50,
            adx_period: 14,
            adx_threshold: dec!(25),
            sl_atr_mult: dec!(1.5),
            tp_r_mult: dec!(2.5),
            min_rr: dec!(1.8),
            base_confidence: dec!(0.65),
            symbol: "EURUSD".to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct TrendHead {
    config: TrendConfig,
    ema_fast: EMA,
    ema_slow: EMA,
    adx: ADX,
    atr: ATR,
    /// Currently established trend direction (set when EMAs cross + ADX confirms).
    trend_dir: Option<Direction>,
    /// Whether the EMA fast is above EMA slow (used to detect cross).
    prev_fast_above: Option<bool>,
    /// Whether a pullback (tag of EMA fast) has been observed in the current trend.
    pullback_armed: bool,
    /// Prevent spamming: one signal per trend leg (reset on cross).
    signal_issued: bool,
}

impl TrendHead {
    pub fn new(config: TrendConfig) -> Self {
        let fast = config.fast_period;
        let slow = config.slow_period;
        let adx_p = config.adx_period;
        Self {
            config,
            ema_fast: EMA::new(fast),
            ema_slow: EMA::new(slow),
            adx: ADX::new(adx_p),
            atr: ATR::new(14),
            trend_dir: None,
            prev_fast_above: None,
            pullback_armed: false,
            signal_issued: false,
        }
    }
}

impl Head for TrendHead {
    fn id(&self) -> HeadId {
        HeadId::Trend
    }

    fn evaluate(
        &mut self,
        bar: &Bar,
        session: &SessionProfile,
        regime: &RegimeSignal9,
    ) -> Vec<TradeSignal> {
        let ema_f = self.ema_fast.update(bar.close);
        let ema_s = self.ema_slow.update(bar.close);
        self.adx.update(bar);
        let atr_val = self.atr.update(bar);

        let (Some(ef), Some(es), Some(atr)) = (ema_f, ema_s, atr_val) else {
            return vec![];
        };
        let adx_val = self.adx.value().unwrap_or(Decimal::ZERO);

        let fast_above = ef > es;

        // Detect EMA cross
        if let Some(prev_above) = self.prev_fast_above {
            if prev_above != fast_above {
                // Cross happened — reset trend state
                self.trend_dir = None;
                self.pullback_armed = false;
                self.signal_issued = false;
            }
        }
        self.prev_fast_above = Some(fast_above);

        // Establish trend only when ADX confirms strength
        if adx_val >= self.config.adx_threshold {
            self.trend_dir = Some(if fast_above {
                Direction::Buy
            } else {
                Direction::Sell
            });
        }

        let Some(dir) = self.trend_dir else {
            return vec![];
        };

        if self.signal_issued {
            return vec![];
        }

        // Dead session → skip
        if session.session == Session::Dead {
            return vec![];
        }

        // Detect pullback: bar touches EMA fast from the correct side
        let touched_ema = match dir {
            Direction::Buy => bar.low <= ef,
            Direction::Sell => bar.high >= ef,
        };
        if touched_ema {
            self.pullback_armed = true;
        }

        if !self.pullback_armed {
            return vec![];
        }

        // Entry: after pullback, bar closes back on trend side
        let entry_triggered = match dir {
            Direction::Buy => bar.close > ef,
            Direction::Sell => bar.close < ef,
        };
        if !entry_triggered {
            return vec![];
        }

        let entry = bar.close;
        let sl_dist = atr * self.config.sl_atr_mult;
        let sl = match dir {
            Direction::Buy => ef - sl_dist,
            Direction::Sell => ef + sl_dist,
        };
        let risk = (entry - sl).abs();
        if risk.is_zero() {
            return vec![];
        }
        let tp = match dir {
            Direction::Buy => entry + risk * self.config.tp_r_mult,
            Direction::Sell => entry - risk * self.config.tp_r_mult,
        };
        let rr = (tp - entry).abs() / risk;
        if rr < self.config.min_rr {
            return vec![];
        }

        self.signal_issued = true;
        self.pullback_armed = false;

        vec![TradeSignal {
            symbol: self.config.symbol.clone(),
            direction: dir,
            kind: SignalKind::Open,
            entry: bar.close, // market order; use bar close as effective entry for R:R gating
            stop_loss: sl,
            take_profit: tp,
            take_profit2: None,
            head: HeadId::Trend,
            head_confidence: self.config.base_confidence,
            regime: regime.regime,
            session: session.session,
            pyramid_level: 0,
            comment: format!(
                "Trend pullback EMA{} ADX={:.1}",
                self.config.fast_period, adx_val
            ),
            generated_at: bar.timestamp,
        }]
    }

    fn reset(&mut self) {
        self.ema_fast.reset();
        self.ema_slow.reset();
        self.adx.reset();
        self.atr.reset();
        self.trend_dir = None;
        self.prev_fast_above = None;
        self.pullback_armed = false;
        self.signal_issued = false;
    }

    fn warmup_bars(&self) -> usize {
        self.config.slow_period + self.config.adx_period * 2
    }

    fn regime_allowed(&self, regime: &RegimeSignal9) -> bool {
        matches!(
            regime.regime,
            Regime9::StrongTrendUp
                | Regime9::StrongTrendDown
                | Regime9::WeakTrendUp
                | Regime9::WeakTrendDown
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Timeframe;
    use rust_decimal_macros::dec;

    fn make_bar(ts: i64, close: Decimal, high: Decimal, low: Decimal) -> Bar {
        Bar {
            timestamp: ts,
            open: close,
            high,
            low,
            close,
            volume: 1000,
            timeframe: Timeframe::M15,
        }
    }

    fn regime(r: Regime9) -> RegimeSignal9 {
        RegimeSignal9 {
            regime: r,
            confidence: dec!(0.8),
            adx: dec!(30),
            hurst: dec!(0.6),
            atr_ratio: dec!(1.0),
            bb_width_pctile: dec!(0.5),
            choppiness_index: dec!(40),
            computed_at: 0,
        }
    }

    #[test]
    fn no_signal_during_warmup() {
        let mut head = TrendHead::new(TrendConfig {
            symbol: "EURUSD".to_string(),
            ..Default::default()
        });
        let sess = SessionProfile::from_utc_hour(10);
        let reg = regime(Regime9::StrongTrendUp);
        for i in 0..40 {
            let close = dec!(1.1000) + Decimal::from(i) * dec!(0.0001);
            let sigs = head.evaluate(
                &make_bar(i * 900, close, close + dec!(0.0010), close - dec!(0.0010)),
                &sess,
                &reg,
            );
            assert!(sigs.is_empty(), "should be no signals during warmup");
        }
    }

    #[test]
    fn regime_not_allowed_in_ranging() {
        let head = TrendHead::new(TrendConfig::default());
        let reg = regime(Regime9::RangingTight);
        assert!(!head.regime_allowed(&reg));
    }

    #[test]
    fn regime_allowed_in_strong_trend() {
        let head = TrendHead::new(TrendConfig::default());
        let reg = regime(Regime9::StrongTrendUp);
        assert!(head.regime_allowed(&reg));
    }
}
