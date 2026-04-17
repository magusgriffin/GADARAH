pub mod aggregator;
pub mod audit;
pub mod csv_import;
pub mod dataset;
pub mod dataset_pipeline;
pub mod db;
pub mod downloader;
pub mod dukascopy;
pub mod error;
pub mod gap_filler;
pub mod schema;
pub mod store;
pub mod trades;
pub mod volume_processor;

pub use aggregator::{aggregate_bars, MultiTfAggregator, MultiTfOutput, StreamAggregator};
pub use audit::{audit_bars, DataAuditResult};
pub use csv_import::{import_csv, CsvFormat};
pub use dataset::{
    build_dataset_readiness_report, derive_timeframes_for_symbol, detect_csv_format,
    discover_dataset_files, import_dataset_dir, DatasetFileSpec, DatasetImportOptions,
    DatasetImportResult, DatasetReadinessReport, DatasetRequirements, DatasetSeriesReport,
    DerivedSeriesResult, FileImportResult,
};
pub use dataset_pipeline::{run_pipeline, PipelineConfig, PipelineReport, SeriesPipelineResult};
pub use db::Database;
pub use downloader::{quick_download, DataDownloader, DataSource, DownloadConfig};
pub use dukascopy::{point_factor, stream_and_insert, FetchConfig, FetchReport};
pub use error::DataError;
pub use gap_filler::{detect_gaps, fill_gaps, GapFillReport, GapRange};
pub use store::{
    bar_time_range, count_bars, delete_bars, insert_bar, insert_bars, list_symbols,
    list_timeframes, load_all_bars, load_bars, str_to_tf,
};
pub use trades::{
    close_trade, insert_equity_snapshot, insert_trade, load_closed_trades, load_equity_snapshots,
    load_trades, load_unclosed_trade_count, load_unclosed_trades, EquitySnapshot, TradeClose,
    TradeRecord,
};
pub use volume_processor::{process_volumes, VolumeProcessStats};
