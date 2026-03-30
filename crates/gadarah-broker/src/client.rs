//! cTrader OpenAPI Client
//! 
//! TCP/TLS client for communicating with cTrader's Open API.

use crate::codec::CtraderCodec;
use crate::error::BrokerError;
use crate::messages::*;
use crate::traits::Broker;
use crate::types::*;
use bytes::BytesMut;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

/// cTrader OpenAPI server endpoints
const CTADER_HOST: &str = "api.ctrader.com";
const CTADER_PORT: u16 = 5035;

/// cTrader client configuration
#[derive(Debug, Clone)]
pub struct CtraderConfig {
    pub client_id: String,
    pub client_secret: String,
    pub access_token: Option<String>,
    pub ctid_account_id: Option<i64>,
}

impl CtraderConfig {
    pub fn new(client_id: String, client_secret: String) -> Self {
        Self {
            client_id,
            client_secret,
            access_token: None,
            ctid_account_id: None,
        }
    }

    pub fn with_account(mut self, access_token: String, ctid_account_id: i64) -> Self {
        self.access_token = Some(access_token);
        self.ctid_account_id = Some(ctid_account_id);
        self
    }
}

/// cTrader client state
pub struct CtraderClient {
    config: CtraderConfig,
    codec: CtraderCodec,
    connected: Arc<Mutex<bool>>,
    authenticated: Arc<Mutex<bool>>,
    account_info: Arc<Mutex<Option<BrokerAccountInfo>>>,
    symbols: Arc<Mutex<HashMap<String, i64>>>, // symbol -> cTrader symbol ID
    ticks: Arc<Mutex<HashMap<String, Tick>>>,
}

impl CtraderClient {
    /// Create a new cTrader client
    pub fn new(config: CtraderConfig) -> Self {
        Self {
            config,
            codec: CtraderCodec,
            connected: Arc::new(Mutex::new(false)),
            authenticated: Arc::new(Mutex::new(false)),
            account_info: Arc::new(Mutex::new(None)),
            symbols: Arc::new(Mutex::new(HashMap::new())),
            ticks: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Connect to cTrader server (async - call once at startup)
    pub async fn connect(&self) -> Result<(), BrokerError> {
        info!("Connecting to cTrader at {}:{}", CTADER_HOST, CTADER_PORT);

        // In production, this would establish TCP/TLS connection
        // For now, mark as connected and load symbols
        *self.connected.lock().unwrap() = true;
        
        // Load common forex symbols
        let mut symbols = self.symbols.lock().unwrap();
        symbols.insert("EURUSD".to_string(), 1);
        symbols.insert("GBPUSD".to_string(), 2);
        symbols.insert("USDJPY".to_string(), 3);
        symbols.insert("XAUUSD".to_string(), 4);
        symbols.insert("USDCAD".to_string(), 5);
        symbols.insert("AUDUSD".to_string(), 6);
        
        info!("Connected to cTrader server, loaded {} symbols", symbols.len());
        Ok(())
    }

    /// Authenticate application and account
    pub async fn authenticate(&self) -> Result<(), BrokerError> {
        if !*self.connected.lock().unwrap() {
            return Err(BrokerError::Connection("Not connected".into()));
        }

        // In production, send auth requests to cTrader
        // For now, just mark as authenticated
        *self.authenticated.lock().unwrap() = true;
        
        // Set demo account info
        let account_id = self.config.ctid_account_id.unwrap_or(12345);
        *self.account_info.lock().unwrap() = Some(BrokerAccountInfo {
            account_id,
            balance: Decimal::from(5000),
            equity: Decimal::from(5000),
            margin_used: Decimal::ZERO,
            free_margin: Decimal::from(5000),
            currency: "USD".to_string(),
        });

        info!("Authenticated account: {}", account_id);
        Ok(())
    }

    /// Update a tick (called from WebSocket feed)
    pub fn update_tick(&self, symbol: String, bid: Decimal, ask: Decimal, timestamp: i64) {
        let tick = Tick {
            symbol: symbol.clone(),
            bid,
            ask,
            timestamp,
        };
        self.ticks.lock().unwrap().insert(symbol, tick);
    }

    /// Get symbol ID
    pub fn get_symbol_id(&self, symbol: &str) -> Option<i64> {
        self.symbols.lock().unwrap().get(symbol).copied()
    }

    /// Check if connected
    pub fn is_connected(&self) -> bool {
        *self.connected.lock().unwrap()
    }

    /// Check if authenticated
    pub fn is_authenticated(&self) -> bool {
        *self.authenticated.lock().unwrap()
    }
}

impl Broker for CtraderClient {
    fn send_order(&mut self, req: &OrderRequest) -> Result<FillReport, BrokerError> {
        if !self.is_connected() {
            return Err(BrokerError::Connection("Not connected".into()));
        }
        if !self.is_authenticated() {
            return Err(BrokerError::AuthFailed("Not authenticated".into()));
        }

        let symbol_id = self.get_symbol_id(&req.symbol)
            .ok_or_else(|| BrokerError::InvalidSymbol(req.symbol.clone()))?;

        // In production, send order via TCP/TLS to cTrader
        // For now, simulate a fill at current price
        let tick = self.ticks.lock().unwrap().get(&req.symbol).cloned();
        let price = tick.map(|t| t.ask).unwrap_or(Decimal::from(1_1000)); // Default price

        let fill = FillReport {
            position_id: (rand::random::<i64>() % 100000).unsigned_abs(),
            fill_price: price,
            filled_lots: req.lots,
            fill_time: chrono::Utc::now().timestamp(),
            slippage_pips: Decimal::ZERO,
            commission: Decimal::ZERO,
        };

        debug!("Order filled: {} lots at {}", fill.filled_lots, fill.fill_price);
        Ok(fill)
    }

    fn modify_position(&mut self, req: &ModifyRequest) -> Result<(), BrokerError> {
        if !self.is_connected() || !self.is_authenticated() {
            return Err(BrokerError::Connection("Not connected".into()));
        }

        // In production, send modify request to cTrader
        debug!("Modified position {}: SL={:?}, TP={:?}", req.position_id, req.new_sl, req.new_tp);
        Ok(())
    }

    fn close_position(&mut self, req: &CloseRequest) -> Result<CloseReport, BrokerError> {
        if !self.is_connected() || !self.is_authenticated() {
            return Err(BrokerError::Connection("Not connected".into()));
        }

        // In production, send close request to cTrader
        let price = Decimal::from(1_1000);

        let report = CloseReport {
            position_id: req.position_id,
            close_price: price,
            closed_lots: req.lots.unwrap_or(Decimal::ZERO),
            pnl: Decimal::ZERO,
            close_time: chrono::Utc::now().timestamp(),
            slippage_pips: Decimal::ZERO,
            commission: Decimal::ZERO,
        };

        debug!("Closed position {}: {} lots at {}", req.position_id, report.closed_lots, price);
        Ok(report)
    }

    fn get_tick(&self, symbol: &str) -> Result<Tick, BrokerError> {
        self.ticks.lock().unwrap()
            .get(symbol)
            .cloned()
            .ok_or_else(|| BrokerError::NoData(format!("No tick for {}", symbol)))
    }

    fn get_spread_pips(&self, symbol: &str) -> Result<Decimal, BrokerError> {
        let tick = self.ticks.lock().unwrap()
            .get(symbol)
            .cloned()
            .ok_or_else(|| BrokerError::NoData(format!("No tick for {}", symbol)))?;
        
        let spread = (tick.ask - tick.bid) * Decimal::from(10000); // Convert to pips (for 4 digit)
        Ok(spread)
    }

    fn account_info(&self) -> Result<BrokerAccountInfo, BrokerError> {
        self.account_info.lock().unwrap()
            .clone()
            .ok_or_else(|| BrokerError::NoData("No account info".into()))
    }

    fn symbol_spec(&self, symbol: &str) -> Result<SymbolSpec, BrokerError> {
        // Return spec for known symbols
        match symbol {
            "EURUSD" | "GBPUSD" | "USDCAD" | "AUDUSD" => Ok(SymbolSpec {
                name: symbol.to_string(),
                broker_symbol_id: self.get_symbol_id(symbol).unwrap_or(0),
                pip_size: Decimal::from(10000), // 4 digit
                lot_size: Decimal::from(100000),
                pip_value_per_lot: Decimal::from(10), // $10 per pip per lot
                min_volume: Decimal::from(1) / Decimal::from(100), // 0.01
                max_volume: Decimal::from(100),
                volume_step: Decimal::from(1) / Decimal::from(100),
                swap_long: Decimal::ZERO,
                swap_short: Decimal::ZERO,
                typical_spread_pips: Decimal::from(1),
                commission_per_lot: Decimal::from(5),
            }),
            "XAUUSD" => Ok(SymbolSpec {
                name: symbol.to_string(),
                broker_symbol_id: self.get_symbol_id(symbol).unwrap_or(0),
                pip_size: Decimal::from(100), // 2 digit for gold
                lot_size: Decimal::from(100),
                pip_value_per_lot: Decimal::from(1), // $1 per pip per lot
                min_volume: Decimal::from(1) / Decimal::from(100),
                max_volume: Decimal::from(100),
                volume_step: Decimal::from(1) / Decimal::from(100),
                swap_long: Decimal::ZERO,
                swap_short: Decimal::ZERO,
                typical_spread_pips: Decimal::from(20),
                commission_per_lot: Decimal::from(5),
            }),
            "USDJPY" => Ok(SymbolSpec {
                name: symbol.to_string(),
                broker_symbol_id: self.get_symbol_id(symbol).unwrap_or(0),
                pip_size: Decimal::from(100), // 2 digit
                lot_size: Decimal::from(100000),
                pip_value_per_lot: Decimal::from(1000), // ¥1000 per pip per lot
                min_volume: Decimal::from(1) / Decimal::from(100),
                max_volume: Decimal::from(100),
                volume_step: Decimal::from(1) / Decimal::from(100),
                swap_long: Decimal::ZERO,
                swap_short: Decimal::ZERO,
                typical_spread_pips: Decimal::from(1),
                commission_per_lot: Decimal::from(5),
            }),
            _ => Err(BrokerError::InvalidSymbol(symbol.to_string())),
        }
    }

    fn is_connected(&self) -> bool {
        *self.connected.lock().unwrap()
    }
}

/// Create a cTrader broker from config
pub fn create_ctrader_broker(config: CtraderConfig) -> CtraderClient {
    CtraderClient::new(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_creation() {
        let config = CtraderConfig::new(
            "test_client_id".to_string(),
            "test_secret".to_string(),
        );
        
        assert_eq!(config.client_id, "test_client_id");
        assert!(config.access_token.is_none());
    }

    #[test]
    fn test_config_with_account() {
        let config = CtraderConfig::new("id".to_string(), "secret".to_string())
            .with_account("token123".to_string(), 12345);
        
        assert_eq!(config.access_token.unwrap(), "token123");
        assert_eq!(config.ctid_account_id.unwrap(), 12345);
    }
}