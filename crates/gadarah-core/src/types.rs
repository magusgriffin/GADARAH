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
    /// Phase 1 MVP: only 3 heads
    pub fn allowed_heads(&self) -> &[HeadId] {
        match self {
            Self::StrongTrendUp | Self::StrongTrendDown => {
                &[HeadId::Momentum, HeadId::Breakout, HeadId::AsianRange]
            }
            Self::WeakTrendUp | Self::WeakTrendDown => {
                &[HeadId::Momentum, HeadId::AsianRange, HeadId::Breakout]
            }
            Self::RangingTight => &[HeadId::AsianRange, HeadId::Breakout],
            Self::RangingWide => &[HeadId::AsianRange, HeadId::Breakout],
            Self::Choppy => &[HeadId::AsianRange],
            Self::BreakoutPending => &[HeadId::Breakout, HeadId::AsianRange],
            Self::Transitioning => &[HeadId::AsianRange, HeadId::Breakout],
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
