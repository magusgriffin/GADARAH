//! Regime confidence / age / whitelist gate.
//!
//! A head's raw evaluation can still fire during ambiguous regime conditions.
//! The regime gate is a single chokepoint that blocks when:
//! - classifier confidence < `min_confidence` (default 0.60), or
//! - the regime changed fewer than `min_age_bars` bars ago (default 3), or
//! - the head is not in the current regime's whitelist.

use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::types::{HeadId, Regime9, RegimeSignal9};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegimeGateDecision {
    Pass,
    LowConfidence,
    TooYoung,
    NotWhitelisted,
}

#[derive(Debug, Clone)]
pub struct RegimeGate {
    min_confidence: Decimal,
    min_age_bars: u32,
    current_regime: Option<Regime9>,
    bars_in_regime: u32,
}

impl RegimeGate {
    pub fn new() -> Self {
        Self {
            min_confidence: dec!(0.60),
            min_age_bars: 3,
            current_regime: None,
            bars_in_regime: 0,
        }
    }

    pub fn with_min_confidence(mut self, conf: Decimal) -> Self {
        self.min_confidence = conf;
        self
    }

    pub fn with_min_age_bars(mut self, age: u32) -> Self {
        self.min_age_bars = age;
        self
    }

    /// Track regime transitions. Call once per closed bar with the latest
    /// classifier output.
    pub fn observe(&mut self, signal: &RegimeSignal9) {
        match self.current_regime {
            Some(prev) if prev == signal.regime => {
                self.bars_in_regime = self.bars_in_regime.saturating_add(1);
            }
            _ => {
                self.current_regime = Some(signal.regime);
                self.bars_in_regime = 1;
            }
        }
    }

    pub fn bars_in_regime(&self) -> u32 {
        self.bars_in_regime
    }

    /// Check whether a head may fire under the observed regime.
    pub fn check(&self, head: HeadId, signal: &RegimeSignal9) -> RegimeGateDecision {
        if signal.confidence < self.min_confidence {
            return RegimeGateDecision::LowConfidence;
        }
        if self.bars_in_regime < self.min_age_bars {
            return RegimeGateDecision::TooYoung;
        }
        if !signal.regime.allowed_heads().contains(&head) {
            return RegimeGateDecision::NotWhitelisted;
        }
        RegimeGateDecision::Pass
    }
}

impl Default for RegimeGate {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sig(regime: Regime9, confidence: Decimal) -> RegimeSignal9 {
        RegimeSignal9 {
            regime,
            confidence,
            adx: Decimal::ZERO,
            hurst: Decimal::ZERO,
            atr_ratio: Decimal::ONE,
            bb_width_pctile: Decimal::ZERO,
            choppiness_index: Decimal::ZERO,
            computed_at: 0,
        }
    }

    #[test]
    fn low_confidence_blocks() {
        let mut gate = RegimeGate::new();
        let s = sig(Regime9::StrongTrendUp, dec!(0.30));
        gate.observe(&s);
        gate.observe(&s);
        gate.observe(&s);
        gate.observe(&s);
        assert_eq!(
            gate.check(HeadId::Momentum, &s),
            RegimeGateDecision::LowConfidence
        );
    }

    #[test]
    fn fresh_regime_blocks_until_age_met() {
        let mut gate = RegimeGate::new();
        let s = sig(Regime9::StrongTrendUp, dec!(0.80));
        gate.observe(&s);
        assert_eq!(
            gate.check(HeadId::Momentum, &s),
            RegimeGateDecision::TooYoung
        );
        gate.observe(&s);
        gate.observe(&s);
        assert_eq!(gate.check(HeadId::Momentum, &s), RegimeGateDecision::Pass);
    }

    #[test]
    fn non_whitelisted_head_blocked() {
        let mut gate = RegimeGate::new();
        let s = sig(Regime9::Choppy, dec!(0.80));
        for _ in 0..5 {
            gate.observe(&s);
        }
        // Momentum is not whitelisted in Choppy.
        assert_eq!(
            gate.check(HeadId::Momentum, &s),
            RegimeGateDecision::NotWhitelisted
        );
        assert_eq!(gate.check(HeadId::Grid, &s), RegimeGateDecision::Pass);
    }

    #[test]
    fn regime_flip_resets_age_counter() {
        let mut gate = RegimeGate::new();
        for _ in 0..5 {
            gate.observe(&sig(Regime9::StrongTrendUp, dec!(0.80)));
        }
        assert!(gate.bars_in_regime() >= 3);
        gate.observe(&sig(Regime9::Choppy, dec!(0.80)));
        assert_eq!(gate.bars_in_regime(), 1);
    }
}
