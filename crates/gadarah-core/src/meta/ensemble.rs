//! Per-bar signal selection.
//!
//! Previously, the live loop took the first head that fired.  The ensemble
//! collects every candidate signal in a bar and selects the single highest-
//! scoring one, where score =
//!     posterior_p * kelly_fraction * session_fit * head_confidence.
//!
//! Correlation and risk caps are enforced upstream by the gate — the ensemble
//! only ranks; it does not veto.

use rust_decimal::Decimal;

use crate::types::{Session, TradeSignal};

use super::signal_scorer::ScoredSegment;

#[derive(Debug, Clone)]
pub struct RankedSignal {
    pub signal: TradeSignal,
    pub scored: ScoredSegment,
    pub score: Decimal,
}

#[derive(Debug, Clone)]
pub struct Ensemble;

impl Ensemble {
    pub fn new() -> Self {
        Self
    }

    /// Combine a signal's scored segment, head confidence, session fit and
    /// Kelly fraction into a single scalar.  Higher is better.
    pub fn rank(
        signal: TradeSignal,
        scored: ScoredSegment,
        kelly_fraction: Decimal,
        session_fit: Decimal,
    ) -> RankedSignal {
        let score =
            scored.posterior_p * kelly_fraction * session_fit * signal.head_confidence;
        RankedSignal {
            signal,
            scored,
            score,
        }
    }

    /// Select the single best signal from a list of ranked candidates.
    pub fn select_best(candidates: Vec<RankedSignal>) -> Option<RankedSignal> {
        candidates
            .into_iter()
            .filter(|r| r.score > Decimal::ZERO)
            .max_by(|a, b| a.score.cmp(&b.score))
    }

    /// Default session-fit scalar: sizing_multiplier scaled so London/Overlap ≈ 1.0.
    pub fn session_fit(session: Session) -> Decimal {
        session.sizing_multiplier()
    }
}

impl Default for Ensemble {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    use crate::types::{Direction, HeadId, Regime9, SignalKind};

    fn mk_signal(head: HeadId, confidence: Decimal) -> TradeSignal {
        TradeSignal {
            symbol: "EURUSD".into(),
            direction: Direction::Buy,
            kind: SignalKind::Open,
            entry: dec!(1.1000),
            stop_loss: dec!(1.0990),
            take_profit: dec!(1.1020),
            take_profit2: None,
            head,
            head_confidence: confidence,
            regime: Regime9::StrongTrendUp,
            session: Session::London,
            pyramid_level: 0,
            comment: String::new(),
            generated_at: 0,
        }
    }

    fn mk_scored(p: Decimal) -> ScoredSegment {
        ScoredSegment {
            head: HeadId::Momentum,
            regime: Regime9::StrongTrendUp,
            session: Session::London,
            posterior_p: p,
            avg_r: dec!(1.5),
            sample_size: 50,
        }
    }

    #[test]
    fn select_best_picks_highest_score() {
        let a = Ensemble::rank(
            mk_signal(HeadId::Momentum, dec!(0.80)),
            mk_scored(dec!(0.60)),
            dec!(0.10),
            dec!(1.0),
        );
        let b = Ensemble::rank(
            mk_signal(HeadId::Breakout, dec!(0.90)),
            mk_scored(dec!(0.70)),
            dec!(0.15),
            dec!(1.0),
        );
        let picked = Ensemble::select_best(vec![a, b]).unwrap();
        assert_eq!(picked.signal.head, HeadId::Breakout);
    }

    #[test]
    fn zero_kelly_is_filtered_out() {
        let a = Ensemble::rank(
            mk_signal(HeadId::Momentum, dec!(0.80)),
            mk_scored(dec!(0.60)),
            Decimal::ZERO,
            dec!(1.0),
        );
        assert!(Ensemble::select_best(vec![a]).is_none());
    }

    #[test]
    fn empty_candidates_returns_none() {
        assert!(Ensemble::select_best(vec![]).is_none());
    }
}
