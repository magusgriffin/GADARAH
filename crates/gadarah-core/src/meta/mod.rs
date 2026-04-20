//! Meta-layer: scoring, gating and selection layered on top of the raw head outputs.
//!
//! Heads still emit candidate `TradeSignal`s on their own. The meta-layer then
//! (a) rejects signals that fail regime/HTF/scorer gates, (b) ranks the surviving
//! ones and (c) returns at most one per bar.
//!
//! Pieces:
//! - [`signal_scorer`] — Bayesian posterior over (head, regime, session) history.
//! - [`regime_gate`]   — regime confidence / age / whitelist gate.
//! - [`mtf_confirm`]   — consult higher-timeframe bias before firing.
//! - [`ensemble`]      — pick at most one signal per bar, subject to scores.
//! - [`orderflow`]     — lightweight tick-based order-flow features.

pub mod ensemble;
pub mod mtf_confirm;
pub mod orderflow;
pub mod regime_gate;
pub mod signal_scorer;
pub mod vol_adjust;

pub use ensemble::{Ensemble, RankedSignal};
pub use mtf_confirm::{MtfConfirm, MtfDecision};
pub use orderflow::{OrderFlowFeatures, OrderFlowTracker};
pub use regime_gate::{RegimeGate, RegimeGateDecision};
pub use signal_scorer::{ScoredSegment, SegmentStatsProvider, SegmentStatsSnapshot, SignalScorer};
pub use vol_adjust::VolAdjustedStops;
