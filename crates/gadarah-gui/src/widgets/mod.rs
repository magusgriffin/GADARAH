//! Dark-fantasy ornament + chrome widgets layered over the base theme.
//!
//! Scope: purely visual. Nothing here touches broker, risk, or broker-adjacent
//! state. Callers pass already-computed data (colors, strings, connection
//! status) — ornaments draw what they are told and nothing else.

pub mod acv;
pub mod alert_banner;
pub mod demo_banner;
pub mod mascot;
pub mod ornaments;
