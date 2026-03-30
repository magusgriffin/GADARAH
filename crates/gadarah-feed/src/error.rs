use thiserror::Error;

#[derive(Error, Debug)]
pub enum FeedError {
    #[error("WebSocket connection failed: {0}")]
    ConnectionFailed(String),

    #[error("Failed to parse message: {0}")]
    ParseError(String),

    #[error("Authentication failed: {0}")]
    AuthFailed(String),

    #[error("Symbol not found: {0}")]
    SymbolNotFound(String),

    #[error("Rate limited: {0}")]
    RateLimited(String),

    #[error("Not implemented: {0}")]
    NotImplemented(String),

    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
}