use std::collections::HashMap;

use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use tracing::debug;

use gadarah_broker::{
    forex_symbol, Broker, CloseRequest, MockBroker, MockConfig, ModifyRequest, OrderRequest,
    OrderType,
};
use gadarah_core::{
    utc_hour, Bar, Direction, Head, HeadId, Regime9, RegimeClassifier, RegimeSignal9, Session,
    SessionProfile, SignalKind, TradeSignal, ATR,
};
use gadarah_risk::{
    calculate_lots, AccountPhase, AccountState, ComplianceBlackoutWindow, ComplianceOpenExposure,
    ConsistencyTracker, DailyPnlConfig, DailyPnlEngine, DriftBenchmarks, DriftConfig,
    DriftDetector, DriftSignal, EquityCurveFilter, EquityCurveFilterConfig, ExecutionConfig,
    ExecutionEngine, FillRecord, FirmConfig, FundingPipsComplianceManager, KillSwitch,
    OpenPosition, PerformanceLedger, PyramidConfig, RiskPercent, SizingInputs,
    TemporalIntelligence, TradeAction, TradeManager, TradeManagerConfig, UrgencyProfile,
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
    pub compliance_blackout_windows: Vec<ComplianceBlackoutWindow>,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            symbol: "EURUSD".to_string(),
            pip_size: dec!(0.0001),
            pip_value_per_lot: dec!(10.0),
            starting_balance: dec!(5000),
            base_risk_pct: dec!(0.74),
            min_rr: dec!(1.5),
            max_spread_pips: dec!(3.0),
            max_positions: 3,
            mock_config: MockConfig::default(),
            firm: FirmConfig {
                name: "The5ers - Hyper Growth".to_string(),
                challenge_type: "hyper_growth".to_string(),
                profit_target_pct: dec!(10.0),
                daily_dd_limit_pct: dec!(3.0),
                max_dd_limit_pct: dec!(6.0),
                dd_mode: "static".to_string(),
                min_trading_days: 0,
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
            compliance_blackout_windows: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Open position metadata
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct PositionMeta {
    head: HeadId,
    direction: Direction,
    regime: Regime9,
    session: Session,
    entry_price: Decimal,
    initial_stop_loss: Decimal,
    stop_loss: Decimal,
    take_profit: Decimal,
    lots: Decimal,
    initial_lots: Decimal,
    risk_pct: Decimal,
    pyramid_level: u8,
    entry_commission: Decimal,
    realized_exit_pnl: Decimal,
    realized_exit_commission: Decimal,
    opened_at: i64,
    mfe: Decimal,
    partial_taken: bool,
    breakeven_set: bool,
    trailing_active: bool,
}

// ---------------------------------------------------------------------------
// Engine result
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct EngineResult {
    pub stats: BacktestStats,
    pub trades: Vec<TradeResult>,
    pub trade_log: Vec<EngineTradeLog>,
    pub equity_curve: Vec<(i64, Decimal)>,
    pub bars_processed: usize,
    pub signals_generated: usize,
    pub signals_rejected: usize,
    pub diagnostics: EngineDiagnostics,
}

#[derive(Debug, Clone, Default)]
pub struct EngineDiagnostics {
    pub bars_with_regime: usize,
    pub bars_without_regime: usize,
    pub blocked_kill_switch: usize,
    pub blocked_drift_halt: usize,
    pub blocked_phase: usize,
    pub blocked_dd_distance: usize,
    pub blocked_daily_stop: usize,
    pub blocked_temporal_protect: usize,
    pub blocked_consistency: usize,
    pub blocked_spread: usize,
    pub observed_open_signals_while_blocked: usize,
    pub observed_close_signals_while_blocked: usize,
    pub bars_by_regime: HashMap<Regime9, usize>,
    pub eligible_bars_by_regime: HashMap<Regime9, usize>,
    pub bars_by_session: HashMap<Session, usize>,
    pub eligible_bars_by_session: HashMap<Session, usize>,
    pub head_signals: HashMap<HeadId, HeadSignalDiagnostics>,
    pub segment_signals: HashMap<(HeadId, Regime9, Session), SegmentSignalDiagnostics>,
}

#[derive(Debug, Clone, Default)]
pub struct HeadSignalDiagnostics {
    pub open_candidates: usize,
    pub close_signals: usize,
    pub blocked_open_signals: usize,
    pub blocked_close_signals: usize,
    pub passed_filters: usize,
    pub rejected_total: usize,
    pub rejected_min_rr: usize,
    pub rejected_sl_distance: usize,
    pub rejected_confidence: usize,
    pub rejected_segment: usize,
    pub rejected_effective_risk: usize,
    pub rejected_max_positions: usize,
    pub rejected_sizing: usize,
    pub rejected_net_rr: usize,
    pub rejected_compliance: usize,
    pub rejected_order_error: usize,
    pub executed_entries: usize,
    pub executed_closes: usize,
}

#[derive(Debug, Clone, Default)]
pub struct SegmentSignalDiagnostics {
    pub open_candidates: usize,
    pub blocked_open_signals: usize,
    pub passed_filters: usize,
    pub executed_entries: usize,
}

#[derive(Debug, Clone)]
pub struct EngineTradeLog {
    pub symbol: String,
    pub direction: Direction,
    pub head: HeadId,
    pub regime: Regime9,
    pub session: Session,
    pub entry_price: Decimal,
    pub stop_loss: Decimal,
    pub take_profit: Decimal,
    pub lots: Decimal,
    pub risk_pct: Decimal,
    pub pyramid_level: u8,
    pub opened_at: i64,
    pub closed_at: i64,
    pub close_price: Decimal,
    pub pnl: Decimal,
    pub r_multiple: Decimal,
    pub close_reason: String,
    pub slippage_pips: Decimal,
}

#[derive(Debug, Clone, Copy)]
enum SignalRejectionReason {
    MinRr,
    SlDistance,
    Confidence,
    Segment,
    EffectiveRisk,
    MaxPositions,
    Sizing,
    NetRr,
    Compliance,
    OrderError,
}

impl EngineDiagnostics {
    fn head_entry(&mut self, head: HeadId) -> &mut HeadSignalDiagnostics {
        self.head_signals.entry(head).or_default()
    }

    fn segment_entry(
        &mut self,
        head: HeadId,
        regime: Regime9,
        session: Session,
    ) -> &mut SegmentSignalDiagnostics {
        self.segment_signals
            .entry((head, regime, session))
            .or_default()
    }

    fn note_session_bar(&mut self, session: Session) {
        *self.bars_by_session.entry(session).or_default() += 1;
    }

    fn note_regime_bar(&mut self, regime: Regime9) {
        self.bars_with_regime += 1;
        *self.bars_by_regime.entry(regime).or_default() += 1;
    }

    fn note_eligible_bar(&mut self, regime: Regime9, session: Session) {
        *self.eligible_bars_by_regime.entry(regime).or_default() += 1;
        *self.eligible_bars_by_session.entry(session).or_default() += 1;
    }

    fn note_open_candidate(&mut self, head: HeadId, regime: Regime9, session: Session) {
        self.head_entry(head).open_candidates += 1;
        self.segment_entry(head, regime, session).open_candidates += 1;
    }

    fn note_close_signal(&mut self, head: HeadId) {
        self.head_entry(head).close_signals += 1;
    }

    fn note_close_executed(&mut self, head: HeadId) {
        self.head_entry(head).executed_closes += 1;
    }

    fn note_filter_pass(&mut self, head: HeadId, regime: Regime9, session: Session) {
        self.head_entry(head).passed_filters += 1;
        self.segment_entry(head, regime, session).passed_filters += 1;
    }

    fn note_rejection(&mut self, head: HeadId, reason: SignalRejectionReason) {
        let stats = self.head_entry(head);
        stats.rejected_total += 1;
        match reason {
            SignalRejectionReason::MinRr => stats.rejected_min_rr += 1,
            SignalRejectionReason::SlDistance => stats.rejected_sl_distance += 1,
            SignalRejectionReason::Confidence => stats.rejected_confidence += 1,
            SignalRejectionReason::Segment => stats.rejected_segment += 1,
            SignalRejectionReason::EffectiveRisk => stats.rejected_effective_risk += 1,
            SignalRejectionReason::MaxPositions => stats.rejected_max_positions += 1,
            SignalRejectionReason::Sizing => stats.rejected_sizing += 1,
            SignalRejectionReason::NetRr => stats.rejected_net_rr += 1,
            SignalRejectionReason::Compliance => stats.rejected_compliance += 1,
            SignalRejectionReason::OrderError => stats.rejected_order_error += 1,
        }
    }

    fn note_execution(&mut self, head: HeadId, regime: Regime9, session: Session) {
        self.head_entry(head).executed_entries += 1;
        self.segment_entry(head, regime, session).executed_entries += 1;
    }

    fn note_blocked_signals(&mut self, observed: &ObservedSignals) {
        self.observed_open_signals_while_blocked += observed.open_signals;
        self.observed_close_signals_while_blocked += observed.close_signals;

        for ((head, regime, session), counts) in &observed.by_segment {
            {
                let head_stats = self.head_entry(*head);
                head_stats.blocked_open_signals += counts.open_signals;
                head_stats.blocked_close_signals += counts.close_signals;
            }
            self.segment_entry(*head, *regime, *session)
                .blocked_open_signals += counts.open_signals;
        }
    }
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
    let mut trade_manager = TradeManager::new(config.trade_manager.clone());
    let mut execution_engine = ExecutionEngine::new(
        ExecutionConfig {
            min_rr_after_spread: dec!(1.2),
            ..ExecutionConfig::default()
        },
        config.mock_config.spread_pips,
    );
    let mut compliance = FundingPipsComplianceManager::for_firm(&config.firm);
    compliance.set_blackout_windows(config.compliance_blackout_windows.clone());
    let mut atr = ATR::new(14);

    let base_risk = RiskPercent::clamped(config.base_risk_pct);
    let mut open_meta: HashMap<u64, PositionMeta> = HashMap::new();
    let mut trade_results: Vec<TradeResult> = Vec::new();
    let mut trade_log: Vec<EngineTradeLog> = Vec::new();
    let mut equity_curve: Vec<(i64, Decimal)> = Vec::new();
    let mut signals_generated: usize = 0;
    let mut signals_rejected: usize = 0;
    let mut last_day: i64 = -1;
    let mut day_pnl_at_open = config.starting_balance;
    let mut diagnostics = EngineDiagnostics::default();

    for bar in bars {
        // Set current price in mock broker
        let half_spread = config.mock_config.spread_pips * config.pip_size / dec!(2);
        let bid = bar.close - half_spread;
        let ask = bar.close + half_spread;
        broker.set_price(&config.symbol, bid, ask, bar.timestamp);
        execution_engine.update_spread(config.mock_config.spread_pips, bar.timestamp);
        let current_atr = atr.update(bar).unwrap_or(Decimal::ZERO);

        // Always advance the regime classifier so blocked bars do not leave
        // the strategy running on stale market context.
        let current_regime = regime.update(bar);
        let session = Session::from_utc_hour(utc_hour(bar.timestamp));
        let session_profile = SessionProfile::from_session(session);
        diagnostics.note_session_bar(session);
        if let Some(regime_signal) = current_regime.as_ref() {
            diagnostics.note_regime_bar(regime_signal.regime);
        }
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
                finalize_closed_position(
                    config,
                    &mut account,
                    &mut broker,
                    &mut daily_pnl,
                    &mut equity_curve_filter,
                    &mut drift_detector,
                    &mut performance_ledger,
                    &mut trade_results,
                    &mut trade_log,
                    meta,
                    cr,
                    "SL/TP".to_string(),
                );
            }
        }

        manage_open_positions(
            config,
            &mut broker,
            &mut open_meta,
            &mut trade_manager,
            current_atr,
            bid,
            ask,
            bar.timestamp,
            &mut account,
            &mut daily_pnl,
            &mut equity_curve_filter,
            &mut drift_detector,
            &mut performance_ledger,
            &mut trade_results,
            &mut trade_log,
        );

        // --- Handle Close signals from heads for open positions ---
        // (Process after SL/TP checks but before new entries)
        // We check this below during head evaluation

        // Update equity curve
        equity_curve.push((bar.timestamp, account.current_equity));

        // === RISK GATE CASCADE (ULTIMATE.md Part 12, steps 1-6) ===

        // Step 1: Kill switch
        if kill_switch.check(&account, bar.timestamp) {
            diagnostics.blocked_kill_switch += 1;
            let observed = advance_heads(bar, heads, &session_profile, current_regime.as_ref());
            diagnostics.note_blocked_signals(&observed);
            continue;
        }

        // Step 2: Drift detection
        let drift_mult = match drift_detector.evaluate() {
            DriftSignal::Halt { reason } => {
                debug!("DRIFT HALT: {}", reason);
                kill_switch.activate(&reason, bar.timestamp);
                diagnostics.blocked_drift_halt += 1;
                let observed = advance_heads(bar, heads, &session_profile, current_regime.as_ref());
                diagnostics.note_blocked_signals(&observed);
                continue;
            }
            DriftSignal::ReduceRisk { multiplier } => multiplier,
            _ => dec!(1.0),
        };

        // Step 3: Account phase check
        let phase_mult = account.phase_risk_multiplier();
        if phase_mult.is_zero() {
            diagnostics.blocked_phase += 1;
            let observed = advance_heads(bar, heads, &session_profile, current_regime.as_ref());
            diagnostics.note_blocked_signals(&observed);
            continue;
        }
        let dd_mult = account.dd_distance_multiplier();
        if dd_mult.is_zero() {
            diagnostics.blocked_dd_distance += 1;
            let observed = advance_heads(bar, heads, &session_profile, current_regime.as_ref());
            diagnostics.note_blocked_signals(&observed);
            continue;
        }

        // Step 4: Daily P&L check
        let day_state = daily_pnl.update(account.current_equity, bar.timestamp);
        if !daily_pnl.can_trade() {
            diagnostics.blocked_daily_stop += 1;
            let observed = advance_heads(bar, heads, &session_profile, current_regime.as_ref());
            diagnostics.note_blocked_signals(&observed);
            continue;
        }

        // Step 5: Temporal intelligence
        let urgency = temporal.urgency_profile(&account);
        if urgency == UrgencyProfile::Protect && account.profit_pct > Decimal::ZERO {
            diagnostics.blocked_temporal_protect += 1;
            let observed = advance_heads(bar, heads, &session_profile, current_regime.as_ref());
            diagnostics.note_blocked_signals(&observed);
            continue;
        }

        // Consistency check
        if consistency.is_paused_for_consistency() {
            diagnostics.blocked_consistency += 1;
            let observed = advance_heads(bar, heads, &session_profile, current_regime.as_ref());
            diagnostics.note_blocked_signals(&observed);
            continue;
        }

        // Step 6: Use the freshly-updated regime
        let regime_signal = match current_regime {
            Some(rs) => rs,
            None => {
                diagnostics.bars_without_regime += 1;
                let warmup_regime = synthetic_transition_regime(bar.timestamp);
                let _ = advance_heads(bar, heads, &session_profile, Some(&warmup_regime));
                continue;
            } // warming up
        };

        // Spread check
        let current_spread = config.mock_config.spread_pips;
        if current_spread > config.max_spread_pips {
            diagnostics.blocked_spread += 1;
            let observed = advance_heads(bar, heads, &session_profile, Some(&regime_signal));
            diagnostics.note_blocked_signals(&observed);
            continue;
        }

        diagnostics.note_eligible_bar(regime_signal.regime, session);

        // Step 8: Evaluate heads (regime-filtered)
        let allowed = regime_signal.regime.allowed_heads();
        let mut all_signals: Vec<TradeSignal> = Vec::new();

        for head in heads.iter_mut() {
            let signals = head.evaluate(bar, &session_profile, &regime_signal);

            // Process Close signals immediately
            for signal in &signals {
                if signal.kind == SignalKind::Close {
                    diagnostics.note_close_signal(signal.head);
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
                                let head = meta.head;
                                finalize_closed_position(
                                    config,
                                    &mut account,
                                    &mut broker,
                                    &mut daily_pnl,
                                    &mut equity_curve_filter,
                                    &mut drift_detector,
                                    &mut performance_ledger,
                                    &mut trade_results,
                                    &mut trade_log,
                                    meta,
                                    &cr,
                                    "HeadClose".to_string(),
                                );
                                diagnostics.note_close_executed(head);
                            }
                        }
                    }
                }
            }

            // Collect Open signals only from regime-allowed heads
            if allowed.contains(&head.id()) {
                for signal in signals {
                    if signal.kind == SignalKind::Open {
                        diagnostics.note_open_candidate(signal.head, regime_signal.regime, session);
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

        let mut filtered: Vec<TradeSignal> = Vec::new();
        for signal in all_signals {
            if signal.rr_ratio().is_none_or(|rr| rr < config.min_rr) {
                diagnostics.note_rejection(signal.head, SignalRejectionReason::MinRr);
                signals_rejected += 1;
                continue;
            }
            if signal.sl_distance_pips(config.pip_size) < dec!(2) {
                diagnostics.note_rejection(signal.head, SignalRejectionReason::SlDistance);
                signals_rejected += 1;
                continue;
            }
            if signal.head_confidence < min_confidence {
                diagnostics.note_rejection(signal.head, SignalRejectionReason::Confidence);
                signals_rejected += 1;
                continue;
            }
            if !performance_ledger.is_segment_allowed(signal.head, regime_signal.regime, session) {
                diagnostics.note_rejection(signal.head, SignalRejectionReason::Segment);
                signals_rejected += 1;
                continue;
            }
            diagnostics.note_filter_pass(signal.head, regime_signal.regime, session);
            filtered.push(signal);
        }

        // Max positions check before executing
        let available_slots = config
            .max_positions
            .saturating_sub(broker.open_position_count());
        if filtered.len() > available_slots {
            for signal in &filtered[available_slots..] {
                diagnostics.note_rejection(signal.head, SignalRejectionReason::MaxPositions);
                signals_rejected += 1;
            }
        }
        let to_execute = &filtered[..filtered.len().min(available_slots)];

        // Step 10: Risk gate — apply combined multiplier stack
        let eq_filter = equity_curve_filter.multiplier();
        let effective_mult = account.effective_risk_multiplier(day_state, eq_filter, drift_mult);

        if effective_mult.is_zero() {
            for signal in to_execute {
                diagnostics.note_rejection(signal.head, SignalRejectionReason::EffectiveRisk);
                signals_rejected += 1;
            }
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
                    diagnostics.note_rejection(signal.head, SignalRejectionReason::Sizing);
                    signals_rejected += 1;
                    continue;
                }
            };

            // Spread-adjusted R:R check (ULTIMATE.md 10.3 Gate 3)
            match execution_engine.adjusted_rr(signal) {
                Some(net_rr) if net_rr < dec!(1.2) => {
                    diagnostics.note_rejection(signal.head, SignalRejectionReason::NetRr);
                    signals_rejected += 1;
                    continue;
                }
                None => {
                    diagnostics.note_rejection(signal.head, SignalRejectionReason::NetRr);
                    signals_rejected += 1;
                    continue;
                }
                _ => {}
            }

            let active_exposures = open_meta
                .values()
                .map(|meta| ComplianceOpenExposure {
                    symbol: config.symbol.clone(),
                    direction: meta.direction,
                    risk_pct: meta.risk_pct,
                    lots: meta.lots,
                    opened_at: meta.opened_at,
                })
                .collect::<Vec<_>>();
            if let Err(rejection) = compliance.evaluate_entry(
                signal,
                adjusted.inner(),
                lots,
                &active_exposures,
                bar.timestamp,
            ) {
                debug!(
                    "Compliance rejected {:?}: {}",
                    signal.head, rejection.detail
                );
                diagnostics.note_rejection(signal.head, SignalRejectionReason::Compliance);
                signals_rejected += 1;
                continue;
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
                    diagnostics.note_rejection(signal.head, SignalRejectionReason::OrderError);
                    signals_rejected += 1;
                    continue;
                }
            };

            execution_engine.record_fill(FillRecord {
                order_id: fill.position_id as i64,
                symbol: config.symbol.clone(),
                direction: signal.direction,
                requested_price: signal.entry,
                fill_price: fill.fill_price,
                slippage_pips: fill.slippage_pips,
                filled_at: fill.fill_time,
                retries: 0,
            });
            compliance.record_entry(signal, adjusted.inner(), lots, fill.fill_time);

            open_meta.insert(
                fill.position_id,
                PositionMeta {
                    head: signal.head,
                    direction: signal.direction,
                    regime: regime_signal.regime,
                    session,
                    entry_price: fill.fill_price,
                    initial_stop_loss: signal.stop_loss,
                    stop_loss: signal.stop_loss,
                    take_profit: signal.take_profit,
                    lots,
                    initial_lots: lots,
                    risk_pct: adjusted.inner(),
                    pyramid_level: signal.pyramid_level,
                    entry_commission: fill.commission,
                    realized_exit_pnl: Decimal::ZERO,
                    realized_exit_commission: Decimal::ZERO,
                    opened_at: bar.timestamp,
                    mfe: Decimal::ZERO,
                    partial_taken: false,
                    breakeven_set: false,
                    trailing_active: false,
                },
            );
            diagnostics.note_execution(signal.head, regime_signal.regime, session);
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
                finalize_closed_position(
                    config,
                    &mut account,
                    &mut broker,
                    &mut daily_pnl,
                    &mut equity_curve_filter,
                    &mut drift_detector,
                    &mut performance_ledger,
                    &mut trade_results,
                    &mut trade_log,
                    meta,
                    &cr,
                    "EndOfRun".to_string(),
                );
            }
        }
    }

    let stats = BacktestStats::compute(&trade_results, config.starting_balance);

    Ok(EngineResult {
        stats,
        trades: trade_results,
        trade_log,
        equity_curve,
        bars_processed: bars.len(),
        signals_generated,
        signals_rejected,
        diagnostics,
    })
}

#[allow(clippy::too_many_arguments)]
fn manage_open_positions(
    config: &EngineConfig,
    broker: &mut MockBroker,
    open_meta: &mut HashMap<u64, PositionMeta>,
    trade_manager: &mut TradeManager,
    current_atr: Decimal,
    bid: Decimal,
    ask: Decimal,
    timestamp: i64,
    account: &mut AccountState,
    daily_pnl: &mut DailyPnlEngine,
    equity_curve_filter: &mut EquityCurveFilter,
    drift_detector: &mut DriftDetector,
    performance_ledger: &mut PerformanceLedger,
    trade_results: &mut Vec<TradeResult>,
    trade_log: &mut Vec<EngineTradeLog>,
) {
    let position_ids: Vec<u64> = open_meta.keys().copied().collect();

    for position_id in position_ids {
        let Some(snapshot) = open_meta.get(&position_id).cloned() else {
            continue;
        };

        let current_price = match snapshot.direction {
            Direction::Buy => bid,
            Direction::Sell => ask,
        };
        let mut open_position = OpenPosition {
            id: position_id,
            entry: snapshot.entry_price,
            current_price,
            sl: snapshot.stop_loss,
            tp: snapshot.take_profit,
            tp2: None,
            lots: snapshot.lots,
            direction: snapshot.direction,
            opened_at: snapshot.opened_at,
            head: snapshot.head,
            max_favorable_excursion: snapshot.mfe,
            partial_taken: snapshot.partial_taken,
            breakeven_set: snapshot.breakeven_set,
            trailing_active: snapshot.trailing_active,
        };

        let actions = trade_manager.manage_position(&mut open_position, timestamp, current_atr);
        sync_position_meta(open_meta.get_mut(&position_id), &open_position);

        for action in actions {
            match action {
                TradeAction::MoveSl { new_sl } => {
                    if broker
                        .modify_position(&ModifyRequest {
                            position_id,
                            new_sl: Some(new_sl),
                            new_tp: None,
                        })
                        .is_ok()
                    {
                        if let Some(meta) = open_meta.get_mut(&position_id) {
                            meta.stop_loss = new_sl;
                            meta.breakeven_set = open_position.breakeven_set;
                            meta.trailing_active = open_position.trailing_active;
                            meta.mfe = open_position.max_favorable_excursion;
                        }
                    }
                }
                TradeAction::ClosePartial { pct } => {
                    let Some(meta) = open_meta.get(&position_id) else {
                        break;
                    };
                    let lots_to_close = (meta.lots * pct).round_dp(2);
                    if lots_to_close <= Decimal::ZERO || lots_to_close >= meta.lots {
                        continue;
                    }

                    if let Ok(close_report) = broker.close_position(&CloseRequest {
                        position_id,
                        lots: Some(lots_to_close),
                    }) {
                        if let Some(meta) = open_meta.get_mut(&position_id) {
                            meta.lots -= close_report.closed_lots;
                            meta.partial_taken = true;
                            meta.realized_exit_pnl += close_report.pnl;
                            meta.realized_exit_commission += close_report.commission;
                        }
                        if let Ok(info) = broker.account_info() {
                            account.update_equity(info.equity);
                            daily_pnl.update(info.equity, timestamp);
                        }
                    }
                }
                TradeAction::CloseAll { reason } => {
                    if let Ok(close_report) = broker.close_position(&CloseRequest {
                        position_id,
                        lots: None,
                    }) {
                        if let Some(meta) = open_meta.remove(&position_id) {
                            finalize_closed_position(
                                config,
                                account,
                                broker,
                                daily_pnl,
                                equity_curve_filter,
                                drift_detector,
                                performance_ledger,
                                trade_results,
                                trade_log,
                                meta,
                                &close_report,
                                reason,
                            );
                        }
                    }
                    break;
                }
                TradeAction::NoAction => {}
            }
        }
    }
}

fn sync_position_meta(meta: Option<&mut PositionMeta>, position: &OpenPosition) {
    let Some(meta) = meta else {
        return;
    };

    meta.stop_loss = position.sl;
    meta.lots = position.lots;
    meta.mfe = position.max_favorable_excursion;
    meta.partial_taken = position.partial_taken;
    meta.breakeven_set = position.breakeven_set;
    meta.trailing_active = position.trailing_active;
}

#[allow(clippy::too_many_arguments)]
fn finalize_closed_position(
    config: &EngineConfig,
    account: &mut AccountState,
    broker: &mut MockBroker,
    daily_pnl: &mut DailyPnlEngine,
    equity_curve_filter: &mut EquityCurveFilter,
    drift_detector: &mut DriftDetector,
    performance_ledger: &mut PerformanceLedger,
    trade_results: &mut Vec<TradeResult>,
    trade_log: &mut Vec<EngineTradeLog>,
    meta: PositionMeta,
    close_report: &gadarah_broker::CloseReport,
    close_reason: String,
) {
    let net_pnl = meta.realized_exit_pnl + close_report.pnl
        - meta.entry_commission
        - meta.realized_exit_commission
        - close_report.commission;
    let dollar_risk = initial_dollar_risk(config, &meta);
    let r_multiple = if dollar_risk > Decimal::ZERO {
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
        daily_pnl.update(info.equity, close_report.close_time);
    }

    equity_curve_filter.record_trade_close(account.current_equity);
    drift_detector.record_trade(gadarah_risk::TradeResult {
        won: is_winner,
        r_multiple,
        slippage_pips: close_report.slippage_pips,
    });
    performance_ledger.record_trade(
        meta.head,
        meta.regime,
        meta.session,
        is_winner,
        r_multiple,
        close_report.close_time,
    );

    trade_results.push(TradeResult {
        head: meta.head,
        pnl: net_pnl,
        r_multiple,
        opened_at: meta.opened_at,
        closed_at: close_report.close_time,
        is_winner,
    });
    trade_log.push(EngineTradeLog {
        symbol: config.symbol.clone(),
        direction: meta.direction,
        head: meta.head,
        regime: meta.regime,
        session: meta.session,
        entry_price: meta.entry_price,
        stop_loss: meta.initial_stop_loss,
        take_profit: meta.take_profit,
        lots: meta.initial_lots,
        risk_pct: meta.risk_pct,
        pyramid_level: meta.pyramid_level,
        opened_at: meta.opened_at,
        closed_at: close_report.close_time,
        close_price: close_report.close_price,
        pnl: net_pnl,
        r_multiple,
        close_reason,
        slippage_pips: close_report.slippage_pips,
    });
}

fn initial_dollar_risk(config: &EngineConfig, meta: &PositionMeta) -> Decimal {
    let sl_pips = (meta.entry_price - meta.initial_stop_loss).abs() / config.pip_size;
    sl_pips * config.pip_value_per_lot * meta.initial_lots
}

fn synthetic_transition_regime(timestamp: i64) -> RegimeSignal9 {
    RegimeSignal9 {
        regime: Regime9::Transitioning,
        confidence: Decimal::ZERO,
        adx: Decimal::ZERO,
        hurst: Decimal::ZERO,
        atr_ratio: Decimal::ZERO,
        bb_width_pctile: Decimal::ZERO,
        choppiness_index: Decimal::ZERO,
        computed_at: timestamp,
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct ObservedSignalCounts {
    open_signals: usize,
    close_signals: usize,
}

#[derive(Debug, Clone, Default)]
struct ObservedSignals {
    open_signals: usize,
    close_signals: usize,
    by_segment: HashMap<(HeadId, Regime9, Session), ObservedSignalCounts>,
}

fn advance_heads(
    bar: &Bar,
    heads: &mut [Box<dyn Head>],
    session_profile: &SessionProfile,
    regime_signal: Option<&RegimeSignal9>,
) -> ObservedSignals {
    let Some(regime_signal) = regime_signal else {
        return ObservedSignals::default();
    };

    let mut observed = ObservedSignals::default();
    for head in heads.iter_mut() {
        for signal in head.evaluate(bar, session_profile, regime_signal) {
            let counts = observed
                .by_segment
                .entry((signal.head, signal.regime, signal.session))
                .or_default();
            match signal.kind {
                SignalKind::Open => {
                    observed.open_signals += 1;
                    counts.open_signals += 1;
                }
                SignalKind::Close => {
                    observed.close_signals += 1;
                    counts.close_signals += 1;
                }
                _ => {}
            }
        }
    }
    observed
}

#[cfg(test)]
mod tests {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    use super::*;
    use gadarah_core::{Timeframe, TradeSignal};

    #[derive(Debug)]
    struct CountingHead {
        calls: Arc<AtomicUsize>,
    }

    impl Head for CountingHead {
        fn id(&self) -> HeadId {
            HeadId::Momentum
        }

        fn evaluate(
            &mut self,
            _bar: &Bar,
            _session: &SessionProfile,
            _regime: &RegimeSignal9,
        ) -> Vec<TradeSignal> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Vec::new()
        }

        fn reset(&mut self) {}

        fn warmup_bars(&self) -> usize {
            0
        }

        fn regime_allowed(&self, _regime: &RegimeSignal9) -> bool {
            true
        }
    }

    fn flat_bar(timestamp: i64, close: Decimal) -> Bar {
        Bar {
            open: close,
            high: close,
            low: close,
            close,
            volume: 0,
            timestamp,
            timeframe: Timeframe::M15,
        }
    }

    #[test]
    fn heads_advance_during_regime_warmup() {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut heads: Vec<Box<dyn Head>> = vec![Box::new(CountingHead {
            calls: Arc::clone(&calls),
        })];
        let bars: Vec<Bar> = (0..64)
            .map(|i| flat_bar(i64::from(i) * 900, dec!(1.1000)))
            .collect();

        let result = run_engine(&bars, &mut heads, &EngineConfig::default()).unwrap();

        assert_eq!(result.diagnostics.bars_without_regime, bars.len());
        assert_eq!(calls.load(Ordering::SeqCst), bars.len());
    }
}
