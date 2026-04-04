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
    cooldown_loss_streak: Option<u8>,
}

impl KillSwitch {
    pub fn new() -> Self {
        Self {
            active: false,
            reason: None,
            activated_at: None,
            cooldown_until: None,
            cooldown_loss_streak: None,
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

        if self
            .cooldown_loss_streak
            .is_some_and(|streak| account.consecutive_losses < streak)
        {
            self.cooldown_loss_streak = None;
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
        if account.consecutive_losses >= 3
            && self.cooldown_loss_streak != Some(account.consecutive_losses)
        {
            self.cooldown_until = Some(timestamp + 1800); // 30 minutes
            self.active = true;
            self.reason = Some("3 consecutive losses -- 30-min cooldown".into());
            self.activated_at = Some(timestamp);
            self.cooldown_loss_streak = Some(account.consecutive_losses);
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
        self.cooldown_loss_streak = None;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account::{AccountPhase, AccountState, FirmConfig};
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;

    fn account_with_dd(dd_pct: Decimal) -> AccountState {
        AccountState {
            phase: AccountPhase::ChallengePhase1,
            firm: FirmConfig {
                name: "Test".into(),
                challenge_type: "2-step".into(),
                profit_target_pct: dec!(6.0),
                daily_dd_limit_pct: dec!(4.0),
                max_dd_limit_pct: dec!(6.0),
                dd_mode: "trailing".into(),
                min_trading_days: 3,
                news_trading_allowed: false,
                max_positions: 1,
                profit_split_pct: dec!(80),
            },
            starting_balance: dec!(5000),
            current_equity: dec!(5000),
            high_water_mark: dec!(5000),
            profit_pct: Decimal::ZERO,
            dd_from_hwm_pct: dd_pct,
            dd_remaining_pct: dec!(6.0) - dd_pct,
            target_remaining: dec!(6.0),
            trading_days: 0,
            min_days_met: false,
            days_since_funded: 0,
            total_trades: 0,
            consecutive_losses: 0,
            phase_start_time: 0,
        }
    }

    #[test]
    fn fires_at_95_pct_of_daily_dd_limit() {
        let mut ks = KillSwitch::new();
        // daily_dd_limit = 4.0%, 95% trigger = 3.8%
        // Below threshold: 3.7% — should NOT fire
        let account_safe = account_with_dd(dec!(3.7));
        assert!(
            !ks.check(&account_safe, 0),
            "should not fire at 3.7% dd (below 3.8% trigger)"
        );

        // At threshold: 3.8% — MUST fire
        let account_trigger = account_with_dd(dec!(3.8));
        assert!(
            ks.check(&account_trigger, 1),
            "must fire at 3.8% dd (= 95% of 4.0% daily limit)"
        );
    }

    #[test]
    fn fires_at_95_pct_of_total_dd_limit() {
        let mut ks = KillSwitch::new();
        // max_dd_limit = 6.0%, 95% trigger = 5.7%
        // Set daily_dd_limit high (10%) so it doesn't fire before total DD trigger.
        let mut safe = account_with_dd(dec!(5.6));
        safe.firm.daily_dd_limit_pct = dec!(10.0);
        assert!(!ks.check(&safe, 0));
        let mut trigger = account_with_dd(dec!(5.7));
        trigger.firm.daily_dd_limit_pct = dec!(10.0);
        assert!(ks.check(&trigger, 1));
    }

    #[test]
    fn consecutive_losses_triggers_cooldown() {
        let mut ks = KillSwitch::new();
        let mut account = account_with_dd(Decimal::ZERO);
        account.consecutive_losses = 3;
        assert!(ks.check(&account, 1000));
        assert!(ks.is_active());
        // After cooldown expires (1800s = 30 min), resumes
        assert!(!ks.check(&account_with_dd(Decimal::ZERO), 1000 + 1800));
    }

    #[test]
    fn cooldown_still_active_before_expiry() {
        let mut ks = KillSwitch::new();
        let mut account = account_with_dd(Decimal::ZERO);
        account.consecutive_losses = 3;
        ks.check(&account, 0);
        // 29 minutes later — still active
        assert!(ks.check(&account_with_dd(Decimal::ZERO), 1799));
    }

    #[test]
    fn cooldown_does_not_rearm_without_new_loss() {
        let mut ks = KillSwitch::new();
        let mut account = account_with_dd(Decimal::ZERO);
        account.consecutive_losses = 3;
        assert!(ks.check(&account, 1000));
        assert!(ks.is_active());

        // Same stale streak after expiry should not immediately re-arm.
        assert!(!ks.check(&account, 1000 + 1800));
        assert!(!ks.is_active());

        // A fresh additional loss should arm it again.
        account.consecutive_losses = 4;
        assert!(ks.check(&account, 1000 + 1801));
    }
}
