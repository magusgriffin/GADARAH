use rust_decimal::Decimal;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum BrokerError {
    #[error("Connection error: {0}")]
    Connection(String),

    #[error("Authentication failed: {0}")]
    AuthFailed(String),

    #[error("Order rejected: {0}")]
    OrderRejected(String),

    #[error("Position not found: {id}")]
    PositionNotFound { id: u64 },

    #[error("Invalid close volume: requested {requested}, available {available}")]
    InvalidCloseVolume {
        requested: Decimal,
        available: Decimal,
    },

    #[error("Symbol not found: {0}")]
    SymbolNotFound(String),

    #[error("Timeout waiting for {operation}")]
    Timeout { operation: String },

    #[error("Protocol error: {0}")]
    Protocol(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}
