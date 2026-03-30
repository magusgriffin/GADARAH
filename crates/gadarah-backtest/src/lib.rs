pub mod challenge_sim;
pub mod engine;
pub mod error;
pub mod monte_carlo;
pub mod replayer;
pub mod stats;
pub mod stress_test;
pub mod walk_forward;

pub use challenge_sim::{
    simulate_challenge, simulate_challenges, ChallengeRules, ChallengeSimResult,
    ChallengeStageRules, ChallengeStageSimResult, DailyDrawdownMode,
};
pub use engine::{run_engine, EngineConfig, EngineResult};
pub use error::BacktestError;
pub use monte_carlo::{run_monte_carlo, MonteCarloConfig, MonteCarloResult};
pub use replayer::{run_replay, ReplayConfig, ReplayResult};
pub use stats::{BacktestStats, TradeResult};
pub use stress_test::{run_stress_test, StressConfig, StressResult};
pub use walk_forward::{run_walk_forward, FoldResult, WalkForwardConfig, WalkForwardResult};
