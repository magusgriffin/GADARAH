use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};

use crate::account::AccountState;

// ---------------------------------------------------------------------------
// KillSwitch
// ---------------------------------------------------------------------------

/// Emergency stop mechanism. Activates when:
/// - 95% of daily DD limit is reached
/// - 95% of total DD limit is reached
/// - 3 consecutive losses (30-minute cooldown)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KillSwitch {
    active: bool,
    reason: Option<String>,
    activated_at: Option<i64>,
    /// If set, trading resumes after this unix timestamp.
    cooldown_until: Option<i64>,
}

impl KillSwitch {
    pub fn new() -> Self {
        Self {
            active: false,
            reason: None,
            activated_at: None,
            cooldown_until: None,
        }
    }

    /// Check all kill-switch triggers. Returns `true` if the switch is active
    /// (trading should be halted).
    pub fn check(&mut self, account: &AccountState, timestamp: i64) -> bool {
        // Check cooldown expiry first
        if let Some(resume_at) = self.cooldown_until {
            if timestamp >= resume_at {
                self.cooldown_until = None;
                self.active = false;
                self.reason = None;
                self.activated_at = None;
            }
        }

        if self.active {
            return true;
        }

        // Trigger 1: 95% of daily DD limit
        let daily_dd_trigger = account.firm.daily_dd_limit_pct * dec!(0.95);
        if account.dd_from_hwm_pct >= daily_dd_trigger {
            self.activate("Daily DD 95% trigger", timestamp);
            return true;
        }

        // Trigger 2: 95% of total DD limit
        let total_dd_trigger = account.firm.max_dd_limit_pct * dec!(0.95);
        if account.dd_from_hwm_pct >= total_dd_trigger {
            self.activate("Total DD 95% trigger", timestamp);
            return true;
        }

        // Trigger 3: 3 consecutive losses -> 30-minute cooldown
        if account.consecutive_losses >= 3 {
            self.cooldown_until = Some(timestamp + 1800); // 30 minutes
            self.active = true;
            self.reason = Some("3 consecutive losses -- 30-min cooldown".into());
            self.activated_at = Some(timestamp);
            return true;
        }

        false
    }

    /// Manually activate the kill switch (e.g. from drift detector halt).
    pub fn activate(&mut self, reason: &str, timestamp: i64) {
        self.active = true;
        self.reason = Some(reason.into());
        self.activated_at = Some(timestamp);
    }

    /// Manually deactivate the kill switch.
    pub fn deactivate(&mut self) {
        self.active = false;
        self.reason = None;
        self.activated_at = None;
        self.cooldown_until = None;
    }

    /// Whether the kill switch is currently active.
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// The reason the kill switch was activated, if any.
    pub fn reason(&self) -> Option<&str> {
        self.reason.as_deref()
    }

    /// Unix timestamp when the kill switch was activated, if any.
    pub fn activated_at(&self) -> Option<i64> {
        self.activated_at
    }

    /// Unix timestamp when cooldown ends (trading can resume), if any.
    pub fn cooldown_until(&self) -> Option<i64> {
        self.cooldown_until
    }
}

impl Default for KillSwitch {
    fn default() -> Self {
        Self::new()
    }
}
