use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};

use crate::daily_pnl::DayState;

// ---------------------------------------------------------------------------
// AccountPhase
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AccountPhase {
    ChallengePhase1,
    ChallengePhase2,
    AwaitingFunded,
    Funded,
    PayoutWindow,
    Failed,
}

// ---------------------------------------------------------------------------
// FirmConfig
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FirmConfig {
    pub name: String,
    pub challenge_type: String,
    pub profit_target_pct: Decimal,
    pub daily_dd_limit_pct: Decimal,
    pub max_dd_limit_pct: Decimal,
    pub dd_mode: String,
    pub min_trading_days: u32,
    pub news_trading_allowed: bool,
    pub max_positions: u8,
    pub profit_split_pct: Decimal,
}

// ---------------------------------------------------------------------------
// AccountState
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountState {
    pub phase: AccountPhase,
    pub firm: FirmConfig,
    pub starting_balance: Decimal,
    pub current_equity: Decimal,
    pub high_water_mark: Decimal,
    pub profit_pct: Decimal,
    pub dd_from_hwm_pct: Decimal,
    pub dd_remaining_pct: Decimal,
    pub target_remaining: Decimal,
    pub trading_days: u32,
    pub min_days_met: bool,
    pub days_since_funded: u32,
    pub total_trades: u32,
    pub consecutive_losses: u8,
    pub phase_start_time: i64,
}

impl AccountState {
    /// Recalculate derived fields after equity changes.
    pub fn update_equity(&mut self, equity: Decimal) {
        self.current_equity = equity;
        if equity > self.high_water_mark {
            self.high_water_mark = equity;
        }
        self.profit_pct = (equity - self.starting_balance) / self.starting_balance * dec!(100);
        let (dd_used_pct, dd_remaining_pct) = match self.firm.dd_mode.as_str() {
            "static" => {
                let dd_used = (self.starting_balance - equity).max(Decimal::ZERO);
                let dd_used_pct = dd_used / self.starting_balance * dec!(100);
                (
                    dd_used_pct,
                    (self.firm.max_dd_limit_pct - dd_used_pct).max(Decimal::ZERO),
                )
            }
            "trailing_locked_to_start" => {
                let max_dd_amount = self.starting_balance * self.firm.max_dd_limit_pct / dec!(100);
                let reference = self
                    .high_water_mark
                    .min(self.starting_balance + max_dd_amount);
                let floor = (self.high_water_mark - max_dd_amount).min(self.starting_balance);
                let dd_used = (reference - equity).max(Decimal::ZERO);
                let dd_remaining = (equity - floor).max(Decimal::ZERO);
                (
                    dd_used / self.starting_balance * dec!(100),
                    dd_remaining / self.starting_balance * dec!(100),
                )
            }
            _ => {
                let dd_used_pct =
                    (self.high_water_mark - equity) / self.high_water_mark * dec!(100);
                (dd_used_pct, self.firm.max_dd_limit_pct - dd_used_pct)
            }
        };
        self.dd_from_hwm_pct = dd_used_pct;
        self.dd_remaining_pct = dd_remaining_pct;
        self.target_remaining = self.firm.profit_target_pct - self.profit_pct;
    }

    /// Risk multiplier based on account phase and challenge progress.
    pub fn phase_risk_multiplier(&self) -> Decimal {
        match self.phase {
            AccountPhase::ChallengePhase1 | AccountPhase::ChallengePhase2 => {
                if self.firm.profit_target_pct.is_zero() {
                    return dec!(1.0);
                }
                let progress = self.profit_pct / self.firm.profit_target_pct;
                if progress >= dec!(0.90) {
                    dec!(0.25)
                } else if progress >= dec!(0.70) {
                    dec!(0.50)
                } else if progress >= dec!(0.50) {
                    dec!(0.75)
                } else {
                    dec!(1.0)
                }
            }
            AccountPhase::Funded => dec!(0.80),
            AccountPhase::PayoutWindow => dec!(0.25),
            AccountPhase::AwaitingFunded | AccountPhase::Failed => dec!(0.0),
        }
    }

    /// DD proximity scaling -- the closer to the limit, the smaller we trade.
    pub fn dd_distance_multiplier(&self) -> Decimal {
        let remaining = self.dd_remaining_pct;
        if remaining <= dec!(0.5) {
            dec!(0.0)
        } else if remaining <= dec!(1.0) {
            dec!(0.15)
        } else if remaining <= dec!(1.5) {
            dec!(0.30)
        } else if remaining <= dec!(2.0) {
            dec!(0.50)
        } else if remaining <= dec!(3.0) {
            dec!(0.75)
        } else {
            dec!(1.0)
        }
    }

    /// Combined effective risk -- MINIMUM of all multipliers.
    pub fn effective_risk_multiplier(
        &self,
        day_state: DayState,
        eq_filter: Decimal,
        drift_mult: Decimal,
    ) -> Decimal {
        self.phase_risk_multiplier()
            .min(self.dd_distance_multiplier())
            .min(day_state.risk_multiplier())
            .min(eq_filter)
            .min(drift_mult)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::daily_pnl::DayState;
    use rust_decimal_macros::dec;

    fn make_account(equity: Decimal) -> AccountState {
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
            current_equity: equity,
            high_water_mark: equity,
            profit_pct: Decimal::ZERO,
            dd_from_hwm_pct: Decimal::ZERO,
            dd_remaining_pct: dec!(6.0),
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
    fn dd_distance_zero_when_within_half_pct_of_limit() {
        let mut account = make_account(dec!(5000));
        // max_dd = 6%, trigger dd_remaining <= 0.5%
        // hwm = 5000, equity must be 5000 * (1 - 0.055) = 4725 → dd = 5.5%, remaining = 0.5%
        account.update_equity(dec!(4725));
        // dd_remaining = 6.0 - 5.5 = 0.5 → returns 0.0
        assert_eq!(account.dd_distance_multiplier(), dec!(0.0));
    }

    #[test]
    fn phase_multiplier_zero_for_awaiting_funded() {
        let mut account = make_account(dec!(5000));
        account.phase = AccountPhase::AwaitingFunded;
        assert_eq!(account.phase_risk_multiplier(), dec!(0.0));
    }

    #[test]
    fn phase_multiplier_zero_for_failed() {
        let mut account = make_account(dec!(5000));
        account.phase = AccountPhase::Failed;
        assert_eq!(account.phase_risk_multiplier(), dec!(0.0));
    }

    #[test]
    fn trailing_locked_to_start_caps_dd_at_initial_balance() {
        let mut account = make_account(dec!(5000));
        account.firm.dd_mode = "trailing_locked_to_start".into();
        account.high_water_mark = dec!(5300);

        account.update_equity(dec!(5000));

        assert_eq!(account.dd_from_hwm_pct, dec!(6.0));
        assert_eq!(account.dd_remaining_pct, Decimal::ZERO);
    }

    #[test]
    fn effective_risk_takes_minimum_of_all_multipliers() {
        let mut account = make_account(dec!(5000));
        account.phase = AccountPhase::Funded; // 0.80 multiplier
                                              // Equity curve filter at 0.50, drift at 1.0
        let result = account.effective_risk_multiplier(
            DayState::Normal, // 1.0
            dec!(0.50),       // equity filter
            dec!(1.0),        // drift
        );
        assert_eq!(result, dec!(0.50), "minimum of all multipliers");
    }
}
