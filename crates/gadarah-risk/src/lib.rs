pub mod account;
pub mod compliance;
pub mod consistency;
pub mod daily_pnl;
pub mod drift;
pub mod equity_curve;
pub mod execution;
pub mod kill_switch;
pub mod performance_ledger;
pub mod pyramid;
pub mod sizing;
pub mod temporal;
pub mod trade_manager;
pub mod types;

// Re-export key types at crate root for ergonomic imports
pub use account::{AccountPhase, AccountState, FirmConfig};
pub use compliance::{
    ComplianceBlackoutWindow, ComplianceConfig, ComplianceOpenExposure, ComplianceRejection,
    FundingPipsComplianceConfig, FundingPipsComplianceManager, PropFirmComplianceManager,
};
pub use consistency::ConsistencyTracker;
pub use daily_pnl::{DailyPnlConfig, DailyPnlEngine, DayState};
pub use drift::{DriftBenchmarks, DriftConfig, DriftDetector, DriftSignal, TradeResult};
pub use equity_curve::{EquityCurveFilter, EquityCurveFilterConfig};
pub use execution::{ExecutionConfig, ExecutionEngine, ExecutionResult, FillRecord, FillStats};
pub use kill_switch::KillSwitch;
pub use performance_ledger::{PerformanceLedger, SegmentStats};
pub use pyramid::{
    can_add_pyramid, create_pyramid_layer, PyramidAddCandidate, PyramidConfig, PyramidLayer,
    PyramidState,
};
pub use sizing::{calculate_lots, SizingInputs};
pub use temporal::{TemporalIntelligence, UrgencyProfile};
pub use trade_manager::{OpenPosition, TradeAction, TradeManager, TradeManagerConfig};
pub use types::{RejectReason, RiskDecision, RiskError, RiskPercent, SizingError};
