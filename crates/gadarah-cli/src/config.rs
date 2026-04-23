#![allow(dead_code)]

use std::path::{Path, PathBuf};

use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::Deserialize;

use gadarah_broker::MockConfig;
use gadarah_risk::{
    ComplianceBlackoutWindow, DailyPnlConfig, DriftBenchmarks, DriftConfig,
    EquityCurveFilterConfig, FirmConfig, PyramidConfig, TradeManagerConfig,
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
    #[serde(default)]
    pub compliance: ComplianceToml,
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

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ComplianceToml {
    #[serde(default)]
    pub fundingpips: FundingPipsComplianceToml,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct FundingPipsComplianceToml {
    pub blackout_file: Option<String>,
    #[serde(default)]
    pub blackout_windows: Vec<ComplianceBlackoutWindow>,
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
    /// Optional phase-2 target. Only set for 2-step programs; when `None` the
    /// firm is single-phase and `profit_target_pct` alone is authoritative.
    #[serde(default)]
    pub phase2_profit_target_pct: Option<Decimal>,
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
    pub access_token_env: Option<String>,
    pub account_id_env: Option<String>,
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

    pub fn fundingpips_blackout_windows(
        &self,
        config_path: &Path,
    ) -> Result<Vec<ComplianceBlackoutWindow>, String> {
        let mut windows = self.compliance.fundingpips.blackout_windows.clone();

        if let Some(path) = self.compliance.fundingpips.blackout_file.as_deref() {
            let resolved = resolve_relative_path(config_path, path);
            let mut file_windows = load_blackout_file(&resolved)?;
            windows.append(&mut file_windows);
        }

        validate_blackout_windows(&windows)?;
        windows.sort_by_key(|window| (window.starts_at, window.ends_at, window.label.clone()));
        Ok(windows)
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

impl BrokerToml {
    pub fn access_token_env_name(&self) -> String {
        self.access_token_env.clone().unwrap_or_else(|| {
            self.client_id_env
                .strip_suffix("_CLIENT_ID")
                .map(|prefix| format!("{prefix}_ACCESS_TOKEN"))
                .unwrap_or_else(|| "GADARAH_CTRADER_ACCESS_TOKEN".to_string())
        })
    }

    pub fn account_id_env_name(&self) -> String {
        self.account_id_env.clone().unwrap_or_else(|| {
            self.client_id_env
                .strip_suffix("_CLIENT_ID")
                .map(|prefix| format!("{prefix}_ACCOUNT_ID"))
                .unwrap_or_else(|| "GADARAH_CTRADER_ACCOUNT_ID".to_string())
        })
    }
}

fn resolve_relative_path(config_path: &Path, raw: &str) -> PathBuf {
    let path = Path::new(raw);
    if path.is_absolute() {
        return path.to_path_buf();
    }

    config_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(path)
}

fn load_blackout_file(path: &Path) -> Result<Vec<ComplianceBlackoutWindow>, String> {
    #[derive(Debug, Default, Deserialize)]
    struct BlackoutFile {
        #[serde(default)]
        blackout_windows: Vec<ComplianceBlackoutWindow>,
    }

    let content = std::fs::read_to_string(path)
        .map_err(|err| format!("Failed to read blackout file {}: {}", path.display(), err))?;
    let parsed = toml::from_str::<BlackoutFile>(&content)
        .map_err(|err| format!("Failed to parse blackout file {}: {}", path.display(), err))?;
    Ok(parsed.blackout_windows)
}

fn validate_blackout_windows(windows: &[ComplianceBlackoutWindow]) -> Result<(), String> {
    for window in windows {
        if window.ends_at < window.starts_at {
            return Err(format!(
                "Invalid blackout window {}: ends_at {} is before starts_at {}",
                window.label, window.ends_at, window.starts_at
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("gadarah_config_{name}_{nanos}"))
    }

    fn minimal_config(extra: &str) -> String {
        format!(
            r#"
[engine]
mode = "challenge"
symbols = ["EURUSD"]
log_level = "info"
db_path = "data/gadarah.db"

[risk]
base_risk_pct = 0.74
max_portfolio_heat = 2.0
daily_stop_pct = 1.5
daily_target_pct = 2.0

[kill_switch]
daily_dd_trigger_pct = 95.0
total_dd_trigger_pct = 95.0
consecutive_loss_limit = 3
cooldown_minutes = 30

[equity_curve]
ma_period = 20
below_ma_mult = 0.50
deep_below_mult = 0.25
deep_threshold_pct = 2.0

[pyramid]
enabled = false
min_r_to_add = 1.0
max_layers = 2
add_size_fraction = 0.5

[drift]
min_trades = 20
win_rate_alert_delta = 0.12
win_rate_halt_delta = 0.20
avg_r_halt = -0.10
slippage_alert_mult = 2.0

[execution]
max_spread_atr_ratio = 0.30
stale_price_seconds = 2
min_net_rr = 1.2

{extra}
"#
        )
    }

    #[test]
    fn fundingpips_blackout_windows_merge_inline_and_file() {
        let dir = temp_path("merge");
        std::fs::create_dir_all(&dir).unwrap();
        let blackout_file = dir.join("fundingpips_blackouts.toml");
        std::fs::write(
            &blackout_file,
            r#"
[[blackout_windows]]
starts_at = 300
ends_at = 360
label = "USD CPI"
"#,
        )
        .unwrap();

        let raw = minimal_config(
            r#"
[compliance.fundingpips]
blackout_file = "fundingpips_blackouts.toml"

[[compliance.fundingpips.blackout_windows]]
starts_at = 120
ends_at = 180
label = "Inline window"
"#,
        );

        let config = toml::from_str::<GadarahConfig>(&raw).unwrap();
        let windows = config
            .fundingpips_blackout_windows(&dir.join("gadarah.toml"))
            .unwrap();

        assert_eq!(windows.len(), 2);
        assert_eq!(windows[0].label, "Inline window");
        assert_eq!(windows[1].label, "USD CPI");

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn fundingpips_blackout_windows_reject_invalid_range() {
        let raw = minimal_config(
            r#"
[[compliance.fundingpips.blackout_windows]]
starts_at = 500
ends_at = 400
label = "Broken window"
"#,
        );

        let config = toml::from_str::<GadarahConfig>(&raw).unwrap();
        let err = config
            .fundingpips_blackout_windows(Path::new("config/gadarah.toml"))
            .unwrap_err();

        assert!(err.contains("Invalid blackout window Broken window"));
    }
}
