use std::collections::VecDeque;

use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// DriftBenchmarks
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriftBenchmarks {
    pub expected_win_rate: Decimal,
    pub expected_avg_r: Decimal,
    pub expected_profit_factor: Decimal,
    pub max_consecutive_losses: u8,
    pub expected_avg_slippage: Decimal,
}

// ---------------------------------------------------------------------------
// DriftConfig
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriftConfig {
    pub min_trades_to_evaluate: usize,
    pub win_rate_alert_delta: Decimal,
    pub win_rate_halt_delta: Decimal,
    pub avg_r_halt_threshold: Decimal,
    pub slippage_alert_mult: Decimal,
}

impl Default for DriftConfig {
    fn default() -> Self {
        Self {
            min_trades_to_evaluate: 20,
            win_rate_alert_delta: dec!(0.12),
            win_rate_halt_delta: dec!(0.20),
            avg_r_halt_threshold: dec!(-0.10),
            slippage_alert_mult: dec!(2.0),
        }
    }
}

// ---------------------------------------------------------------------------
// TradeResult
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeResult {
    pub won: bool,
    pub r_multiple: Decimal,
    pub slippage_pips: Decimal,
}

// ---------------------------------------------------------------------------
// DriftSignal
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DriftSignal {
    InsufficientData,
    Healthy,
    ReduceRisk { multiplier: Decimal },
    Halt { reason: String },
}

// ---------------------------------------------------------------------------
// DriftDetector
// ---------------------------------------------------------------------------

const MAX_ROLLING_TRADES: usize = 30;

#[derive(Debug, Clone)]
pub struct DriftDetector {
    config: DriftConfig,
    rolling_trades: VecDeque<TradeResult>,
    backtest_benchmarks: DriftBenchmarks,
}

impl DriftDetector {
    pub fn new(config: DriftConfig, backtest_benchmarks: DriftBenchmarks) -> Self {
        Self {
            config,
            rolling_trades: VecDeque::with_capacity(MAX_ROLLING_TRADES + 1),
            backtest_benchmarks,
        }
    }

    /// Record a completed trade result. Keeps a rolling window of the last 30 trades.
    pub fn record_trade(&mut self, result: TradeResult) {
        self.rolling_trades.push_back(result);
        if self.rolling_trades.len() > MAX_ROLLING_TRADES {
            self.rolling_trades.pop_front();
        }
    }

    /// Evaluate the rolling window against backtest benchmarks.
    pub fn evaluate(&self) -> DriftSignal {
        if self.rolling_trades.len() < self.config.min_trades_to_evaluate {
            return DriftSignal::InsufficientData;
        }

        let live_avg_r = self.rolling_avg_r();
        let live_wr = self.rolling_win_rate();
        let live_slippage = self.rolling_avg_slippage();

        // HALT: negative expectancy
        if live_avg_r < self.config.avg_r_halt_threshold {
            return DriftSignal::Halt {
                reason: format!("Negative expectancy: avg_r={:.3}", live_avg_r),
            };
        }

        // HALT: win rate collapsed
        let wr_delta = self.backtest_benchmarks.expected_win_rate - live_wr;
        if wr_delta > self.config.win_rate_halt_delta {
            return DriftSignal::Halt {
                reason: format!(
                    "Win rate collapse: {:.1}% vs expected {:.1}%",
                    live_wr * dec!(100),
                    self.backtest_benchmarks.expected_win_rate * dec!(100)
                ),
            };
        }

        // ALERT: win rate degraded but not critical
        if wr_delta > self.config.win_rate_alert_delta {
            return DriftSignal::ReduceRisk {
                multiplier: dec!(0.50),
            };
        }

        // ALERT: slippage much worse than demo
        if self.backtest_benchmarks.expected_avg_slippage > Decimal::ZERO
            && live_slippage
                > self.backtest_benchmarks.expected_avg_slippage * self.config.slippage_alert_mult
        {
            return DriftSignal::ReduceRisk {
                multiplier: dec!(0.75),
            };
        }

        DriftSignal::Healthy
    }

    /// Rolling win rate over the window.
    fn rolling_win_rate(&self) -> Decimal {
        if self.rolling_trades.is_empty() {
            return Decimal::ZERO;
        }
        let wins = self.rolling_trades.iter().filter(|t| t.won).count();
        Decimal::from(wins as u32) / Decimal::from(self.rolling_trades.len() as u32)
    }

    /// Rolling average R-multiple over the window.
    fn rolling_avg_r(&self) -> Decimal {
        if self.rolling_trades.is_empty() {
            return Decimal::ZERO;
        }
        let total_r: Decimal = self.rolling_trades.iter().map(|t| t.r_multiple).sum();
        total_r / Decimal::from(self.rolling_trades.len() as u32)
    }

    /// Rolling average slippage over the window.
    fn rolling_avg_slippage(&self) -> Decimal {
        if self.rolling_trades.is_empty() {
            return Decimal::ZERO;
        }
        let total: Decimal = self.rolling_trades.iter().map(|t| t.slippage_pips).sum();
        total / Decimal::from(self.rolling_trades.len() as u32)
    }

    /// Current number of recorded trades in the rolling window.
    pub fn trade_count(&self) -> usize {
        self.rolling_trades.len()
    }

    /// Access to the config.
    pub fn config(&self) -> &DriftConfig {
        &self.config
    }

    /// Access to backtest benchmarks.
    pub fn benchmarks(&self) -> &DriftBenchmarks {
        &self.backtest_benchmarks
    }
}
