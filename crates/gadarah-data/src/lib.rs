pub mod aggregator;
pub mod csv_import;
pub mod db;
pub mod downloader;
pub mod error;
pub mod schema;
pub mod store;
pub mod trades;

pub use aggregator::{aggregate_bars, MultiTfAggregator, MultiTfOutput, StreamAggregator};
pub use csv_import::{import_csv, CsvFormat};
pub use db::Database;
pub use downloader::{DataDownloader, DataSource, DownloadConfig, quick_download};
pub use error::DataError;
pub use store::{
    bar_time_range, count_bars, delete_bars, insert_bar, insert_bars, list_symbols,
    list_timeframes, load_all_bars, load_bars, str_to_tf,
};
pub use trades::{
    close_trade, insert_equity_snapshot, insert_trade, load_closed_trades, load_equity_snapshots,
    load_trades, EquitySnapshot, TradeClose, TradeRecord,
};
