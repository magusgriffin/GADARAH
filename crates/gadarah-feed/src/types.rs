use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Configuration for a feed stream.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamConfig {
    pub feed_type: String,
    pub symbols: Vec<String>,
    pub timeframe: String,
    pub endpoint: Option<String>,
    pub api_key: Option<String>,
    pub api_secret: Option<String>,
}

impl Default for StreamConfig {
    fn default() -> Self {
        Self {
            feed_type: "binance".to_string(),
            symbols: vec!["EURUSD".to_string()],
            timeframe: "M1".to_string(),
            endpoint: None,
            api_key: None,
            api_secret: None,
        }
    }
}

/// Incoming tick data from a feed.
#[derive(Debug, Clone)]
pub struct Tick {
    pub symbol: String,
    pub bid: Decimal,
    pub ask: Decimal,
    pub volume: u64,
    pub timestamp: i64,
}

/// Messages that can be received from a feed.
#[derive(Debug, Clone)]
pub enum FeedMessage {
    /// A new tick was received.
    Tick(Tick),
    /// A bar was completed.
    Bar(gadarah_core::Bar),
    /// Connection status update.
    Connected,
    Disconnected,
    /// Error occurred.
    Error(String),
    /// Heartbeat/keep-alive.
    Heartbeat,
}

impl FeedMessage {
    pub fn is_tick(&self) -> bool {
        matches!(self, FeedMessage::Tick(_))
    }
    
    pub fn is_bar(&self) -> bool {
        matches!(self, FeedMessage::Bar(_))
    }
}