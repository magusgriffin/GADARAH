//! SmcHead — Smart Money Concepts: Order Blocks + Fair Value Gaps.
//!
//! Detects two SMC patterns in a rolling bar history:
//!
//! **Order Block (OB)**
//!   - Bullish OB: the last bearish candle immediately before a sequence of
//!     3+ strong bullish candles (body ≥ ATR × 0.3 each).
//!     Entry: price returns into [OB.low, OB.high] and the current bar closes
//!     bullish. SL: OB.low - ATR×0.5. TP: swing high above the impulse (or 2R).
//!   - Bearish OB: symmetric — last bullish candle before a bearish impulse.
//!
//! **Fair Value Gap (FVG)**
//!   - Bullish FVG: bars[i-2].high < bars[i].low — a gap that price should
//!     return to fill from above before resuming up.
//!     Entry: price enters the gap from above (bar.low ≤ bars[i-2].high + buffer)
//!     and closes bullish. SL: gap.low - ATR×0.5. TP: 2R.
//!   - Bearish FVG: symmetric.
//!
//! Allowed regimes: StrongTrendUp, StrongTrendDown, RangingWide, BreakoutPending.

use std::collections::VecDeque;

use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::heads::Head;
use crate::indicators::{ATR, EMA};
use crate::types::{
    Bar, Direction, HeadId, Regime9, RegimeSignal9, Session, SessionProfile, SignalKind,
    TradeSignal, Timeframe,
};

const HISTORY: usize = 50;
const IMPULSE_BARS: usize = 3; // min consecutive bars for impulse
const BODY_ATR_MULT: &str = "0.3"; // minimum body size as fraction of ATR

#[derive(Debug, Clone)]
pub struct SmcConfig {
    /// Minimum impulse candle body size as a multiple of ATR.
    pub min_body_atr_mult: Decimal,
    /// OB / FVG SL buffer below the block low (or above block high).
    pub sl_atr_mult: Decimal,
    /// TP as a multiple of risk.
    pub tp_r_mult: Decimal,
    pub min_rr: Decimal,
    pub base_confidence: Decimal,
    pub symbol: String,
}

impl Default for SmcConfig {
    fn default() -> Self {
        Self {
            min_body_atr_mult: BODY_ATR_MULT.parse().unwrap(),
            sl_atr_mult: dec!(0.5),
            tp_r_mult: dec!(2.0),
            min_rr: dec!(1.8),
            base_confidence: dec!(0.70),
            symbol: "EURUSD".to_string(),
        }
    }
}

/// A detected order block zone.
#[derive(Debug, Clone)]
struct OrderBlock {
    high: Decimal,
    low: Decimal,
    direction: Direction, // direction of the impulse AFTER this OB
    swing_target: Decimal, // approximate TP (swing high/low of impulse)
}

/// A detected fair value gap.
#[derive(Debug, Clone)]
struct Fvg {
    top: Decimal,    // upper edge of gap
    bottom: Decimal, // lower edge of gap
    direction: Direction, // direction for continuation (bullish FVG → buy)
}

#[derive(Debug, Clone)]
pub struct SmcHead {
    config: SmcConfig,
    atr: ATR,
    ema: EMA, // trend bias filter (EMA50)
    history: VecDeque<Bar>,
    active_obs: Vec<OrderBlock>,
    active_fvgs: Vec<Fvg>,
    last_signal_ts: i64,
    min_bars_between_signals: i64,
}

impl SmcHead {
    pub fn new(config: SmcConfig) -> Self {
        Self {
            config,
            atr: ATR::new(14),
            ema: EMA::new(50),
            history: VecDeque::with_capacity(HISTORY + 1),
            active_obs: Vec::new(),
            active_fvgs: Vec::new(),
            last_signal_ts: i64::MIN,
            min_bars_between_signals: 3600,
        }
    }

    /// Scan the rolling history for new OBs and FVGs.
    fn scan_patterns(&mut self, atr: Decimal) {
        let bars: Vec<&Bar> = self.history.iter().collect();
        let n = bars.len();
        if n < IMPULSE_BARS + 2 {
            return;
        }

        let min_body = atr * self.config.min_body_atr_mult;

        // ----- Order Block detection -----
        // Look at the last IMPULSE_BARS bars; check whether they form an impulse.
        let impulse_end = n - 1;
        let impulse_start = impulse_end.saturating_sub(IMPULSE_BARS - 1);

        let mut bullish_impulse = true;
        let mut bearish_impulse = true;
        for i in impulse_start..=impulse_end {
            let b = bars[i];
            let body = (b.close - b.open).abs();
            if b.close <= b.open || body < min_body {
                bullish_impulse = false;
            }
            if b.close >= b.open || body < min_body {
                bearish_impulse = false;
            }
        }

        if bullish_impulse && impulse_start >= 1 {
            let ob_bar = bars[impulse_start - 1];
            // OB must be bearish (last down-candle before the up-impulse)
            if ob_bar.close < ob_bar.open {
                let swing_high = bars[impulse_start..=impulse_end]
                    .iter()
                    .map(|b| b.high)
                    .max()
                    .unwrap_or(ob_bar.high);
                self.active_obs.push(OrderBlock {
                    high: ob_bar.high,
                    low: ob_bar.low,
                    direction: Direction::Buy,
                    swing_target: swing_high,
                });
            }
        }

        if bearish_impulse && impulse_start >= 1 {
            let ob_bar = bars[impulse_start - 1];
            if ob_bar.close > ob_bar.open {
                let swing_low = bars[impulse_start..=impulse_end]
                    .iter()
                    .map(|b| b.low)
                    .min()
                    .unwrap_or(ob_bar.low);
                self.active_obs.push(OrderBlock {
                    high: ob_bar.high,
                    low: ob_bar.low,
                    direction: Direction::Sell,
                    swing_target: swing_low,
                });
            }
        }

        // ----- FVG detection (on the last completed 3-bar sequence) -----
        if n >= 3 {
            let b0 = bars[n - 3];
            let b2 = bars[n - 1];
            // Bullish FVG: gap between b0.high and b2.low
            if b0.high < b2.low {
                self.active_fvgs.push(Fvg {
                    top: b2.low,
                    bottom: b0.high,
                    direction: Direction::Buy,
                });
            }
            // Bearish FVG: gap between b2.high and b0.low
            if b2.high < b0.low {
                self.active_fvgs.push(Fvg {
                    top: b0.low,
                    bottom: b2.high,
                    direction: Direction::Sell,
                });
            }
        }

        // Prune stale patterns (older than 20 bars equivalent)
        self.active_obs.retain(|_| true); // keep all until invalidated below
        self.active_fvgs.retain(|_| true);
        // Cap lists to avoid unbounded growth
        if self.active_obs.len() > 10 {
            self.active_obs.drain(0..self.active_obs.len() - 10);
        }
        if self.active_fvgs.len() > 10 {
            self.active_fvgs.drain(0..self.active_fvgs.len() - 10);
        }
    }

    fn try_ob_entry(
        &mut self,
        bar: &Bar,
        atr: Decimal,
        regime: &RegimeSignal9,
        session: &SessionProfile,
    ) -> Option<TradeSignal> {
        for ob in &self.active_obs {
            match ob.direction {
                Direction::Buy => {
                    // Price returns into OB zone from above; bar closes bullish
                    if bar.low <= ob.high
                        && bar.close > ob.low
                        && bar.close > bar.open
                    {
                        let entry = bar.close;
                        let sl = ob.low - atr * self.config.sl_atr_mult;
                        let risk = entry - sl;
                        if risk <= Decimal::ZERO {
                            continue;
                        }
                        let tp = entry + risk * self.config.tp_r_mult;
                        let rr = (tp - entry) / risk;
                        if rr < self.config.min_rr {
                            continue;
                        }
                        let tp = tp.max(ob.swing_target); // use swing high as TP if larger
                        return Some(self.make_signal(
                            bar,
                            Direction::Buy,
                            sl,
                            tp,
                            format!("SMC OB BUY [{:.5},{:.5}]", ob.low, ob.high),
                            regime,
                            session,
                        ));
                    }
                }
                Direction::Sell => {
                    if bar.high >= ob.low
                        && bar.close < ob.high
                        && bar.close < bar.open
                    {
                        let entry = bar.close;
                        let sl = ob.high + atr * self.config.sl_atr_mult;
                        let risk = sl - entry;
                        if risk <= Decimal::ZERO {
                            continue;
                        }
                        let tp = entry - risk * self.config.tp_r_mult;
                        let rr = (entry - tp) / risk;
                        if rr < self.config.min_rr {
                            continue;
                        }
                        let tp = tp.min(ob.swing_target); // use swing low as TP if smaller
                        return Some(self.make_signal(
                            bar,
                            Direction::Sell,
                            sl,
                            tp,
                            format!("SMC OB SELL [{:.5},{:.5}]", ob.low, ob.high),
                            regime,
                            session,
                        ));
                    }
                }
            }
        }
        None
    }

    fn try_fvg_entry(
        &mut self,
        bar: &Bar,
        atr: Decimal,
        regime: &RegimeSignal9,
        session: &SessionProfile,
    ) -> Option<TradeSignal> {
        for fvg in &self.active_fvgs {
            match fvg.direction {
                Direction::Buy => {
                    // Price drops into FVG (bullish), then closes up
                    if bar.low <= fvg.top
                        && bar.low >= fvg.bottom
                        && bar.close > fvg.top // closes back above gap top
                    {
                        let entry = bar.close;
                        let sl = fvg.bottom - atr * self.config.sl_atr_mult;
                        let risk = entry - sl;
                        if risk <= Decimal::ZERO {
                            continue;
                        }
                        let tp = entry + risk * self.config.tp_r_mult;
                        let rr = (tp - entry) / risk;
                        if rr < self.config.min_rr {
                            continue;
                        }
                        return Some(self.make_signal(
                            bar,
                            Direction::Buy,
                            sl,
                            tp,
                            format!("SMC FVG BUY [{:.5},{:.5}]", fvg.bottom, fvg.top),
                            regime,
                            session,
                        ));
                    }
                }
                Direction::Sell => {
                    if bar.high >= fvg.bottom
                        && bar.high <= fvg.top
                        && bar.close < fvg.bottom
                    {
                        let entry = bar.close;
                        let sl = fvg.top + atr * self.config.sl_atr_mult;
                        let risk = sl - entry;
                        if risk <= Decimal::ZERO {
                            continue;
                        }
                        let tp = entry - risk * self.config.tp_r_mult;
                        let rr = (entry - tp) / risk;
                        if rr < self.config.min_rr {
                            continue;
                        }
                        return Some(self.make_signal(
                            bar,
                            Direction::Sell,
                            sl,
                            tp,
                            format!("SMC FVG SELL [{:.5},{:.5}]", fvg.bottom, fvg.top),
                            regime,
                            session,
                        ));
                    }
                }
            }
        }
        None
    }

    fn make_signal(
        &self,
        bar: &Bar,
        dir: Direction,
        sl: Decimal,
        tp: Decimal,
        comment: String,
        regime: &RegimeSignal9,
        session: &SessionProfile,
    ) -> TradeSignal {
        TradeSignal {
            symbol: self.config.symbol.clone(),
            direction: dir,
            kind: SignalKind::Open,
            entry: dec!(0), // market
            stop_loss: sl,
            take_profit: tp,
            take_profit2: None,
            head: HeadId::Smc,
            head_confidence: self.config.base_confidence,
            regime: regime.regime,
            session: session.session,
            pyramid_level: 0,
            comment,
            generated_at: bar.timestamp,
        }
    }
}

impl Head for SmcHead {
    fn id(&self) -> HeadId {
        HeadId::Smc
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

        self.history.push_back(bar.clone());
        if self.history.len() > HISTORY {
            self.history.pop_front();
        }

        let Some(atr) = atr_val else {
            return vec![];
        };

        if self.history.len() < IMPULSE_BARS + 5 {
            return vec![];
        }

        if session.session == Session::Dead {
            return vec![];
        }

        if bar.timestamp.saturating_sub(self.last_signal_ts) < self.min_bars_between_signals {
            return vec![];
        }

        // Scan for new patterns each bar
        self.scan_patterns(atr);

        // Try OB entry first, then FVG
        if let Some(sig) = self.try_ob_entry(bar, atr, regime, session) {
            // Invalidate all OBs that would overlap with this zone
            self.active_obs.clear();
            self.last_signal_ts = bar.timestamp;
            return vec![sig];
        }
        if let Some(sig) = self.try_fvg_entry(bar, atr, regime, session) {
            self.active_fvgs.clear();
            self.last_signal_ts = bar.timestamp;
            return vec![sig];
        }

        vec![]
    }

    fn reset(&mut self) {
        self.atr.reset();
        self.ema.reset();
        self.history.clear();
        self.active_obs.clear();
        self.active_fvgs.clear();
        self.last_signal_ts = i64::MIN;
    }

    fn warmup_bars(&self) -> usize {
        50 + 14 // history + ATR
    }

    fn regime_allowed(&self, regime: &RegimeSignal9) -> bool {
        matches!(
            regime.regime,
            Regime9::StrongTrendUp
                | Regime9::StrongTrendDown
                | Regime9::RangingWide
                | Regime9::BreakoutPending
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
        let mut head = SmcHead::new(SmcConfig::default());
        let sess = SessionProfile::from_utc_hour(10);
        let reg = regime(Regime9::StrongTrendUp);
        for i in 0..20 {
            let c = dec!(1.1000);
            let b = Bar {
                timestamp: i * 900,
                open: c,
                high: c + dec!(0.0010),
                low: c - dec!(0.0010),
                close: c,
                volume: 500,
                timeframe: Timeframe::M15,
            };
            assert!(head.evaluate(&b, &sess, &reg).is_empty());
        }
    }

    #[test]
    fn regime_allowed_check() {
        let head = SmcHead::new(SmcConfig::default());
        assert!(head.regime_allowed(&regime(Regime9::StrongTrendUp)));
        assert!(head.regime_allowed(&regime(Regime9::BreakoutPending)));
        assert!(!head.regime_allowed(&regime(Regime9::RangingTight)));
        assert!(!head.regime_allowed(&regime(Regime9::Choppy)));
    }
}
