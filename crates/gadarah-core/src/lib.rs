pub mod decimal_math;
pub mod heads;
pub mod indicators;
pub mod regime;
pub mod session;
pub mod types;

// Re-export core types at crate root for ergonomic imports
pub use types::{
    utc_day, utc_hour, Bar, Direction, HeadId, Regime9, RegimeSignal9, Session, SessionProfile,
    SignalKind, Timeframe, TradeSignal,
};

pub use decimal_math::{decimal_ln, decimal_sqrt};

pub use regime::RegimeClassifier;

pub use heads::{AsianRangeHead, BreakoutHead, Head, MomentumHead};

pub use indicators::{
    BBValues, BBWidthPercentile, BollingerBands, ChoppinessIndex, HurstExponent, WilderSmooth, ADX,
    ATR, EMA, VWAP,
};
