//! Exit-management primitives shared across heads.

pub mod trail;

pub use trail::{ExitState, TrailConfig, TrailDecision, TrailMachine};
