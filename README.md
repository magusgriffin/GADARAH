# GADARAH

GADARAH is a Rust workspace for research and prototyping around rule-based FX trading:
signal generation, risk controls, historical data storage, replay/backtesting, and
validation tooling.

## Workspace

- `crates/gadarah-core`: market types, indicators, regime classifier, strategy heads
- `crates/gadarah-risk`: sizing, drawdown controls, drift/performance utilities
- `crates/gadarah-broker`: broker abstraction plus a mock execution engine
- `crates/gadarah-data`: SQLite storage, bar import, aggregation, trade/equity storage
- `crates/gadarah-backtest`: replay, Monte Carlo, challenge simulation, stress tests
- `crates/gadarah-cli`: manual CLI for import, replay, validation, synthetic data

## Common Commands

```bash
cargo test --workspace
cargo run -p gadarah-cli -- help
cargo run -p gadarah-cli -- synth --bars 1000
cargo run -p gadarah-cli -- backtest --db data/gadarah.db --symbol EURUSD
cargo run -p gadarah-cli -- validate --db data/gadarah.db --symbol EURUSD
cargo run -p gadarah-cli -- portfolio --symbols EURUSD --risk 0.74
```

## Data And Artifacts

- Generated build output lives under `target/`
- Local SQLite data lives under `data/*.db`
- Fetched CSV data lives under `data/fetched/`

These paths are ignored so local runs do not pollute the worktree.

## Current Local Baseline

As of 2026-04-02, the default CLI/profile is aligned to **The5ers Hyper Growth**
`$5k` ruleset: `10%` target, `3%` daily pause from the higher of start-of-day
balance/equity, and a `6%` stopout below initial balance. Optional non-default
firm profiles remain under `config/firms/`. FundingPips profiles can now load
blackout windows from [fundingpips_blackouts.toml](/home/ilovehvn/GADARAH/config/compliance/fundingpips_blackouts.toml).

As of 2026-04-16, the clean U.S. `cTrader` + bot-friendly targets verified from
official sources are **The5ers Hyper Growth** and **FTMO** (`1-Step` / `2-Step`),
with **The5ers** still the best-aligned live baseline for this repo. Read
[PROJECT_READINESS_2026-04-16.md](/home/ilovehvn/GADARAH/PROJECT_READINESS_2026-04-16.md)
before using any non-default firm profile: FundingPips is now comparison-only /
conditional, while Blue Guardian and Alpha One are not valid U.S. `cTrader`
bot deployment targets for this project.
