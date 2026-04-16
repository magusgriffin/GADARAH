pub mod auth;
pub mod client;
pub mod codec;
pub mod ctrader;
pub mod error;
pub mod messages;
pub mod mock;
pub mod traits;
pub mod types;

// prost-generated cTrader OpenAPI types (compiled from proto/*.proto)
pub(crate) mod proto {
    #![allow(clippy::enum_variant_names)]
    include!(concat!(env!("OUT_DIR"), "/_.rs"));
}

pub use client::{CtraderClient, CtraderConfig};
pub use error::BrokerError;
pub use mock::{forex_symbol, MockBroker, MockConfig};
pub use traits::Broker;
pub use types::{
    BrokerAccountInfo, CloseReport, CloseRequest, FillReport, ModifyRequest, OrderRequest,
    OrderType, ReconcileResult, SymbolSpec, Tick,
};
