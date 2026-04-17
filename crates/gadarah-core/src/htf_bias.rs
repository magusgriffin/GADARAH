use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};

use crate::indicators::{EMA, ATR};
use crate::types::{Bar, Direction};

// ---------------------------------------------------------------------------
// Higher-Timeframe Bias — confirms LTF signals against HTF structure
// ---------------------------------------------------------------------------

/// Directional bias derived from higher-timeframe price structure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HtfBias {
    Bullish,
    Bearish,
    Neutral,
}

impl HtfBias {
    /// Whether this bias supports the given trade direction.
    pub fn supports(&self, direction: Direction) -> bool {
        match (self, direction) {
            (HtfBias::Bullish, Direction::Buy) => true,
            (HtfBias::Bearish, Direction::Sell) => true,
            (HtfBias::Neutral, _) => true, // neutral doesn't block
            _ => false,
        }
    }

    /// Confidence multiplier: aligned trades get a boost, counter-trend get scaled down.
    pub fn confidence_multiplier(&self, direction: Direction) -> Decimal {
        match (self, direction) {
            (HtfBias::Bullish, Direction::Buy) => dec!(1.15),
            (HtfBias::Bearish, Direction::Sell) => dec!(1.15),
            (HtfBias::Neutral, _) => dec!(1.0),
            // Counter-trend: allow but penalize
            _ => dec!(0.70),
        }
    }
}

/// Streaming HTF bias filter.  Feed it higher-timeframe bars (e.g. H4 or H1)
/// and it maintains a bias signal that lower-timeframe heads can check.
///
/// Logic:
/// - EMA(21) vs EMA(50): trend direction
/// - Close relative to both EMAs: confirmation
/// - ATR(14) expansion/contraction: conviction
#[derive(Debug, Clone)]
pub struct HtfBiasFilter {
    ema_fast: EMA,
    ema_slow: EMA,
    atr: ATR,
    last_bias: HtfBias,
    bars_processed: u64,
}

impl HtfBiasFilter {
    pub fn new() -> Self {
        Self {
            ema_fast: EMA::new(21),
            ema_slow: EMA::new(50),
            atr: ATR::new(14),
            last_bias: HtfBias::Neutral,
            bars_processed: 0,
        }
    }

    /// Process one higher-timeframe bar. Returns the updated bias after warmup
    /// (50 bars required for EMA-50).
    pub fn update(&mut self, bar: &Bar) -> Option<HtfBias> {
        let fast = self.ema_fast.update(bar.close)?;
        let slow = self.ema_slow.update(bar.close)?;
        let _atr = self.atr.update(bar)?;

        self.bars_processed += 1;

        let bias = if bar.close > fast && fast > slow {
            HtfBias::Bullish
        } else if bar.close < fast && fast < slow {
            HtfBias::Bearish
        } else {
            HtfBias::Neutral
        };

        self.last_bias = bias;
        Some(bias)
    }

    /// Current bias (Neutral during warmup).
    pub fn bias(&self) -> HtfBias {
        self.last_bias
    }

    /// Number of bars that must be fed before the bias is meaningful.
    pub fn warmup_bars(&self) -> u64 {
        50
    }

    pub fn reset(&mut self) {
        self.ema_fast.reset();
        self.ema_slow.reset();
        self.atr.reset();
        self.last_bias = HtfBias::Neutral;
        self.bars_processed = 0;
    }
}

impl Default for HtfBiasFilter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Timeframe;

    fn bar(close: Decimal, high: Decimal, low: Decimal) -> Bar {
        Bar {
            open: close,
            high,
            low,
            close,
            volume: 100,
            timestamp: 0,
            timeframe: Timeframe::H4,
        }
    }

    fn trending_up_bars(count: usize, start: f64) -> Vec<Bar> {
        (0..count)
            .map(|i| {
                let c = Decimal::try_from(start + i as f64 * 0.0050).unwrap();
                bar(c, c + dec!(0.0030), c - dec!(0.0020))
            })
            .collect()
    }

    fn trending_down_bars(count: usize, start: f64) -> Vec<Bar> {
        (0..count)
            .map(|i| {
                let c = Decimal::try_from(start - i as f64 * 0.0050).unwrap();
                bar(c, c + dec!(0.0020), c - dec!(0.0030))
            })
            .collect()
    }

    #[test]
    fn warmup_returns_none() {
        let mut filter = HtfBiasFilter::new();
        let b = bar(dec!(1.1000), dec!(1.1010), dec!(1.0990));
        // First bar should return None (warmup)
        assert!(filter.update(&b).is_none());
    }

    #[test]
    fn uptrend_produces_bullish_bias() {
        let mut filter = HtfBiasFilter::new();
        // 150 bars ensures both EMAs are fully warmed up and separated
        let bars = trending_up_bars(150, 1.0500);
        let mut last_bias = None;
        for b in &bars {
            if let Some(bias) = filter.update(b) {
                last_bias = Some(bias);
            }
        }
        assert_eq!(last_bias, Some(HtfBias::Bullish));
    }

    #[test]
    fn downtrend_produces_bearish_bias() {
        let mut filter = HtfBiasFilter::new();
        let bars = trending_down_bars(150, 1.5000);
        let mut last_bias = None;
        for b in &bars {
            if let Some(bias) = filter.update(b) {
                last_bias = Some(bias);
            }
        }
        assert_eq!(last_bias, Some(HtfBias::Bearish));
    }

    #[test]
    fn bullish_supports_buy_not_sell() {
        assert!(HtfBias::Bullish.supports(Direction::Buy));
        assert!(!HtfBias::Bullish.supports(Direction::Sell));
    }

    #[test]
    fn neutral_supports_both() {
        assert!(HtfBias::Neutral.supports(Direction::Buy));
        assert!(HtfBias::Neutral.supports(Direction::Sell));
    }

    #[test]
    fn counter_trend_gets_confidence_penalty() {
        let mult = HtfBias::Bullish.confidence_multiplier(Direction::Sell);
        assert!(mult < dec!(1.0));
    }

    #[test]
    fn aligned_trend_gets_confidence_boost() {
        let mult = HtfBias::Bearish.confidence_multiplier(Direction::Sell);
        assert!(mult > dec!(1.0));
    }
}
