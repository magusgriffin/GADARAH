use std::collections::VecDeque;

use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// EquityCurveFilterConfig
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EquityCurveFilterConfig {
    /// Number of closed trades for the moving average. Default: 20.
    pub ma_period: usize,
    /// Risk multiplier when equity is below MA. Default: 0.50.
    pub below_ma_risk_mult: Decimal,
    /// Risk multiplier when equity is deeply below MA (> deep_threshold_pct). Default: 0.25.
    pub deep_below_mult: Decimal,
    /// Percentage below MA that triggers deep reduction. Default: 2.0%.
    pub deep_threshold_pct: Decimal,
}

impl Default for EquityCurveFilterConfig {
    fn default() -> Self {
        Self {
            ma_period: 20,
            below_ma_risk_mult: dec!(0.50),
            deep_below_mult: dec!(0.25),
            deep_threshold_pct: dec!(2.0),
        }
    }
}

// ---------------------------------------------------------------------------
// EquityCurveFilter
// ---------------------------------------------------------------------------

/// 20-trade SMA of equity at each trade close.
/// When equity is below the MA, position sizing is reduced.
/// Cold start (< 20 trades): full size (1.0x).
#[derive(Debug, Clone)]
pub struct EquityCurveFilter {
    config: EquityCurveFilterConfig,
    equity_history: VecDeque<Decimal>,
    equity_ma: Option<Decimal>,
}

impl EquityCurveFilter {
    pub fn new(config: EquityCurveFilterConfig) -> Self {
        Self {
            equity_history: VecDeque::with_capacity(config.ma_period + 1),
            equity_ma: None,
            config,
        }
    }

    /// Record the account equity at trade close. Updates the moving average.
    pub fn record_trade_close(&mut self, equity: Decimal) {
        self.equity_history.push_back(equity);
        if self.equity_history.len() > self.config.ma_period {
            self.equity_history.pop_front();
        }
        if self.equity_history.len() == self.config.ma_period {
            let sum: Decimal = self.equity_history.iter().copied().sum();
            self.equity_ma = Some(sum / Decimal::from(self.config.ma_period));
        }
    }

    /// Returns the risk multiplier based on equity curve position relative to its MA.
    ///
    /// - Cold start (< ma_period trades): 1.0 (full size)
    /// - At or above MA: 1.0
    /// - Below MA but within deep_threshold_pct: below_ma_risk_mult (0.50)
    /// - Deeply below MA (>= deep_threshold_pct): deep_below_mult (0.25)
    pub fn multiplier(&self) -> Decimal {
        let ma = match self.equity_ma {
            None => return dec!(1.0), // Cold start
            Some(m) => m,
        };
        let current = match self.equity_history.back() {
            None => return dec!(1.0),
            Some(e) => *e,
        };
        if current >= ma {
            return dec!(1.0);
        }
        if ma.is_zero() {
            return dec!(1.0);
        }
        let pct_below = (ma - current) / ma * dec!(100);
        if pct_below >= self.config.deep_threshold_pct {
            self.config.deep_below_mult
        } else {
            self.config.below_ma_risk_mult
        }
    }

    /// The current moving average value, if enough data exists.
    pub fn current_ma(&self) -> Option<Decimal> {
        self.equity_ma
    }

    /// Number of trade equity data points recorded so far.
    pub fn trade_count(&self) -> usize {
        self.equity_history.len()
    }
}
