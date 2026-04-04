//! GridHead — range-trading with ATR-spaced limit orders.
//!
//! When the market is ranging (BB width in low percentile and ADX weak),
//! the grid head emits limit-order signals at EMA50 ± N×(ATR×spacing_mult)
//! for N = 1, 2, 3.  Only one grid signal fires per bar (the nearest level
//! that price is approaching from the correct side).
//!
//! Allowed regimes: RangingTight, RangingWide, Choppy.

use std::collections::VecDeque;

use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::heads::Head;
use crate::indicators::{BBWidthPercentile, BollingerBands, ATR, EMA};
use crate::types::{
    Bar, Direction, HeadId, Regime9, RegimeSignal9, Session, SessionProfile, SignalKind,
    TradeSignal, Timeframe,
};

#[derive(Debug, Clone)]
pub struct GridConfig {
    /// Half-width spacing multiplier applied to ATR for each grid level.
    pub spacing_atr_mult: Decimal,
    /// Number of grid levels on each side of centre (default 3).
    pub grid_levels: u32,
    /// BB squeeze threshold (percentile < this → grid is active).
    pub squeeze_pctile: Decimal,
    /// ADX ceiling — grid only active when ADX is below this (trend absent).
    pub max_adx: Decimal,
    /// SL = spacing × sl_spacing_mult beyond the triggered level.
    pub sl_spacing_mult: Decimal,
    /// TP = next grid level (one spacing away).
    pub tp_spacing_mult: Decimal,
    pub min_rr: Decimal,
    pub base_confidence: Decimal,
    pub symbol: String,
}

impl Default for GridConfig {
    fn default() -> Self {
        Self {
            spacing_atr_mult: dec!(0.5),
            grid_levels: 3,
            squeeze_pctile: dec!(0.45),
            max_adx: dec!(25),
            sl_spacing_mult: dec!(1.2),
            tp_spacing_mult: dec!(1.0),
            min_rr: dec!(1.5),
            base_confidence: dec!(0.55),
            symbol: "EURUSD".to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct GridHead {
    config: GridConfig,
    ema: EMA,
    atr: ATR,
    bb: BollingerBands,
    bb_pctile: BBWidthPercentile,
    /// Recent bar closes for ATR context; also used to determine direction of approach.
    recent_closes: VecDeque<Decimal>,
    last_signal_ts: i64,
    min_bars_between_signals: i64, // in seconds (e.g. 4 bars × 900 s = 3600 s)
}

impl GridHead {
    pub fn new(config: GridConfig) -> Self {
        Self {
            config,
            ema: EMA::new(50),
            atr: ATR::new(14),
            bb: BollingerBands::new(20, dec!(2.0)),
            bb_pctile: BBWidthPercentile::new(100),
            recent_closes: VecDeque::with_capacity(4),
            last_signal_ts: i64::MIN,
            min_bars_between_signals: 3600, // 4 × M15
        }
    }

    /// Returns the grid centre and level spacing, or None if not ready / not in squeeze.
    fn grid_state(
        &self,
        centre: Decimal,
        atr: Decimal,
        bb_pctile_val: Decimal,
    ) -> Option<(Decimal, Decimal)> {
        if bb_pctile_val > self.config.squeeze_pctile {
            return None; // not in squeeze
        }
        let spacing = atr * self.config.spacing_atr_mult;
        Some((centre, spacing))
    }
}

impl Head for GridHead {
    fn id(&self) -> HeadId {
        HeadId::Grid
    }

    fn evaluate(
        &mut self,
        bar: &Bar,
        session: &SessionProfile,
        regime: &RegimeSignal9,
    ) -> Vec<TradeSignal> {
        // Grid needs M15 bars; skip other timeframes
        if bar.timeframe != Timeframe::M15 {
            return vec![];
        }

        let centre = self.ema.update(bar.close);
        let atr_val = self.atr.update(bar);
        let bb_vals = self.bb.update(bar.close);
        let bb_p = bb_vals
            .as_ref()
            .map(|v| self.bb_pctile.update(v.width));

        self.recent_closes.push_back(bar.close);
        if self.recent_closes.len() > 4 {
            self.recent_closes.pop_front();
        }

        let (Some(c), Some(atr), Some(bb_p_val)) = (centre, atr_val, bb_p) else {
            return vec![];
        };

        // ADX gate — grid should not be active when there's a real trend
        if regime.adx > self.config.max_adx {
            return vec![];
        }

        // Dead session → skip
        if session.session == Session::Dead {
            return vec![];
        }

        // Rate limit: one signal per N seconds
        if bar.timestamp.saturating_sub(self.last_signal_ts) < self.min_bars_between_signals {
            return vec![];
        }

        let Some((grid_centre, spacing)) = self.grid_state(c, atr, bb_p_val) else {
            return vec![];
        };

        // Find the nearest grid level that bar.close is approaching
        let prev_close = self
            .recent_closes
            .iter()
            .rev()
            .nth(1)
            .copied()
            .unwrap_or(bar.close);

        for n in 1..=self.config.grid_levels {
            let level_buy = grid_centre - spacing * Decimal::from(n);
            let level_sell = grid_centre + spacing * Decimal::from(n);

            // BUY grid: price approaches level from above, touches it, closes up
            if prev_close > level_buy && bar.low <= level_buy && bar.close > level_buy {
                let sl = level_buy - spacing * self.config.sl_spacing_mult;
                let tp = level_buy + spacing * self.config.tp_spacing_mult;
                let risk = (level_buy - sl).abs();
                if risk.is_zero() {
                    continue;
                }
                let rr = (tp - level_buy).abs() / risk;
                if rr < self.config.min_rr {
                    continue;
                }
                self.last_signal_ts = bar.timestamp;
                return vec![TradeSignal {
                    symbol: self.config.symbol.clone(),
                    direction: Direction::Buy,
                    kind: SignalKind::Open,
                    entry: level_buy, // limit order
                    stop_loss: sl,
                    take_profit: tp,
                    take_profit2: Some(grid_centre), // centre as secondary target
                    head: HeadId::Grid,
                    head_confidence: self.config.base_confidence,
                    regime: regime.regime,
                    session: session.session,
                    pyramid_level: 0,
                    comment: format!("Grid BUY L{n} @ {level_buy:.5}"),
                    generated_at: bar.timestamp,
                }];
            }

            // SELL grid: price approaches level from below, touches it, closes down
            if prev_close < level_sell && bar.high >= level_sell && bar.close < level_sell {
                let sl = level_sell + spacing * self.config.sl_spacing_mult;
                let tp = level_sell - spacing * self.config.tp_spacing_mult;
                let risk = (level_sell - sl).abs();
                if risk.is_zero() {
                    continue;
                }
                let rr = (level_sell - tp).abs() / risk;
                if rr < self.config.min_rr {
                    continue;
                }
                self.last_signal_ts = bar.timestamp;
                return vec![TradeSignal {
                    symbol: self.config.symbol.clone(),
                    direction: Direction::Sell,
                    kind: SignalKind::Open,
                    entry: level_sell, // limit order
                    stop_loss: sl,
                    take_profit: tp,
                    take_profit2: Some(grid_centre),
                    head: HeadId::Grid,
                    head_confidence: self.config.base_confidence,
                    regime: regime.regime,
                    session: session.session,
                    pyramid_level: 0,
                    comment: format!("Grid SELL L{n} @ {level_sell:.5}"),
                    generated_at: bar.timestamp,
                }];
            }
        }

        vec![]
    }

    fn reset(&mut self) {
        self.ema.reset();
        self.atr.reset();
        self.bb.reset();
        self.bb_pctile.reset();
        self.recent_closes.clear();
        self.last_signal_ts = i64::MIN;
    }

    fn warmup_bars(&self) -> usize {
        100 + 50 // BB pctile lookback + EMA slow
    }

    fn regime_allowed(&self, regime: &RegimeSignal9) -> bool {
        matches!(
            regime.regime,
            Regime9::RangingTight | Regime9::RangingWide | Regime9::Choppy
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn ranging_regime() -> RegimeSignal9 {
        RegimeSignal9 {
            regime: Regime9::RangingTight,
            confidence: dec!(0.8),
            adx: dec!(18),
            hurst: dec!(0.5),
            atr_ratio: dec!(1.0),
            bb_width_pctile: dec!(0.25),
            choppiness_index: dec!(60),
            computed_at: 0,
        }
    }

    #[test]
    fn no_signal_during_warmup() {
        let mut head = GridHead::new(GridConfig::default());
        let sess = SessionProfile::from_utc_hour(10);
        let reg = ranging_regime();
        for i in 0..50 {
            let c = dec!(1.1000) + Decimal::from(i % 10) * dec!(0.0005);
            let b = Bar {
                timestamp: i * 900,
                open: c,
                high: c + dec!(0.0010),
                low: c - dec!(0.0010),
                close: c,
                volume: 500,
                timeframe: Timeframe::M15,
            };
            let sigs = head.evaluate(&b, &sess, &reg);
            assert!(sigs.is_empty());
        }
    }

    #[test]
    fn regime_allowed_only_for_ranging() {
        let head = GridHead::new(GridConfig::default());
        assert!(head.regime_allowed(&ranging_regime()));
        assert!(!head.regime_allowed(&RegimeSignal9 {
            regime: Regime9::StrongTrendUp,
            confidence: dec!(0.9),
            adx: dec!(35),
            hurst: dec!(0.7),
            atr_ratio: dec!(1.0),
            bb_width_pctile: dec!(0.8),
            choppiness_index: dec!(30),
            computed_at: 0,
        }));
    }
}
