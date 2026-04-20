use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use tracing::debug;

use gadarah_broker::{
    forex_symbol, Broker, CloseRequest, MockBroker, MockConfig, OrderRequest, OrderType,
};
use gadarah_core::{utc_hour, Bar, Head, RegimeClassifier, SessionProfile, SignalKind};
use gadarah_risk::{
    calculate_lots, DailyPnlConfig, DailyPnlEngine, KillSwitch, RiskPercent, SizingInputs,
};

use crate::error::BacktestError;
use crate::stats::{BacktestStats, TradeResult};

// ---------------------------------------------------------------------------
// Replayer configuration
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ReplayConfig {
    pub symbol: String,
    pub pip_size: Decimal,
    pub pip_value_per_lot: Decimal,
    pub starting_balance: Decimal,
    pub risk_pct: Decimal,
    pub daily_dd_limit_pct: Decimal,
    pub max_dd_limit_pct: Decimal,
    pub max_positions: usize,
    pub min_rr: Decimal,
    pub max_spread_pips: Decimal,
    pub mock_config: MockConfig,
    pub consecutive_loss_halt: usize,
}

impl Default for ReplayConfig {
    fn default() -> Self {
        Self {
            symbol: "EURUSD".to_string(),
            pip_size: dec!(0.0001),
            pip_value_per_lot: dec!(10.0),
            starting_balance: dec!(10000),
            risk_pct: dec!(0.50),
            daily_dd_limit_pct: dec!(1.75),
            max_dd_limit_pct: dec!(6.0),
            max_positions: 3,
            min_rr: dec!(1.5),
            max_spread_pips: dec!(3.0),
            mock_config: MockConfig::default(),
            consecutive_loss_halt: 3,
        }
    }
}

// ---------------------------------------------------------------------------
// Replayer: bar-by-bar backtest engine
// ---------------------------------------------------------------------------

/// The result of a full backtest replay.
#[derive(Debug, Clone)]
pub struct ReplayResult {
    pub stats: BacktestStats,
    pub trades: Vec<TradeResult>,
    pub equity_curve: Vec<(i64, Decimal)>,
    pub bars_processed: usize,
}

/// Run a bar-by-bar replay of the given heads against historical bars.
///
/// This is the exact same evaluation pipeline as live:
/// 1. Update regime classifier
/// 2. Determine session
/// 3. Check risk gates (kill switch, daily DD, consecutive losses)
/// 4. Evaluate each head
/// 5. Filter signals (regime allowed, session, spread, R:R)
/// 6. Size and execute via mock broker
/// 7. Check SL/TP fills
pub fn run_replay(
    bars: &[Bar],
    heads: &mut [Box<dyn Head>],
    config: &ReplayConfig,
) -> Result<ReplayResult, BacktestError> {
    if bars.is_empty() {
        return Err(BacktestError::NoBars);
    }

    let mut regime = RegimeClassifier::new();
    let mut broker = MockBroker::new(config.mock_config.clone(), config.starting_balance);
    broker.add_symbol(forex_symbol(
        &config.symbol,
        config.pip_size,
        config.pip_value_per_lot,
    ));

    let mut daily_pnl = DailyPnlEngine::new(
        DailyPnlConfig {
            daily_target_pct: dec!(99.0), // effectively disable target cap in backtest
            daily_stop_pct: config.daily_dd_limit_pct,
            ..DailyPnlConfig::default()
        },
        config.starting_balance,
    );

    let mut kill_switch = KillSwitch::new();
    let mut consecutive_losses = 0usize;
    let mut trade_results: Vec<TradeResult> = Vec::new();
    let mut equity_curve: Vec<(i64, Decimal)> = Vec::new();

    // Track open position metadata
    // (position_id → head, entry, sl, lots, entry_commission, opened_at)
    let mut open_meta: std::collections::HashMap<
        u64,
        (
            gadarah_core::HeadId,
            Decimal,
            Decimal,
            Decimal,
            Decimal,
            i64,
        ),
    > = std::collections::HashMap::new();

    let risk_pct = RiskPercent::clamped(config.risk_pct);

    for bar in bars {
        // Set current price in mock broker (mid as bid, mid + spread as ask)
        let half_spread = config.mock_config.spread_pips * config.pip_size / dec!(2);
        let bid = bar.close - half_spread;
        let ask = bar.close + half_spread;
        broker.set_price(&config.symbol, bid, ask, bar.timestamp);

        // Check SL/TP fills on open positions
        let close_reports = broker.check_sl_tp();
        for cr in &close_reports {
            if let Some((head, entry, sl, lots, entry_commission, opened_at)) =
                open_meta.remove(&cr.position_id)
            {
                // R-multiple = PnL / dollar_risk, where dollar_risk = sl_pips * pip_value * lots
                let sl_pips = (entry - sl).abs() / config.pip_size;
                let dollar_risk = sl_pips * config.pip_value_per_lot * lots;
                let net_pnl = cr.pnl - entry_commission - cr.commission;
                let r_mult = if dollar_risk > Decimal::ZERO {
                    net_pnl / dollar_risk
                } else {
                    Decimal::ZERO
                };
                let is_winner = net_pnl > Decimal::ZERO;

                if is_winner {
                    consecutive_losses = 0;
                } else {
                    consecutive_losses += 1;
                }

                // Update daily PnL engine with new equity after trade close
                if let Ok(info) = broker.account_info() {
                    daily_pnl.update(info.equity, bar.timestamp);
                }

                trade_results.push(TradeResult {
                    head,
                    pnl: net_pnl,
                    r_multiple: r_mult,
                    opened_at,
                    closed_at: bar.timestamp,
                    is_winner,
                });
            }
        }

        // Update equity curve
        if let Ok(info) = broker.account_info() {
            equity_curve.push((bar.timestamp, info.equity));
        }

        // Risk gates
        if kill_switch.is_active() {
            continue;
        }
        if consecutive_losses >= config.consecutive_loss_halt {
            continue;
        }

        let account_equity = broker
            .account_info()
            .map(|i| i.equity)
            .unwrap_or(config.starting_balance);

        // Check max drawdown
        let dd_pct = if config.starting_balance > Decimal::ZERO {
            (config.starting_balance - account_equity) / config.starting_balance * dec!(100)
        } else {
            Decimal::ZERO
        };
        if dd_pct >= config.max_dd_limit_pct {
            kill_switch.activate(gadarah_risk::KillReason::TotalDD, bar.timestamp);
            continue;
        }

        // Check daily DD via DailyPnlEngine
        daily_pnl.update(account_equity, bar.timestamp);
        if !daily_pnl.can_trade() {
            continue;
        }

        // Max positions check
        if broker.open_position_count() >= config.max_positions {
            continue;
        }

        // Update regime classifier
        let regime_signal = match regime.update(bar) {
            Some(rs) => rs,
            None => continue, // still warming up
        };

        // Session
        let session_profile = SessionProfile::from_utc_hour(utc_hour(bar.timestamp));

        // Spread check
        let current_spread = config.mock_config.spread_pips;
        if current_spread > config.max_spread_pips {
            continue;
        }

        // Evaluate heads
        for head in heads.iter_mut() {
            if !head.regime_allowed(&regime_signal) {
                // Still feed the bar to maintain internal state
                let _ = head.evaluate(bar, &session_profile, &regime_signal);
                continue;
            }

            let signals = head.evaluate(bar, &session_profile, &regime_signal);

            for signal in signals {
                if signal.kind != SignalKind::Open {
                    continue; // replayer only handles new entries for now
                }

                // R:R filter
                if let Some(rr) = signal.rr_ratio() {
                    if rr < config.min_rr {
                        continue;
                    }
                } else {
                    continue;
                }

                // Size the trade
                let sl_distance = (signal.entry - signal.stop_loss).abs();
                // Phase A5: backtest costs = current spread + configured commission per lot.
                let cost_per_lot_usd = config.mock_config.spread_pips * config.pip_value_per_lot
                    + config.mock_config.commission_per_lot;
                let lots = match calculate_lots(&SizingInputs {
                    risk_pct,
                    account_equity,
                    sl_distance_price: sl_distance,
                    pip_size: config.pip_size,
                    pip_value_per_lot: config.pip_value_per_lot,
                    min_lot: dec!(0.01),
                    max_lot: dec!(50.0),
                    lot_step: dec!(0.01),
                    cost_per_lot_usd,
                    // Margin cap disabled in backtest until A4 surfaces firm
                    // leverage/contract-size metadata into ReplayConfig.
                    contract_size: Decimal::ZERO,
                    price: Decimal::ZERO,
                    leverage: Decimal::ZERO,
                    max_margin_util_pct: Decimal::ZERO,
                }) {
                    Ok(l) => l,
                    Err(_) => continue,
                };

                // Execute via mock broker. Replayer bypasses the live gate.
                let fill = match broker.send_order(
                    &OrderRequest {
                        symbol: config.symbol.clone(),
                        direction: signal.direction,
                        lots,
                        order_type: OrderType::Market,
                        stop_loss: signal.stop_loss,
                        take_profit: signal.take_profit,
                        comment: format!("{:?}", signal.head),
                    },
                    &gadarah_risk::gate::ExecutionWitness::for_simulation(),
                ) {
                    Ok(f) => f,
                    Err(e) => {
                        debug!("Order rejected: {e}");
                        continue;
                    }
                };

                open_meta.insert(
                    fill.position_id,
                    (
                        signal.head,
                        fill.fill_price,
                        signal.stop_loss,
                        lots,
                        fill.commission,
                        bar.timestamp,
                    ),
                );
            }
        }
    }

    // Close any remaining open positions at the last bar's price
    let remaining_ids = broker.open_position_ids();
    for id in remaining_ids {
        if let Ok(cr) = broker.close_position(&CloseRequest {
            position_id: id,
            lots: None,
        }) {
            if let Some((head, entry, sl, lots, entry_commission, opened_at)) =
                open_meta.remove(&id)
            {
                let sl_pips = (entry - sl).abs() / config.pip_size;
                let dollar_risk = sl_pips * config.pip_value_per_lot * lots;
                let net_pnl = cr.pnl - entry_commission - cr.commission;
                let r_mult = if dollar_risk > Decimal::ZERO {
                    net_pnl / dollar_risk
                } else {
                    Decimal::ZERO
                };
                trade_results.push(TradeResult {
                    head,
                    pnl: net_pnl,
                    r_multiple: r_mult,
                    opened_at,
                    closed_at: cr.close_time,
                    is_winner: net_pnl > Decimal::ZERO,
                });
            }
        }
    }

    let stats = BacktestStats::compute(&trade_results, config.starting_balance);

    Ok(ReplayResult {
        stats,
        trades: trade_results,
        equity_curve,
        bars_processed: bars.len(),
    })
}
