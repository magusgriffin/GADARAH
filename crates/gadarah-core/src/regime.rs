use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::indicators::{
    BBWidthPercentile, BollingerBands, ChoppinessIndex, HurstExponent, ADX, ATR, EMA,
};
use crate::types::{Bar, Regime9, RegimeSignal9};

#[derive(Debug, Clone, Copy)]
struct RegimeInputs {
    close: Decimal,
    ema20: Decimal,
    ema200: Decimal,
    adx: Decimal,
    hurst: Decimal,
    bb_pctile: Decimal,
    ci: Decimal,
}

/// Regime Classifier — streaming, one bar at a time.
///
/// Internally maintains EMA(20), EMA(200), ATR(14), ATR(50), ADX(14),
/// Hurst(100), BB(20, 2.0), ChoppinessIndex(14), and BBWidthPercentile(100).
///
/// Requires 200 bars of warmup (driven by EMA-200). After warmup, each call
/// to `update()` returns a `RegimeSignal9` with the current regime classification
/// and confidence score.
#[derive(Debug, Clone)]
pub struct RegimeClassifier {
    ema20: EMA,
    ema200: EMA,
    atr14: ATR,
    atr50: ATR,
    adx: ADX,
    hurst: HurstExponent,
    bb: BollingerBands,
    bb_pctile: BBWidthPercentile,
    ci: ChoppinessIndex,
    squeeze_count: u32,
}

impl RegimeClassifier {
    pub fn new() -> Self {
        Self {
            ema20: EMA::new(20),
            ema200: EMA::new(200),
            atr14: ATR::new(14),
            atr50: ATR::new(50),
            adx: ADX::new(14),
            hurst: HurstExponent::new(100),
            bb: BollingerBands::new(20, dec!(2.0)),
            bb_pctile: BBWidthPercentile::new(100),
            ci: ChoppinessIndex::new(14),
            squeeze_count: 0,
        }
    }

    /// Process one closed bar. Returns `None` during warmup (first ~200 bars),
    /// then returns the regime classification.
    pub fn update(&mut self, bar: &Bar) -> Option<RegimeSignal9> {
        // Update all indicators; early return None if any is not yet ready
        let ema20 = self.ema20.update(bar.close)?;
        let ema200 = self.ema200.update(bar.close)?;
        let atr14 = self.atr14.update(bar)?;
        let atr50 = self.atr50.update(bar)?;
        let adx = self.adx.update(bar)?;
        let hurst = self.hurst.update(bar.close)?;

        // BB returns a reference; we need to clone the width out
        let bb = self.bb.update(bar.close)?;
        let bb_width = bb.width;

        let ci = self.ci.update(bar)?;

        let atr_ratio = if atr50.is_zero() {
            dec!(1)
        } else {
            atr14 / atr50
        };
        let bb_pctile = self.bb_pctile.update(bb_width);

        // Track BB squeeze duration
        if bb_pctile < dec!(0.20) {
            self.squeeze_count += 1;
        } else {
            self.squeeze_count = 0;
        }

        // Classification with confidence scoring (priority order)
        let (regime, confidence) = self.classify(RegimeInputs {
            close: bar.close,
            ema20,
            ema200,
            adx,
            hurst,
            bb_pctile,
            ci,
        });

        Some(RegimeSignal9 {
            regime,
            confidence,
            adx,
            hurst,
            atr_ratio,
            bb_width_pctile: bb_pctile,
            choppiness_index: ci,
            computed_at: bar.timestamp,
        })
    }

    /// Classify the current market regime. Returns (regime, confidence).
    /// Rules are applied in strict priority order.
    fn classify(&self, inputs: RegimeInputs) -> (Regime9, Decimal) {
        // 1. BreakoutPending: BB squeeze for 10+ consecutive bars
        if self.squeeze_count >= 10 {
            return (Regime9::BreakoutPending, dec!(0.75));
        }

        // 2. Choppy: CI > 61.8 and ADX < 20
        if inputs.ci > dec!(61.8) && inputs.adx < dec!(20) {
            return (Regime9::Choppy, dec!(0.70));
        }

        // 3. StrongTrendUp: ADX > 25, Hurst > 0.60, close > EMA20 > EMA200
        if inputs.adx > dec!(25)
            && inputs.hurst > dec!(0.60)
            && inputs.close > inputs.ema20
            && inputs.ema20 > inputs.ema200
        {
            return (Regime9::StrongTrendUp, dec!(0.80));
        }

        // 4. StrongTrendDown: ADX > 25, Hurst > 0.60, close < EMA20 < EMA200
        if inputs.adx > dec!(25)
            && inputs.hurst > dec!(0.60)
            && inputs.close < inputs.ema20
            && inputs.ema20 < inputs.ema200
        {
            return (Regime9::StrongTrendDown, dec!(0.80));
        }

        // 5. WeakTrendUp: ADX 18-27, Hurst 0.50-0.60, close > EMA200
        //    Widened from ADX 20-25 / Hurst 0.52-0.60 to capture borderline
        //    trending conditions that would otherwise fall to Transitioning.
        if inputs.adx >= dec!(18)
            && inputs.adx <= dec!(27)
            && inputs.hurst >= dec!(0.50)
            && inputs.hurst <= dec!(0.60)
            && inputs.close > inputs.ema200
        {
            return (Regime9::WeakTrendUp, dec!(0.55));
        }

        // 6. WeakTrendDown: ADX 18-27, Hurst 0.50-0.60, close < EMA200
        if inputs.adx >= dec!(18)
            && inputs.adx <= dec!(27)
            && inputs.hurst >= dec!(0.50)
            && inputs.hurst <= dec!(0.60)
            && inputs.close < inputs.ema200
        {
            return (Regime9::WeakTrendDown, dec!(0.55));
        }

        // 7. RangingTight: Hurst < 0.45, BB width pctile < 0.30, CI > 55
        if inputs.hurst < dec!(0.45) && inputs.bb_pctile < dec!(0.30) && inputs.ci > dec!(55) {
            return (Regime9::RangingTight, dec!(0.65));
        }

        // 8. RangingWide: Hurst < 0.45, BB width pctile < 0.60
        if inputs.hurst < dec!(0.45) && inputs.bb_pctile < dec!(0.60) {
            return (Regime9::RangingWide, dec!(0.60));
        }

        // 9. Default: Transitioning (low confidence catchall)
        (Regime9::Transitioning, dec!(0.25))
    }

    pub fn warmup_bars(&self) -> usize {
        200
    }

    pub fn reset(&mut self) {
        self.ema20.reset();
        self.ema200.reset();
        self.atr14.reset();
        self.atr50.reset();
        self.adx.reset();
        self.hurst.reset();
        self.bb.reset();
        self.bb_pctile.reset();
        self.ci.reset();
        self.squeeze_count = 0;
    }
}

impl Default for RegimeClassifier {
    fn default() -> Self {
        Self::new()
    }
}
