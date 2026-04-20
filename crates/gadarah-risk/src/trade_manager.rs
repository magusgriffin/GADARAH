use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};

use gadarah_core::Direction;

// ---------------------------------------------------------------------------
// TradeManagerConfig
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeManagerConfig {
    /// R-multiple at which to move SL to breakeven (entry).
    pub breakeven_at_r: Decimal,
    /// R-multiple at which to take the first partial close.
    pub partial1_at_r: Decimal,
    /// Fraction of position to close at partial1.
    pub partial1_pct: Decimal,
    /// ATR multiplier for trailing stop.
    pub trail_atr_mult: Decimal,
    /// Hours after which a stale position is closed if below min R.
    pub time_exit_hours: u32,
    /// Minimum R to keep a position alive past time_exit_hours.
    pub time_exit_min_r: Decimal,
    /// If (MFE - current_profit) > adverse_retrace_pct * MFE, close the position.
    pub adverse_retrace_pct: Decimal,
}

impl Default for TradeManagerConfig {
    fn default() -> Self {
        Self {
            breakeven_at_r: dec!(1.0),
            partial1_at_r: dec!(1.5),
            partial1_pct: dec!(0.50),
            trail_atr_mult: dec!(1.0),
            time_exit_hours: 48,
            time_exit_min_r: dec!(0.5),
            adverse_retrace_pct: dec!(0.50),
        }
    }
}

// ---------------------------------------------------------------------------
// OpenPosition
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenPosition {
    pub id: u64,
    pub entry: Decimal,
    pub current_price: Decimal,
    pub sl: Decimal,
    pub tp: Decimal,
    pub tp2: Option<Decimal>,
    pub lots: Decimal,
    pub direction: Direction,
    pub opened_at: i64,
    pub head: gadarah_core::HeadId,
    pub max_favorable_excursion: Decimal,
    pub partial_taken: bool,
    pub breakeven_set: bool,
    pub trailing_active: bool,
}

impl OpenPosition {
    /// Calculate the current R-multiple.
    /// R = (current profit in price) / (risk distance from entry to SL).
    /// Positive R means in profit, negative means in drawdown.
    pub fn current_r(&self) -> Decimal {
        let risk_distance = (self.entry - self.sl).abs();
        if risk_distance.is_zero() {
            return Decimal::ZERO;
        }
        let profit = match self.direction {
            Direction::Buy => self.current_price - self.entry,
            Direction::Sell => self.entry - self.current_price,
        };
        profit / risk_distance
    }

    /// Current unrealized profit in price units (positive = profit, negative = loss).
    fn current_profit(&self) -> Decimal {
        match self.direction {
            Direction::Buy => self.current_price - self.entry,
            Direction::Sell => self.entry - self.current_price,
        }
    }

    /// Update the max favorable excursion if current profit exceeds the previous max.
    pub fn update_mfe(&mut self) {
        let profit = self.current_profit();
        if profit > self.max_favorable_excursion {
            self.max_favorable_excursion = profit;
        }
    }
}

// ---------------------------------------------------------------------------
// TradeAction
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TradeAction {
    MoveSl { new_sl: Decimal },
    ClosePartial { pct: Decimal },
    CloseAll { reason: String },
    NoAction,
}

// ---------------------------------------------------------------------------
// TradeManager
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct TradeManager {
    config: TradeManagerConfig,
}

impl TradeManager {
    pub fn new(config: TradeManagerConfig) -> Self {
        Self { config }
    }

    /// Evaluate a single open position and return a list of management actions.
    /// An empty vec means no action needed.
    pub fn manage_position(
        &self,
        pos: &mut OpenPosition,
        current_time: i64,
        current_atr: Decimal,
    ) -> Vec<TradeAction> {
        // Update MFE with latest price
        pos.update_mfe();

        let mut actions = Vec::new();
        let current_r = pos.current_r();
        let age_seconds = current_time - pos.opened_at;
        let time_exit_seconds = i64::from(self.config.time_exit_hours) * 3600;

        // 1. Check time exit: position age > time_exit_hours AND current_r < time_exit_min_r
        if age_seconds > time_exit_seconds && current_r < self.config.time_exit_min_r {
            actions.push(TradeAction::CloseAll {
                reason: format!(
                    "Time exit: {}h elapsed, R={:.2} < {:.2}",
                    age_seconds / 3600,
                    current_r,
                    self.config.time_exit_min_r
                ),
            });
            return actions;
        }

        // 2. Check adverse retrace: (MFE - current_profit) > adverse_retrace_pct * MFE
        let current_profit = pos.current_profit();
        let mfe = pos.max_favorable_excursion;
        if mfe > Decimal::ZERO {
            let retrace = mfe - current_profit;
            if retrace > self.config.adverse_retrace_pct * mfe {
                actions.push(TradeAction::CloseAll {
                    reason: format!(
                        "Adverse retrace: retraced {:.1}% of MFE",
                        if mfe.is_zero() {
                            Decimal::ZERO
                        } else {
                            retrace / mfe * dec!(100)
                        }
                    ),
                });
                return actions;
            }
        }

        // 3. Check breakeven: current_r >= breakeven_at_r AND not yet set
        if current_r >= self.config.breakeven_at_r && !pos.breakeven_set {
            actions.push(TradeAction::MoveSl { new_sl: pos.entry });
            pos.breakeven_set = true;
        }

        // 4. Check partial: current_r >= partial1_at_r AND no partial taken
        if current_r >= self.config.partial1_at_r && !pos.partial_taken {
            actions.push(TradeAction::ClosePartial {
                pct: self.config.partial1_pct,
            });
            pos.partial_taken = true;
            // Also move SL to entry if not already done
            if !pos.breakeven_set {
                actions.push(TradeAction::MoveSl { new_sl: pos.entry });
                pos.breakeven_set = true;
            }
        }

        // 5. Check trailing: if partial taken, trail SL by atr_mult * ATR in profit direction
        if pos.partial_taken && current_atr > Decimal::ZERO {
            let trail_distance = self.config.trail_atr_mult * current_atr;
            let new_trail_sl = match pos.direction {
                Direction::Buy => pos.current_price - trail_distance,
                Direction::Sell => pos.current_price + trail_distance,
            };
            // Only move SL if it improves (tighter in profit direction, never widen)
            let should_move = match pos.direction {
                Direction::Buy => new_trail_sl > pos.sl,
                Direction::Sell => new_trail_sl < pos.sl,
            };
            if should_move {
                actions.push(TradeAction::MoveSl {
                    new_sl: new_trail_sl,
                });
                pos.trailing_active = true;
            }
        }

        actions
    }

    /// Access to the config.
    pub fn config(&self) -> &TradeManagerConfig {
        &self.config
    }

    /// Emit `CloseAll` for every open position with a uniform reason.
    ///
    /// Caller uses this to translate a single `ProtectiveClose` event into a
    /// batch of exit intents — one per open position — that the order path
    /// then sends to the broker. The intents are yielded in input order.
    pub fn flatten_all(&self, positions: &[OpenPosition], reason: &str) -> Vec<(u64, TradeAction)> {
        positions
            .iter()
            .map(|p| {
                (
                    p.id,
                    TradeAction::CloseAll {
                        reason: reason.to_string(),
                    },
                )
            })
            .collect()
    }
}

#[cfg(test)]
mod flatten_tests {
    use super::*;
    use gadarah_core::HeadId;
    use rust_decimal_macros::dec;

    fn pos(id: u64) -> OpenPosition {
        OpenPosition {
            id,
            entry: dec!(1.0),
            current_price: dec!(1.0),
            sl: dec!(0.99),
            tp: dec!(1.02),
            tp2: None,
            lots: dec!(0.1),
            direction: Direction::Buy,
            opened_at: 0,
            head: HeadId::Momentum,
            max_favorable_excursion: dec!(0),
            partial_taken: false,
            breakeven_set: false,
            trailing_active: false,
        }
    }

    #[test]
    fn flatten_all_emits_one_close_per_position() {
        let tm = TradeManager::new(TradeManagerConfig::default());
        let positions = vec![pos(1), pos(2), pos(3)];
        let intents = tm.flatten_all(&positions, "DailyStopReached");
        assert_eq!(intents.len(), 3);
        for (pid, action) in &intents {
            match action {
                TradeAction::CloseAll { reason } => {
                    assert_eq!(reason, "DailyStopReached");
                    assert!(*pid >= 1 && *pid <= 3);
                }
                _ => panic!("flatten_all must emit CloseAll only"),
            }
        }
    }

    #[test]
    fn flatten_all_on_empty_is_empty() {
        let tm = TradeManager::new(TradeManagerConfig::default());
        assert!(tm.flatten_all(&[], "whatever").is_empty());
    }
}
