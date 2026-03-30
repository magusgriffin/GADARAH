use std::collections::VecDeque;

use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::heads::Head;
use crate::indicators::{BBWidthPercentile, BollingerBands, ATR};
use crate::types::{
    Bar, Direction, HeadId, RegimeSignal9, SessionProfile, SignalKind, TradeSignal,
};

/// Configuration for the BreakoutHead.
#[derive(Debug, Clone)]
pub struct BreakoutConfig {
    /// BB width percentile threshold for squeeze detection.
    pub squeeze_pctile: Decimal,
    /// BB width percentile threshold for expansion confirmation.
    pub expansion_pctile: Decimal,
    /// Minimum consecutive squeeze bars before a breakout is considered.
    pub min_squeeze_bars: u32,
    /// Volume multiplier: current bar volume must be >= this * avg_20_volume.
    pub volume_mult: Decimal,
    /// TP1 as a multiple of ATR from entry.
    pub tp1_atr_mult: Decimal,
    /// TP2 as a multiple of ATR from entry.
    pub tp2_atr_mult: Decimal,
    /// Minimum R:R ratio.
    pub min_rr: Decimal,
    /// Number of bars within which price closing back inside BB triggers
    /// a fake-breakout Close signal.
    pub fakeout_bars: u32,
    /// Symbol name for emitted signals.
    pub symbol: String,
    /// Base confidence for this head.
    pub base_confidence: Decimal,
}

impl Default for BreakoutConfig {
    fn default() -> Self {
        Self {
            squeeze_pctile: dec!(0.30),
            expansion_pctile: dec!(0.50),
            min_squeeze_bars: 10,
            volume_mult: dec!(1.3),
            tp1_atr_mult: dec!(2.5),
            tp2_atr_mult: dec!(4.0),
            min_rr: dec!(1.3),
            fakeout_bars: 3,
            symbol: String::from("GBPUSD"),
            base_confidence: dec!(0.60),
        }
    }
}

/// BreakoutHead — captures volatility expansion after compression.
///
/// Internally uses BB(20, 2.0), BBWidthPercentile(100), ATR(14), and a
/// 20-bar volume average. Detects squeeze conditions and entries on
/// expansion with volume confirmation.
///
/// Also tracks fake breakouts: if price closes back inside BB within
/// `fakeout_bars` after a breakout signal, emits a Close signal.
#[derive(Debug, Clone)]
pub struct BreakoutHead {
    config: BreakoutConfig,
    bb: BollingerBands,
    bb_pctile: BBWidthPercentile,
    atr: ATR,
    /// Rolling window of bar volumes for computing the 20-bar average.
    vol_history: VecDeque<u64>,
    /// Consecutive bars in squeeze (bb_width_pctile < squeeze_pctile threshold).
    squeeze_bars: u32,
    /// Whether a sufficient squeeze has been detected (persists until signal or long expansion).
    squeeze_armed: bool,
    /// Timestamp of the breakout bar for fake-out detection.
    breakout_bar: Option<i64>,
    /// Direction of the most recent breakout (for fake-out tracking).
    breakout_dir: Option<Direction>,
    /// Bars elapsed since the breakout was issued.
    bars_since_bo: u32,
    /// BB values at breakout time (for SL and fake-out detection).
    breakout_bb_upper: Decimal,
    breakout_bb_lower: Decimal,
    bars_processed: usize,
}

impl BreakoutHead {
    pub fn new(config: BreakoutConfig) -> Self {
        Self {
            config,
            bb: BollingerBands::new(20, dec!(2.0)),
            bb_pctile: BBWidthPercentile::new(100),
            atr: ATR::new(14),
            vol_history: VecDeque::with_capacity(21),
            squeeze_bars: 0,
            squeeze_armed: false,
            breakout_bar: None,
            breakout_dir: None,
            bars_since_bo: 0,
            breakout_bb_upper: Decimal::ZERO,
            breakout_bb_lower: Decimal::ZERO,
            bars_processed: 0,
        }
    }

    /// Compute the 20-bar average volume from the rolling window.
    fn avg_volume(&self) -> Decimal {
        if self.vol_history.is_empty() {
            return Decimal::ZERO;
        }
        let sum: u64 = self.vol_history.iter().sum();
        Decimal::from(sum) / Decimal::from(self.vol_history.len())
    }
}

impl Head for BreakoutHead {
    fn id(&self) -> HeadId {
        HeadId::Breakout
    }

    fn evaluate(
        &mut self,
        bar: &Bar,
        session: &SessionProfile,
        regime: &RegimeSignal9,
    ) -> Vec<TradeSignal> {
        self.bars_processed += 1;

        // --- Always update indicators regardless of regime ---

        // Update volume tracking
        self.vol_history.push_back(bar.volume);
        if self.vol_history.len() > 20 {
            self.vol_history.pop_front();
        }

        // Update ATR
        let atr_val = self.atr.update(bar);

        // Update BB
        let bb_opt = self.bb.update(bar.close);
        let (bb_upper, _bb_mid, bb_lower, bb_width) = match bb_opt {
            Some(bb) => (bb.upper, bb.mid, bb.lower, bb.width),
            None => return Vec::new(),
        };

        // Update BB width percentile
        let bb_width_pctile = self.bb_pctile.update(bb_width);

        // Track squeeze: consecutive bars where bb_width_pctile < squeeze threshold.
        if bb_width_pctile < self.config.squeeze_pctile {
            self.squeeze_bars += 1;
            if self.squeeze_bars >= self.config.min_squeeze_bars {
                self.squeeze_armed = true;
            }
        } else {
            self.squeeze_bars = 0;
        }

        // --- Fake breakout detection ---
        if let (Some(_bo_ts), Some(bo_dir)) = (self.breakout_bar, self.breakout_dir) {
            self.bars_since_bo += 1;

            // Check if price has closed back inside BB within fakeout_bars
            if self.bars_since_bo <= self.config.fakeout_bars {
                let back_inside = bar.close < bb_upper && bar.close > bb_lower;

                if back_inside {
                    // Fake breakout detected: emit Close signal
                    let close_signal = TradeSignal {
                        symbol: self.config.symbol.clone(),
                        direction: bo_dir,
                        kind: SignalKind::Close,
                        entry: bar.close,
                        stop_loss: Decimal::ZERO,
                        take_profit: Decimal::ZERO,
                        take_profit2: None,
                        head: HeadId::Breakout,
                        head_confidence: self.config.base_confidence,
                        regime: regime.regime,
                        session: session.session,
                        pyramid_level: 0,
                        comment: format!(
                            "Breakout: FAKE breakout detected, price {} back inside BB within {} bars",
                            bar.close, self.bars_since_bo,
                        ),
                        generated_at: bar.timestamp,
                    };

                    // Clear breakout tracking
                    self.breakout_bar = None;
                    self.breakout_dir = None;
                    self.bars_since_bo = 0;

                    self.squeeze_armed = false;
                    self.squeeze_bars = 0;

                    return vec![close_signal];
                }
            }

            // If we've passed the fakeout window, clear tracking
            if self.bars_since_bo > self.config.fakeout_bars {
                self.breakout_bar = None;
                self.breakout_dir = None;
                self.bars_since_bo = 0;
            }
        }

        // --- Entry logic ---
        // Squeeze persistence: don't reset squeeze_bars on early exits.
        // Squeeze is a structural condition that persists across regime changes.
        // Only reset after the entry evaluation section.

        // Check regime
        if !self.regime_allowed(regime) {
            return Vec::new();
        }

        // Warmup guard
        if self.bars_processed < self.warmup_bars() {
            return Vec::new();
        }

        // Dead session = no entries
        if session.sizing_mult.is_zero() {
            return Vec::new();
        }

        // Need ATR for TP calculation
        let atr = match atr_val {
            Some(a) if !a.is_zero() => a,
            _ => return Vec::new(),
        };

        // Entry conditions (ALL required):
        // 1. squeeze_bars >= min_squeeze_bars (must have had a preceding squeeze)
        // 2. current bb_width_pctile > expansion_pctile (expansion happening)
        // 3. close > bb.upper (Buy) or close < bb.lower (Sell)
        // 4. bar.volume >= volume_mult * avg_20_volume

        let had_squeeze = self.squeeze_armed;
        let expanding = bb_width_pctile > self.config.expansion_pctile;
        let avg_vol = self.avg_volume();
        // Skip volume check if data has no volume (all zeros)
        let vol_ok =
            avg_vol.is_zero() || Decimal::from(bar.volume) >= self.config.volume_mult * avg_vol;

        // Don't emit a new breakout if we're already tracking one for fake-out
        let already_tracking = self.breakout_bar.is_some();

        if had_squeeze && expanding && vol_ok && !already_tracking {
            let direction = if bar.close > bb_upper {
                Some(Direction::Buy)
            } else if bar.close < bb_lower {
                Some(Direction::Sell)
            } else {
                None
            };

            if let Some(dir) = direction {
                let entry = bar.close;

                // SL: BB midpoint (tighter than opposite band, better R:R)
                let stop_loss = _bb_mid;

                // TP based on BB width (captures expansion better than lagging ATR)
                let half_bb = (bb_upper - bb_lower) / dec!(2);
                let tp_dist = (half_bb * self.config.tp1_atr_mult).max(atr * dec!(2));
                let tp2_dist = (half_bb * self.config.tp2_atr_mult).max(atr * dec!(3));
                let (take_profit, take_profit2) = match dir {
                    Direction::Buy => (entry + tp_dist, entry + tp2_dist),
                    Direction::Sell => (entry - tp_dist, entry - tp2_dist),
                };

                // Check minimum R:R
                let risk = (entry - stop_loss).abs();
                if !risk.is_zero() {
                    let reward = (take_profit - entry).abs();
                    let rr = reward / risk;

                    if rr >= self.config.min_rr {
                        // Record breakout for fake-out detection
                        self.breakout_bar = Some(bar.timestamp);
                        self.breakout_dir = Some(dir);
                        self.bars_since_bo = 0;
                        self.breakout_bb_upper = bb_upper;
                        self.breakout_bb_lower = bb_lower;

                        // Clear squeeze state now that a breakout has been issued
                        self.squeeze_bars = 0;
                        self.squeeze_armed = false;

                        return vec![TradeSignal {
                            symbol: self.config.symbol.clone(),
                            direction: dir,
                            kind: SignalKind::Open,
                            entry,
                            stop_loss,
                            take_profit,
                            take_profit2: Some(take_profit2),
                            head: HeadId::Breakout,
                            head_confidence: self.config.base_confidence,
                            regime: regime.regime,
                            session: session.session,
                            pyramid_level: 0,
                            comment: format!(
                                "Breakout: {} expansion after {} bar squeeze, vol={} >= {:.0}*avg, ATR={}, R:R={}",
                                match dir {
                                    Direction::Buy => "bullish",
                                    Direction::Sell => "bearish",
                                },
                                self.config.min_squeeze_bars,
                                bar.volume,
                                self.config.volume_mult,
                                atr.round_dp(5),
                                rr.round_dp(2),
                            ),
                            generated_at: bar.timestamp,
                        }];
                    }
                }
            }
        }

        // Disarm squeeze if expansion was strong but didn't produce a signal
        // (prevents stale squeeze state from triggering on unrelated future bars)
        if bb_width_pctile > dec!(0.70) {
            self.squeeze_armed = false;
        }

        Vec::new()
    }

    fn reset(&mut self) {
        self.bb.reset();
        self.bb_pctile.reset();
        self.atr.reset();
        self.vol_history.clear();
        self.squeeze_bars = 0;
        self.squeeze_armed = false;
        self.breakout_bar = None;
        self.breakout_dir = None;
        self.bars_since_bo = 0;
        self.breakout_bb_upper = Decimal::ZERO;
        self.breakout_bb_lower = Decimal::ZERO;
        self.bars_processed = 0;
    }

    fn warmup_bars(&self) -> usize {
        100
    }

    fn regime_allowed(&self, regime: &RegimeSignal9) -> bool {
        regime.regime.allowed_heads().contains(&HeadId::Breakout)
    }
}
