//! cTrader OpenAPI Message Types
//! 
//! Simplified message types for cTrader communication.
//! These map to the ProtoOAPayloadType enum values from the OpenAPI protocol.

use bytes::Bytes;
use serde::{Deserialize, Serialize};

/// Payload type IDs from ProtoOAPayloadType enum
pub const PROTO_OA_APPLICATION_AUTH_REQ: u16 = 2100;
pub const PROTO_OA_APPLICATION_AUTH_RES: u16 = 2101;
pub const PROTO_OA_ACCOUNT_AUTH_REQ: u16 = 2102;
pub const PROTO_OA_ACCOUNT_AUTH_RES: u16 = 2103;
pub const PROTO_OA_VERSION_REQ: u16 = 2104;
pub const PROTO_OA_VERSION_RES: u16 = 2105;
pub const PROTO_OA_NEW_ORDER_REQ: u16 = 2106;
pub const PROTO_OA_CANCEL_ORDER_REQ: u16 = 2108;
pub const PROTO_OA_AMEND_ORDER_REQ: u16 = 2109;
pub const PROTO_OA_AMEND_POSITION_SLTP_REQ: u16 = 2110;
pub const PROTO_OA_CLOSE_POSITION_REQ: u16 = 2111;
pub const PROTO_OA_ASSET_LIST_REQ: u16 = 2112;
pub const PROTO_OA_ASSET_LIST_RES: u16 = 2113;
pub const PROTO_OA_SYMBOLS_LIST_REQ: u16 = 2114;
pub const PROTO_OA_SYMBOLS_LIST_RES: u16 = 2115;
pub const PROTO_OA_SYMBOL_BY_ID_REQ: u16 = 2116;
pub const PROTO_OA_SYMBOL_BY_ID_RES: u16 = 2117;
pub const PROTO_OA_TRADER_REQ: u16 = 2121;
pub const PROTO_OA_TRADER_RES: u16 = 2122;
pub const PROTO_OA_TRADER_UPDATE_EVENT: u16 = 2123;
pub const PROTO_OA_RECONCILE_REQ: u16 = 2124;
pub const PROTO_OA_RECONCILE_RES: u16 = 2125;
pub const PROTO_OA_EXECUTION_EVENT: u16 = 2126;
pub const PROTO_OA_SUBSCRIBE_SPOTS_REQ: u16 = 2127;
pub const PROTO_OA_SUBSCRIBE_SPOTS_RES: u16 = 2128;
pub const PROTO_OA_SPOT_EVENT: u16 = 2131;
pub const PROTO_OA_ERROR_RES: u16 = 2142;
pub const PROTO_OA_GET_TICKDATA_REQ: u16 = 2145;
pub const PROTO_OA_GET_TICKDATA_RES: u16 = 2146;
pub const PROTO_OA_GET_TRENDBARS_REQ: u16 = 2137;
pub const PROTO_OA_GET_TRENDBARS_RES: u16 = 2138;
pub const PROTO_OA_ORDER_LIST_REQ: u16 = 2175;
pub const PROTO_OA_ORDER_LIST_RES: u16 = 2176;

/// Application authentication request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplicationAuthReq {
    pub client_id: String,
    pub client_secret: String,
}

/// Account authentication request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountAuthReq {
    pub ctid_trader_account_id: i64,
    pub access_token: String,
}

/// New order request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewOrderReq {
    pub ctid_trader_account_id: i64,
    pub symbol_id: i64,
    pub order_type: OrderType,
    pub trade_side: TradeSide,
    pub volume: i64, // in 0.01 units (1000 = 10.00)
    pub limit_price: Option<f64>,
    pub stop_price: Option<f64>,
    pub stop_loss: Option<f64>,
    pub take_profit: Option<f64>,
    pub comment: Option<String>,
    pub label: Option<String>,
    pub position_id: Option<i64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OrderType {
    Market,
    Limit,
    Stop,
    MarketRange,
    StopLimit,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TradeSide {
    Buy,
    Sell,
}

/// Order response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderResponse {
    pub ctid_trader_account_id: i64,
    pub order_id: Option<i64>,
    pub position_id: Option<i64>,
    pub error_code: Option<String>,
    pub message: Option<String>,
}

/// Close position request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClosePositionReq {
    pub ctid_trader_account_id: i64,
    pub position_id: i64,
    pub volume: i64,
}

/// Get historical bars request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetTrendBarsReq {
    pub ctid_trader_account_id: i64,
    pub symbol_id: i64,
    pub timeframe: i32, // seconds
    pub from_timestamp: i64,
    pub to_timestamp: i64,
}

/// Spot (tick) data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpotEvent {
    pub symbol_id: i64,
    pub bid: f64,
    pub ask: f64,
    pub last: Option<f64>,
    pub volume: Option<i64>,
    pub timestamp: i64,
}

/// Execution event (order fill)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionEvent {
    pub ctid_trader_account_id: i64,
    pub order_id: i64,
    pub position_id: Option<i64>,
    pub symbol_id: i64,
    pub trade_side: TradeSide,
    pub volume: i64,
    pub price: f64,
    pub stop_loss: Option<f64>,
    pub take_profit: Option<f64>,
    pub comment: Option<String>,
    pub label: Option<String>,
    pub execution_type: ExecutionType,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionType {
    Fill,
    PartialFill,
    Cancelled,
    Rejected,
}

/// Trader info response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraderRes {
    pub ctid_trader_account_id: i64,
    pub balance: f64,
    pub equity: f64,
    pub margin: f64,
    pub free_margin: f64,
    pub margin_level: Option<f64>,
    pub unrealized_pnl: f64,
}

/// Error response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorRes {
    pub ctid_trader_account_id: Option<i64>,
    pub error_code: String,
    pub description: Option<String>,
}

/// Version request (no body needed)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VersionReq {}

/// Version response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionRes {
    pub version: String,
}

/// Reconcile request - gets current positions/orders
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconcileReq {
    pub ctid_trader_account_id: i64,
}

/// Reconcile response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconcileRes {
    pub ctid_trader_account_id: i64,
    pub positions: Vec<PositionInfo>,
    pub orders: Vec<OrderInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PositionInfo {
    pub position_id: i64,
    pub symbol_id: i64,
    pub trade_side: TradeSide,
    pub volume: i64,
    pub price: f64,
    pub stop_loss: Option<f64>,
    pub take_profit: Option<f64>,
    pub unrealized_pnl: f64,
    pub open_timestamp: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderInfo {
    pub order_id: i64,
    pub symbol_id: i64,
    pub order_type: OrderType,
    pub trade_side: TradeSide,
    pub volume: i64,
    pub price: Option<f64>,
    pub stop_loss: Option<f64>,
    pub take_profit: Option<f64>,
    pub status: OrderStatus,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OrderStatus {
    Pending,
    Filled,
    PartiallyFilled,
    Cancelled,
    Rejected,
}

/// Generic message envelope
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum CtraderMessage {
    ApplicationAuthReq(ApplicationAuthReq),
    ApplicationAuthRes,
    AccountAuthReq(AccountAuthReq),
    AccountAuthRes,
    VersionReq(VersionReq),
    VersionRes(VersionRes),
    NewOrderReq(NewOrderReq),
    OrderResponse(OrderResponse),
    ClosePositionReq(ClosePositionReq),
    SpotEvent(SpotEvent),
    ExecutionEvent(ExecutionEvent),
    TraderRes(TraderRes),
    ErrorRes(ErrorRes),
    ReconcileReq(ReconcileReq),
    ReconcileRes(ReconcileRes),
}

impl CtraderMessage {
    pub fn payload_type(&self) -> u16 {
        match self {
            CtraderMessage::ApplicationAuthReq(_) => PROTO_OA_APPLICATION_AUTH_REQ,
            CtraderMessage::ApplicationAuthRes => PROTO_OA_APPLICATION_AUTH_RES,
            CtraderMessage::AccountAuthReq(_) => PROTO_OA_ACCOUNT_AUTH_REQ,
            CtraderMessage::AccountAuthRes => PROTO_OA_ACCOUNT_AUTH_RES,
            CtraderMessage::VersionReq(_) => PROTO_OA_VERSION_REQ,
            CtraderMessage::VersionRes(_) => PROTO_OA_VERSION_RES,
            CtraderMessage::NewOrderReq(_) => PROTO_OA_NEW_ORDER_REQ,
            CtraderMessage::OrderResponse(_) => PROTO_OA_NEW_ORDER_REQ, // Response uses same ID
            CtraderMessage::ClosePositionReq(_) => PROTO_OA_CLOSE_POSITION_REQ,
            CtraderMessage::SpotEvent(_) => PROTO_OA_SPOT_EVENT,
            CtraderMessage::ExecutionEvent(_) => PROTO_OA_EXECUTION_EVENT,
            CtraderMessage::TraderRes(_) => PROTO_OA_TRADER_RES,
            CtraderMessage::ErrorRes(_) => PROTO_OA_ERROR_RES,
            CtraderMessage::ReconcileReq(_) => PROTO_OA_RECONCILE_REQ,
            CtraderMessage::ReconcileRes(_) => PROTO_OA_RECONCILE_RES,
        }
    }
}