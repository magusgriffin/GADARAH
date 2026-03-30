use std::path::Path;

use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::Deserialize;

use gadarah_broker::MockConfig;
use gadarah_risk::{
    DailyPnlConfig, DriftBenchmarks, DriftConfig, EquityCurveFilterConfig, FirmConfig,
    PyramidConfig, TradeManagerConfig,
};

// ---------------------------------------------------------------------------
// Top-level config (gadarah.toml)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct GadarahConfig {
    pub engine: EngineConfig,
    pub risk: RiskConfig,
    pub kill_switch: KillSwitchConfig,
    pub equity_curve: EquityCurveConfig,
    pub pyramid: PyramidToml,
    pub drift: DriftToml,
    pub execution: ExecutionConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EngineConfig {
    pub mode: String,
    pub symbols: Vec<String>,
    pub log_level: String,
    pub db_path: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RiskConfig {
    pub base_risk_pct: Decimal,
    pub max_portfolio_heat: Decimal,
    pub daily_stop_pct: Decimal,
    pub daily_target_pct: Decimal,
}

#[derive(Debug, Clone, Deserialize)]
pub struct KillSwitchConfig {
    pub daily_dd_trigger_pct: Decimal,
    pub total_dd_trigger_pct: Decimal,
    pub consecutive_loss_limit: u8,
    pub cooldown_minutes: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EquityCurveConfig {
    pub ma_period: usize,
    pub below_ma_mult: Decimal,
    pub deep_below_mult: Decimal,
    pub deep_threshold_pct: Decimal,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PyramidToml {
    pub enabled: bool,
    pub min_r_to_add: Decimal,
    pub max_layers: u8,
    pub add_size_fraction: Decimal,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DriftToml {
    pub min_trades: usize,
    pub win_rate_alert_delta: Decimal,
    pub win_rate_halt_delta: Decimal,
    pub avg_r_halt: Decimal,
    pub slippage_alert_mult: Decimal,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ExecutionConfig {
    pub max_spread_atr_ratio: Decimal,
    pub stale_price_seconds: i64,
    pub min_net_rr: Decimal,
}

// ---------------------------------------------------------------------------
// Firm config file (config/firms/*.toml)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct FirmConfigFile {
    pub firm: FirmToml,
    pub broker: BrokerToml,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FirmToml {
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

#[derive(Debug, Clone, Deserialize)]
pub struct BrokerToml {
    pub host: String,
    pub port: u16,
    pub client_id_env: String,
    pub client_secret_env: String,
}

// ---------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------

pub fn load_config(path: &Path) -> Result<GadarahConfig, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read config {}: {}", path.display(), e))?;
    toml::from_str(&content)
        .map_err(|e| format!("Failed to parse config {}: {}", path.display(), e))
}

pub fn load_firm_config(path: &Path) -> Result<FirmConfigFile, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read firm config {}: {}", path.display(), e))?;
    toml::from_str(&content)
        .map_err(|e| format!("Failed to parse firm config {}: {}", path.display(), e))
}

// ---------------------------------------------------------------------------
// Conversion to internal types
// ---------------------------------------------------------------------------

impl GadarahConfig {
    pub fn daily_pnl_config(&self) -> DailyPnlConfig {
        DailyPnlConfig {
            daily_target_pct: self.risk.daily_target_pct,
            cruise_threshold_pct: dec!(0.60),
            cruise_risk_mult: dec!(0.75),
            protect_threshold_pct: dec!(1.00),
            protect_risk_mult: dec!(0.25),
            daily_stop_pct: self.risk.daily_stop_pct,
        }
    }

    pub fn equity_curve_filter_config(&self) -> EquityCurveFilterConfig {
        EquityCurveFilterConfig {
            ma_period: self.equity_curve.ma_period,
            below_ma_risk_mult: self.equity_curve.below_ma_mult,
            deep_below_mult: self.equity_curve.deep_below_mult,
            deep_threshold_pct: self.equity_curve.deep_threshold_pct,
        }
    }

    pub fn pyramid_config(&self) -> PyramidConfig {
        PyramidConfig {
            min_r_to_add: self.pyramid.min_r_to_add,
            max_layers: self.pyramid.max_layers,
            add_size_fraction: self.pyramid.add_size_fraction,
            require_same_regime: true,
        }
    }

    pub fn drift_config(&self) -> DriftConfig {
        DriftConfig {
            min_trades_to_evaluate: self.drift.min_trades,
            win_rate_alert_delta: self.drift.win_rate_alert_delta,
            win_rate_halt_delta: self.drift.win_rate_halt_delta,
            avg_r_halt_threshold: self.drift.avg_r_halt,
            slippage_alert_mult: self.drift.slippage_alert_mult,
        }
    }

    pub fn trade_manager_config(&self) -> TradeManagerConfig {
        TradeManagerConfig::default()
    }

    pub fn mock_config(&self) -> MockConfig {
        MockConfig::default()
    }

    /// Default drift benchmarks (conservative placeholders until populated from backtest).
    pub fn default_drift_benchmarks(&self) -> DriftBenchmarks {
        DriftBenchmarks {
            expected_win_rate: dec!(0.50),
            expected_avg_r: dec!(0.20),
            expected_profit_factor: dec!(1.40),
            max_consecutive_losses: 5,
            expected_avg_slippage: dec!(0.3),
        }
    }

    pub fn is_challenge_mode(&self) -> bool {
        self.engine.mode == "challenge"
    }
}

impl FirmToml {
    pub fn to_firm_config(&self) -> FirmConfig {
        FirmConfig {
            name: self.name.clone(),
            challenge_type: self.challenge_type.clone(),
            profit_target_pct: self.profit_target_pct,
            daily_dd_limit_pct: self.daily_dd_limit_pct,
            max_dd_limit_pct: self.max_dd_limit_pct,
            dd_mode: self.dd_mode.clone(),
            min_trading_days: self.min_trading_days,
            news_trading_allowed: self.news_trading_allowed,
            max_positions: self.max_positions,
            profit_split_pct: self.profit_split_pct,
        }
    }
}
