use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Timeframe
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Timeframe {
    M1,
    M5,
    M15,
    H1,
    H4,
    D1,
}

impl Timeframe {
    pub fn seconds(&self) -> i64 {
        match self {
            Timeframe::M1 => 60,
            Timeframe::M5 => 300,
            Timeframe::M15 => 900,
            Timeframe::H1 => 3600,
            Timeframe::H4 => 14400,
            Timeframe::D1 => 86400,
        }
    }
}

// ---------------------------------------------------------------------------
// Bar
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bar {
    pub open: Decimal,
    pub high: Decimal,
    pub low: Decimal,
    pub close: Decimal,
    pub volume: u64,
    pub timestamp: i64, // Unix seconds UTC (bar open time)
    pub timeframe: Timeframe,
}

// ---------------------------------------------------------------------------
// HeadId
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HeadId {
    // Phase 1 MVP (3 heads)
    Momentum,
    AsianRange,
    Breakout,
    // Phase 2 expansion (7 heads -- added AFTER first payout)
    Trend,
    Grid,
    Smc,
    News,
    ScalpM1,
    ScalpM5,
    VolProfile,
}

// ---------------------------------------------------------------------------
// Direction
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Direction {
    Buy,
    Sell,
}

// ---------------------------------------------------------------------------
// SignalKind
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SignalKind {
    Open,
    Close,
    AddPyramid,
    ReEntry,
    Adjust,
}

// ---------------------------------------------------------------------------
// Regime9
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Regime9 {
    StrongTrendUp,
    StrongTrendDown,
    WeakTrendUp,
    WeakTrendDown,
    RangingTight,
    RangingWide,
    Choppy,
    BreakoutPending,
    Transitioning,
}

impl Regime9 {
    /// Phase 2: all 10 heads gated by regime suitability.
    ///
    /// ScalpM1/ScalpM5 are further session-gated internally and will self-block
    /// outside Overlap/London regardless of this list.
    /// News fires on spike detection in any regime; compliance layer gates per firm.
    /// Grid is active in all ranging/choppy regimes.
    /// Smc and Trend require trending/breakout context.
    pub fn allowed_heads(&self) -> &[HeadId] {
        match self {
            Self::StrongTrendUp | Self::StrongTrendDown => &[
                HeadId::Momentum,
                HeadId::Breakout,
                HeadId::Trend,
                HeadId::Smc,
                HeadId::ScalpM5,
                HeadId::ScalpM1,
            ],
            Self::WeakTrendUp | Self::WeakTrendDown => &[
                HeadId::Momentum,
                HeadId::AsianRange,
                HeadId::Trend,
                HeadId::VolProfile,
            ],
            Self::RangingTight => &[
                HeadId::AsianRange,
                HeadId::Grid,
                HeadId::VolProfile,
            ],
            Self::RangingWide => &[
                HeadId::AsianRange,
                HeadId::Breakout,
                HeadId::Grid,
                HeadId::Smc,
            ],
            Self::Choppy => &[HeadId::Grid, HeadId::ScalpM5],
            Self::BreakoutPending => &[HeadId::Breakout, HeadId::Smc, HeadId::News],
            Self::Transitioning => &[
                HeadId::AsianRange,
                HeadId::Momentum,
                HeadId::VolProfile,
                HeadId::ScalpM5,
            ],
        }
    }

    pub fn is_trending(&self) -> bool {
        matches!(
            self,
            Self::StrongTrendUp | Self::StrongTrendDown | Self::WeakTrendUp | Self::WeakTrendDown
        )
    }
}

// ---------------------------------------------------------------------------
// RegimeSignal9
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegimeSignal9 {
    pub regime: Regime9,
    pub confidence: Decimal,
    pub adx: Decimal,
    pub hurst: Decimal,
    pub atr_ratio: Decimal,
    pub bb_width_pctile: Decimal,
    pub choppiness_index: Decimal,
    pub computed_at: i64,
}

// ---------------------------------------------------------------------------
// TradeSignal
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeSignal {
    pub symbol: String,
    pub direction: Direction,
    pub kind: SignalKind,
    pub entry: Decimal, // 0 = market order
    pub stop_loss: Decimal,
    pub take_profit: Decimal,
    pub take_profit2: Option<Decimal>,
    pub head: HeadId,
    pub head_confidence: Decimal, // [0.0, 1.0]
    pub regime: Regime9,
    pub session: Session,
    pub pyramid_level: u8,
    pub comment: String,
    pub generated_at: i64,
}

impl TradeSignal {
    pub fn sl_distance_pips(&self, pip_size: Decimal) -> Decimal {
        (self.entry - self.stop_loss).abs() / pip_size
    }

    pub fn rr_ratio(&self) -> Option<Decimal> {
        let risk = (self.entry - self.stop_loss).abs();
        if risk.is_zero() {
            return None;
        }
        Some((self.take_profit - self.entry).abs() / risk)
    }
}

// ---------------------------------------------------------------------------
// Session
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Session {
    Asian,
    London,
    Overlap,
    NyPm,
    Dead,
}

impl Session {
    pub fn from_utc_hour(h: u8) -> Self {
        match h {
            0..=6 => Self::Asian,
            7..=11 => Self::London,
            12..=15 => Self::Overlap,
            16..=20 => Self::NyPm,
            _ => Self::Dead,
        }
    }

    pub fn sizing_multiplier(&self) -> Decimal {
        match self {
            Self::Asian => dec!(0.7),
            Self::London => dec!(1.0),
            Self::Overlap => dec!(1.0),
            Self::NyPm => dec!(0.8),
            Self::Dead => dec!(0.0),
        }
    }

    pub fn slippage_multiplier(&self) -> Decimal {
        match self {
            Self::Asian => dec!(1.8),
            Self::London => dec!(1.0),
            Self::Overlap => dec!(1.1),
            Self::NyPm => dec!(1.2),
            Self::Dead => dec!(2.0),
        }
    }
}

// ---------------------------------------------------------------------------
// SessionProfile
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct SessionProfile {
    pub session: Session,
    pub sizing_mult: Decimal,
    pub slippage_mult: Decimal,
    pub min_rr_override: Option<Decimal>,
}

impl SessionProfile {
    pub fn from_session(session: Session) -> Self {
        Self {
            session,
            sizing_mult: session.sizing_multiplier(),
            slippage_mult: session.slippage_multiplier(),
            min_rr_override: None,
        }
    }

    pub fn from_utc_hour(h: u8) -> Self {
        Self::from_session(Session::from_utc_hour(h))
    }
}

// ---------------------------------------------------------------------------
// Helper: extract UTC hour from unix timestamp
// ---------------------------------------------------------------------------

pub fn utc_hour(timestamp: i64) -> u8 {
    ((timestamp % 86400) / 3600) as u8
}

/// Extract the day number (unix days since epoch) from a unix timestamp.
pub fn utc_day(timestamp: i64) -> i64 {
    timestamp.div_euclid(86400)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowed_heads_phase2_spec() {
        // Strong trend: core 3 Phase 1 heads + Trend, Smc, scalpers
        let strong = Regime9::StrongTrendUp.allowed_heads();
        assert!(strong.contains(&HeadId::Momentum));
        assert!(strong.contains(&HeadId::Breakout));
        assert!(strong.contains(&HeadId::Trend));
        assert!(strong.contains(&HeadId::Smc));
        assert!(strong.contains(&HeadId::ScalpM5));
        assert!(strong.contains(&HeadId::ScalpM1));

        // Weak trend: momentum, asian, trend, vol profile
        let weak = Regime9::WeakTrendUp.allowed_heads();
        assert!(weak.contains(&HeadId::Momentum));
        assert!(weak.contains(&HeadId::AsianRange));
        assert!(weak.contains(&HeadId::Trend));
        assert!(weak.contains(&HeadId::VolProfile));

        // Ranging tight: asian range + grid + vol profile
        let tight = Regime9::RangingTight.allowed_heads();
        assert!(tight.contains(&HeadId::AsianRange));
        assert!(tight.contains(&HeadId::Grid));
        assert!(tight.contains(&HeadId::VolProfile));

        // Choppy: only grid + scalp m5
        let choppy = Regime9::Choppy.allowed_heads();
        assert!(choppy.contains(&HeadId::Grid));
        assert!(choppy.contains(&HeadId::ScalpM5));
        assert!(!choppy.contains(&HeadId::Momentum));

        // Breakout pending: breakout + smc + news
        let bo = Regime9::BreakoutPending.allowed_heads();
        assert!(bo.contains(&HeadId::Breakout));
        assert!(bo.contains(&HeadId::Smc));
        assert!(bo.contains(&HeadId::News));
    }

    #[test]
    fn trending_flag_only_marks_trend_regimes() {
        assert!(Regime9::StrongTrendDown.is_trending());
        assert!(Regime9::WeakTrendUp.is_trending());
        assert!(!Regime9::RangingWide.is_trending());
        assert!(!Regime9::Transitioning.is_trending());
    }
}
