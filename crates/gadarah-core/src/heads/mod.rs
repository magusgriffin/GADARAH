pub mod asian_range;
pub mod breakout;
pub mod grid;
pub mod momentum;
pub mod news;
pub mod scalp_m1;
pub mod scalp_m5;
pub mod smc;
pub mod trend;
pub mod vol_profile;

pub use asian_range::AsianRangeHead;
pub use breakout::BreakoutHead;
pub use grid::GridHead;
pub use momentum::MomentumHead;
pub use news::NewsHead;
pub use scalp_m1::ScalpM1Head;
pub use scalp_m5::ScalpM5Head;
pub use smc::SmcHead;
pub use trend::TrendHead;
pub use vol_profile::VolProfileHead;

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
