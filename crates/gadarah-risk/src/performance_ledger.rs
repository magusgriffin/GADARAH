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
