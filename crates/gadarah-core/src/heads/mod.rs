pub mod asian_range;
pub mod breakout;
pub mod momentum;

pub use asian_range::AsianRangeHead;
pub use breakout::BreakoutHead;
pub use momentum::MomentumHead;

use crate::types::{Bar, HeadId, RegimeSignal9, SessionProfile, TradeSignal};

/// CRITICAL: evaluate() receives ONE bar (the just-closed bar).
/// Heads maintain their own streaming indicator state internally.
/// The caller NEVER passes a buffer slice -- this makes HYDRA's
/// indicator double-counting bug impossible by construction.
pub trait Head: Send + Sync {
    fn id(&self) -> HeadId;

    /// Process one new closed bar. Returns zero or more trade signals.
    /// INVARIANT: Must be called exactly once per closed bar, in order.
    fn evaluate(
        &mut self,
        bar: &Bar,
        session: &SessionProfile,
        regime: &RegimeSignal9,
    ) -> Vec<TradeSignal>;

    fn reset(&mut self);
    fn warmup_bars(&self) -> usize;
    fn regime_allowed(&self, regime: &RegimeSignal9) -> bool;
}
