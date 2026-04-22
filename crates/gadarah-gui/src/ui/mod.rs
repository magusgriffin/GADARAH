//! UI module for GADARAH GUI - contains all tab panels and components

pub mod backtest;
pub mod config_tab;
pub mod dashboard;
pub mod logs;
pub mod oracle;
pub mod payout;
pub mod performance;
pub mod price_chart;
pub mod sessions;

pub use backtest::BacktestPanel;
pub use config_tab::ConfigPanel;
pub use dashboard::DashboardPanel;
pub use logs::LogsPanel;
pub use oracle::OraclePanel;
pub use payout::PayoutPanel;
pub use performance::PerformancePanel;
pub use price_chart::PriceChartPanel;
pub use sessions::SessionsPanel;

use gadarah_backtest::ChallengeRules;

use crate::config::FirmConfig;

/// Resolve a firm config to the correct [`ChallengeRules`] constructor so that
/// the full rule model (trailing DD modes, consistency caps, etc.) is used
/// instead of the flat TOML values.
pub fn challenge_rules_for(firm: &FirmConfig) -> ChallengeRules {
    let name = firm.firm.name.to_lowercase();
    let ctype = firm.firm.challenge_type.to_lowercase();

    if name.contains("ftmo") || ctype.starts_with("ftmo") {
        if name.contains("2-step") || name.contains("2 step") || ctype.contains("2step") {
            return ChallengeRules::ftmo_2step();
        }
        return ChallengeRules::ftmo_1step();
    }

    if name.contains("alpha capital") || name.contains("alpha one") || ctype == "alpha_one" {
        return ChallengeRules::alpha_one();
    }

    if name.contains("fundingpips") && (name.contains("zero") || ctype == "fundingpips_zero") {
        return ChallengeRules::fundingpips_zero();
    }

    if name.contains("fundingpips")
        && (name.contains("1-step") || name.contains("1 step") || ctype == "fundingpips_1step")
    {
        return ChallengeRules::fundingpips_1step();
    }

    if name.contains("the5ers") || name.contains("hyper growth") || ctype == "hyper_growth" {
        return ChallengeRules::the5ers_hyper_growth();
    }

    if name.contains("blue guardian") || ctype == "instant" {
        return ChallengeRules::blue_guardian_instant();
    }

    if name.contains("brightfunded") {
        return ChallengeRules::brightfunded_evaluation();
    }

    ChallengeRules::two_step_pro()
}

/// Return the number of evaluation stages for a challenge type string.
/// Used by the payout panel to build scaling phases.
pub fn challenge_stage_count(challenge_type: &str) -> usize {
    match challenge_type {
        "ftmo_2step" | "2step" => 2,
        "ftmo_1step" | "1step" | "hyper_growth" | "fundingpips_1step" | "fundingpips_zero"
        | "alpha_one" | "instant" => 1,
        ct if ct.contains("2step") => 2,
        _ => 1,
    }
}

/// Return human-readable stage names for a challenge type.
pub fn challenge_stage_names(challenge_type: &str) -> Vec<&'static str> {
    match challenge_type {
        "ftmo_2step" | "2step" => vec!["Phase 1 — Challenge", "Phase 2 — Verification"],
        "ftmo_1step" => vec!["FTMO Challenge"],
        "hyper_growth" => vec!["Level 1"],
        "fundingpips_1step" => vec!["Student"],
        "fundingpips_zero" => vec!["Master"],
        "alpha_one" => vec!["Assessment"],
        "instant" => vec!["Instant"],
        ct if ct.contains("2step") => vec!["Phase 1 — Challenge", "Phase 2 — Verification"],
        _ => vec!["Evaluation"],
    }
}
