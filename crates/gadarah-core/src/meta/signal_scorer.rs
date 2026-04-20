//! Bayesian posterior over historical (head, regime, session) outcomes.
//!
//! The scorer consults a [`SegmentStatsProvider`] — typically the
//! `gadarah_risk::PerformanceLedger` — and blends its empirical win rate with a
//! Beta(α, β) prior.  The posterior mean is returned as a calibrated probability
//! of win, usable both as a gate (`p < 0.50 ⇒ reject`) and as a Kelly input.
//!
//! Because `gadarah_core` does not depend on `gadarah_risk`, the provider is a
//! trait: risk crate implements it for `PerformanceLedger` at call time.

use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::types::{HeadId, Regime9, Session};

/// Minimal, serde-free snapshot of a segment's historical stats.
#[derive(Debug, Clone, Copy, Default)]
pub struct SegmentStatsSnapshot {
    pub total_trades: u32,
    pub wins: u32,
    pub total_r: Decimal,
}

impl SegmentStatsSnapshot {
    pub fn win_rate(&self) -> Decimal {
        if self.total_trades == 0 {
            return Decimal::ZERO;
        }
        Decimal::from(self.wins) / Decimal::from(self.total_trades)
    }

    pub fn avg_r(&self) -> Decimal {
        if self.total_trades == 0 {
            return Decimal::ZERO;
        }
        self.total_r / Decimal::from(self.total_trades)
    }
}

/// Anything that can produce a snapshot for a (head, regime, session) segment.
///
/// Implemented by `gadarah_risk::PerformanceLedger`.
pub trait SegmentStatsProvider {
    fn snapshot(
        &self,
        head: HeadId,
        regime: Regime9,
        session: Session,
    ) -> Option<SegmentStatsSnapshot>;
}

/// A segment paired with its Bayesian posterior win probability.
#[derive(Debug, Clone, Copy)]
pub struct ScoredSegment {
    pub head: HeadId,
    pub regime: Regime9,
    pub session: Session,
    pub posterior_p: Decimal,
    pub avg_r: Decimal,
    pub sample_size: u32,
}

/// Bayesian signal scorer with a configurable Beta(α, β) prior.
#[derive(Debug, Clone)]
pub struct SignalScorer {
    /// Pseudo-wins in the prior (default: 5 → neutral-ish prior @ 0.50).
    prior_alpha: Decimal,
    /// Pseudo-losses in the prior (default: 5).
    prior_beta: Decimal,
    /// Minimum posterior probability to pass the gate (default: 0.50).
    min_posterior: Decimal,
}

impl SignalScorer {
    pub fn new() -> Self {
        Self {
            prior_alpha: dec!(5),
            prior_beta: dec!(5),
            min_posterior: dec!(0.50),
        }
    }

    pub fn with_prior(mut self, alpha: Decimal, beta: Decimal) -> Self {
        self.prior_alpha = alpha;
        self.prior_beta = beta;
        self
    }

    pub fn with_min_posterior(mut self, p: Decimal) -> Self {
        self.min_posterior = p;
        self
    }

    pub fn min_posterior(&self) -> Decimal {
        self.min_posterior
    }

    /// Posterior mean of Beta(α + wins, β + losses).
    pub fn posterior(&self, stats: &SegmentStatsSnapshot) -> Decimal {
        let wins = Decimal::from(stats.wins);
        let losses = Decimal::from(stats.total_trades.saturating_sub(stats.wins));
        let alpha = self.prior_alpha + wins;
        let beta = self.prior_beta + losses;
        let denom = alpha + beta;
        if denom.is_zero() {
            return dec!(0.50);
        }
        alpha / denom
    }

    /// Score a (head, regime, session) triple. When the provider has no record
    /// the posterior reduces to the prior mean (α / (α + β)).
    pub fn score<P: SegmentStatsProvider>(
        &self,
        provider: &P,
        head: HeadId,
        regime: Regime9,
        session: Session,
    ) -> ScoredSegment {
        let snapshot = provider
            .snapshot(head, regime, session)
            .unwrap_or_default();
        let posterior_p = self.posterior(&snapshot);
        ScoredSegment {
            head,
            regime,
            session,
            posterior_p,
            avg_r: snapshot.avg_r(),
            sample_size: snapshot.total_trades,
        }
    }

    /// Whether the scored segment passes the minimum-posterior gate.
    pub fn passes(&self, scored: &ScoredSegment) -> bool {
        scored.posterior_p >= self.min_posterior
    }

    /// Kelly fraction given current posterior and average R-multiple.
    ///
    /// Half-Kelly by default: `0.5 * (p - (1 - p) / avg_r)` clamped to [0, 0.25].
    /// Returns 0 when avg_r ≤ 0 (no edge, no bet).
    pub fn kelly_fraction(&self, scored: &ScoredSegment) -> Decimal {
        if scored.avg_r <= Decimal::ZERO {
            return Decimal::ZERO;
        }
        let p = scored.posterior_p;
        let q = Decimal::ONE - p;
        let raw = p - q / scored.avg_r;
        if raw <= Decimal::ZERO {
            return Decimal::ZERO;
        }
        let half = raw / dec!(2);
        half.min(dec!(0.25))
    }
}

impl Default for SignalScorer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct FakeProvider {
        map: HashMap<(HeadId, Regime9, Session), SegmentStatsSnapshot>,
    }

    impl SegmentStatsProvider for FakeProvider {
        fn snapshot(
            &self,
            head: HeadId,
            regime: Regime9,
            session: Session,
        ) -> Option<SegmentStatsSnapshot> {
            self.map.get(&(head, regime, session)).copied()
        }
    }

    fn provider(entries: Vec<(HeadId, Regime9, Session, SegmentStatsSnapshot)>) -> FakeProvider {
        let mut map = HashMap::new();
        for (h, r, s, snap) in entries {
            map.insert((h, r, s), snap);
        }
        FakeProvider { map }
    }

    #[test]
    fn unknown_segment_returns_prior_mean() {
        let scorer = SignalScorer::new();
        let p = provider(vec![]);
        let scored = scorer.score(&p, HeadId::Momentum, Regime9::StrongTrendUp, Session::London);
        // Prior α = β = 5 → posterior mean 0.50.
        assert_eq!(scored.posterior_p, dec!(0.50));
        assert!(scorer.passes(&scored));
    }

    #[test]
    fn strong_winner_pushes_posterior_above_prior() {
        let scorer = SignalScorer::new();
        let p = provider(vec![(
            HeadId::Momentum,
            Regime9::StrongTrendUp,
            Session::London,
            SegmentStatsSnapshot {
                total_trades: 40,
                wins: 28,
                total_r: dec!(30),
            },
        )]);
        let scored = scorer.score(&p, HeadId::Momentum, Regime9::StrongTrendUp, Session::London);
        // (5 + 28) / (5 + 28 + 5 + 12) = 33 / 50 = 0.66
        assert_eq!(scored.posterior_p, dec!(0.66));
    }

    #[test]
    fn losing_segment_falls_below_gate() {
        let scorer = SignalScorer::new();
        let p = provider(vec![(
            HeadId::Grid,
            Regime9::Choppy,
            Session::Dead,
            SegmentStatsSnapshot {
                total_trades: 40,
                wins: 8,
                total_r: dec!(-10),
            },
        )]);
        let scored = scorer.score(&p, HeadId::Grid, Regime9::Choppy, Session::Dead);
        assert!(!scorer.passes(&scored));
    }

    #[test]
    fn kelly_fraction_zero_when_no_avg_r() {
        let scorer = SignalScorer::new();
        let scored = ScoredSegment {
            head: HeadId::Momentum,
            regime: Regime9::StrongTrendUp,
            session: Session::London,
            posterior_p: dec!(0.65),
            avg_r: Decimal::ZERO,
            sample_size: 10,
        };
        assert_eq!(scorer.kelly_fraction(&scored), Decimal::ZERO);
    }

    #[test]
    fn kelly_fraction_capped_at_quarter() {
        let scorer = SignalScorer::new();
        let scored = ScoredSegment {
            head: HeadId::Momentum,
            regime: Regime9::StrongTrendUp,
            session: Session::London,
            posterior_p: dec!(0.99),
            avg_r: dec!(5),
            sample_size: 100,
        };
        assert!(scorer.kelly_fraction(&scored) <= dec!(0.25));
    }
}
