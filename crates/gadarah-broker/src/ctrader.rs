//! cTrader OpenAPI TCP/TLS Client
//! 
//! Implements the cTrader Open API protocol over TCP with TLS encryption.
//! This enables direct communication with Spotware's trading infrastructure.

pub use crate::client::CtraderClient;
pub use crate::codec::CtraderCodec;
pub use crate::messages::{ApplicationAuthReq, AccountAuthReq, NewOrderReq, OrderResponse};