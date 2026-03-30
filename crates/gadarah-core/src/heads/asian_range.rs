use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::heads::Head;
use crate::types::{
    utc_day, utc_hour, Bar, Direction, HeadId, RegimeSignal9, SessionProfile, SignalKind,
    TradeSignal,
};

/// Configuration for the AsianRangeHead.
#[derive(Debug, Clone)]
pub struct AsianRangeConfig {
    /// UTC hour when the Asian range building starts (inclusive).
    pub asian_start_utc: u8,
    /// UTC hour when the Asian range building ends (exclusive).
    pub asian_end_utc: u8,
    /// UTC hour when the entry window closes (exclusive).
    pub entry_window_end: u8,
    /// Minimum range in pips to consider valid.
    pub min_range_pips: Decimal,
    /// Maximum range in pips to consider valid.
    pub max_range_pips: Decimal,
    /// Buffer in pips beyond asian high/low for entry trigger.
    pub sl_buffer_pips: Decimal,
    /// TP1 multiplier of range beyond asian_high (Buy).
    pub tp1_multiplier: Decimal,
    /// TP2 multiplier of range beyond asian_high (Buy).
    pub tp2_multiplier: Decimal,
    /// Minimum R:R ratio.
    pub min_rr: Decimal,
    /// Maximum trades per day.
    pub max_trades_per_day: u8,
    /// Symbol name for emitted signals.
    pub symbol: String,
    /// Base confidence for this head.
    pub base_confidence: Decimal,
}

impl Default for AsianRangeConfig {
    fn default() -> Self {
        Self {
            asian_start_utc: 0,
            asian_end_utc: 7,
            entry_window_end: 12,
            min_range_pips: dec!(10.0),
            max_range_pips: dec!(80.0),
            sl_buffer_pips: dec!(5.0),
            tp1_multiplier: dec!(1.0),
            tp2_multiplier: dec!(1.5),
            min_rr: dec!(1.2),
            max_trades_per_day: 1,
            symbol: String::from("GBPUSD"),
            base_confidence: dec!(0.70),
        }
    }
}

/// Internal state for the Asian range.
#[derive(Debug, Clone)]
pub struct AsianRangeState {
    pub asian_high: Option<Decimal>,
    pub asian_low: Option<Decimal>,
    pub trade_taken_today: bool,
    pub current_day: i64,
}

impl AsianRangeState {
    fn new() -> Self {
        Self {
            asian_high: None,
            asian_low: None,
            trade_taken_today: false,
            current_day: -1,
        }
    }
}

/// AsianRangeHead — captures structured breakout from the overnight range.
///
/// Builds the Asian range during UTC 00:00-07:00 by tracking the highest high
/// and lowest low. Entry window: UTC 07:00-09:00 only.
///
/// Highly mechanical, easy to test, well-suited to challenge consistency.
#[derive(Debug, Clone)]
pub struct AsianRangeHead {
    config: AsianRangeConfig,
    state: AsianRangeState,
    /// Pip size: 0.0001 for forex majors, 0.01 for gold/indices.
    pip_size: Decimal,
    bars_processed: usize,
}

impl AsianRangeHead {
    pub fn new(config: AsianRangeConfig, pip_size: Decimal) -> Self {
        Self {
            config,
            state: AsianRangeState::new(),
            pip_size,
            bars_processed: 0,
        }
    }

    /// Update the Asian range from the current bar. Handles daily reset and
    /// range accumulation during the Asian session window.
    fn update_asian_range(&mut self, bar: &Bar) {
        let h = utc_hour(bar.timestamp);
        let day = utc_day(bar.timestamp);

        // Daily reset at UTC midnight
        if day != self.state.current_day {
            self.state.asian_high = None;
            self.state.asian_low = None;
            self.state.trade_taken_today = false;
            self.state.current_day = day;
        }

        // Build range during Asian session (00:00-07:00 UTC)
        if h >= self.config.asian_start_utc && h < self.config.asian_end_utc {
            self.state.asian_high = Some(self.state.asian_high.unwrap_or(bar.high).max(bar.high));
            self.state.asian_low = Some(self.state.asian_low.unwrap_or(bar.low).min(bar.low));
        }
    }

    /// Check whether the current bar is within the entry window (UTC 07:00-09:00).
    fn in_entry_window(&self, bar: &Bar) -> bool {
        let h = utc_hour(bar.timestamp);
        h >= self.config.asian_end_utc && h < self.config.entry_window_end
    }

    /// Compute the range in pips.
    fn range_pips(&self) -> Option<Decimal> {
        match (self.state.asian_high, self.state.asian_low) {
            (Some(high), Some(low)) => {
                let range_price = high - low;
                Some(range_price / self.pip_size)
            }
            _ => None,
        }
    }
}

impl Head for AsianRangeHead {
    fn id(&self) -> HeadId {
        HeadId::AsianRange
    }

    fn evaluate(
        &mut self,
        bar: &Bar,
        session: &SessionProfile,
        regime: &RegimeSignal9,
    ) -> Vec<TradeSignal> {
        self.bars_processed += 1;

        // Always update the range, even if regime disallows trading
        self.update_asian_range(bar);

        // Check regime
        if !self.regime_allowed(regime) {
            return Vec::new();
        }

        // Warmup guard
        if self.bars_processed < self.warmup_bars() {
            return Vec::new();
        }

        // Dead session = no new entries
        if session.sizing_mult.is_zero() {
            return Vec::new();
        }

        // Already taken max trades today
        if self.state.trade_taken_today {
            return Vec::new();
        }

        // Must be in the entry window
        if !self.in_entry_window(bar) {
            return Vec::new();
        }

        // Need a formed range
        let asian_high = match self.state.asian_high {
            Some(h) => h,
            None => return Vec::new(),
        };
        let asian_low = match self.state.asian_low {
            Some(l) => l,
            None => return Vec::new(),
        };

        // Range gate: 15 <= range_pips <= 80
        let range_pips = match self.range_pips() {
            Some(rp) => rp,
            None => return Vec::new(),
        };

        if range_pips < self.config.min_range_pips || range_pips > self.config.max_range_pips {
            return Vec::new();
        }

        let range_price = asian_high - asian_low;
        let buffer = self.config.sl_buffer_pips * self.pip_size;

        // Determine direction based on breakout
        // Entry: bar close above asian_high + 5*pip_size (Buy)
        //    OR  bar close below asian_low - 5*pip_size (Sell)
        let direction = if bar.close > asian_high + buffer {
            Direction::Buy
        } else if bar.close < asian_low - buffer {
            Direction::Sell
        } else {
            return Vec::new();
        };

        let entry = bar.close;

        // Stop loss and take profits depend on direction
        let (stop_loss, take_profit, take_profit2) = match direction {
            Direction::Buy => {
                // SL: asian_low + range/2
                let sl = asian_low + range_price / dec!(2);
                // TP1: asian_high + range * 1.0
                let tp1 = asian_high + range_price * self.config.tp1_multiplier;
                // TP2: asian_high + range * 1.5
                let tp2 = asian_high + range_price * self.config.tp2_multiplier;
                (sl, tp1, tp2)
            }
            Direction::Sell => {
                // SL: asian_high - range/2
                let sl = asian_high - range_price / dec!(2);
                // TP1: asian_low - range * 1.0
                let tp1 = asian_low - range_price * self.config.tp1_multiplier;
                // TP2: asian_low - range * 1.5
                let tp2 = asian_low - range_price * self.config.tp2_multiplier;
                (sl, tp1, tp2)
            }
        };

        // Verify minimum R:R
        let risk = (entry - stop_loss).abs();
        if risk.is_zero() {
            return Vec::new();
        }
        let reward = (take_profit - entry).abs();
        let rr = reward / risk;

        if rr < self.config.min_rr {
            return Vec::new();
        }

        self.state.trade_taken_today = true;

        vec![TradeSignal {
            symbol: self.config.symbol.clone(),
            direction,
            kind: SignalKind::Open,
            entry,
            stop_loss,
            take_profit,
            take_profit2: Some(take_profit2),
            head: HeadId::AsianRange,
            head_confidence: self.config.base_confidence,
            regime: regime.regime,
            session: session.session,
            pyramid_level: 0,
            comment: format!(
                "AsianRange: {} breakout, range={:.1} pips [{}-{}], R:R={}",
                match direction {
                    Direction::Buy => "bullish",
                    Direction::Sell => "bearish",
                },
                range_pips,
                asian_low,
                asian_high,
                rr.round_dp(2),
            ),
            generated_at: bar.timestamp,
        }]
    }

    fn reset(&mut self) {
        self.state = AsianRangeState::new();
        self.bars_processed = 0;
    }

    fn warmup_bars(&self) -> usize {
        200
    }

    fn regime_allowed(&self, regime: &RegimeSignal9) -> bool {
        regime.regime.allowed_heads().contains(&HeadId::AsianRange)
    }
}
