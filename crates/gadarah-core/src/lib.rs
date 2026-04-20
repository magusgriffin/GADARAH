pub mod decimal_math;
pub mod exit;
pub mod heads;
pub mod htf_bias;
pub mod indicators;
pub mod meta;
pub mod regime;
pub mod session;
pub mod types;

// Re-export core types at crate root for ergonomic imports
pub use types::{
    utc_day, utc_hour, Bar, Direction, HeadId, Regime9, RegimeSignal9, Session, SessionProfile,
    SignalKind, Timeframe, TradeSignal,
};

pub use decimal_math::{decimal_ln, decimal_sqrt};

pub use exit::{ExitState, TrailConfig, TrailDecision, TrailMachine};
pub use htf_bias::{HtfBias, HtfBiasFilter};
pub use meta::{
    Ensemble, MtfConfirm, MtfDecision, OrderFlowFeatures, OrderFlowTracker, RankedSignal,
    RegimeGate, RegimeGateDecision, ScoredSegment, SegmentStatsProvider, SegmentStatsSnapshot,
    SignalScorer, VolAdjustedStops,
};
pub use regime::{RegimeClassifier, RegimeThresholds};

pub use heads::{
    AsianRangeHead, BreakoutHead, GridHead, Head, MomentumHead, NewsHead, ScalpM1Head, ScalpM5Head,
    SmcHead, TrendHead, VolProfileHead,
};

pub use heads::{
    grid::GridConfig, news::NewsConfig, scalp_m1::ScalpM1Config, scalp_m5::ScalpM5Config,
    smc::SmcConfig, trend::TrendConfig, vol_profile::VolProfileConfig,
};

pub use indicators::{
    BBValues, BBWidthPercentile, BollingerBands, ChoppinessIndex, HurstExponent, WilderSmooth, ADX,
    ATR, EMA, RSI, VWAP,
};
