use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};

use crate::account::{AccountPhase, AccountState};

// ---------------------------------------------------------------------------
// UrgencyProfile
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UrgencyProfile {
    /// Full normal trading.
    Normal,
    /// Slightly more aggressive -- accept lower confidence.
    PushSelective,
    /// Near target -- only A+ setups, shrink sizing.
    Coast,
    /// Payout window -- minimize risk, protect P&L.
    Protect,
}

// ---------------------------------------------------------------------------
// TemporalIntelligence
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemporalIntelligence {
    pub challenge_day: u32,
    pub min_days_remaining: i32,
    pub payout_window_day: Option<u32>,
    pub days_to_next_payout: Option<u32>,
    pub is_friday_afternoon: bool,
    pub is_month_end: bool,
}

impl TemporalIntelligence {
    pub fn new() -> Self {
        Self {
            challenge_day: 0,
            min_days_remaining: 0,
            payout_window_day: None,
            days_to_next_payout: None,
            is_friday_afternoon: false,
            is_month_end: false,
        }
    }

    /// Determine the urgency profile based on temporal state and account progress.
    pub fn urgency_profile(&self, account: &AccountState) -> UrgencyProfile {
        // Payout window: PROTECT
        if account.phase == AccountPhase::PayoutWindow {
            return UrgencyProfile::Protect;
        }

        // Challenge near completion (remaining <= 20% of target): COAST
        if account.target_remaining <= account.firm.profit_target_pct * dec!(0.20) {
            return UrgencyProfile::Coast;
        }

        // Challenge stalling (day 15+ with <30% of target earned): PUSH
        if account.trading_days > 15
            && account.profit_pct < account.firm.profit_target_pct * dec!(0.30)
        {
            return UrgencyProfile::PushSelective;
        }

        // Friday afternoon: reduce risk (weekend gap)
        if self.is_friday_afternoon {
            return UrgencyProfile::Coast;
        }

        UrgencyProfile::Normal
    }
}

impl Default for TemporalIntelligence {
    fn default() -> Self {
        Self::new()
    }
}
