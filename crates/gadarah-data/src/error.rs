use thiserror::Error;

#[derive(Debug, Error)]
pub enum DataError {
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("Database error: {0}")]
    Database(String),

    #[error("Download error: {0}")]
    Download(String),

    #[error("Parse error: {0}")]
    Parse(String),

    #[error("No bars found for {symbol} {timeframe}")]
    NoBars { symbol: String, timeframe: String },

    #[error("CSV parse error at line {line}: {msg}")]
    CsvParse { line: usize, msg: String },

    #[error("Invalid decimal in {field}: {value}")]
    InvalidDecimal { field: &'static str, value: String },

    #[error("Invalid timeframe: {0}")]
    InvalidTimeframe(String),

    #[error("Bar timestamp {ts} is not aligned to {tf} boundary")]
    MisalignedTimestamp { ts: i64, tf: String },

    #[error("Aggregation requires at least one bar")]
    EmptyAggregation,
}
