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

On the bundled local data as of 2026-03-30, the default CLI/profile is tuned to
`EURUSD` at `0.74%` base risk because that is the lowest tested default that
passes the repo's updated BrightFunded evaluation simulation while keeping the
Monte Carlo ruin rate under `5%`.
