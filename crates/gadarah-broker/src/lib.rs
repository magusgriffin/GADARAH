pub mod client;
pub mod codec;
pub mod ctrader;
pub mod error;
pub mod messages;
pub mod mock;
pub mod traits;
pub mod types;

pub use client::{create_ctrader_broker, CtraderClient, CtraderConfig};
pub use codec::CtraderCodec;
pub use ctrader::{ApplicationAuthReq, AccountAuthReq, NewOrderReq, OrderResponse};
pub use error::BrokerError;
pub use messages::*;
pub use mock::{forex_symbol, MockBroker, MockConfig};
pub use traits::Broker;
pub use types::{
    BrokerAccountInfo, CloseReport, CloseRequest, FillReport, ModifyRequest, OrderRequest,
    OrderType, SymbolSpec, Tick,
};
