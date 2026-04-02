mod config;
mod dataset_cli;
mod phase1;
mod synth;
mod tuner;

use std::time::Instant;

use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use std::collections::HashMap;

use crate::tuner::{find_robust_params, tune_stress_params};
use gadarah_backtest::{
    run_monte_carlo, run_replay, run_stress_test, simulate_challenges, BacktestStats,
    ChallengeRules, ChallengeSimResult, MonteCarloConfig, ReplayConfig, StressConfig,
};
use gadarah_broker::MockConfig;
use gadarah_core::{
    heads::{
        asian_range::{AsianRangeConfig, AsianRangeHead},
        breakout::{BreakoutConfig, BreakoutHead},
        momentum::{MomentumConfig, MomentumHead},
    },
    utc_hour, BBWidthPercentile, BollingerBands, Head, RegimeClassifier, SessionProfile, Timeframe,
};
use gadarah_data::{
    aggregate_bars, import_csv, insert_bars, list_symbols, load_all_bars, load_bars, CsvFormat,
    Database,
};

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "gadarah=info".parse().unwrap()),
        )
        .compact()
        .init();

    let args: Vec<String> = std::env::args().collect();
    let cmd = args.get(1).map(|s| s.as_str()).unwrap_or("help");

    match cmd {
        "import" => cmd_import(&args[2..]),
        "bulk-import" => cmd_bulk_import(&args[2..]),
        "aggregate" => cmd_aggregate(&args[2..]),
        "audit-data" => cmd_audit_data(&args[2..]),
        "backtest" => cmd_backtest(&args[2..]),
        "dataset-build" => cmd_dataset_build(&args[2..]),
        "dataset-report" => cmd_dataset_report(&args[2..]),
        "dataset-pipeline" => cmd_dataset_pipeline(&args[2..]),
        "diagnose" => cmd_diagnose(&args[2..]),
        "validate" => cmd_validate(&args[2..]),
        "portfolio" => cmd_portfolio(&args[2..]),
        "synth" => cmd_synth(&args[2..]),
        "full" => cmd_full(&args[2..]),
        "tune" => cmd_tune(&args[2..]),
        "live" => cmd_live(&args[2..]),
        "benchmarks" => cmd_benchmarks(&args[2..]),
        _ => print_help(),
    }
}

fn print_help() {
    println!("GADARAH — Prop Trading Engine CLI");
    println!();
    println!("Usage: gadarah <command> [options]");
    println!();
    println!("Commands:");
    println!("  import       <csv_file> <symbol> <timeframe> [format]   Import bars from CSV");
    println!(
        "  bulk-import  <dir> [--db <path>]                        Import all CSVs from directory"
    );
    println!(
        "  aggregate    [--db <path>] [--symbol <sym>] <from> <to> Aggregate timeframes in DB"
    );
    println!(
        "  audit-data   [--config <path>] [--symbol <sym>] [--timeframe <tf>]  Audit stored bars for gaps and alignment"
    );
    println!(
        "  backtest     [--config <path>] [--firm <path>] [--symbol <sym>] [--balance <bal>]  Run Phase 1 engine backtest"
    );
    println!(
        "  dataset-build [--source <dir>] [--db <path>] [--symbols <csv>] [--timeframes <csv>] [--derive-from <tf>] [--derive-to <csv>]
</final_file_content>

IMPORTANT: For any future changes to this file, use the final_file_content shown above as your reference. This content reflects the current state of the file, including any auto-formatting (e.g., if you used single quotes but the formatter converted them to double quotes). Always base your SEARCH/REPLACE operations on this final version to ensure accuracy.



# task_progress RECOMMENDED

When starting a new task, it is recommended to include a todo list using the task_progress parameter.


1. Include a todo list using the task_progress parameter in your next tool call
2. Create a comprehensive checklist of all steps needed
3. Use markdown format: - [ ] for incomplete, - [x] for complete

**Benefits of creating a todo/task_progress list now:**
	- Clear roadmap for implementation
	- Progress tracking throughout the task
	- Nothing gets forgotten or missed
	- Users can see, monitor, and edit the plan

**Example structure:**```
- [ ] Analyze requirements
- [ ] Set up necessary files
- [ ] Implement main functionality
- [ ] Handle edge cases
- [ ] Test the implementation
- [ ] Verify results```

Keeping the task_progress list updated helps track progress and ensures nothing is missed.

<environment_details>
# Cline CLI - Node.js Visible Files
(No visible files)

# Cline CLI - Node.js Open Tabs
(No open tabs)

# Current Time
4/2/2026, 12/19/2026, 12:19:12 AM (America/Chicago, UTC-5:00)

# Context Window Usage
104,093 / 131K tokens used (79%)

# Current Mode
ACT MODE
</environment_details>