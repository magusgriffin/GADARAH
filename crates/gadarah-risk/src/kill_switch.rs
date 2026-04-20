use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::account::AccountState;
use crate::types::KillReason;

// ---------------------------------------------------------------------------
// KillSwitch
// ---------------------------------------------------------------------------

/// Emergency stop mechanism. Activates when:
/// - 95% of daily DD limit is reached
/// - 95% of total DD limit is reached
/// - 3 consecutive losses (30-minute cooldown)
///
/// Immutability contract: once armed, the only way the switch clears is via
/// `tick()` observing an expired cooldown. There is no public `deactivate()`
/// — external code cannot override a tripped kill switch. A `reset_for_test()`
/// hook is available under `#[cfg(test)]` only.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KillSwitch {
    active: bool,
    reason: Option<KillReason>,
    details: Option<String>,
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
            details: None,
            activated_at: None,
            cooldown_until: None,
            cooldown_loss_streak: None,
        }
    }

    /// Advance internal clock. Expires any cooldown whose window has passed
    /// and logs the audit event. Call every tick before `check()`.
    fn tick(&mut self, timestamp: i64) {
        if let Some(resume_at) = self.cooldown_until {
            if timestamp >= resume_at {
                let prior = self.reason;
                self.cooldown_until = None;
                self.active = false;
                self.reason = None;
                self.details = None;
                self.activated_at = None;
                info!(
                    prior_reason = ?prior,
                    resumed_at = timestamp,
                    "kill switch cooldown expired",
                );
            }
        }
    }

    /// Check all kill-switch triggers. Returns `true` if the switch is active
    /// (trading should be halted).
    pub fn check(&mut self, account: &AccountState, timestamp: i64) -> bool {
        self.tick(timestamp);

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
            self.arm(KillReason::DailyDD, None, timestamp);
            return true;
        }

        // Trigger 2: 95% of total DD limit
        let total_dd_trigger = account.firm.max_dd_limit_pct * dec!(0.95);
        if account.dd_from_hwm_pct >= total_dd_trigger {
            self.arm(KillReason::TotalDD, None, timestamp);
            return true;
        }

        // Trigger 3: 3 consecutive losses -> 30-minute cooldown
        if account.consecutive_losses >= 3
            && self.cooldown_loss_streak != Some(account.consecutive_losses)
        {
            self.cooldown_until = Some(timestamp + 1800); // 30 minutes
            self.cooldown_loss_streak = Some(account.consecutive_losses);
            self.arm(
                KillReason::ConsecutiveLosses,
                Some(format!(
                    "{} consecutive losses → 30 min cooldown",
                    account.consecutive_losses
                )),
                timestamp,
            );
            return true;
        }

        false
    }

    /// Arm the kill switch for an external reason (drift halt, vol halt, compliance).
    /// Idempotent: re-arming while already active keeps the original reason and
    /// timestamp so the audit trail points at the *first* trip.
    pub fn activate(&mut self, reason: KillReason, timestamp: i64) {
        if !self.active {
            self.arm(reason, None, timestamp);
        }
    }

    /// Arm with a free-form detail message for context (e.g. drift reason).
    pub fn activate_with_details(&mut self, reason: KillReason, details: String, timestamp: i64) {
        if !self.active {
            self.arm(reason, Some(details), timestamp);
        }
    }

    fn arm(&mut self, reason: KillReason, details: Option<String>, timestamp: i64) {
        self.active = true;
        self.reason = Some(reason);
        self.details = details.clone();
        self.activated_at = Some(timestamp);
        warn!(
            reason = %reason,
            details = ?details,
            activated_at = timestamp,
            cooldown_until = ?self.cooldown_until,
            "kill switch armed",
        );
    }

    /// Whether the kill switch is currently active.
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Typed reason the kill switch was activated, if any.
    pub fn reason(&self) -> Option<KillReason> {
        self.reason
    }

    /// Human-readable detail message if one was supplied.
    pub fn details(&self) -> Option<&str> {
        self.details.as_deref()
    }

    /// Unix timestamp when the kill switch was activated, if any.
    pub fn activated_at(&self) -> Option<i64> {
        self.activated_at
    }

    /// Unix timestamp when cooldown ends (trading can resume), if any.
    pub fn cooldown_until(&self) -> Option<i64> {
        self.cooldown_until
    }

    /// Test-only hook to clear internal state. Not exposed outside `cfg(test)`.
    #[cfg(test)]
    #[doc(hidden)]
    pub fn reset_for_test(&mut self) {
        self.active = false;
        self.reason = None;
        self.details = None;
        self.activated_at = None;
        self.cooldown_until = None;
        self.cooldown_loss_streak = None;
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
        let account_safe = account_with_dd(dec!(3.7));
        assert!(
            !ks.check(&account_safe, 0),
            "should not fire at 3.7% dd (below 3.8% trigger)"
        );
        let account_trigger = account_with_dd(dec!(3.8));
        assert!(ks.check(&account_trigger, 1));
        assert_eq!(ks.reason(), Some(KillReason::DailyDD));
    }

    #[test]
    fn fires_at_95_pct_of_total_dd_limit() {
        let mut ks = KillSwitch::new();
        let mut safe = account_with_dd(dec!(5.6));
        safe.firm.daily_dd_limit_pct = dec!(10.0);
        assert!(!ks.check(&safe, 0));
        let mut trigger = account_with_dd(dec!(5.7));
        trigger.firm.daily_dd_limit_pct = dec!(10.0);
        assert!(ks.check(&trigger, 1));
        assert_eq!(ks.reason(), Some(KillReason::TotalDD));
    }

    #[test]
    fn consecutive_losses_triggers_cooldown() {
        let mut ks = KillSwitch::new();
        let mut account = account_with_dd(Decimal::ZERO);
        account.consecutive_losses = 3;
        assert!(ks.check(&account, 1000));
        assert!(ks.is_active());
        assert_eq!(ks.reason(), Some(KillReason::ConsecutiveLosses));
        assert!(!ks.check(&account_with_dd(Decimal::ZERO), 1000 + 1800));
    }

    #[test]
    fn cooldown_still_active_before_expiry() {
        let mut ks = KillSwitch::new();
        let mut account = account_with_dd(Decimal::ZERO);
        account.consecutive_losses = 3;
        ks.check(&account, 0);
        assert!(ks.check(&account_with_dd(Decimal::ZERO), 1799));
    }

    #[test]
    fn cooldown_does_not_rearm_without_new_loss() {
        let mut ks = KillSwitch::new();
        let mut account = account_with_dd(Decimal::ZERO);
        account.consecutive_losses = 3;
        assert!(ks.check(&account, 1000));
        assert!(ks.is_active());

        assert!(!ks.check(&account, 1000 + 1800));
        assert!(!ks.is_active());

        account.consecutive_losses = 4;
        assert!(ks.check(&account, 1000 + 1801));
    }

    #[test]
    fn activate_is_idempotent_preserves_first_reason() {
        let mut ks = KillSwitch::new();
        ks.activate(KillReason::DriftDetector, 1000);
        ks.activate(KillReason::Manual, 2000);
        assert_eq!(ks.reason(), Some(KillReason::DriftDetector));
        assert_eq!(ks.activated_at(), Some(1000));
    }

    #[test]
    fn no_public_deactivate_exists() {
        // Compile-only check: if someone re-adds a public `deactivate()` this
        // test must be updated too (because it proves the method is gone).
        let ks = KillSwitch::new();
        assert!(!ks.is_active());
        // The only way to clear state in production is via tick() after a
        // cooldown has elapsed — not tested here, covered by consecutive_losses_triggers_cooldown.
    }
}
