use crate::error::FeedError;
use crate::types::{FeedMessage, Tick};
use futures_util::StreamExt;
use gadarah_core::Timeframe;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

/// Binance WebSocket kline (candlestick) stream.
pub struct BinanceFeed {
    symbols: Vec<String>,
    _timeframe: Timeframe,
    endpoint: String,
}

impl crate::Feed for BinanceFeed {
    fn subscribe(&self) -> mpsc::Receiver<FeedMessage> {
        use tokio::sync::mpsc;

        // Create channel for communication
        let (tx, rx) = mpsc::channel(1000);

        // Clone data needed for the async task
        let url = self.endpoint.clone();
        let symbols = self.symbols.clone();

        // Spawn a background thread that keeps the runtime alive
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("Failed to create runtime");

            rt.block_on(async move {
                info!("Connecting to Binance: {}", url);

                let result = tokio_tungstenite::connect_async(&url).await;

                match result {
                    Ok((ws_stream, _)) => {
                        let _ = tx.send(FeedMessage::Connected).await;
                        info!("Connected to Binance WebSocket");

                        let (_, mut read) = ws_stream.split();

                        while let Some(msg) = read.next().await {
                            match msg {
                                Ok(data) => {
                                    let text = data.to_text().unwrap_or("");
                                    if let Err(e) = Self::process_message(text, &symbols, &tx).await
                                    {
                                        warn!("Message parse error: {}", e);
                                    }
                                }
                                Err(e) => {
                                    error!("WebSocket error: {}", e);
                                    let _ = tx.send(FeedMessage::Error(e.to_string())).await;
                                    break;
                                }
                            }
                        }

                        let _ = tx.send(FeedMessage::Disconnected).await;
                    }
                    Err(e) => {
                        error!("Failed to connect: {}", e);
                        let _ = tx
                            .send(FeedMessage::Error(format!("Connection failed: {}", e)))
                            .await;
                    }
                }
            });
        });

        rx
    }

    fn name(&self) -> &str {
        "Binance"
    }
}

impl BinanceFeed {
    pub fn new(symbols: Vec<String>, timeframe: Timeframe) -> Result<Self, FeedError> {
        let interval = match timeframe {
            Timeframe::M1 => "1m",
            Timeframe::M5 => "5m",
            Timeframe::M15 => "15m",
            Timeframe::H1 => "1h",
            Timeframe::H4 => "4h",
            Timeframe::D1 => "1d",
        };

        // Use combined stream for multiple symbols
        let streams: String = symbols
            .iter()
            .map(|s| format!("{}@kline_{}", s.to_lowercase(), interval))
            .collect::<Vec<_>>()
            .join("/");

        let endpoint = format!("wss://stream.binance.com:9443/stream?streams={}", streams);

        Ok(Self {
            symbols,
            _timeframe: timeframe,
            endpoint,
        })
    }

    /// Async subscribe - returns a receiver.
    pub async fn subscribe_async(&self) -> mpsc::Receiver<FeedMessage> {
        let (tx, rx) = mpsc::channel(1000);

        let url = self.endpoint.clone();
        let symbols = self.symbols.clone();

        tokio::spawn(async move {
            info!("Connecting to Binance: {}", url);

            let result = tokio_tungstenite::connect_async(&url).await;

            match result {
                Ok((ws_stream, _)) => {
                    let _ = tx.send(FeedMessage::Connected).await;
                    info!("Connected to Binance WebSocket");

                    let (_, mut read) = ws_stream.split();

                    while let Some(msg) = read.next().await {
                        match msg {
                            Ok(data) => {
                                let text = data.to_text().unwrap_or("");
                                if let Err(e) = Self::process_message(text, &symbols, &tx).await {
                                    warn!("Message parse error: {}", e);
                                }
                            }
                            Err(e) => {
                                error!("WebSocket error: {}", e);
                                let _ = tx.send(FeedMessage::Error(e.to_string())).await;
                                break;
                            }
                        }
                    }

                    let _ = tx.send(FeedMessage::Disconnected).await;
                }
                Err(e) => {
                    error!("Failed to connect: {}", e);
                    let _ = tx
                        .send(FeedMessage::Error(format!("Connection failed: {}", e)))
                        .await;
                }
            }
        });

        rx
    }

    async fn process_message(
        text: &str,
        _symbols: &[String],
        tx: &mpsc::Sender<FeedMessage>,
    ) -> Result<(), FeedError> {
        // Binance wrapped message format: {"stream":"...","data":{...}}
        #[derive(Deserialize)]
        struct BinanceWrapper {
            #[serde(rename = "stream")]
            _stream: Option<String>,
            data: Option<BinanceKline>,
        }

        // Try parsing as wrapped message first
        if let Ok(wrapper) = serde_json::from_str::<BinanceWrapper>(text) {
            if let Some(kline) = wrapper.data {
                return Self::handle_kline(kline, tx).await;
            }
        }

        // Try direct kline format
        if let Ok(kline) = serde_json::from_str::<BinanceKline>(text) {
            return Self::handle_kline(kline, tx).await;
        }

        Ok(())
    }

    async fn handle_kline(
        kline: BinanceKline,
        tx: &mpsc::Sender<FeedMessage>,
    ) -> Result<(), FeedError> {
        let k = &kline.k;

        // Only emit on bar close
        if !k.x {
            return Ok(());
        }

        let symbol = k.s.clone();
        let open = Decimal::from_str(&k.o).map_err(|e| FeedError::ParseError(e.to_string()))?;
        let high = Decimal::from_str(&k.h).map_err(|e| FeedError::ParseError(e.to_string()))?;
        let low = Decimal::from_str(&k.l).map_err(|e| FeedError::ParseError(e.to_string()))?;
        let close = Decimal::from_str(&k.c).map_err(|e| FeedError::ParseError(e.to_string()))?;
        let volume: u64 = k.v.parse().unwrap_or(0);
        let timestamp = k.t;

        // Create a tick from the kline data
        let tick = Tick {
            symbol: symbol.clone(),
            bid: close,
            ask: close, // Binance is mid-price
            volume,
            timestamp,
        };

        let _ = tx.send(FeedMessage::Tick(tick)).await;

        debug!(
            "Bar: {} {} {} O:{} H:{} L:{} C:{} V:{}",
            symbol, kline.e, timestamp, open, high, low, close, volume
        );

        Ok(())
    }
}

/// Binance kline/candlestick WebSocket message.
#[derive(Debug, Deserialize, Serialize)]
struct BinanceKline {
    #[serde(rename = "e")]
    e: String, // Event type
    #[serde(rename = "E")]
    event_time: i64,
    #[serde(rename = "s")]
    s: String, // Symbol
    #[serde(rename = "k")]
    k: BinanceKlineData,
}

#[derive(Debug, Deserialize, Serialize)]
struct BinanceKlineData {
    #[serde(rename = "t")]
    t: i64, // Kline start time
    #[serde(rename = "T")]
    close_time: i64,
    #[serde(rename = "s")]
    s: String, // Symbol
    #[serde(rename = "i")]
    i: String, // Interval
    #[serde(rename = "f")]
    f: i64, // First trade ID
    #[serde(rename = "L")]
    last_trade_id: i64,
    #[serde(rename = "o")]
    o: String, // Open price
    #[serde(rename = "c")]
    c: String, // Close price
    #[serde(rename = "h")]
    h: String, // High price
    #[serde(rename = "l")]
    l: String, // Low price
    #[serde(rename = "v")]
    v: String, // Base asset volume
    #[serde(rename = "n")]
    n: i64, // Number of trades
    #[serde(rename = "x")]
    x: bool, // Is this kline closed?
    #[serde(rename = "q")]
    q: String, // Quote asset volume
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_binance_feed_creation() {
        let feed = BinanceFeed::new(vec!["EURUSDT".to_string()], Timeframe::M1).unwrap();

        assert!(feed.endpoint.contains("binance.com"));
        assert!(feed.endpoint.contains("kline_1m"));
    }

    #[test]
    fn test_timeframe_mapping() {
        for (tf, expected) in [
            (Timeframe::M1, "1m"),
            (Timeframe::M5, "5m"),
            (Timeframe::M15, "15m"),
            (Timeframe::H1, "1h"),
            (Timeframe::H4, "4h"),
            (Timeframe::D1, "1d"),
        ] {
            let feed = BinanceFeed::new(vec!["EURUSDT".to_string()], tf).unwrap();
            assert!(feed.endpoint.contains(expected));
        }
    }
}
