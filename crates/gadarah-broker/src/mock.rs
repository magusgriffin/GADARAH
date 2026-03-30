use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::collections::HashMap;

use gadarah_core::Direction;

use crate::error::BrokerError;
use crate::traits::Broker;
use crate::types::*;

// ---------------------------------------------------------------------------
// MockBroker: simulated broker for backtesting and paper trading
// ---------------------------------------------------------------------------

/// A mock position tracked internally by the MockBroker.
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct MockPosition {
    id: u64,
    symbol: String,
    direction: Direction,
    lots: Decimal,
    entry_price: Decimal,
    sl: Decimal,
    tp: Decimal,
    opened_at: i64,
}

/// Configuration for the mock broker's fill simulation.
#[derive(Debug, Clone)]
pub struct MockConfig {
    /// Fixed slippage in pips applied to every fill.
    pub slippage_pips: Decimal,
    /// Fixed commission per lot per side.
    pub commission_per_lot: Decimal,
    /// Fixed spread in pips.
    pub spread_pips: Decimal,
}

impl Default for MockConfig {
    fn default() -> Self {
        Self {
            slippage_pips: dec!(0.3),
            commission_per_lot: dec!(3.50),
            spread_pips: dec!(1.2),
        }
    }
}

pub struct MockBroker {
    config: MockConfig,
    symbols: HashMap<String, SymbolSpec>,
    positions: HashMap<u64, MockPosition>,
    next_id: u64,
    current_time: i64,
    current_prices: HashMap<String, (Decimal, Decimal)>, // symbol → (bid, ask)
    balance: Decimal,
    equity: Decimal,
}

impl MockBroker {
    pub fn new(config: MockConfig, starting_balance: Decimal) -> Self {
        Self {
            config,
            symbols: HashMap::new(),
            positions: HashMap::new(),
            next_id: 1,
            current_time: 0,
            current_prices: HashMap::new(),
            balance: starting_balance,
            equity: starting_balance,
        }
    }

    /// Register a symbol specification.
    pub fn add_symbol(&mut self, spec: SymbolSpec) {
        self.symbols.insert(spec.name.clone(), spec);
    }

    /// Set the current simulation time and price for a symbol.
    pub fn set_price(&mut self, symbol: &str, bid: Decimal, ask: Decimal, time: i64) {
        self.current_prices.insert(symbol.to_string(), (bid, ask));
        self.current_time = time;
    }

    /// Check all open positions against current prices for SL/TP hits.
    /// Returns close reports for any positions that were stopped out or hit TP.
    pub fn check_sl_tp(&mut self) -> Vec<CloseReport> {
        let mut to_close = Vec::new();

        for (id, pos) in &self.positions {
            let Some(&(bid, ask)) = self.current_prices.get(&pos.symbol) else {
                continue;
            };

            let pip_size = self
                .symbols
                .get(&pos.symbol)
                .map(|s| s.pip_size)
                .unwrap_or(dec!(0.0001));

            match pos.direction {
                Direction::Buy => {
                    // Buy closes at bid
                    if bid <= pos.sl {
                        to_close.push((*id, pos.sl, "SL"));
                    } else if bid >= pos.tp {
                        to_close.push((*id, pos.tp, "TP"));
                    }
                }
                Direction::Sell => {
                    // Sell closes at ask
                    if ask >= pos.sl {
                        to_close.push((*id, pos.sl, "SL"));
                    } else if ask <= pos.tp {
                        to_close.push((*id, pos.tp, "TP"));
                    }
                }
            }
            let _ = pip_size; // used for future slippage model
        }

        let mut reports = Vec::new();
        for (id, price, _reason) in to_close {
            if let Some(pos) = self.positions.remove(&id) {
                let close_price =
                    apply_exit_slippage(&pos, price, &self.symbols, self.config.slippage_pips);
                let pnl = calculate_pnl(&pos, close_price, &self.symbols);
                let commission = self.config.commission_per_lot * pos.lots;
                self.balance += pnl - commission;
                self.equity = self.balance + self.unrealized_pnl();
                reports.push(CloseReport {
                    position_id: id,
                    close_price,
                    closed_lots: pos.lots,
                    pnl,
                    close_time: self.current_time,
                    slippage_pips: self.config.slippage_pips,
                    commission,
                });
            }
        }

        reports
    }

    /// Get all open position IDs.
    pub fn open_position_ids(&self) -> Vec<u64> {
        self.positions.keys().copied().collect()
    }

    /// Get the number of open positions.
    pub fn open_position_count(&self) -> usize {
        self.positions.len()
    }

    fn unrealized_pnl(&self) -> Decimal {
        let mut total = Decimal::ZERO;
        for pos in self.positions.values() {
            if let Some(&(bid, ask)) = self.current_prices.get(&pos.symbol) {
                let current = match pos.direction {
                    Direction::Buy => bid,
                    Direction::Sell => ask,
                };
                total += calculate_pnl(pos, current, &self.symbols);
            }
        }
        total
    }
}

impl Broker for MockBroker {
    fn send_order(&mut self, req: &OrderRequest) -> Result<FillReport, BrokerError> {
        let spec = self
            .symbols
            .get(&req.symbol)
            .ok_or_else(|| BrokerError::SymbolNotFound(req.symbol.clone()))?;

        let (bid, ask) = self
            .current_prices
            .get(&req.symbol)
            .copied()
            .ok_or_else(|| BrokerError::SymbolNotFound(req.symbol.clone()))?;

        // Market fills at ask for buy, bid for sell (+ slippage)
        let slippage_price = self.config.slippage_pips * spec.pip_size;
        let fill_price = match req.direction {
            Direction::Buy => ask + slippage_price,
            Direction::Sell => bid - slippage_price,
        };

        let commission = self.config.commission_per_lot * req.lots;
        self.balance -= commission;

        let id = self.next_id;
        self.next_id += 1;

        self.positions.insert(
            id,
            MockPosition {
                id,
                symbol: req.symbol.clone(),
                direction: req.direction,
                lots: req.lots,
                entry_price: fill_price,
                sl: req.stop_loss,
                tp: req.take_profit,
                opened_at: self.current_time,
            },
        );

        self.equity = self.balance + self.unrealized_pnl();

        Ok(FillReport {
            position_id: id,
            fill_price,
            filled_lots: req.lots,
            fill_time: self.current_time,
            slippage_pips: self.config.slippage_pips,
            commission,
        })
    }

    fn modify_position(&mut self, req: &ModifyRequest) -> Result<(), BrokerError> {
        let pos =
            self.positions
                .get_mut(&req.position_id)
                .ok_or(BrokerError::PositionNotFound {
                    id: req.position_id,
                })?;

        if let Some(sl) = req.new_sl {
            pos.sl = sl;
        }
        if let Some(tp) = req.new_tp {
            pos.tp = tp;
        }
        Ok(())
    }

    fn close_position(&mut self, req: &CloseRequest) -> Result<CloseReport, BrokerError> {
        let pos =
            self.positions
                .get(&req.position_id)
                .cloned()
                .ok_or(BrokerError::PositionNotFound {
                    id: req.position_id,
                })?;

        let (bid, ask) = self
            .current_prices
            .get(&pos.symbol)
            .copied()
            .ok_or_else(|| BrokerError::SymbolNotFound(pos.symbol.clone()))?;

        let lots_to_close = req.lots.unwrap_or(pos.lots);
        if lots_to_close <= Decimal::ZERO || lots_to_close > pos.lots {
            return Err(BrokerError::InvalidCloseVolume {
                requested: lots_to_close,
                available: pos.lots,
            });
        }

        let market_price = match pos.direction {
            Direction::Buy => bid,
            Direction::Sell => ask,
        };
        let close_price =
            apply_exit_slippage(&pos, market_price, &self.symbols, self.config.slippage_pips);
        let pnl = calculate_pnl_lots(&pos, close_price, lots_to_close, &self.symbols);
        let commission = self.config.commission_per_lot * lots_to_close;

        // If partial close, put remaining back
        if lots_to_close == pos.lots {
            self.positions.remove(&req.position_id);
        } else if let Some(remaining) = self.positions.get_mut(&req.position_id) {
            remaining.lots -= lots_to_close;
        }

        self.balance += pnl - commission;
        self.equity = self.balance + self.unrealized_pnl();

        Ok(CloseReport {
            position_id: req.position_id,
            close_price,
            closed_lots: lots_to_close,
            pnl,
            close_time: self.current_time,
            slippage_pips: self.config.slippage_pips,
            commission,
        })
    }

    fn get_tick(&self, symbol: &str) -> Result<Tick, BrokerError> {
        let (bid, ask) = self
            .current_prices
            .get(symbol)
            .copied()
            .ok_or_else(|| BrokerError::SymbolNotFound(symbol.to_string()))?;

        Ok(Tick {
            symbol: symbol.to_string(),
            bid,
            ask,
            timestamp: self.current_time,
        })
    }

    fn get_spread_pips(&self, symbol: &str) -> Result<Decimal, BrokerError> {
        let tick = self.get_tick(symbol)?;
        let pip_size = self
            .symbols
            .get(symbol)
            .map(|s| s.pip_size)
            .unwrap_or(dec!(0.0001));
        Ok(tick.spread_pips(pip_size))
    }

    fn account_info(&self) -> Result<BrokerAccountInfo, BrokerError> {
        Ok(BrokerAccountInfo {
            account_id: 0,
            balance: self.balance,
            equity: self.equity,
            margin_used: Decimal::ZERO, // simplified for mock
            free_margin: self.equity,
            currency: "USD".to_string(),
        })
    }

    fn symbol_spec(&self, symbol: &str) -> Result<SymbolSpec, BrokerError> {
        self.symbols
            .get(symbol)
            .cloned()
            .ok_or_else(|| BrokerError::SymbolNotFound(symbol.to_string()))
    }

    fn is_connected(&self) -> bool {
        true
    }
}

// ---------------------------------------------------------------------------
// PnL calculation helpers
// ---------------------------------------------------------------------------

fn calculate_pnl(
    pos: &MockPosition,
    close_price: Decimal,
    symbols: &HashMap<String, SymbolSpec>,
) -> Decimal {
    calculate_pnl_lots(pos, close_price, pos.lots, symbols)
}

fn calculate_pnl_lots(
    pos: &MockPosition,
    close_price: Decimal,
    lots: Decimal,
    symbols: &HashMap<String, SymbolSpec>,
) -> Decimal {
    let pip_size = symbols
        .get(&pos.symbol)
        .map(|s| s.pip_size)
        .unwrap_or(dec!(0.0001));
    let pip_value = symbols
        .get(&pos.symbol)
        .map(|s| s.pip_value_per_lot)
        .unwrap_or(dec!(10.0));

    let price_diff = match pos.direction {
        Direction::Buy => close_price - pos.entry_price,
        Direction::Sell => pos.entry_price - close_price,
    };

    let pips = price_diff / pip_size;
    pips * pip_value * lots
}

fn apply_exit_slippage(
    pos: &MockPosition,
    base_price: Decimal,
    symbols: &HashMap<String, SymbolSpec>,
    slippage_pips: Decimal,
) -> Decimal {
    let pip_size = symbols
        .get(&pos.symbol)
        .map(|s| s.pip_size)
        .unwrap_or(dec!(0.0001));
    let slippage_price = slippage_pips * pip_size;
    match pos.direction {
        Direction::Buy => base_price - slippage_price,
        Direction::Sell => base_price + slippage_price,
    }
}

// ---------------------------------------------------------------------------
// Convenience: create a standard forex symbol spec
// ---------------------------------------------------------------------------

pub fn forex_symbol(name: &str, pip_size: Decimal, pip_value_per_lot: Decimal) -> SymbolSpec {
    SymbolSpec {
        name: name.to_string(),
        broker_symbol_id: 0,
        pip_size,
        lot_size: dec!(100000),
        pip_value_per_lot,
        min_volume: dec!(0.01),
        max_volume: dec!(50.0),
        volume_step: dec!(0.01),
        swap_long: Decimal::ZERO,
        swap_short: Decimal::ZERO,
        typical_spread_pips: dec!(1.2),
        commission_per_lot: dec!(3.50),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_broker() -> MockBroker {
        let mut broker = MockBroker::new(MockConfig::default(), dec!(10000));
        broker.add_symbol(forex_symbol("EURUSD", dec!(0.0001), dec!(10.0)));
        broker.set_price("EURUSD", dec!(1.10000), dec!(1.10012), 1700000000);
        broker
    }

    #[test]
    fn market_buy_and_close() {
        let mut broker = setup_broker();

        let fill = broker
            .send_order(&OrderRequest {
                symbol: "EURUSD".into(),
                direction: Direction::Buy,
                lots: dec!(0.10),
                order_type: OrderType::Market,
                stop_loss: dec!(1.09800),
                take_profit: dec!(1.10400),
                comment: "test".into(),
            })
            .unwrap();

        assert!(fill.fill_price > dec!(1.10012)); // ask + slippage
        assert_eq!(broker.open_position_count(), 1);

        // Move price up
        broker.set_price("EURUSD", dec!(1.10400), dec!(1.10412), 1700003600);

        let close = broker
            .close_position(&CloseRequest {
                position_id: fill.position_id,
                lots: None,
            })
            .unwrap();

        assert!(close.pnl > Decimal::ZERO);
        assert_eq!(close.commission, dec!(0.35));
        assert_eq!(broker.open_position_count(), 0);
        assert_eq!(broker.account_info().unwrap().balance, dec!(10037.50));
    }

    #[test]
    fn sl_tp_check() {
        let mut broker = setup_broker();

        broker
            .send_order(&OrderRequest {
                symbol: "EURUSD".into(),
                direction: Direction::Buy,
                lots: dec!(0.10),
                order_type: OrderType::Market,
                stop_loss: dec!(1.09800),
                take_profit: dec!(1.10400),
                comment: "test".into(),
            })
            .unwrap();

        // Price drops to SL
        broker.set_price("EURUSD", dec!(1.09750), dec!(1.09762), 1700003600);
        let reports = broker.check_sl_tp();
        assert_eq!(reports.len(), 1);
        assert!(reports[0].close_price < dec!(1.09800));
        assert_eq!(reports[0].commission, dec!(0.35));
        assert!(reports[0].pnl < Decimal::ZERO);
        assert_eq!(broker.open_position_count(), 0);
    }

    #[test]
    fn modify_position() {
        let mut broker = setup_broker();

        let fill = broker
            .send_order(&OrderRequest {
                symbol: "EURUSD".into(),
                direction: Direction::Buy,
                lots: dec!(0.10),
                order_type: OrderType::Market,
                stop_loss: dec!(1.09800),
                take_profit: dec!(1.10400),
                comment: "test".into(),
            })
            .unwrap();

        broker
            .modify_position(&ModifyRequest {
                position_id: fill.position_id,
                new_sl: Some(dec!(1.10000)), // move to breakeven
                new_tp: None,
            })
            .unwrap();

        // Price drops to new SL (breakeven)
        broker.set_price("EURUSD", dec!(1.09990), dec!(1.10002), 1700003600);
        let reports = broker.check_sl_tp();
        assert_eq!(reports.len(), 1);
        // PnL should be near zero (entry was ~1.10015 due to slippage, SL at 1.10000)
    }

    #[test]
    fn spread_calculation() {
        let broker = setup_broker();
        let spread = broker.get_spread_pips("EURUSD").unwrap();
        assert_eq!(spread, dec!(1.2)); // (1.10012 - 1.10000) / 0.0001
    }

    #[test]
    fn reject_invalid_partial_close_volume() {
        let mut broker = setup_broker();
        let fill = broker
            .send_order(&OrderRequest {
                symbol: "EURUSD".into(),
                direction: Direction::Buy,
                lots: dec!(0.10),
                order_type: OrderType::Market,
                stop_loss: dec!(1.09800),
                take_profit: dec!(1.10400),
                comment: "test".into(),
            })
            .unwrap();

        let err = broker
            .close_position(&CloseRequest {
                position_id: fill.position_id,
                lots: Some(dec!(0.20)),
            })
            .unwrap_err();

        assert!(matches!(
            err,
            BrokerError::InvalidCloseVolume {
                requested,
                available
            } if requested == dec!(0.20) && available == dec!(0.10)
        ));
        assert_eq!(broker.open_position_count(), 1);
    }
}
