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
            FeedType::Oanda => Err(FeedError::NotImplemented(
                "OANDA feed not yet implemented".into(),
            )),
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

        match self.current_bars.get_mut(&tick.symbol) {
            Some(current) if current.timestamp == bar_ts => {
                if tick.bid > current.high {
                    current.high = tick.bid;
                }
                if tick.bid < current.low {
                    current.low = tick.bid;
                }
                current.close = tick.bid;
                current.volume += tick.volume;
                None
            }
            Some(current) => {
                let completed = current.clone();
                *current = Bar {
                    timestamp: bar_ts,
                    open: tick.bid,
                    high: tick.bid,
                    low: tick.bid,
                    close: tick.bid,
                    volume: tick.volume,
                    timeframe: self.timeframe,
                };
                Some(completed)
            }
            None => {
                self.current_bars.insert(
                    tick.symbol.clone(),
                    Bar {
                        timestamp: bar_ts,
                        open: tick.bid,
                        high: tick.bid,
                        low: tick.bid,
                        close: tick.bid,
                        volume: tick.volume,
                        timeframe: self.timeframe,
                    },
                );
                None
            }
        }
    }

    /// Get the current incomplete bar for a symbol.
    pub fn current_bar(&self, symbol: &str) -> Option<&Bar> {
        self.current_bars.get(symbol)
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

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn tick(ts: i64, bid: rust_decimal::Decimal) -> Tick {
        Tick {
            symbol: "EURUSD".to_string(),
            bid,
            ask: bid + dec!(0.0001),
            volume: 1,
            timestamp: ts,
        }
    }

    #[test]
    fn emits_completed_bar_when_bucket_rolls() {
        let mut streamer = BarStreamer::new(gadarah_core::Timeframe::M1);

        assert!(streamer.process_tick(&tick(60, dec!(1.1000))).is_none());
        assert!(streamer.process_tick(&tick(90, dec!(1.1005))).is_none());

        let completed = streamer
            .process_tick(&tick(120, dec!(1.1010)))
            .expect("expected previous bar to close on new bucket");

        assert_eq!(completed.timestamp, 60);
        assert_eq!(completed.open, dec!(1.1000));
        assert_eq!(completed.close, dec!(1.1005));
        assert_eq!(completed.high, dec!(1.1005));
        assert_eq!(completed.low, dec!(1.1000));
        assert_eq!(completed.volume, 2);
    }

    #[test]
    fn current_bar_tracks_by_symbol() {
        let mut streamer = BarStreamer::new(gadarah_core::Timeframe::M5);
        streamer.process_tick(&tick(300, dec!(1.2000)));

        let current = streamer.current_bar("EURUSD").expect("missing current bar");
        assert_eq!(current.timestamp, 300);
        assert_eq!(current.open, dec!(1.2000));
    }
}
