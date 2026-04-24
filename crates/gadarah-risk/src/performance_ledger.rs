use std::collections::HashMap;

use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};

use gadarah_core::meta::{SegmentStatsProvider, SegmentStatsSnapshot};
use gadarah_core::{HeadId, Regime9, Session};

// ---------------------------------------------------------------------------
// SegmentStats
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SegmentStats {
    pub total_trades: u32,
    pub wins: u32,
    pub total_r: Decimal,
    pub consecutive_losses: u8,
    pub max_consecutive_l: u8,
    pub last_updated: i64,
    /// Running sum of squared Brier residuals: Σ(p_predicted − outcome)².
    /// `outcome` is 1 when the trade closed a winner, 0 otherwise. Paired
    /// with `brier_samples` so the mean is a single division away.
    #[serde(default)]
    pub brier_sum_sq: Decimal,
    /// Count of `(predicted_p, outcome)` samples folded into `brier_sum_sq`.
    #[serde(default)]
    pub brier_samples: u32,
}

impl SegmentStats {
    /// Win rate as a fraction [0.0, 1.0].
    pub fn win_rate(&self) -> Decimal {
        if self.total_trades == 0 {
            return Decimal::ZERO;
        }
        Decimal::from(self.wins) / Decimal::from(self.total_trades)
    }

    /// Average R-multiple.
    pub fn avg_r(&self) -> Decimal {
        if self.total_trades == 0 {
            return Decimal::ZERO;
        }
        self.total_r / Decimal::from(self.total_trades)
    }

    /// Mean Brier score over recorded forecasts. Lower is better; 0.25 is
    /// the "always 0.5" naive baseline for a binary outcome. Returns `None`
    /// when no calibrated forecasts have been recorded for the segment.
    pub fn brier_score(&self) -> Option<Decimal> {
        if self.brier_samples == 0 {
            return None;
        }
        Some(self.brier_sum_sq / Decimal::from(self.brier_samples))
    }

    /// Average winning R-multiple (take-profit side) given the segment's
    /// trade history. Used by the expected-value gate. Falls back to the
    /// signal's raw R:R when no history is available.
    pub fn avg_tp_r(&self) -> Option<Decimal> {
        if self.wins == 0 {
            return None;
        }
        // Total_r is `Σ(r_multiple)`; wins contribute positive R, losses −1R
        // by convention (signal.rr_ratio assumes a −1R loss). So winners'
        // aggregate R ≈ total_r + losses_count.
        let losses = Decimal::from(self.total_trades - self.wins);
        let winners_total_r = self.total_r + losses;
        Some(winners_total_r / Decimal::from(self.wins))
    }
}

/// Finite-horizon gambler's-ruin probability for a fixed-fraction bettor.
///
/// Classic formula from Kelly/Thorp: given win probability `p`, average
/// winning R `tp_r`, average losing R `sl_r`, and risk-per-trade as a
/// fraction of equity `f`, estimates the probability of losing the full
/// equity within `horizon_trades` bets. A conservative upper bound is used
/// when `tp_r ≈ sl_r` (symmetric payoff) and a geometric approximation
/// otherwise. Output is clipped to `[0, 1]`.
pub fn risk_of_ruin(
    p: Decimal,
    tp_r: Decimal,
    sl_r: Decimal,
    f: Decimal,
    horizon_trades: u32,
) -> Decimal {
    if horizon_trades == 0 || f <= Decimal::ZERO {
        return Decimal::ZERO;
    }
    if p <= Decimal::ZERO {
        return dec!(1);
    }
    if p >= dec!(1) {
        return Decimal::ZERO;
    }
    // Convert to f64 for the exponent math; Decimal has no pow(exp).
    let pf = decimal_to_f64(p);
    let qf = 1.0 - pf;
    let tp = decimal_to_f64(tp_r).abs().max(1e-6);
    let sl = decimal_to_f64(sl_r).abs().max(1e-6);
    let edge = pf * tp - qf * sl;
    if edge <= 0.0 {
        // Negative-edge bettors are ruined with probability 1 over any
        // non-trivial horizon; return 1.0 so the gate blocks.
        return dec!(1);
    }
    // Thorp's finite-horizon ruin bound for an asymmetric Bernoulli:
    //   a = q * sl / (p * tp)                (< 1 when edge > 0)
    //   R ≈ a^(equity_units)
    // where equity_units = 1 / f gives the "number of risk units of
    // cushion" before ruin. Horizon just bounds how many rolls of the dice
    // we take; the result monotonically grows with horizon but asymptotes
    // to this bound, so it is a conservative upper estimate.
    let a = (qf * sl) / (pf * tp);
    let equity_units = (1.0_f64 / decimal_to_f64(f).max(1e-6)).max(1.0);
    // Scale slightly by horizon: R(N) ≈ 1 − (1 − a^units)^N (Chebyshev-ish)
    let per_game = a.powf(equity_units).clamp(0.0, 1.0);
    let ror = 1.0 - (1.0 - per_game).powi(horizon_trades as i32);
    f64_to_decimal(ror.clamp(0.0, 1.0))
}

fn decimal_to_f64(d: Decimal) -> f64 {
    use rust_decimal::prelude::ToPrimitive;
    d.to_f64().unwrap_or(0.0)
}

fn f64_to_decimal(f: f64) -> Decimal {
    Decimal::from_f64_retain(f).unwrap_or(Decimal::ZERO)
}

// ---------------------------------------------------------------------------
// PerformanceLedger
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct PerformanceLedger {
    segments: HashMap<(HeadId, Regime9, Session), SegmentStats>,
}

impl PerformanceLedger {
    pub fn new() -> Self {
        Self {
            segments: HashMap::new(),
        }
    }

    /// Check whether a segment (Head x Regime x Session) is allowed to trade.
    ///
    /// Blocks:
    /// - 5+ consecutive losses
    /// - Win rate < 30% after 15+ trades
    /// - Negative avg R after 20+ trades
    pub fn is_segment_allowed(&self, head: HeadId, regime: Regime9, session: Session) -> bool {
        match self.segments.get(&(head, regime, session)) {
            None => true,
            Some(stats) if stats.total_trades < 10 => true,
            Some(stats) => {
                if stats.consecutive_losses >= 5 {
                    return false;
                }
                let wr = stats.win_rate();
                if stats.total_trades >= 15 && wr < dec!(0.30) {
                    return false;
                }
                let avg_r = stats.avg_r();
                if stats.total_trades >= 20 && avg_r < Decimal::ZERO {
                    return false;
                }
                true
            }
        }
    }

    /// Risk multiplier for a segment based on historical performance.
    ///
    /// - Unknown segment: 1.0
    /// - < 10 trades: 0.80 (new/unproven)
    /// - Strong (wr > 55%, avg_r > 0.5): 1.2
    /// - Good (wr > 45%, avg_r > 0.2): 1.0
    /// - Weak: 0.60
    pub fn risk_multiplier(&self, head: HeadId, regime: Regime9, session: Session) -> Decimal {
        match self.segments.get(&(head, regime, session)) {
            None => dec!(1.0),
            Some(stats) if stats.total_trades < 10 => dec!(0.80),
            Some(stats) => {
                let wr = stats.win_rate();
                let avg_r = stats.avg_r();
                if wr > dec!(0.55) && avg_r > dec!(0.5) {
                    dec!(1.2)
                } else if wr > dec!(0.45) && avg_r > dec!(0.2) {
                    dec!(1.0)
                } else {
                    dec!(0.60)
                }
            }
        }
    }

    /// Record a trade result for a given segment.
    pub fn record_trade(
        &mut self,
        head: HeadId,
        regime: Regime9,
        session: Session,
        won: bool,
        r_multiple: Decimal,
        timestamp: i64,
    ) {
        let key = (head, regime, session);
        let stats = self.segments.entry(key).or_default();
        stats.total_trades += 1;
        if won {
            stats.wins += 1;
            stats.consecutive_losses = 0;
        } else {
            stats.consecutive_losses += 1;
        }
        if stats.consecutive_losses > stats.max_consecutive_l {
            stats.max_consecutive_l = stats.consecutive_losses;
        }
        stats.total_r += r_multiple;
        stats.last_updated = timestamp;
    }

    /// Record a posterior forecast paired with its actual outcome. Updates
    /// the running Brier score for the segment so meta-layer scorers can
    /// detect miscalibration drift.
    pub fn record_forecast(
        &mut self,
        head: HeadId,
        regime: Regime9,
        session: Session,
        predicted_p: Decimal,
        outcome_won: bool,
    ) {
        let key = (head, regime, session);
        let stats = self.segments.entry(key).or_default();
        let outcome = if outcome_won { dec!(1) } else { Decimal::ZERO };
        let residual = predicted_p - outcome;
        stats.brier_sum_sq += residual * residual;
        stats.brier_samples += 1;
    }

    /// Get stats for a specific segment, if any trades have been recorded.
    pub fn get_stats(
        &self,
        head: HeadId,
        regime: Regime9,
        session: Session,
    ) -> Option<&SegmentStats> {
        self.segments.get(&(head, regime, session))
    }
}

impl Default for PerformanceLedger {
    fn default() -> Self {
        Self::new()
    }
}

impl SegmentStatsProvider for PerformanceLedger {
    fn snapshot(
        &self,
        head: HeadId,
        regime: Regime9,
        session: Session,
    ) -> Option<SegmentStatsSnapshot> {
        self.segments
            .get(&(head, regime, session))
            .map(|s| SegmentStatsSnapshot {
                total_trades: s.total_trades,
                wins: s.wins,
                total_r: s.total_r,
            })
    }
}
