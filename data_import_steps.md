# GADARAH Data Import Guide

## Overview
This document outlines the complete process of downloading and importing forex trading data into the GADARAH repository for backtesting and analysis.

## Prerequisites
- Rust toolchain installed
- Python 3.x with required packages
- SQLite database support

## Step 1: Download Data Using Python Script

### Script Location
`scripts/fetch_data.py`

### Configuration
The script is pre-configured to download data for 8 major forex pairs across multiple timeframes:

**Symbols:**
- EURUSD, GBPUSD, USDJPY, AUDUSD, NZDUSD, USDCAD, USDCHF, EURGBP

**Timeframes:**
- M5: ~60 days (intraday limit)
- M15: ~60 days (intraday limit) 
- H1: ~730 days (2 years)
- H4: ~730 days (2 years)
- D1: ~3650 days (10 years)

### Execution
```bash
python3 scripts/fetch_data.py
```

### Output
- CSV files saved to `data/fetched/`
- Format: `timestamp,open,high,low,close,volume`
- Total: 30 files, 32,543 bars

## Step 2: Import Data into GADARAH Database

### Command
```bash
cargo run -p gadarah-cli -- bulk-import data/fetched --db data/gadarah.db
```

### Results
- **Database**: `data/gadarah.db` (32MB)
- **Total bars imported**: 70,247 bars from 40 files
- **Coverage**:
  - Daily data (D1): 2600-2601 bars per pair
  - H1 data: 519-525 bars per pair
  - H4 data: 132 bars per pair
  - M15 data: 3752-4907 bars per pair

## Step 3: Verify Data Import

### Dataset Report
```bash
cargo run -p gadarah-cli -- dataset-report --db data/gadarah.db
```

### Backtest Verification
```bash
cargo run -p gadarah-cli -- backtest --db data/gadarah.db --symbol EURUSD
```

## Data Quality Assessment

### Strengths
- Comprehensive daily data (10 years)
- Substantial H1 data (2 years)
- Multiple currency pairs covered
- Proper CSV format for GADARAH

### Limitations
- M15 data has short history (57-73 days vs 730 day requirement)
- Many M15 series have zero volume
- Some data gaps and alignment issues

## Usage Recommendations

### For Backtesting
- Use H1 and D1 data for longer-term strategies
- Use M15 data for shorter-term testing and development
- Consider data quality limitations for M15 strategies

### For Analysis
- Daily data suitable for trend analysis
- H1 data good for intraday pattern recognition
- Multiple timeframes enable multi-timeframe analysis

## Troubleshooting

### Common Issues
1. **Yahoo Finance API limits**: M5 and M15 data limited to 60 days
2. **Zero volume data**: Common in forex data from Yahoo Finance
3. **Short history**: Some timeframes may not meet 730-day requirement

### Solutions
- Use H1/D1 data for longer-term analysis
- Consider alternative data sources for M15 data
- Implement data validation and cleaning

## File Structure
```
data/
├── fetched/          # Downloaded CSV files
├── gadarah.db       # SQLite database with imported data
└── candles/         # Optional: Binance data (if using Rust downloader)
```

## Performance Metrics
- **Import time**: ~1.64 seconds for 70,247 bars
- **Database size**: 32MB
- **Processing speed**: ~42,000 bars/second

## Next Steps
1. Use imported data for backtesting strategies
2. Analyze regime distributions and session statistics
3. Implement risk management testing
4. Validate strategy performance across different timeframes

## Maintenance
- Regularly update data using the Python script
- Monitor data quality and completeness
- Consider adding more currency pairs or timeframes as needed