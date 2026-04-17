//! Dukascopy tick-data streaming fetcher.
//!
//! Downloads bi5 (LZMA-compressed tick) files one hour at a time from
//! Dukascopy's public data feed, decompresses and parses them entirely in
//! memory, aggregates into OHLCV bars, and inserts directly into the SQLite
//! database.  Nothing is ever written to disk.
//!
//! URL format:
//!   https://datafeed.dukascopy.com/datafeed/{SYMBOL}/{YEAR}/{MONTH}/{DAY}/{HOUR}h_ticks.bi5
//!   where MONTH is 0-indexed (Jan=00), DAY is 1-indexed.
//!
//! Bi5 tick record (20 bytes, big-endian):
//!   u32  ms since start of hour
//!   u32  ask × point_factor
//!   u32  bid × point_factor
//!   f32  ask volume (lots)
//!   f32  bid volume (lots)

use std::io::Cursor;
use std::time::Duration;

use chrono::{Datelike, NaiveDate};
use rusqlite::Connection;
use rust_decimal::Decimal;
use tracing::{debug, info, warn};

use gadarah_core::{Bar, Timeframe};

use crate::aggregator::aggregate_bars;
use crate::error::DataError;
use crate::store::insert_bars;

const TICK_BYTES: usize = 20;
const BASE_URL: &str = "https://datafeed.dukascopy.com/datafeed";

/// Price divisor for a given symbol.
/// JPY and some other crosses have 3 decimal places; everything else has 5.
pub fn point_factor(symbol: &str) -> u32 {
    let s = symbol.to_uppercase();
    if s.contains("JPY") || s.contains("HUF") || s.contains("KRW") || s.contains("IDR") {
        1_000
    } else {
        100_000
    }
}

/// Build the Dukascopy bi5 URL for a specific hour.
fn bi5_url(symbol: &str, date: NaiveDate, hour: u8) -> String {
    format!(
        "{}/{}/{}/{:02}/{:02}/{:02}h_ticks.bi5",
        BASE_URL,
        symbol.to_uppercase(),
        date.year(),
        date.month0(), // 0-indexed months
        date.day(),    // 1-indexed days
        hour,
    )
}

/// Parse a decompressed bi5 payload into `(timestamp_ms, mid_price, volume)` tuples.
fn parse_bi5(data: &[u8], hour_start_ms: i64, pf: u32) -> Vec<(i64, Decimal, u64)> {
    let pf_dec = Decimal::from(pf);
    let mut ticks = Vec::with_capacity(data.len() / TICK_BYTES);
    let mut i = 0;
    while i + TICK_BYTES <= data.len() {
        let ms_in_hour = u32::from_be_bytes(data[i..i + 4].try_into().unwrap());
        let ask_raw = u32::from_be_bytes(data[i + 4..i + 8].try_into().unwrap());
        let bid_raw = u32::from_be_bytes(data[i + 8..i + 12].try_into().unwrap());
        let ask_vol = f32::from_be_bytes(data[i + 12..i + 16].try_into().unwrap());
        let bid_vol = f32::from_be_bytes(data[i + 16..i + 20].try_into().unwrap());

        let ts_ms = hour_start_ms + ms_in_hour as i64;
        let mid = (Decimal::from(ask_raw) + Decimal::from(bid_raw)) / (Decimal::from(2) * pf_dec);
        let vol = ((ask_vol + bid_vol) * 1_000.0) as u64; // convert lots to micro-lots for integer storage

        ticks.push((ts_ms, mid, vol));
        i += TICK_BYTES;
    }
    ticks
}

/// Aggregate `(ts_ms, mid, volume)` ticks into M1 OHLCV bars.
fn ticks_to_m1(symbol: &str, ticks: &[(i64, Decimal, u64)]) -> Vec<Bar> {
    let mut bars: Vec<Bar> = Vec::new();
    for &(ts_ms, mid, vol) in ticks {
        let ts_sec = ts_ms / 1_000;
        let minute_ts = ts_sec - (ts_sec % 60); // floor to minute boundary

        match bars.last_mut() {
            Some(b) if b.timestamp == minute_ts => {
                if mid > b.high {
                    b.high = mid;
                }
                if mid < b.low {
                    b.low = mid;
                }
                b.close = mid;
                b.volume += vol;
            }
            _ => bars.push(Bar {
                timestamp: minute_ts,
                open: mid,
                high: mid,
                low: mid,
                close: mid,
                volume: vol,
                timeframe: Timeframe::M1,
            }),
        }
        // Set symbol implicitly — Bar doesn't have a symbol field, stored per-key in DB
        let _ = symbol; // symbol used by caller for DB insertion
    }
    bars
}

/// Configuration for a streaming fetch run.
#[derive(Debug, Clone)]
pub struct FetchConfig {
    pub symbol: String,
    /// First date to fetch (inclusive).
    pub from: NaiveDate,
    /// Last date to fetch (inclusive).
    pub to: NaiveDate,
    /// Target timeframes to store (M1 is always computed as intermediate).
    pub timeframes: Vec<Timeframe>,
    /// HTTP request timeout.
    pub timeout_secs: u64,
    /// Delay between each hourly HTTP request to avoid rate-limiting (ms).
    pub request_delay_ms: u64,
}

impl FetchConfig {
    pub fn new(symbol: &str, from: NaiveDate, to: NaiveDate) -> Self {
        Self {
            symbol: symbol.to_string(),
            from,
            to,
            timeframes: vec![Timeframe::M15],
            timeout_secs: 30,
            request_delay_ms: 200,
        }
    }
}

/// Summary returned after a completed fetch run.
#[derive(Debug, Default)]
pub struct FetchReport {
    pub days_fetched: usize,
    pub hours_fetched: usize,
    pub ticks_parsed: usize,
    pub bars_inserted: usize,
}

/// Stream Dukascopy tick data for `config.symbol` between `config.from` and
/// `config.to` and insert OHLCV bars directly into `conn`.
///
/// Nothing is written to disk.  Each hour's compressed payload is held in
/// memory only while it is being processed.
pub fn stream_and_insert(
    conn: &mut Connection,
    config: &FetchConfig,
) -> Result<FetchReport, DataError> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(config.timeout_secs))
        .build()
        .map_err(|e| DataError::Download(e.to_string()))?;

    let pf = point_factor(&config.symbol);
    let mut report = FetchReport::default();
    let mut date = config.from;

    while date <= config.to {
        let mut day_m1: Vec<Bar> = Vec::new();

        for hour in 0u8..24 {
            if config.request_delay_ms > 0 {
                std::thread::sleep(Duration::from_millis(config.request_delay_ms));
            }
            let url = bi5_url(&config.symbol, date, hour);
            debug!("Fetching {url}");

            let resp = client
                .get(&url)
                .send()
                .map_err(|e| DataError::Download(e.to_string()))?;

            if resp.status() == 404 || resp.status() == 204 {
                continue; // no data for this hour (weekend, holiday, etc.)
            }
            if !resp.status().is_success() {
                warn!("HTTP {} for {url}", resp.status());
                continue;
            }

            let compressed = resp
                .bytes()
                .map_err(|e| DataError::Download(e.to_string()))?;

            if compressed.is_empty() {
                continue;
            }

            // Decompress LZMA1 in memory
            let mut decompressed: Vec<u8> = Vec::new();
            lzma_rs::lzma_decompress(&mut Cursor::new(&*compressed), &mut decompressed)
                .map_err(|e| DataError::Download(format!("LZMA decompress {url}: {e:?}")))?;

            if decompressed.is_empty() {
                continue;
            }

            // hour_start in ms since epoch
            let hour_start_ms = {
                use chrono::{TimeZone, Utc};
                Utc.with_ymd_and_hms(date.year(), date.month(), date.day(), hour as u32, 0, 0)
                    .single()
                    .map(|dt| dt.timestamp_millis())
                    .unwrap_or(0)
            };

            let ticks = parse_bi5(&decompressed, hour_start_ms, pf);
            report.ticks_parsed += ticks.len();

            let m1_bars = ticks_to_m1(&config.symbol, &ticks);
            day_m1.extend(m1_bars);
            report.hours_fetched += 1;
        }

        if !day_m1.is_empty() {
            // Insert requested timeframes
            for &tf in &config.timeframes {
                let bars = if tf == Timeframe::M1 {
                    day_m1.clone()
                } else {
                    match aggregate_bars(&day_m1, tf) {
                        Ok(b) => b,
                        Err(_) => continue,
                    }
                };
                let n = insert_bars(conn, &config.symbol, &bars)?;
                report.bars_inserted += n;
            }

            // Always store M1 if not already requested (needed for further aggregation)
            if !config.timeframes.contains(&Timeframe::M1) {
                insert_bars(conn, &config.symbol, &day_m1)?;
            }

            report.days_fetched += 1;
            info!("{} {} — {} M1 bars", config.symbol, date, day_m1.len());
        }

        date = match date.succ_opt() {
            Some(d) => d,
            None => break,
        };
    }

    Ok(report)
}
