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

pub use heads::{
    AsianRangeHead, BreakoutHead, GridHead, Head, MomentumHead, NewsHead, ScalpM1Head,
    ScalpM5Head, SmcHead, TrendHead, VolProfileHead,
};

pub use heads::{
    grid::GridConfig, news::NewsConfig, scalp_m1::ScalpM1Config, scalp_m5::ScalpM5Config,
    smc::SmcConfig, trend::TrendConfig, vol_profile::VolProfileConfig,
};

pub use indicators::{
    BBValues, BBWidthPercentile, BollingerBands, ChoppinessIndex, HurstExponent, WilderSmooth, ADX,
    ATR, EMA, RSI, VWAP,
};
