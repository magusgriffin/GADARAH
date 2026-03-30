use thiserror::Error;

#[derive(Debug, Error)]
pub enum BacktestError {
    #[error("No bars provided for backtest")]
    NoBars,

    #[error("Insufficient bars: need {needed} warmup bars, got {got}")]
    InsufficientBars { needed: usize, got: usize },

    #[error("Broker error: {0}")]
    Broker(#[from] gadarah_broker::BrokerError),

    #[error("Data error: {0}")]
    Data(#[from] gadarah_data::DataError),

    #[error("Risk error: {0}")]
    Risk(#[from] gadarah_risk::RiskError),

    #[error("Walk-forward: {0}")]
    WalkForward(String),
}
