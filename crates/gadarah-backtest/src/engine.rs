use std::collections::HashMap;

use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use tracing::debug;

use gadarah_broker::{
    forex_symbol, Broker, CloseRequest, MockBroker, MockConfig, OrderRequest, OrderType,
};
use gadarah_core::{
    utc_hour, Bar, Head, HeadId, Regime9, RegimeClassifier, RegimeSignal9, Session, SessionProfile,
    SignalKind, TradeSignal,
};
use gadarah_risk::{
    calculate_lots, AccountPhase, AccountState, ConsistencyTracker, DailyPnlConfig, DailyPnlEngine,
    DriftBenchmarks, DriftConfig, DriftDetector, DriftSignal, EquityCurveFilter,
    EquityCurveFilterConfig, FirmConfig, KillSwitch, PerformanceLedger, PyramidConfig,
    RiskPercent, SizingInputs, TemporalIntelligence, TradeManagerConfig, UrgencyProfile,
};

use crate::error::BacktestError;
use crate::stats::{BacktestStats, TradeResult};

// ---------------------------------------------------------------------------
// Engine configuration
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct EngineConfig {
    pub symbol: String,
    pub pip_size: Decimal,
    pub pip_value_per_lot: Decimal,
    pub starting_balance: Decimal,
    pub base_risk_pct: Decimal,
    pub min_rr: Decimal,
    pub max_spread_pips: Decimal,
    pub max_positions: usize,
    pub mock_config: MockConfig,
    pub firm: FirmConfig,
    pub daily_pnl: DailyPnlConfig,
    pub equity_curve: EquityCurveFilterConfig,
    pub pyramid: PyramidConfig,
    pub pyramid_enabled: bool,
    pub drift: DriftConfig,
    pub drift_benchmarks: DriftBenchmarks,
    pub trade_manager: TradeManagerConfig,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            symbol: "EURUSD".to_string(),
            pip_size: dec!(0.0001),
            pip_value_per_lot: dec!(10.0),
            starting_balance: dec!(10000),
            base_risk_pct: dec!(0.74),
            min_rr: dec!(1.5),
            max_spread_pips: dec!(3.0),
            max_positions: 3,
            mock_config: MockConfig::default(),
            firm: FirmConfig {
                name: "FundingPips".to_string(),
                challenge_type: "2step".to_string(),
                profit_target_pct: dec!(6.0),
                daily_dd_limit_pct: dec!(4.0),
                max_dd_limit_pct: dec!(6.0),
                dd_mode: "trailing".to_string(),
                min_trading_days: 3,
                news_trading_allowed: true,
                max_positions: 5,
                profit_split_pct: dec!(80.0),
            },
            daily_pnl: DailyPnlConfig::default(),
            equity_curve: EquityCurveFilterConfig::default(),
            pyramid: PyramidConfig::default(),
            pyramid_enabled: false,
            drift: DriftConfig::default(),
            drift_benchmarks: DriftBenchmarks {
                expected_win_rate: dec!(0.50),
                expected_avg_r: dec!(0.20),
                expected_profit_factor: dec!(1.40),
                max_consecutive_losses: 5,
                expected_avg_slippage: dec!(0.3),
            },
            trade_manager: TradeManagerConfig::default(),
        }
    }
}

// ---------------------------------------------------------------------------
// Open position metadata
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct PositionMeta {
    head: HeadId,
    regime: Regime9,
    session: Session,
    entry_price: Decimal,
    stop_loss: Decimal,
    lots: Decimal,
    entry_commission: Decimal,
    opened_at: i64,
    mfe: Decimal,
    partial_taken: bool,
}

// ---------------------------------------------------------------------------
// Engine result
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct EngineResult {
    pub stats: BacktestStats,
    pub trades: Vec<TradeResult>,
    pub equity_curve: Vec<(i64, Decimal)>,
    pub bars_processed: usize,
    pub signals_generated: usize,
    pub signals_rejected: usize,
}

// ---------------------------------------------------------------------------
// Unified engine: full process_bar pipeline from ULTIMATE.md Part 12
// ---------------------------------------------------------------------------

pub fn run_engine(
    bars: &[Bar],
    heads: &mut [Box<dyn Head>],
    config: &EngineConfig,
) -> Result<EngineResult, BacktestError> {
    if bars.is_empty() {
        return Err(BacktestError::NoBars);
    }

    // Initialize all components
    let mut regime = RegimeClassifier::new();
    let mut broker = MockBroker::new(config.mock_config.clone(), config.starting_balance);
    broker.add_symbol(forex_symbol(
        &config.symbol,
        config.pip_size,
        config.pip_value_per_lot,
    ));

    let mut account = AccountState {
        phase: AccountPhase::ChallengePhase1,
        firm: config.firm.clone(),
        starting_balance: config.starting_balance,
        current_equity: config.starting_balance,
        high_water_mark: config.starting_balance,
        profit_pct: Decimal::ZERO,
        dd_from_hwm_pct: Decimal::ZERO,
        dd_remaining_pct: config.firm.max_dd_limit_pct,
        target_remaining: config.firm.profit_target_pct,
        trading_days: 0,
        min_days_met: config.firm.min_trading_days == 0,
        days_since_funded: 0,
        total_trades: 0,
        consecutive_losses: 0,
        phase_start_time: bars[0].timestamp,
    };

    let mut daily_pnl = DailyPnlEngine::new(config.daily_pnl.clone(), config.starting_balance);
    let mut kill_switch = KillSwitch::new();
    let mut equity_curve_filter = EquityCurveFilter::new(config.equity_curve.clone());
    let mut drift_detector =
        DriftDetector::new(config.drift.clone(), config.drift_benchmarks.clone());
    let mut performance_ledger = PerformanceLedger::new();
    let mut consistency = ConsistencyTracker::new();
    let mut temporal = TemporalIntelligence::new();

    let base_risk = RiskPercent::clamped(config.base_risk_pct);
    let mut open_meta: HashMap<u64, PositionMeta> = HashMap::new();
    let mut trade_results: Vec<TradeResult> = Vec::new();
    let mut equity_curve: Vec<(i64, Decimal)> = Vec::new();
    let mut signals_generated: usize = 0;
    let mut signals_rejected: usize = 0;
    let mut last_day: i64 = -1;
    let mut day_pnl_at_open = config.starting_balance;
    let mut last_regime: Option<RegimeSignal9> = None;

    for bar in bars {
        // Set current price in mock broker
        let half_spread = config.mock_config.spread_pips * config.pip_size / dec!(2);
        let bid = bar.close - half_spread;
        let ask = bar.close + half_spread;
        broker.set_price(&config.symbol, bid, ask, bar.timestamp);

        // --- Step 0: Update account equity from broker ---
        let equity = broker
            .account_info()
            .map(|i| i.equity)
            .unwrap_or(config.starting_balance);
        account.update_equity(equity);

        // Track trading days
        let day = bar.timestamp.div_euclid(86400);
        if day != last_day {
            // Record previous day's consistency
            if last_day >= 0 {
                let day_pnl = equity - day_pnl_at_open;
                consistency.record_day(bar.timestamp, day_pnl);
                account.trading_days += 1;
                if account.trading_days >= account.firm.min_trading_days {
                    account.min_days_met = true;
                }
            }
            day_pnl_at_open = equity;
            last_day = day;

            // Update temporal intelligence
            temporal.challenge_day = account.trading_days;
            let weekday = ((day % 7) + 4) % 7; // 0=Mon for Unix epoch
            let hour = utc_hour(bar.timestamp);
            temporal.is_friday_afternoon = weekday == 4 && hour >= 12;
        }

        // --- Check SL/TP fills on open positions ---
        let close_reports = broker.check_sl_tp();
        for cr in &close_reports {
            if let Some(meta) = open_meta.remove(&cr.position_id) {
                let sl_pips = (meta.entry_price - meta.stop_loss).abs() / config.pip_size;
                let dollar_risk = sl_pips * config.pip_value_per_lot * meta.lots;
                let net_pnl = cr.pnl - meta.entry_commission - cr.commission;
                let r_mult = if dollar_risk > Decimal::ZERO {
                    net_pnl / dollar_risk
                } else {
                    Decimal::ZERO
                };
                let is_winner = net_pnl > Decimal::ZERO;

                // Update account state
                account.total_trades += 1;
                if is_winner {
                    account.consecutive_losses = 0;
                } else {
                    account.consecutive_losses += 1;
                }

                // Update equity from broker after close
                if let Ok(info) = broker.account_info() {
                    account.update_equity(info.equity);
                    daily_pnl.update(info.equity, bar.timestamp);
                }

                // Feed into risk components
                equity_curve_filter.record_trade_close(account.current_equity);
                drift_detector.record_trade(gadarah_risk::TradeResult {
                    won: is_winner,
                    r_multiple: r_mult,
                    slippage_pips: Decimal::ZERO, // mock broker has known slippage
                });
                performance_ledger.record_trade(
                    meta.head,
                    meta.regime,
                    meta.session,
                    is_winner,
                    r_mult,
                    bar.timestamp,
                );

                trade_results.push(TradeResult {
                    head: meta.head,
                    pnl: net_pnl,
                    r_multiple: r_mult,
                    opened_at: meta.opened_at,
                    closed_at: bar.timestamp,
                    is_winner,
                });
            }
        }

        // --- Handle Close signals from heads for open positions ---
        // (Process after SL/TP checks but before new entries)
        // We check this below during head evaluation

        // Update equity curve
        equity_curve.push((bar.timestamp, account.current_equity));

        // === RISK GATE CASCADE (ULTIMATE.md Part 12, steps 1-6) ===

        // Step 1: Kill switch
        if kill_switch.check(&account, bar.timestamp) {
            // Still need to evaluate heads to maintain internal state
            feed_heads_without_signals(bar, heads, &last_regime);
            continue;
        }

        // Step 2: Drift detection
        let drift_mult = match drift_detector.evaluate() {
            DriftSignal::Halt { reason } => {
                debug!("DRIFT HALT: {}", reason);
                kill_switch.activate(&reason, bar.timestamp);
                feed_heads_without_signals(bar, heads, &last_regime);
                continue;
            }
            DriftSignal::ReduceRisk { multiplier } => multiplier,
            _ => dec!(1.0),
        };

        // Step 3: Account phase check
        let phase_mult = account.phase_risk_multiplier();
        if phase_mult.is_zero() {
            feed_heads_without_signals(bar, heads, &last_regime);
            continue;
        }
        let dd_mult = account.dd_distance_multiplier();
        if dd_mult.is_zero() {
            feed_heads_without_signals(bar, heads, &last_regime);
            continue;
        }

        // Step 4: Daily P&L check
        let day_state = daily_pnl.update(account.current_equity, bar.timestamp);
        if !daily_pnl.can_trade() {
            feed_heads_without_signals(bar, heads, &last_regime);
            continue;
        }

        // Step 5: Temporal intelligence
        let urgency = temporal.urgency_profile(&account);
        if urgency == UrgencyProfile::Protect && account.profit_pct > Decimal::ZERO {
            feed_heads_without_signals(bar, heads, &last_regime);
            continue;
        }

        // Consistency check
        if consistency.is_paused_for_consistency() {
            feed_heads_without_signals(bar, heads, &last_regime);
            continue;
        }

        // Step 6: Session check
        let session = Session::from_utc_hour(utc_hour(bar.timestamp));
        let session_profile = SessionProfile::from_session(session);
        if session_profile.sizing_mult.is_zero() {
            feed_heads_without_signals(bar, heads, &last_regime);
            continue;
        }

        // Step 7: Update regime
        let regime_signal = match regime.update(bar) {
            Some(rs) => rs,
            None => continue, // warming up
        };
        last_regime = Some(regime_signal.clone());

        // Spread check
        let current_spread = config.mock_config.spread_pips;
        if current_spread > config.max_spread_pips {
            feed_heads_without_signals(bar, heads, &last_regime);
            continue;
        }

        // Step 8: Evaluate heads (regime-filtered)
        let allowed = regime_signal.regime.allowed_heads();
        let mut all_signals: Vec<TradeSignal> = Vec::new();

        for head in heads.iter_mut() {
            let signals = head.evaluate(bar, &session_profile, &regime_signal);

            // Process Close signals immediately
            for signal in &signals {
                if signal.kind == SignalKind::Close {
                    // Find matching open position by head and close it
                    let to_close: Vec<u64> = open_meta
                        .iter()
                        .filter(|(_, m)| m.head == signal.head)
                        .map(|(id, _)| *id)
                        .collect();
                    for pos_id in to_close {
                        if let Ok(cr) = broker.close_position(&CloseRequest {
                            position_id: pos_id,
                            lots: None,
                        }) {
                            if let Some(meta) = open_meta.remove(&pos_id) {
                                let sl_pips = (meta.entry_price - meta.stop_loss).abs()
                                    / config.pip_size;
                                let dollar_risk =
                                    sl_pips * config.pip_value_per_lot * meta.lots;
                                let net_pnl = cr.pnl - meta.entry_commission - cr.commission;
                                let r_mult = if dollar_risk > Decimal::ZERO {
                                    net_pnl / dollar_risk
                                } else {
                                    Decimal::ZERO
                                };
                                let is_winner = net_pnl > Decimal::ZERO;

                                account.total_trades += 1;
                                if is_winner {
                                    account.consecutive_losses = 0;
                                } else {
                                    account.consecutive_losses += 1;
                                }
                                if let Ok(info) = broker.account_info() {
                                    account.update_equity(info.equity);
                                }
                                equity_curve_filter.record_trade_close(account.current_equity);
                                drift_detector.record_trade(gadarah_risk::TradeResult {
                                    won: is_winner,
                                    r_multiple: r_mult,
                                    slippage_pips: Decimal::ZERO,
                                });
                                performance_ledger.record_trade(
                                    meta.head,
                                    meta.regime,
                                    meta.session,
                                    is_winner,
                                    r_mult,
                                    bar.timestamp,
                                );
                                trade_results.push(TradeResult {
                                    head: meta.head,
                                    pnl: net_pnl,
                                    r_multiple: r_mult,
                                    opened_at: meta.opened_at,
                                    closed_at: bar.timestamp,
                                    is_winner,
                                });
                            }
                        }
                    }
                }
            }

            // Collect Open signals only from regime-allowed heads
            if allowed.contains(&head.id()) {
                for signal in signals {
                    if signal.kind == SignalKind::Open {
                        all_signals.push(signal);
                    }
                }
            }
        }

        let total_signals = all_signals.len();
        signals_generated += total_signals;

        // Step 9: Filter signals
        let min_confidence = match urgency {
            UrgencyProfile::Protect => dec!(0.90),
            UrgencyProfile::Coast => dec!(0.75),
            UrgencyProfile::Normal => dec!(0.55),
            UrgencyProfile::PushSelective => dec!(0.45),
        };

        let filtered: Vec<TradeSignal> = all_signals
            .into_iter()
            .filter(|s| {
                s.rr_ratio()
                    .map_or(false, |rr| rr >= config.min_rr)
            })
            .filter(|s| s.sl_distance_pips(config.pip_size) >= dec!(2))
            .filter(|s| s.head_confidence >= min_confidence)
            .filter(|s| {
                performance_ledger.is_segment_allowed(s.head, regime_signal.regime, session)
            })
            .collect();

        signals_rejected += total_signals.saturating_sub(filtered.len());

        // Max positions check before executing
        let available_slots = config.max_positions.saturating_sub(broker.open_position_count());
        let to_execute = &filtered[..filtered.len().min(available_slots)];

        // Step 10: Risk gate — apply combined multiplier stack
        let eq_filter = equity_curve_filter.multiplier();
        let effective_mult =
            account.effective_risk_multiplier(day_state, eq_filter, drift_mult);

        if effective_mult.is_zero() {
            continue;
        }

        // Step 11: Size and execute
        for signal in to_execute {
            let seg_mult =
                performance_ledger.risk_multiplier(signal.head, regime_signal.regime, session);
            let final_mult = effective_mult * seg_mult * session_profile.sizing_mult;
            let adjusted = RiskPercent::clamped(base_risk.inner() * final_mult);

            let sl_distance = (signal.entry - signal.stop_loss).abs();
            let lots = match calculate_lots(&SizingInputs {
                risk_pct: adjusted,
                account_equity: account.current_equity,
                sl_distance_price: sl_distance,
                pip_size: config.pip_size,
                pip_value_per_lot: config.pip_value_per_lot,
                min_lot: dec!(0.01),
                max_lot: dec!(50.0),
                lot_step: dec!(0.01),
            }) {
                Ok(l) => l,
                Err(_) => {
                    signals_rejected += 1;
                    continue;
                }
            };

            // Spread-adjusted R:R check (ULTIMATE.md 10.3 Gate 3)
            let spread_cost = config.mock_config.spread_pips * config.pip_size;
            let net_tp_distance = (signal.take_profit - signal.entry).abs() - spread_cost;
            let net_sl_distance = (signal.stop_loss - signal.entry).abs() + spread_cost;
            if net_sl_distance > Decimal::ZERO {
                let net_rr = net_tp_distance / net_sl_distance;
                if net_rr < dec!(1.2) {
                    signals_rejected += 1;
                    continue;
                }
            }

            // Execute via mock broker
            let fill = match broker.send_order(&OrderRequest {
                symbol: config.symbol.clone(),
                direction: signal.direction,
                lots,
                order_type: OrderType::Market,
                stop_loss: signal.stop_loss,
                take_profit: signal.take_profit,
                comment: format!("{:?}", signal.head),
            }) {
                Ok(f) => f,
                Err(e) => {
                    debug!("Order rejected: {e}");
                    signals_rejected += 1;
                    continue;
                }
            };

            open_meta.insert(
                fill.position_id,
                PositionMeta {
                    head: signal.head,
                    regime: regime_signal.regime,
                    session,
                    entry_price: fill.fill_price,
                    stop_loss: signal.stop_loss,
                    lots,
                    entry_commission: fill.commission,
                    opened_at: bar.timestamp,
                    mfe: Decimal::ZERO,
                    partial_taken: false,
                },
            );
        }
    }

    // Record final day
    if last_day >= 0 {
        let final_equity = broker
            .account_info()
            .map(|i| i.equity)
            .unwrap_or(config.starting_balance);
        let final_day_pnl = final_equity - day_pnl_at_open;
        consistency.record_day(bars.last().map_or(0, |b| b.timestamp), final_day_pnl);
    }

    // Close remaining open positions at last bar price
    let remaining_ids = broker.open_position_ids();
    for id in remaining_ids {
        if let Ok(cr) = broker.close_position(&CloseRequest {
            position_id: id,
            lots: None,
        }) {
            if let Some(meta) = open_meta.remove(&id) {
                let sl_pips =
                    (meta.entry_price - meta.stop_loss).abs() / config.pip_size;
                let dollar_risk = sl_pips * config.pip_value_per_lot * meta.lots;
                let net_pnl = cr.pnl - meta.entry_commission - cr.commission;
                let r_mult = if dollar_risk > Decimal::ZERO {
                    net_pnl / dollar_risk
                } else {
                    Decimal::ZERO
                };
                trade_results.push(TradeResult {
                    head: meta.head,
                    pnl: net_pnl,
                    r_multiple: r_mult,
                    opened_at: meta.opened_at,
                    closed_at: cr.close_time,
                    is_winner: net_pnl > Decimal::ZERO,
                });
            }
        }
    }

    let stats = BacktestStats::compute(&trade_results, config.starting_balance);

    Ok(EngineResult {
        stats,
        trades: trade_results,
        equity_curve,
        bars_processed: bars.len(),
        signals_generated,
        signals_rejected,
    })
}

/// Feed bars to heads to maintain their internal state without collecting signals.
/// Used when risk gates prevent trading but heads still need to track state.
fn feed_heads_without_signals(
    bar: &Bar,
    heads: &mut [Box<dyn Head>],
    last_regime: &Option<RegimeSignal9>,
) {
    if let Some(regime_signal) = last_regime {
        let session = Session::from_utc_hour(utc_hour(bar.timestamp));
        let session_profile = SessionProfile::from_session(session);
        for head in heads.iter_mut() {
            let _ = head.evaluate(bar, &session_profile, regime_signal);
        }
    }
}
