use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use gadarah_core::Direction;

// ---------------------------------------------------------------------------
// Symbol specification (broker-resolved)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolSpec {
    pub name: String,
    pub broker_symbol_id: i64,
    pub pip_size: Decimal,
    pub lot_size: Decimal,
    pub pip_value_per_lot: Decimal,
    pub min_volume: Decimal,
    pub max_volume: Decimal,
    pub volume_step: Decimal,
    pub swap_long: Decimal,
    pub swap_short: Decimal,
    pub typical_spread_pips: Decimal,
    pub commission_per_lot: Decimal,
}

// ---------------------------------------------------------------------------
// Order types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderRequest {
    pub symbol: String,
    pub direction: Direction,
    pub lots: Decimal,
    pub order_type: OrderType,
    pub stop_loss: Decimal,
    pub take_profit: Decimal,
    pub comment: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderType {
    Market,
    Limit { price: Decimal },
    Stop { price: Decimal },
}

// ---------------------------------------------------------------------------
// Fill report
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FillReport {
    pub position_id: u64,
    pub fill_price: Decimal,
    pub filled_lots: Decimal,
    pub fill_time: i64,
    pub slippage_pips: Decimal,
    pub commission: Decimal,
}

// ---------------------------------------------------------------------------
// Position modification
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ModifyRequest {
    pub position_id: u64,
    pub new_sl: Option<Decimal>,
    pub new_tp: Option<Decimal>,
}

// ---------------------------------------------------------------------------
// Close request
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct CloseRequest {
    pub position_id: u64,
    pub lots: Option<Decimal>, // None = close all
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloseReport {
    pub position_id: u64,
    pub close_price: Decimal,
    pub closed_lots: Decimal,
    pub pnl: Decimal,
    pub close_time: i64,
    pub slippage_pips: Decimal,
    pub commission: Decimal,
}

// ---------------------------------------------------------------------------
// Account info from broker
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrokerAccountInfo {
    pub account_id: i64,
    pub balance: Decimal,
    pub equity: Decimal,
    pub margin_used: Decimal,
    pub free_margin: Decimal,
    pub currency: String,
}

// ---------------------------------------------------------------------------
// Tick / quote
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Tick {
    pub symbol: String,
    pub bid: Decimal,
    pub ask: Decimal,
    pub timestamp: i64,
}

impl Tick {
    pub fn spread(&self) -> Decimal {
        self.ask - self.bid
    }

    pub fn mid(&self) -> Decimal {
        (self.bid + self.ask) / Decimal::TWO
    }

    pub fn spread_pips(&self, pip_size: Decimal) -> Decimal {
        self.spread() / pip_size
    }
}
