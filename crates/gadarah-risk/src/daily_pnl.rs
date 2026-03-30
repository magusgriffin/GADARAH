use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};

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
        }
    }

    /// Update with the current equity and timestamp. Returns the new day state.
    /// State transitions are monotonic within a day: Normal -> Cruising -> Protecting -> DailyStopped.
    /// Never regresses within the same day.
    pub fn update(&mut self, current_equity: Decimal, timestamp: i64) -> DayState {
        // Daily reset: check if we moved to a new day
        let day = timestamp.div_euclid(86400);
        if day != self.last_day {
            self.day_open_equity = current_equity;
            self.intraday_peak = current_equity;
            self.day_pnl_usd = Decimal::ZERO;
            self.state = DayState::Normal;
            self.last_day = day;
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
            self.state = candidate;
        }

        self.state
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
