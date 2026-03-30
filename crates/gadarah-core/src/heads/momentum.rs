use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::heads::Head;
use crate::indicators::VWAP;
use crate::types::{
    Bar, Direction, HeadId, Regime9, RegimeSignal9, Session, SessionProfile, SignalKind,
    TradeSignal,
};

/// Configuration for the MomentumHead.
#[derive(Debug, Clone)]
pub struct MomentumConfig {
    /// Minimum R:R ratio to emit a signal.
    pub min_rr: Decimal,
    /// Base confidence for this head.
    pub base_confidence: Decimal,
    /// Number of M15 bars that form the opening range (8 = 120 min).
    pub first_hour_bars: u32,
    /// Minimum range height in pips to consider a valid setup.
    pub min_range_pips: Decimal,
    /// Buffer pips: close must exceed range boundary by this amount.
    pub breakout_buffer_pips: Decimal,
    /// Pip size for this symbol (0.0001 for majors, 0.01 for JPY).
    pub pip_size: Decimal,
    /// Symbol name for emitted signals.
    pub symbol: String,
}

impl Default for MomentumConfig {
    fn default() -> Self {
        Self {
            min_rr: dec!(1.5),
            base_confidence: dec!(0.65),
            first_hour_bars: 8,
            min_range_pips: dec!(15.0),
            breakout_buffer_pips: dec!(3.0),
            pip_size: dec!(0.0001),
            symbol: String::from("GBPUSD"),
        }
    }
}

/// MomentumHead — captures session-open continuation and clean expansion.
///
/// Tracks the first-hour range per session (London UTC 7-9:30, NY UTC 13:30-16).
/// Uses VWAP as a directional filter.
///
/// Entry: close above first_hour_high (Buy) or below first_hour_low (Sell).
/// SL: midpoint of first-hour range.
/// TP1 (50%): range height x 1 beyond breakout level.
/// TP2 (50%): range height x 2 beyond breakout level.
#[derive(Debug, Clone)]
pub struct MomentumHead {
    config: MomentumConfig,
    vwap: VWAP,
    session_high: Option<Decimal>,
    session_low: Option<Decimal>,
    range_formed: bool,
    bars_since_open: u32,
    current_session: Option<Session>,
    trade_taken: bool,
    bars_processed: usize,
}

impl MomentumHead {
    pub fn new(config: MomentumConfig) -> Self {
        Self {
            config,
            vwap: VWAP::new(),
            session_high: None,
            session_low: None,
            range_formed: false,
            bars_since_open: 0,
            current_session: None,
            trade_taken: false,
            bars_processed: 0,
        }
    }

    /// Returns true if the given hour falls within a valid momentum trading session.
    /// London: UTC 7-11, NY: UTC 12-20 (we track range during the first portion).
    fn is_momentum_session(session: Session) -> bool {
        matches!(session, Session::London | Session::Overlap | Session::NyPm)
    }

    /// Update the opening range tracking for the current session.
    fn update_session_range(&mut self, bar: &Bar, session: &SessionProfile) {
        let is_session_start = self.current_session != Some(session.session);

        if is_session_start {
            // Reset range tracking on session change
            self.session_high = None;
            self.session_low = None;
            self.range_formed = false;
            self.bars_since_open = 0;
            self.trade_taken = false;
            self.current_session = Some(session.session);
        }

        self.bars_since_open += 1;

        // Build opening range (8 M15 bars = 120 min by default)
        if self.bars_since_open <= self.config.first_hour_bars {
            self.session_high = Some(self.session_high.unwrap_or(bar.high).max(bar.high));
            self.session_low = Some(self.session_low.unwrap_or(bar.low).min(bar.low));
            if self.bars_since_open == self.config.first_hour_bars {
                self.range_formed = true;
            }
        }
    }

    /// Compute the head confidence, boosted for strong trend regimes.
    fn compute_confidence(&self, regime: &RegimeSignal9) -> Decimal {
        let base = self.config.base_confidence;
        match regime.regime {
            Regime9::StrongTrendUp | Regime9::StrongTrendDown => (base + dec!(0.15)).min(dec!(1.0)),
            _ => base,
        }
    }
}

impl Head for MomentumHead {
    fn id(&self) -> HeadId {
        HeadId::Momentum
    }

    fn evaluate(
        &mut self,
        bar: &Bar,
        session: &SessionProfile,
        regime: &RegimeSignal9,
    ) -> Vec<TradeSignal> {
        self.bars_processed += 1;

        // Check if this head is allowed in the current regime
        if !self.regime_allowed(regime) {
            // Still update internal state even if regime disallows trading
            let session_changed = self.current_session != Some(session.session);
            self.vwap.update(bar, session_changed);
            self.update_session_range(bar, session);
            return Vec::new();
        }

        // Check if we are in a valid momentum session
        if !Self::is_momentum_session(session.session) {
            let session_changed = self.current_session != Some(session.session);
            self.vwap.update(bar, session_changed);
            self.update_session_range(bar, session);
            return Vec::new();
        }

        // Update VWAP
        let session_changed = self.current_session != Some(session.session);
        let vwap_val = self.vwap.update(bar, session_changed);

        // Update range tracking
        self.update_session_range(bar, session);

        // Warmup guard
        if self.bars_processed < self.warmup_bars() {
            return Vec::new();
        }

        // Dead session means zero sizing -> no trade
        if session.sizing_mult.is_zero() {
            return Vec::new();
        }

        // Already taken a trade this session
        if self.trade_taken {
            return Vec::new();
        }

        // Range must be formed before we look for breakouts
        if !self.range_formed {
            return Vec::new();
        }

        let high = match self.session_high {
            Some(h) => h,
            None => return Vec::new(),
        };
        let low = match self.session_low {
            Some(l) => l,
            None => return Vec::new(),
        };

        let range = high - low;
        if range.is_zero() {
            return Vec::new();
        }

        // Minimum range height filter — skip tiny ranges that produce noise breakouts
        let range_pips = range / self.config.pip_size;
        if range_pips < self.config.min_range_pips {
            return Vec::new();
        }

        // Determine direction: close must exceed range boundary by buffer
        let buffer = self.config.breakout_buffer_pips * self.config.pip_size;
        let direction = if bar.close > high + buffer {
            Direction::Buy
        } else if bar.close < low - buffer {
            Direction::Sell
        } else {
            return Vec::new();
        };

        // VWAP filter: only apply when we have real volume data
        // (cum avg typical price is a poor proxy — skip it)
        if bar.volume > 0 {
            match direction {
                Direction::Buy if bar.close <= vwap_val => return Vec::new(),
                Direction::Sell if bar.close >= vwap_val => return Vec::new(),
                _ => {}
            }
        }

        // SL: midpoint of first-hour range
        let stop_loss = (high + low) / dec!(2);

        // Entry: current close price
        let entry = bar.close;

        // TP1: range height x 1 beyond breakout level
        // TP2: range height x 2 beyond breakout level
        let (take_profit, take_profit2) = match direction {
            Direction::Buy => (high + range, high + range * dec!(2)),
            Direction::Sell => (low - range, low - range * dec!(2)),
        };

        // Compute R:R on TP1 (the closer target)
        let risk = (entry - stop_loss).abs();
        if risk.is_zero() {
            return Vec::new();
        }
        let reward = (take_profit - entry).abs();
        let rr = reward / risk;

        if rr < self.config.min_rr {
            return Vec::new();
        }

        self.trade_taken = true;

        let confidence = self.compute_confidence(regime);

        vec![TradeSignal {
            symbol: self.config.symbol.clone(),
            direction,
            kind: SignalKind::Open,
            entry,
            stop_loss,
            take_profit,
            take_profit2: Some(take_profit2),
            head: HeadId::Momentum,
            head_confidence: confidence,
            regime: regime.regime,
            session: session.session,
            pyramid_level: 0,
            comment: format!(
                "Momentum: {} breakout of first-hour range [{}-{}], VWAP={}, R:R={}",
                match direction {
                    Direction::Buy => "bullish",
                    Direction::Sell => "bearish",
                },
                low,
                high,
                vwap_val,
                rr.round_dp(2),
            ),
            generated_at: bar.timestamp,
        }]
    }

    fn reset(&mut self) {
        self.vwap.reset();
        self.session_high = None;
        self.session_low = None;
        self.range_formed = false;
        self.bars_since_open = 0;
        self.current_session = None;
        self.trade_taken = false;
        self.bars_processed = 0;
    }

    fn warmup_bars(&self) -> usize {
        200
    }

    fn regime_allowed(&self, regime: &RegimeSignal9) -> bool {
        regime.regime.allowed_heads().contains(&HeadId::Momentum)
    }
}
