//! Configuration management for GADARAH GUI

use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Main configuration loaded from gadarah.toml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GadarahConfig {
    pub engine: EngineConfig,
    pub risk: RiskConfig,
    pub kill_switch: KillSwitchConfig,
    pub equity_curve: EquityCurveConfig,
    pub pyramid: PyramidConfig,
    pub drift: DriftConfig,
    pub execution: ExecutionConfig,
    #[serde(default)]
    pub compliance: HashMap<String, toml::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineConfig {
    pub mode: String,
    pub symbols: Vec<String>,
    pub log_level: String,
    pub db_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskConfig {
    pub base_risk_pct: Decimal,
    pub max_portfolio_heat: Decimal,
    pub daily_stop_pct: Decimal,
    pub daily_target_pct: Decimal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KillSwitchConfig {
    pub daily_dd_trigger_pct: Decimal,
    pub total_dd_trigger_pct: Decimal,
    pub consecutive_loss_limit: u8,
    pub cooldown_minutes: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EquityCurveConfig {
    pub ma_period: u32,
    pub below_ma_mult: Decimal,
    pub deep_below_mult: Decimal,
    pub deep_threshold_pct: Decimal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PyramidConfig {
    pub enabled: bool,
    pub min_r_to_add: Decimal,
    pub max_layers: u8,
    pub add_size_fraction: Decimal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriftConfig {
    pub min_trades: u32,
    pub win_rate_alert_delta: Decimal,
    pub win_rate_halt_delta: Decimal,
    pub avg_r_halt: Decimal,
    pub slippage_alert_mult: Decimal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionConfig {
    pub max_spread_atr_ratio: Decimal,
    pub stale_price_seconds: u32,
    pub min_net_rr: Decimal,
}

impl Default for GadarahConfig {
    fn default() -> Self {
        Self {
            engine: EngineConfig {
                mode: "challenge".to_string(),
                symbols: vec!["EURUSD".to_string(), "GBPUSD".to_string()],
                log_level: "info".to_string(),
                db_path: "data/gadarah.db".to_string(),
            },
            risk: RiskConfig {
                base_risk_pct: dec!(0.74),
                max_portfolio_heat: dec!(2.0),
                daily_stop_pct: dec!(1.5),
                daily_target_pct: dec!(2.0),
            },
            kill_switch: KillSwitchConfig {
                daily_dd_trigger_pct: dec!(95.0),
                total_dd_trigger_pct: dec!(95.0),
                consecutive_loss_limit: 3,
                cooldown_minutes: 30,
            },
            equity_curve: EquityCurveConfig {
                ma_period: 20,
                below_ma_mult: dec!(0.50),
                deep_below_mult: dec!(0.25),
                deep_threshold_pct: dec!(2.0),
            },
            pyramid: PyramidConfig {
                enabled: false,
                min_r_to_add: dec!(1.0),
                max_layers: 2,
                add_size_fraction: dec!(0.5),
            },
            drift: DriftConfig {
                min_trades: 20,
                win_rate_alert_delta: dec!(0.12),
                win_rate_halt_delta: dec!(0.20),
                avg_r_halt: dec!(-0.10),
                slippage_alert_mult: dec!(2.0),
            },
            execution: ExecutionConfig {
                max_spread_atr_ratio: dec!(0.30),
                stale_price_seconds: 2,
                min_net_rr: dec!(1.2),
            },
            compliance: HashMap::new(),
        }
    }
}

impl GadarahConfig {
    /// Load configuration from file
    pub fn load(path: &PathBuf) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let config: GadarahConfig = toml::from_str(&content)?;
        Ok(config)
    }

    /// Save configuration to file
    pub fn save(&self, path: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
        let content = toml::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }
}

/// Firm configuration loaded from config/firms/*.toml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FirmConfig {
    pub firm: FirmDetails,
    #[serde(default)]
    pub broker: Option<BrokerDetails>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FirmDetails {
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrokerDetails {
    pub host: String,
    pub port: u16,
    pub client_id_env: String,
    pub client_secret_env: String,
}

impl FirmConfig {
    /// Load firm configuration from file
    pub fn load(path: &PathBuf) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let config: FirmConfig = toml::from_str(&content)?;
        Ok(config)
    }
}
