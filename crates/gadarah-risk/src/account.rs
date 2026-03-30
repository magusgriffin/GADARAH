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
        self.dd_from_hwm_pct = (self.high_water_mark - equity) / self.high_water_mark * dec!(100);
        self.dd_remaining_pct = self.firm.max_dd_limit_pct - self.dd_from_hwm_pct;
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
