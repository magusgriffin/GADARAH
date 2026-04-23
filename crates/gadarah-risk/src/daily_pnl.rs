use chrono::{DateTime, Datelike, TimeZone, Utc};
use chrono_tz::America::New_York;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};

/// Return a monotonically-increasing day index that advances each time the
/// wall clock crosses 17:00 America/New_York. Used as the reset boundary for
/// the daily P&L engine so the daily-loss and daily-profit windows line up
/// with prop-firm convention through both EST and EDT.
pub fn trading_day_index(timestamp: i64) -> i64 {
    let utc_dt: DateTime<Utc> = match Utc.timestamp_opt(timestamp, 0).single() {
        Some(dt) => dt,
        None => return timestamp.div_euclid(86_400),
    };
    let ny = utc_dt.with_timezone(&New_York);
    // Shift the clock back by 17 hours so the day boundary sits at 17:00 NY
    // rather than midnight — truncating after the shift then gives the day
    // index that advances at each NY close.
    let shifted = ny - chrono::Duration::hours(17);
    shifted.date_naive().num_days_from_ce() as i64
}

// ---------------------------------------------------------------------------
// ProtectiveClose
// ---------------------------------------------------------------------------

/// Reason for an automatic protective close (flatten-all).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProtectiveCloseReason {
    /// Daily stop reached — close everything before it bleeds further.
    DailyStopReached,
    /// Kill switch armed — hard stop.
    KillSwitchArmed,
    /// Total drawdown exhausted.
    TotalDdExhausted,
    /// Broker lost sync; we cannot size new intents safely.
    BrokerDesynced,
}

/// Edge-triggered request emitted once when a protection boundary is crossed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtectiveClose {
    pub reason: ProtectiveCloseReason,
    pub at_timestamp: i64,
}

// ---------------------------------------------------------------------------
// DayState
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DayState {
    /// Full risk allowed.
    Normal,
    /// 60% of daily target hit -- risk x 0.75.
    Cruising,
    /// 100% of daily target hit -- risk x 0.25.
    Protecting,
    /// Daily stop hit -- no new trades.
    DailyStopped,
}

impl DayState {
    pub fn risk_multiplier(&self) -> Decimal {
        match self {
            Self::Normal => dec!(1.0),
            Self::Cruising => dec!(0.75),
            Self::Protecting => dec!(0.25),
            Self::DailyStopped => dec!(0.0),
        }
    }

    /// Numeric ordering: higher = more restrictive, never regress within day.
    fn severity(&self) -> u8 {
        match self {
            Self::Normal => 0,
            Self::Cruising => 1,
            Self::Protecting => 2,
            Self::DailyStopped => 3,
        }
    }
}

// ---------------------------------------------------------------------------
// DailyPnlConfig
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyPnlConfig {
    /// Target daily P&L as percentage of account. Challenge: 2.0%, Funded: 0.40%.
    pub daily_target_pct: Decimal,
    /// Fraction of daily_target that triggers Cruising. Default: 0.60 (60%).
    pub cruise_threshold_pct: Decimal,
    /// Risk multiplier when cruising. Default: 0.75.
    pub cruise_risk_mult: Decimal,
    /// Fraction of daily_target that triggers Protecting. Default: 1.00 (100%).
    pub protect_threshold_pct: Decimal,
    /// Risk multiplier when protecting. Default: 0.25.
    pub protect_risk_mult: Decimal,
    /// Daily stop loss as percentage of account. Challenge: 1.5%, Funded: 0.8%.
    pub daily_stop_pct: Decimal,
}

impl Default for DailyPnlConfig {
    fn default() -> Self {
        Self {
            daily_target_pct: dec!(2.0),
            cruise_threshold_pct: dec!(0.60),
            cruise_risk_mult: dec!(0.75),
            protect_threshold_pct: dec!(1.00),
            protect_risk_mult: dec!(0.25),
            daily_stop_pct: dec!(1.5),
        }
    }
}

impl DailyPnlConfig {
    /// Preset for funded accounts.
    pub fn funded() -> Self {
        Self {
            daily_target_pct: dec!(0.40),
            cruise_threshold_pct: dec!(0.60),
            cruise_risk_mult: dec!(0.75),
            protect_threshold_pct: dec!(1.00),
            protect_risk_mult: dec!(0.25),
            daily_stop_pct: dec!(0.8),
        }
    }
}

// ---------------------------------------------------------------------------
// DailyPnlEngine
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct DailyPnlEngine {
    config: DailyPnlConfig,
    day_open_equity: Decimal,
    day_pnl_usd: Decimal,
    intraday_peak: Decimal,
    state: DayState,
    last_day: i64,
    /// Edge-triggered latch: set when we transition INTO DailyStopped, cleared
    /// when `take_protective_close()` is called. Guarantees exactly-once
    /// liquidation emission per day.
    pending_protective_close: Option<ProtectiveClose>,
}

impl DailyPnlEngine {
    pub fn new(config: DailyPnlConfig, initial_equity: Decimal) -> Self {
        Self {
            config,
            day_open_equity: initial_equity,
            day_pnl_usd: Decimal::ZERO,
            intraday_peak: initial_equity,
            state: DayState::Normal,
            last_day: -1, // Force reset on first update
            pending_protective_close: None,
        }
    }

    /// Update with the current equity and timestamp. Returns the new day state.
    /// State transitions are monotonic within a day: Normal -> Cruising -> Protecting -> DailyStopped.
    /// Never regresses within the same day.
    pub fn update(&mut self, current_equity: Decimal, timestamp: i64) -> DayState {
        // Daily reset uses the prop-firm convention: 5 pm America/New_York.
        // Every supported firm scores the daily-loss / daily-profit clock
        // against that cutoff regardless of broker or trader locale, and it
        // shifts with DST. `trading_day_index` returns a monotonically
        // increasing integer that bumps at each 17:00 NY boundary.
        let day = trading_day_index(timestamp);
        if day != self.last_day {
            // On genuine day rollovers reset to current equity.
            // On the very first call (last_day == -1 sentinel) preserve the initial
            // equity supplied to new(), which is already set in day_open_equity.
            if self.last_day >= 0 {
                self.day_open_equity = current_equity;
            }
            self.intraday_peak = current_equity;
            self.day_pnl_usd = Decimal::ZERO;
            self.state = DayState::Normal;
            self.last_day = day;
            self.pending_protective_close = None;
        }

        self.day_pnl_usd = current_equity - self.day_open_equity;
        if current_equity > self.intraday_peak {
            self.intraday_peak = current_equity;
        }

        let pnl_pct = self.day_pnl_usd / self.day_open_equity * dec!(100);
        let intraday_dd = (self.intraday_peak - current_equity) / self.day_open_equity * dec!(100);

        // Compute candidate state based on current metrics.
        // State transitions: only advance severity, never regress within the same day.
        let candidate = if intraday_dd >= self.config.daily_stop_pct
            || pnl_pct <= -self.config.daily_stop_pct
        {
            DayState::DailyStopped
        } else if pnl_pct >= self.config.daily_target_pct * self.config.protect_threshold_pct {
            DayState::Protecting
        } else if pnl_pct >= self.config.daily_target_pct * self.config.cruise_threshold_pct {
            DayState::Cruising
        } else {
            DayState::Normal
        };

        // Never regress: only accept the candidate if it is at least as severe as current state.
        if candidate.severity() > self.state.severity() {
            let prev = self.state;
            self.state = candidate;
            if prev != DayState::DailyStopped && candidate == DayState::DailyStopped {
                self.pending_protective_close = Some(ProtectiveClose {
                    reason: ProtectiveCloseReason::DailyStopReached,
                    at_timestamp: timestamp,
                });
            }
        }

        self.state
    }

    /// Drain the edge-triggered protective-close signal if one was armed.
    /// Returns `Some(..)` exactly once per transition into `DailyStopped`.
    pub fn take_protective_close(&mut self) -> Option<ProtectiveClose> {
        self.pending_protective_close.take()
    }

    /// Peek at a pending protective-close request without consuming it.
    pub fn peek_protective_close(&self) -> Option<&ProtectiveClose> {
        self.pending_protective_close.as_ref()
    }

    /// Whether new trades can be opened in this day state.
    pub fn can_trade(&self) -> bool {
        self.state != DayState::DailyStopped
    }

    /// Current day state.
    pub fn state(&self) -> DayState {
        self.state
    }

    /// Current intraday P&L in USD.
    pub fn day_pnl_usd(&self) -> Decimal {
        self.day_pnl_usd
    }

    /// Current intraday P&L as percentage of day-open equity.
    pub fn day_pnl_pct(&self) -> Decimal {
        if self.day_open_equity.is_zero() {
            return Decimal::ZERO;
        }
        self.day_pnl_usd / self.day_open_equity * dec!(100)
    }

    /// Day-open equity for reference.
    pub fn day_open_equity(&self) -> Decimal {
        self.day_open_equity
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn engine() -> DailyPnlEngine {
        DailyPnlEngine::new(DailyPnlConfig::default(), dec!(10000))
    }

    #[test]
    fn day_state_never_regresses_within_same_day() {
        let mut e = engine();
        let t = 86400; // Day 1
                       // Push into Cruising (60% of 2% = 1.2% gain)
        e.update(dec!(10120), t);
        assert_eq!(e.state(), DayState::Cruising);
        // Drop back below Cruising threshold — state must NOT revert
        e.update(dec!(10050), t);
        assert_eq!(
            e.state(),
            DayState::Cruising,
            "state must not regress to Normal"
        );
    }

    #[test]
    fn protecting_never_reverts_to_cruising() {
        let mut e = engine();
        let t = 86400;
        // Push to Protecting (100% of 2% = 2.0% gain)
        e.update(dec!(10200), t);
        assert_eq!(e.state(), DayState::Protecting);
        // Equity drops but still positive — still Protecting
        e.update(dec!(10100), t);
        assert_eq!(
            e.state(),
            DayState::Protecting,
            "must not regress from Protecting to Cruising"
        );
    }

    #[test]
    fn daily_stop_halts_trading() {
        let mut e = engine();
        let t = 86400;
        // 1.5% loss triggers DailyStopped
        e.update(dec!(9850), t);
        assert_eq!(e.state(), DayState::DailyStopped);
        assert!(!e.can_trade());
    }

    #[test]
    fn daily_reset_clears_state() {
        let mut e = engine();
        // Day 1 hits DailyStopped
        e.update(dec!(9850), 86400);
        assert_eq!(e.state(), DayState::DailyStopped);
        // Day 2 — new day resets to Normal
        e.update(dec!(9850), 86400 * 2);
        assert_eq!(e.state(), DayState::Normal, "new day should reset state");
        assert!(e.can_trade());
    }

    #[test]
    fn transition_into_stopped_arms_protective_close_exactly_once() {
        let mut e = engine();
        let t = 86400;
        // Step 1: before the stop, nothing is armed.
        e.update(dec!(10050), t);
        assert!(e.peek_protective_close().is_none());
        // Step 2: cross the daily stop → protective close armed with DailyStopReached.
        e.update(dec!(9850), t + 1);
        let pc = e
            .peek_protective_close()
            .expect("protective close must be armed on transition into DailyStopped")
            .clone();
        assert_eq!(pc.reason, ProtectiveCloseReason::DailyStopReached);
        assert_eq!(pc.at_timestamp, t + 1);
        // Step 3: takeing drains the latch.
        let taken = e.take_protective_close().expect("should drain");
        assert_eq!(taken, pc);
        assert!(
            e.take_protective_close().is_none(),
            "latch must not re-fire on subsequent ticks in the same day"
        );
        // Step 4: a further tick while already stopped must not re-arm.
        e.update(dec!(9800), t + 2);
        assert!(
            e.take_protective_close().is_none(),
            "no re-arm while already stopped"
        );
    }

    #[test]
    fn protective_close_latch_clears_on_day_reset() {
        let mut e = engine();
        // Day 1 — arm and drain.
        e.update(dec!(9850), 86400);
        let _ = e.take_protective_close();
        // Day 2 — no latent signal.
        e.update(dec!(10000), 86400 * 2);
        assert!(e.peek_protective_close().is_none());
    }
}
