use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};

use gadarah_core::{Direction, Regime9};

use crate::daily_pnl::DayState;

// ---------------------------------------------------------------------------
// PyramidConfig
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PyramidConfig {
    /// Minimum R in profit before adding a pyramid layer. Default: 1.0R.
    pub min_r_to_add: Decimal,
    /// Maximum pyramid layers (initial + max_layers adds). Default: 2.
    pub max_layers: u8,
    /// Each add is this fraction of original position size. Default: 0.50.
    pub add_size_fraction: Decimal,
    /// Whether the regime must remain the same to add. Default: true.
    pub require_same_regime: bool,
}

impl Default for PyramidConfig {
    fn default() -> Self {
        Self {
            min_r_to_add: dec!(1.0),
            max_layers: 2,
            add_size_fraction: dec!(0.50),
            require_same_regime: true,
        }
    }
}

// ---------------------------------------------------------------------------
// PyramidLayer
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PyramidLayer {
    pub lots: Decimal,
    pub entry: Decimal,
    pub added_at: i64,
}

// ---------------------------------------------------------------------------
// PyramidState
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PyramidState {
    pub initial_lots: Decimal,
    pub initial_entry: Decimal,
    pub initial_sl: Decimal,
    pub initial_risk_usd: Decimal,
    pub direction: Direction,
    pub original_regime: Regime9,
    pub layers: Vec<PyramidLayer>,
}

impl PyramidState {
    pub fn new(
        lots: Decimal,
        entry: Decimal,
        sl: Decimal,
        risk_usd: Decimal,
        direction: Direction,
        regime: Regime9,
    ) -> Self {
        Self {
            initial_lots: lots,
            initial_entry: entry,
            initial_sl: sl,
            initial_risk_usd: risk_usd,
            direction,
            original_regime: regime,
            layers: Vec::new(),
        }
    }

    /// Number of pyramid layers added (does not count the initial position).
    pub fn layer_count(&self) -> u8 {
        self.layers.len() as u8
    }

    /// Total lots across initial position and all pyramid layers.
    pub fn total_lots(&self) -> Decimal {
        let layer_lots: Decimal = self.layers.iter().map(|l| l.lots).sum();
        self.initial_lots + layer_lots
    }

    /// Calculate the current R-multiple of the initial position.
    /// R = (current_price - entry) / (entry - sl) for buys, inverted for sells.
    pub fn current_r(&self, current_price: Decimal) -> Decimal {
        let risk_distance = (self.initial_entry - self.initial_sl).abs();
        if risk_distance.is_zero() {
            return Decimal::ZERO;
        }
        let profit_distance = match self.direction {
            Direction::Buy => current_price - self.initial_entry,
            Direction::Sell => self.initial_entry - current_price,
        };
        profit_distance / risk_distance
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct PyramidAddCandidate {
    pub current_price: Decimal,
    pub current_regime: Regime9,
    pub day_state: DayState,
    pub pip_value_per_lot: Decimal,
    pub pip_size: Decimal,
    pub take_profit: Decimal,
}

// ---------------------------------------------------------------------------
// Pyramid eligibility check
// ---------------------------------------------------------------------------

/// Check whether a pyramid add is eligible. All conditions must pass.
///
/// Conditions (from ULTIMATE.md 9.6):
/// 1. Position is open and in profit >= min_r_to_add (1.0R)
/// 2. Layer count < max_layers
/// 3. Regime has not changed from original trade (if require_same_regime)
/// 4. DayState is Normal or Cruising (NOT Protecting or DailyStopped)
/// 5. New SL placement at breakeven gives R:R >= 1.0 for the pyramid add
/// 6. INVARIANT: total risk after pyramid <= original risk_usd
pub fn can_add_pyramid(
    config: &PyramidConfig,
    state: &PyramidState,
    candidate: &PyramidAddCandidate,
) -> bool {
    // Condition 1: In profit >= min_r_to_add
    let current_r = state.current_r(candidate.current_price);
    if current_r < config.min_r_to_add {
        return false;
    }

    // Condition 2: Layer count < max_layers
    if state.layer_count() >= config.max_layers {
        return false;
    }

    // Condition 3: Regime unchanged
    if config.require_same_regime && candidate.current_regime != state.original_regime {
        return false;
    }

    // Condition 4: DayState is Normal or Cruising
    match candidate.day_state {
        DayState::Normal | DayState::Cruising => {}
        DayState::Protecting | DayState::DailyStopped => return false,
    }

    // Condition 5: R:R >= 1.0 for the pyramid add with SL at breakeven (initial entry)
    // For the pyramid add, risk = |current_price - initial_entry| (SL moved to breakeven)
    // Reward = |take_profit - current_price|
    let pyramid_risk_distance = (candidate.current_price - state.initial_entry).abs();
    let pyramid_reward_distance = match state.direction {
        Direction::Buy => candidate.take_profit - candidate.current_price,
        Direction::Sell => candidate.current_price - candidate.take_profit,
    };
    if pyramid_risk_distance.is_zero() || pyramid_reward_distance <= Decimal::ZERO {
        return false;
    }
    let pyramid_rr = pyramid_reward_distance / pyramid_risk_distance;
    if pyramid_rr < dec!(1.0) {
        return false;
    }

    // Condition 6: INVARIANT: total risk after pyramid <= original risk_usd
    // Pyramid add lots = initial_lots * add_size_fraction
    let add_lots = state.initial_lots * config.add_size_fraction;
    // Risk of pyramid add = add_lots * sl_pips * pip_value_per_lot
    // SL is at breakeven (initial_entry), so SL distance = |current_price - initial_entry|
    let sl_pips = pyramid_risk_distance / candidate.pip_size;
    let add_risk_usd = add_lots * sl_pips * candidate.pip_value_per_lot;

    // The original position's SL also moves to breakeven, so its risk is now zero.
    // Previous layers also have their SL at breakeven relative to initial_entry.
    // Total new risk = sum of all layer risks with SL at breakeven
    let existing_layer_risk: Decimal = state
        .layers
        .iter()
        .map(|layer| {
            let layer_sl_distance = (layer.entry - state.initial_entry).abs() / candidate.pip_size;
            layer.lots * layer_sl_distance * candidate.pip_value_per_lot
        })
        .sum();

    let total_risk_after = existing_layer_risk + add_risk_usd;
    if total_risk_after > state.initial_risk_usd {
        return false;
    }

    true
}

/// Create a new pyramid layer. Call this after `can_add_pyramid` returns true.
pub fn create_pyramid_layer(
    config: &PyramidConfig,
    state: &mut PyramidState,
    current_price: Decimal,
    timestamp: i64,
) -> PyramidLayer {
    let lots = state.initial_lots * config.add_size_fraction;
    let layer = PyramidLayer {
        lots,
        entry: current_price,
        added_at: timestamp,
    };
    state.layers.push(layer.clone());
    layer
}
