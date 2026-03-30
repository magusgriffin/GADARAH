pub mod binance;
pub mod error;
pub mod types;

pub use error::FeedError;
pub use types::{FeedMessage, StreamConfig, Tick};

use gadarah_core::Bar;
use tokio::sync::mpsc;

/// Core trait for WebSocket data feeds.
pub trait Feed: Send + Sync {
    /// Start the feed and stream messages (async).
    fn subscribe(&self) -> mpsc::Receiver<FeedMessage>;
    
    /// Get the feed name.
    fn name(&self) -> &str;
}

/// Builder for creating feed connections.
pub struct FeedBuilder {
    feed_type: FeedType,
    symbols: Vec<String>,
    timeframe: gadarah_core::Timeframe,
}

#[derive(Debug, Clone, Copy)]
pub enum FeedType {
    Binance,
    Oanda,
}

impl FeedBuilder {
    pub fn new(feed_type: FeedType) -> Self {
        Self {
            feed_type,
            symbols: Vec::new(),
            timeframe: gadarah_core::Timeframe::M1,
        }
    }

    pub fn symbols(mut self, symbols: Vec<String>) -> Self {
        self.symbols = symbols;
        self
    }

    pub fn timeframe(mut self, tf: gadarah_core::Timeframe) -> Self {
        self.timeframe = tf;
        self
    }

    pub fn build(self) -> Result<Box<dyn Feed>, FeedError> {
        match self.feed_type {
            FeedType::Binance => Ok(Box::new(binance::BinanceFeed::new(
                self.symbols,
                self.timeframe,
            )?)),
            FeedType::Oanda => Err(FeedError::NotImplemented("OANDA feed not yet implemented".into())),
        }
    }
}

/// Stream aggregator that builds bars from tick data.
pub struct BarStreamer {
    current_bars: std::collections::HashMap<String, Bar>,
    timeframe: gadarah_core::Timeframe,
}

impl BarStreamer {
    pub fn new(timeframe: gadarah_core::Timeframe) -> Self {
        Self {
            current_bars: std::collections::HashMap::new(),
            timeframe,
        }
    }

    /// Process a tick and return a completed bar if the period closed.
    pub fn process_tick(&mut self, tick: &Tick) -> Option<Bar> {
        let bar_ts = align_to_timeframe(tick.timestamp, self.timeframe);
        
        let key = format!("{}:{}", tick.symbol, bar_ts);
        
        let bar = self.current_bars.entry(key.clone()).or_insert_with(|| Bar {
            timestamp: bar_ts,
            open: tick.bid,
            high: tick.bid,
            low: tick.bid,
            close: tick.bid,
            volume: 0,
            timeframe: self.timeframe,
        });

        // Update OHLC
        if tick.bid > bar.high { bar.high = tick.bid; }
        if tick.bid < bar.low { bar.low = tick.bid; }
        bar.close = tick.bid;
        bar.volume += tick.volume;

        // Check if we crossed to a new period
        if tick.timestamp >= bar_ts + duration_secs(self.timeframe) as i64 {
            let completed = self.current_bars.remove(&key);
            completed
        } else {
            None
        }
    }

    /// Get the current incomplete bar for a symbol.
    pub fn current_bar(&self, symbol: &str) -> Option<&Bar> {
        let now = chrono::Utc::now().timestamp();
        let bar_ts = align_to_timeframe(now, self.timeframe);
        let key = format!("{}:{}", symbol, bar_ts);
        self.current_bars.get(&key)
    }
}

fn align_to_timeframe(ts: i64, tf: gadarah_core::Timeframe) -> i64 {
    let secs = duration_secs(tf) as i64;
    ts.div_euclid(secs) * secs
}

fn duration_secs(tf: gadarah_core::Timeframe) -> u32 {
    match tf {
        gadarah_core::Timeframe::M1 => 60,
        gadarah_core::Timeframe::M5 => 300,
        gadarah_core::Timeframe::M15 => 900,
        gadarah_core::Timeframe::H1 => 3600,
        gadarah_core::Timeframe::H4 => 14400,
        gadarah_core::Timeframe::D1 => 86400,
    }
}