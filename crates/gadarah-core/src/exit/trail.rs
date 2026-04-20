//! Trailing stop / break-even state machine used by every head.
//!
//! Progression (assumes signal's initial stop is the reference distance `1R`):
//!
//! 1. `Initial` — stop at the signal's original SL.
//! 2. At +1R unrealized, move stop to entry → `Breakeven`.
//! 3. At +2R, start trailing: stop = last bar's extreme ∓ `trail_atr_mult * ATR`.
//! 4. At +3R OR when `flatten_on_regime_flip` is signalled upstream, close.
//!
//! The machine is stateless w.r.t. the broker — the caller submits new SLs
//! / close orders using the `TrailDecision` output.

use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::types::Direction;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitState {
    Initial,
    Breakeven,
    Trailing,
    Closed,
}

#[derive(Debug, Clone)]
pub struct TrailConfig {
    /// Unrealized R at which to move SL to entry.
    pub move_to_be_at_r: Decimal,
    /// Unrealized R at which to start trailing.
    pub start_trail_at_r: Decimal,
    /// Unrealized R at which to flatten regardless of regime.
    pub flatten_at_r: Decimal,
    /// Trail distance expressed in ATR multiples.
    pub trail_atr_mult: Decimal,
}

impl Default for TrailConfig {
    fn default() -> Self {
        Self {
            move_to_be_at_r: dec!(1.0),
            start_trail_at_r: dec!(2.0),
            flatten_at_r: dec!(3.0),
            trail_atr_mult: dec!(0.5),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TrailDecision {
    /// No action; caller should leave the SL where it is.
    Hold,
    /// Move the SL to the given price.
    MoveStop { new_sl: Decimal },
    /// Close the position (+3R or externally requested).
    Close,
}

#[derive(Debug, Clone)]
pub struct TrailMachine {
    direction: Direction,
    entry: Decimal,
    initial_sl: Decimal,
    state: ExitState,
    config: TrailConfig,
    last_sl: Decimal,
}

impl TrailMachine {
    pub fn new(
        direction: Direction,
        entry: Decimal,
        initial_sl: Decimal,
        config: TrailConfig,
    ) -> Self {
        Self {
            direction,
            entry,
            initial_sl,
            state: ExitState::Initial,
            config,
            last_sl: initial_sl,
        }
    }

    pub fn state(&self) -> ExitState {
        self.state
    }

    pub fn current_sl(&self) -> Decimal {
        self.last_sl
    }

    /// Update with the latest bar close and ATR.  Also accepts a regime-flip
    /// flag: when true the machine closes regardless of R achieved.
    pub fn on_bar(
        &mut self,
        price: Decimal,
        atr: Decimal,
        regime_flipped: bool,
    ) -> TrailDecision {
        if matches!(self.state, ExitState::Closed) {
            return TrailDecision::Hold;
        }

        let r_value = (self.entry - self.initial_sl).abs();
        if r_value.is_zero() {
            return TrailDecision::Hold;
        }

        let unrealized = match self.direction {
            Direction::Buy => (price - self.entry) / r_value,
            Direction::Sell => (self.entry - price) / r_value,
        };

        if regime_flipped && unrealized > Decimal::ZERO {
            self.state = ExitState::Closed;
            return TrailDecision::Close;
        }

        if unrealized >= self.config.flatten_at_r {
            self.state = ExitState::Closed;
            return TrailDecision::Close;
        }

        if unrealized >= self.config.start_trail_at_r {
            let offset = atr * self.config.trail_atr_mult;
            let candidate = match self.direction {
                Direction::Buy => price - offset,
                Direction::Sell => price + offset,
            };
            let new_sl = match self.direction {
                Direction::Buy => candidate.max(self.last_sl),
                Direction::Sell => {
                    if self.last_sl.is_zero() {
                        candidate
                    } else {
                        candidate.min(self.last_sl)
                    }
                }
            };
            let moved = new_sl != self.last_sl;
            self.state = ExitState::Trailing;
            self.last_sl = new_sl;
            return if moved {
                TrailDecision::MoveStop { new_sl }
            } else {
                TrailDecision::Hold
            };
        }

        if unrealized >= self.config.move_to_be_at_r && self.state == ExitState::Initial {
            self.state = ExitState::Breakeven;
            self.last_sl = self.entry;
            return TrailDecision::MoveStop { new_sl: self.entry };
        }

        TrailDecision::Hold
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn long_machine() -> TrailMachine {
        TrailMachine::new(
            Direction::Buy,
            dec!(100),
            dec!(99),
            TrailConfig::default(),
        )
    }

    #[test]
    fn initial_state_holds() {
        let mut m = long_machine();
        assert_eq!(m.on_bar(dec!(100.5), dec!(1), false), TrailDecision::Hold);
        assert_eq!(m.state(), ExitState::Initial);
    }

    #[test]
    fn moves_to_be_at_plus_one_r() {
        let mut m = long_machine();
        let d = m.on_bar(dec!(101), dec!(1), false);
        assert_eq!(d, TrailDecision::MoveStop { new_sl: dec!(100) });
        assert_eq!(m.state(), ExitState::Breakeven);
    }

    #[test]
    fn trails_after_plus_two_r() {
        let mut m = long_machine();
        m.on_bar(dec!(101), dec!(1), false); // → Breakeven
        let d = m.on_bar(dec!(102), dec!(1), false); // +2R, trail at 0.5 ATR
        match d {
            TrailDecision::MoveStop { new_sl } => {
                assert_eq!(new_sl, dec!(101.5));
            }
            other => panic!("expected MoveStop, got {other:?}"),
        }
        assert_eq!(m.state(), ExitState::Trailing);
    }

    #[test]
    fn never_loosens_trailing_stop() {
        let mut m = long_machine();
        m.on_bar(dec!(101), dec!(1), false);
        m.on_bar(dec!(102), dec!(1), false);
        // Price pulls back → SL must not loosen
        let d = m.on_bar(dec!(101.8), dec!(1), false);
        assert_eq!(d, TrailDecision::Hold);
        assert_eq!(m.current_sl(), dec!(101.5));
    }

    #[test]
    fn closes_at_plus_three_r() {
        let mut m = long_machine();
        let d = m.on_bar(dec!(103), dec!(1), false);
        assert_eq!(d, TrailDecision::Close);
        assert_eq!(m.state(), ExitState::Closed);
    }

    #[test]
    fn regime_flip_exits_only_when_in_profit() {
        let mut m = long_machine();
        let d = m.on_bar(dec!(99.5), dec!(1), true);
        assert_eq!(d, TrailDecision::Hold);
        let d2 = m.on_bar(dec!(101), dec!(1), true);
        assert_eq!(d2, TrailDecision::Close);
    }

    #[test]
    fn short_side_trails_upward() {
        let mut m = TrailMachine::new(
            Direction::Sell,
            dec!(100),
            dec!(101),
            TrailConfig::default(),
        );
        m.on_bar(dec!(99), dec!(1), false); // → BE
        let d = m.on_bar(dec!(98), dec!(1), false); // +2R
        match d {
            TrailDecision::MoveStop { new_sl } => {
                assert_eq!(new_sl, dec!(98.5));
            }
            other => panic!("expected MoveStop, got {other:?}"),
        }
    }
}
