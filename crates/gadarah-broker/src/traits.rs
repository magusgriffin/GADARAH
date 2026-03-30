use crate::error::BrokerError;
use crate::types::*;

// ---------------------------------------------------------------------------
// Broker trait — implemented by MockBroker (backtest) and CTraderBroker (live)
// ---------------------------------------------------------------------------

/// The core abstraction over any broker connection.
/// Synchronous API for the hot path (strategy evaluation is sync per the plan).
pub trait Broker {
    /// Place a market or pending order.
    fn send_order(&mut self, req: &OrderRequest) -> Result<FillReport, BrokerError>;

    /// Modify SL/TP on an open position.
    fn modify_position(&mut self, req: &ModifyRequest) -> Result<(), BrokerError>;

    /// Close a position (full or partial).
    fn close_position(&mut self, req: &CloseRequest) -> Result<CloseReport, BrokerError>;

    /// Get the latest tick/quote for a symbol.
    fn get_tick(&self, symbol: &str) -> Result<Tick, BrokerError>;

    /// Get the current spread in pips for a symbol.
    fn get_spread_pips(&self, symbol: &str) -> Result<rust_decimal::Decimal, BrokerError>;

    /// Get account balance/equity info.
    fn account_info(&self) -> Result<BrokerAccountInfo, BrokerError>;

    /// Get symbol specification.
    fn symbol_spec(&self, symbol: &str) -> Result<SymbolSpec, BrokerError>;

    /// Check if the connection is alive.
    fn is_connected(&self) -> bool;
}
