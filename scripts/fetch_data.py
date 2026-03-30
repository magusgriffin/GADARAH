#!/usr/bin/env python3
"""
GADARAH Data Fetcher — download max historical forex data from Yahoo Finance.

Downloads:
  - H1: ~2 years (730 days) — for regime analysis and long-term testing
  - M15: ~60 days (yfinance intraday limit) — primary execution timeframe
  - M5: ~60 days — for aggregation and granular analysis
  - D1: ~3 years — for context

Outputs CSV files in the format: timestamp,open,high,low,close,volume
Compatible with `gadarah bulk-import`.
"""

import logging
import os
import sys
import time
from datetime import datetime, timedelta, timezone
from pathlib import Path

import pandas as pd
import yfinance as yf

logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s %(levelname)s %(message)s",
)
log = logging.getLogger("fetch")

# Symbols to fetch — forex majors we trade
SYMBOLS = {
    "GBPUSD": "GBPUSD=X",
    "EURUSD": "EURUSD=X",
    "USDJPY": "USDJPY=X",
    "AUDUSD": "AUDUSD=X",
    "NZDUSD": "NZDUSD=X",
    "USDCAD": "USDCAD=X",
    "USDCHF": "USDCHF=X",
    "EURGBP": "EURGBP=X",
    "EURJPY": "EURJPY=X",
    "GBPJPY": "GBPJPY=X",
}

# Timeframes and their yfinance config
TIMEFRAMES = {
    "M5":  {"interval": "5m",  "max_days": 60,  "chunk_days": 55},
    "M15": {"interval": "15m", "max_days": 60,  "chunk_days": 55},
    "H1":  {"interval": "1h",  "max_days": 730, "chunk_days": 700},
    "H4":  {"interval": "4h",  "max_days": 730, "chunk_days": 700},
    "D1":  {"interval": "1d",  "max_days": 3650, "chunk_days": 3650},
}

OUT_DIR = Path("data/fetched")


def download_chunk(ticker: str, interval: str, start: str, end: str) -> pd.DataFrame:
    """Download a single chunk from yfinance."""
    df = yf.download(ticker, start=start, end=end, interval=interval,
                     auto_adjust=True, progress=False)
    if df is None or df.empty:
        return pd.DataFrame()
    if isinstance(df.columns, pd.MultiIndex):
        df.columns = df.columns.get_level_values(0)
    return df


def download_with_chunks(ticker: str, interval: str, max_days: int,
                         chunk_days: int) -> pd.DataFrame:
    """Download data, chunking if needed to respect yfinance limits."""
    end_dt = datetime.now(tz=timezone.utc)
    start_dt = end_dt - timedelta(days=max_days)

    if max_days <= chunk_days + 5:
        return download_chunk(ticker, interval,
                              start_dt.strftime("%Y-%m-%d"),
                              end_dt.strftime("%Y-%m-%d"))

    frames = []
    cursor = start_dt
    while cursor < end_dt:
        chunk_end = min(cursor + timedelta(days=chunk_days), end_dt)
        chunk_df = download_chunk(ticker, interval,
                                  cursor.strftime("%Y-%m-%d"),
                                  chunk_end.strftime("%Y-%m-%d"))
        if not chunk_df.empty:
            frames.append(chunk_df)
        cursor = chunk_end
        if cursor < end_dt:
            time.sleep(0.3)

    if not frames:
        return pd.DataFrame()

    combined = pd.concat(frames)
    combined = combined[~combined.index.duplicated(keep="first")]
    combined.sort_index(inplace=True)
    return combined


def normalize(df: pd.DataFrame) -> pd.DataFrame:
    """Normalize to GADARAH CSV format: timestamp,open,high,low,close,volume"""
    if df.empty:
        return pd.DataFrame(columns=["timestamp", "open", "high", "low", "close", "volume"])

    col_map = {}
    for col in df.columns:
        lower = col.lower()
        if lower in ("open", "high", "low", "close", "volume"):
            col_map[col] = lower
    df = df.rename(columns=col_map)

    for req in ("open", "high", "low", "close"):
        if req not in df.columns:
            raise ValueError(f"Missing column: {req}")
    if "volume" not in df.columns:
        df["volume"] = 0

    idx = df.index
    if hasattr(idx.dtype, "tz") and idx.dtype.tz is not None:
        idx = idx.tz_convert("UTC")
    timestamps = idx.map(lambda t: int(t.timestamp())).astype("int64")

    result = pd.DataFrame({
        "timestamp": timestamps,
        "open": df["open"].astype("float64"),
        "high": df["high"].astype("float64"),
        "low": df["low"].astype("float64"),
        "close": df["close"].astype("float64"),
        "volume": df["volume"].astype("int64"),
    })
    result.reset_index(drop=True, inplace=True)
    result.dropna(subset=["open", "high", "low", "close"], inplace=True)
    result.reset_index(drop=True, inplace=True)
    return result


def fetch_all():
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    total_bars = 0
    total_files = 0

    for symbol, ticker in SYMBOLS.items():
        for tf_name, tf_cfg in TIMEFRAMES.items():
            fname = f"{symbol}_{tf_name}.csv"
            fpath = OUT_DIR / fname

            log.info("Fetching %s %s (%s)...", symbol, tf_name, ticker)
            try:
                raw = download_with_chunks(
                    ticker, tf_cfg["interval"],
                    tf_cfg["max_days"], tf_cfg["chunk_days"]
                )
                if raw.empty:
                    log.warning("  No data returned for %s %s", symbol, tf_name)
                    continue

                df = normalize(raw)
                if df.empty:
                    log.warning("  Normalized to 0 rows for %s %s", symbol, tf_name)
                    continue

                df.to_csv(str(fpath), index=False)
                total_bars += len(df)
                total_files += 1

                ts_start = datetime.fromtimestamp(int(df["timestamp"].iloc[0]),
                                                   tz=timezone.utc)
                ts_end = datetime.fromtimestamp(int(df["timestamp"].iloc[-1]),
                                                 tz=timezone.utc)
                log.info("  %s %s: %d bars (%s to %s)",
                         symbol, tf_name, len(df),
                         ts_start.strftime("%Y-%m-%d"),
                         ts_end.strftime("%Y-%m-%d"))

            except Exception as e:
                log.error("  Error fetching %s %s: %s", symbol, tf_name, e)

            time.sleep(0.5)  # rate limit

    log.info("\nDone! %d files, %d total bars in %s", total_files, total_bars, OUT_DIR)


if __name__ == "__main__":
    fetch_all()
