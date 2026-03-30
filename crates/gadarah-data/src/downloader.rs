//! Historical Data Downloader
//!
//! Downloads historical OHLCV data from various sources.

use crate::error::DataError;
use crate::store::insert_bars;
use gadarah_core::{Bar, Timeframe};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use tracing::info;

/// Data source for historical downloads
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DataSource {
    /// Binance public API
    Binance,
    /// MetaTrader 5 terminal export
    Mt5,
    /// CSV file import
    Csv(PathBuf),
}

/// Download configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadConfig {
    pub source: DataSource,
    pub symbols: Vec<String>,
    pub timeframes: Vec<String>,
    pub start_date: Option<DateTime<Utc>>,
    pub end_date: Option<DateTime<Utc>>,
    pub output_dir: PathBuf,
}

impl Default for DownloadConfig {
    fn default() -> Self {
        Self {
            source: DataSource::Binance,
            symbols: vec!["EURUSD".to_string()],
            timeframes: vec!["M15".to_string()],
            start_date: None,
            end_date: None,
            output_dir: PathBuf::from("data/candles"),
        }
    }
}

/// Downloader for historical data
pub struct DataDownloader {
    config: DownloadConfig,
}

impl DataDownloader {
    pub fn new(config: DownloadConfig) -> Self {
        Self { config }
    }

    /// Download historical klines from Binance (sync version using blocking reqwest)
    pub fn download_binance_klines(
        &self,
        symbol: &str,
        interval: &str,
        start_time: i64,
        end_time: i64,
    ) -> Result<Vec<Bar>, DataError> {
        let mut all_bars = Vec::new();
        let mut current_start = start_time;
        let limit = 1000; // Binance API limit
        let client = reqwest::blocking::Client::new();

        while current_start < end_time {
            let url = format!(
                "https://api.binance.com/api/v3/klines?symbol={}&interval={}&startTime={}&endTime={}&limit={}",
                symbol.to_uppercase(),
                interval,
                current_start,
                end_time,
                limit
            );

            let response = client.get(&url)
                .send()
                .map_err(|e| DataError::Download(format!("Request failed: {}", e)))?;

            let data: Vec<Vec<serde_json::Value>> = response
                .json()
                .map_err(|e| DataError::Download(format!("Parse failed: {}", e)))?;

            if data.is_empty() {
                break;
            }

            // Get last timestamp before consuming data
            let last_timestamp = data.last()
                .and_then(|k| k[0].as_i64())
                .unwrap_or(current_start);

            for kline in data {
                let bar = parse_binance_kline(&kline, symbol, interval)?;
                all_bars.push(bar);
            }

            // Move start time forward
            current_start = last_timestamp + 1;

            // Rate limit
            std::thread::sleep(std::time::Duration::from_millis(100));
        }

        info!("Downloaded {} bars for {}", all_bars.len(), symbol);
        Ok(all_bars)
    }

    /// Download all configured symbols/timeframes
    pub fn download_all(&self) -> HashMap<String, Vec<Bar>> {
        let mut results: HashMap<String, Vec<Bar>> = HashMap::new();

        for symbol in &self.config.symbols {
            for tf in &self.config.timeframes {
                let interval = tf_to_binance_interval(tf);
                let start = self.config.start_date
                    .map(|d| d.timestamp_millis())
                    .unwrap_or(0);
                let end = self.config.end_date
                    .map(|d| d.timestamp_millis())
                    .unwrap_or(Utc::now().timestamp_millis());

                match self.download_binance_klines(symbol, &interval, start, end) {
                    Ok(bars) => {
                        let key = format!("{}_{}", symbol, tf);
                        results.insert(key, bars);
                    }
                    Err(e) => {
                        tracing::warn!("Failed to download {} {}: {}", symbol, tf, e);
                    }
                }
            }
        }

        results
    }

    /// Save bars to SQLite database
    pub fn save_to_db(&self, db_path: &PathBuf, bars: &[(String, Vec<Bar>)]) -> Result<(), DataError> {
        use rusqlite::Connection;

        let mut conn = Connection::open(db_path)
            .map_err(|e| DataError::Database(e.to_string()))?;

        for (symbol, bar_list) in bars {
            insert_bars(&mut conn, symbol, bar_list)?;
        }

        info!("Saved {} symbols to database", bars.len());
        Ok(())
    }
}

/// Convert timeframe string to Binance interval
fn tf_to_binance_interval(tf: &str) -> &str {
    match tf {
        "M1" => "1m",
        "M5" => "5m",
        "M15" => "15m",
        "H1" => "1h",
        "H4" => "4h",
        "D1" => "1d",
        _ => "15m",
    }
}

/// Parse a Binance kline array into our Bar type
fn parse_binance_kline(data: &[serde_json::Value], symbol: &str, interval: &str) -> Result<Bar, DataError> {
    let open_time = data[0].as_i64().ok_or_else(|| DataError::Parse("Invalid open time".into()))?;
    let open = parse_decimal(&data[1])?;
    let high = parse_decimal(&data[2])?;
    let low = parse_decimal(&data[3])?;
    let close = parse_decimal(&data[4])?;
    let volume = data[5].as_f64().unwrap_or(0.0) as u64;

    let timeframe = match interval {
        "1m" => Timeframe::M1,
        "5m" => Timeframe::M5,
        "15m" => Timeframe::M15,
        "1h" => Timeframe::H1,
        "4h" => Timeframe::H4,
        "1d" => Timeframe::D1,
        _ => Timeframe::M15,
    };

    Ok(Bar {
        open,
        high,
        low,
        close,
        volume,
        timestamp: open_time / 1000,
        timeframe,
    })
}

/// Parse a decimal from JSON value
fn parse_decimal(val: &serde_json::Value) -> Result<Decimal, DataError> {
    let s = val.as_str().ok_or_else(|| DataError::Parse("Not a string".into()))?;
    s.parse::<Decimal>()
        .map_err(|e| DataError::Parse(format!("Decimal parse error: {}", e)))
}

/// Quick download function for common symbols
pub fn quick_download(
    symbols: Vec<String>,
    start_date: DateTime<Utc>,
    end_date: DateTime<Utc>,
) -> HashMap<String, Vec<Bar>> {
    let config = DownloadConfig {
        source: DataSource::Binance,
        symbols,
        timeframes: vec!["M15".to_string()],
        start_date: Some(start_date),
        end_date: Some(end_date),
        output_dir: PathBuf::from("data/candles"),
    };

    let downloader = DataDownloader::new(config);
    downloader.download_all()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tf_conversion() {
        assert_eq!(tf_to_binance_interval("M15"), "15m");
        assert_eq!(tf_to_binance_interval("H1"), "1h");
        assert_eq!(tf_to_binance_interval("D1"), "1d");
    }
}