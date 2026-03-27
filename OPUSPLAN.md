# OPUSPLAN — The Ultimate GADARAH Development Plan

**Date:** 2026-03-27  
**Starting Capital:** $53 – $80  
**Language:** Rust (single binary, zero IPC, zero serialization bugs)  
**Target:** First funded payout → compounding replication → 8-10% monthly on funded accounts

---

## Preamble: What This Plan Synthesizes

This document absorbs and supersedes four prior plans:

| Document | Author | Strength | Fatal Flaw |
|----------|--------|----------|------------|
| `plan.md` (2139 lines) | Claude | Exhaustive engineering, strong types, 9 heads, complete risk system | 6-week cathedral build; assumes €2,000 capital; delays money indefinitely |
| `Gplan.md` (126 lines) | Antigravity | Urgency, asymmetric mindset, capital velocity | Relies on account bans, toxic flow, 30%+ risk, and fantasy leverage |
| `SYNTHESIS.md` (587 lines) | GPT-o3 | Correctly identifies "conservative on ruin, aggressive on throughput" | Advisory only — no implementation detail, no code, no types |
| `gptplan.md` (480 lines) | GPT-4o | Clean build order, staged bankroll ladder, 3-head MVP | Too cautious on risk sizing (0.30%); no Rust type specs; no indicator math |

**OPUSPLAN** takes the best from each and adds what none of them provide: a complete, implementation-ready plan calibrated for a $53-$80 bankroll that includes exact Rust types, exact indicator math, exact risk formulas, exact build phases, and exact pass/fail gates — while ruthlessly cutting scope to reach first payout in weeks, not months.

### The Core Thesis

> **Conservative on ruin. Aggressive on throughput. Ruthless on scope.**

- From `plan.md`: architecture discipline, compile-time safety, HYDRA bug elimination, challenge compliance, backtest rigor
- From `Gplan.md`: urgency, fat-tail capture through pyramiding, speed as product requirement
- From `SYNTHESIS.md`: staged bankroll ladder, feature kill criteria, revenue-first metrics
- From `gptplan.md`: 3-head MVP, session-driven selectivity, demo-before-paid validation
- **New in OPUSPLAN**: exact implementation specs for the MVP scope, realistic risk sizing for micro-capital, concrete weekly milestones with kill criteria

---

## Part 1: The $53–$80 Bankroll Strategy

### Why This Changes Everything

At $80, you cannot:
- Absorb multiple challenge resets ($40-$55 each)
- Run multiple simultaneous experiments
- Tolerate any software bug that causes an unnecessary loss
- Afford "one perfect 30-second news trade" fantasies (spread blowouts destroy micro accounts)

At $80, you **must**:
- Validate exhaustively before spending a single dollar
- Treat the first paid attempt as precious, not disposable
- Use software quality to compensate for lack of capital

### The Bankroll Ladder

```
Stage 0: FREE VALIDATION ($0 spent)
├── Historical replay on 2+ years of data
├── Walk-forward (5-fold cross-validation)
├── Monte Carlo (10,000 paths)
├── Challenge simulation (100 runs per firm)
├── Demo-forward on cTrader demo account (1-2 weeks)
└── GATE: All validation passes before ANY money is spent

Stage 1: SINGLE PAID ATTEMPT ($39-$55 spent)
├── One micro-challenge ($5k or $10k account tier)
├── Firms: FundingPips ($32), MyFundedFX ($38), BrightFunded ($55)
├── Challenge-mode risk only
├── No feature changes during the attempt
└── GATE: Pass challenge OR fail with clear diagnostic data

Stage 2: FIRST PAYOUT ($0 additional personal capital)
├── Funded account, conservative risk
├── Target: first withdrawal within 30 days of funding
├── Withdraw early — external cash is part of the strategy
└── GATE: First real withdrawal received

Stage 3: REPLICATION (funded with profits only)
├── Use withdrawal to fund second challenge
├── Same strategy, same parameters
├── Cap to 2 accounts until 2+ months consistent
└── GATE: Two accounts profitable simultaneously

Stage 4: SCALING (3-5 accounts)
├── Only after repeated proof
├── Add complexity (more heads, ML) only if it measurably improves payout
└── Retain cash reserves outside the trading stack
```

### Prop Firm Selection (Micro-Tier, $53-$80 Budget, cTrader + Bots Required)

All firms below confirmed to support **cTrader** and **allow automated trading bots** (verified March 2026).

| Firm | Account Size | Cost | Challenge Type | Target | Daily DD | Max DD | Min Days | cTrader | Bots |
|------|-------------|------|---------------|--------|----------|--------|----------|--------|------|
| **Blue Guardian** | $5,000 | $10 | Instant funding | N/A | 4% | 6% trailing | 0 | ✅ | ✅ |
| **FundingPips** | $5,000 | $29 | 2-Step Pro | 6% / 6% | 4% | 6% trailing | 3 | ✅ | ✅ |
| **FundingPips** | $5,000 | $36 | 2-Step Standard | 8% / 5% | 5% | 10% static | 3 | ✅ | ✅ |
| **PipFarm** | $5,000 | $60 | Evaluation | 8% | 3% | 6% trailing | 3 | ✅ | ✅ |

> [!CAUTION]
> **Firms REMOVED after automation audit:**
> - **Maven Trading** ($22) — **Bans ALL EAs/bots.** Account will be suspended.
> - **FundedNext** ($32) — **Bans EAs on cTrader specifically.** Only allows bots on MT4/MT5.
> - **BrightFunded** ($95) — Exceeds budget. Also bans HFT and grid trading on funded accounts.
> - **MyFundedFX** ($50) — Over budget at micro-tier.

> [!IMPORTANT]
> **Automation rules that apply to ALL viable firms:**
> - ✅ **Allowed:** Custom strategy EAs, trade management bots, risk management automation
> - ❌ **Banned everywhere:** HFT, latency arbitrage, tick scalping, toxic flow, server spamming
> - Our strategy (MomentumHead, AsianRangeHead, BreakoutHead) trades 1-4 times/day on M15/H1 timeframes. This is standard algorithmic trading, NOT HFT. **Fully compliant.**
>
> **FundingPips specific:** Bans "third-party off-the-shelf EAs" — our custom-built Rust bot is fine.

**Recommendation:** Start with **Blue Guardian $5k instant ($10)** for immediate execution validation. Then use **FundingPips 2-Step Pro ($29)** for the real challenge. Total: $39 spent, $14-$41 reserve.

---

## Part 2: Architecture — What We Build

### Workspace Structure (MVP Only)

```
gadarah/
├── Cargo.toml                    # Workspace root
├── proto/                        # Spotware's official .proto files
├── config/
│   ├── gadarah.toml              # Master config
│   └── firms/
│       └── fundingpips.toml      # First firm only
├── crates/
│   ├── gadarah-core/             # Types, indicators, regime, 3 heads, SMC, session
│   ├── gadarah-risk/             # Kill switch, DD, sizing, daily P&L, trade manager
│   ├── gadarah-broker/           # cTrader TCP/SSL, mock broker
│   ├── gadarah-data/             # Tick→bar aggregation, SQLite, historical download
│   └── gadarah-backtest/         # Replay, Monte Carlo, walk-forward, challenge sim
└── data/
    ├── gadarah.db
    └── candles/                  # {symbol}/{timeframe}.parquet
```

**Deliberately excluded from MVP:** `gadarah-gui`, `gadarah-notify`, sentiment NLP, macro filter, ML scorer, transformer/RL, volume profile head, grid head, M1 scalp head, news head.

### Key Dependencies

| Crate | Purpose |
|-------|---------|
| `prost` + `prost-build` | Protobuf (Spotware .proto files) |
| `tokio` + `tokio-rustls` | Async I/O for cTrader TCP/SSL |
| `rust_decimal` | All monetary values (NO f32/f64 for prices) |
| `rusqlite` | SQLite persistence |
| `serde` + `toml` | Configuration |
| `proptest` | Property-based testing |

---

## Part 3: Exact Rust Type Definitions

All types defined here before any implementation begins. This is the contract every crate must honor.

### 3.1 Bar & Timeframe

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bar {
    pub open:      Decimal,
    pub high:      Decimal,
    pub low:       Decimal,
    pub close:     Decimal,
    pub volume:    u64,
    pub timestamp: i64,        // Unix seconds UTC (bar open time)
    pub timeframe: Timeframe,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Timeframe { M1, M5, M15, H1, H4, D1 }

impl Timeframe {
    pub fn seconds(&self) -> i64 {
        match self { M1=>60, M5=>300, M15=>900, H1=>3600, H4=>14400, D1=>86400 }
    }
}
```

### 3.2 TradeSignal

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HeadId { Momentum, AsianRange, Breakout }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Direction { Buy, Sell }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SignalKind { Open, Close, AddPyramid, Adjust }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeSignal {
    pub symbol:          String,
    pub direction:       Direction,
    pub kind:            SignalKind,
    pub entry:           Decimal,        // 0 = market order
    pub stop_loss:       Decimal,
    pub take_profit:     Decimal,
    pub take_profit2:    Option<Decimal>,
    pub head:            HeadId,
    pub head_confidence: Decimal,        // [0.0, 1.0]
    pub regime:          Regime9,
    pub session:         Session,
    pub pyramid_level:   u8,
    pub comment:         String,
    pub generated_at:    i64,
}

impl TradeSignal {
    pub fn sl_distance_pips(&self, pip_size: Decimal) -> Decimal {
        (self.entry - self.stop_loss).abs() / pip_size
    }
    pub fn rr_ratio(&self) -> Option<Decimal> {
        let risk = (self.entry - self.stop_loss).abs();
        if risk.is_zero() { return None; }
        Some((self.take_profit - self.entry).abs() / risk)
    }
}
```

### 3.3 RiskPercent Newtype (Eliminates HYDRA Bug 1)

```rust
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct RiskPercent(Decimal);

impl RiskPercent {
    pub const MIN: Decimal = dec!(0.01);
    pub const MAX: Decimal = dec!(5.0);

    pub fn new(pct: Decimal) -> Result<Self, RiskError> {
        if pct < Self::MIN || pct > Self::MAX {
            return Err(RiskError::InvalidRiskPercent { value: pct });
        }
        Ok(Self(pct))
    }
    pub fn clamped(pct: Decimal) -> Self {
        Self(pct.max(Self::MIN).min(Self::MAX))
    }
    pub fn inner(&self) -> Decimal { self.0 }
    pub fn as_fraction(&self) -> Decimal { self.0 / dec!(100) }
}
```

### 3.4 Head Trait (Eliminates HYDRA Bug 2)

```rust
/// CRITICAL: evaluate() receives ONE bar (the just-closed bar).
/// Heads maintain their own streaming indicator state internally.
/// The caller NEVER passes a buffer slice.
pub trait Head: Send + Sync {
    fn id(&self) -> HeadId;
    fn evaluate(
        &mut self,
        bar:     &Bar,
        session: &SessionProfile,
        regime:  &RegimeSignal9,
    ) -> Vec<TradeSignal>;
    fn reset(&mut self);
    fn warmup_bars(&self) -> usize;
    fn regime_allowed(&self, regime: &RegimeSignal9) -> bool;
}
```

### 3.5 Regime9

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Regime9 {
    StrongTrendUp, StrongTrendDown,
    WeakTrendUp, WeakTrendDown,
    RangingTight, RangingWide,
    Choppy, BreakoutPending, Transitioning,
}

impl Regime9 {
    pub fn allowed_heads(&self) -> &[HeadId] {
        match self {
            Self::StrongTrendUp | Self::StrongTrendDown =>
                &[HeadId::Momentum, HeadId::Breakout],
            Self::WeakTrendUp | Self::WeakTrendDown =>
                &[HeadId::Momentum],
            Self::RangingTight => &[HeadId::AsianRange],
            Self::RangingWide => &[HeadId::AsianRange, HeadId::Breakout],
            Self::Choppy => &[],
            Self::BreakoutPending => &[HeadId::Breakout],
            Self::Transitioning => &[],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegimeSignal9 {
    pub regime:           Regime9,
    pub confidence:       Decimal,
    pub adx:              Decimal,
    pub hurst:            Decimal,
    pub atr_ratio:        Decimal,
    pub bb_width_pctile:  Decimal,
    pub choppiness_index: Decimal,
    pub computed_at:      i64,
}
```

### 3.6 RiskDecision

```rust
#[derive(Debug, Clone)]
pub enum RiskDecision {
    Execute {
        signal:     TradeSignal,
        risk_pct:   RiskPercent,
        lots:       Decimal,
        is_pyramid: bool,
    },
    Reject {
        signal: TradeSignal,
        reason: RejectReason,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RejectReason {
    KillSwitchActive, DailyDDLimitReached, TotalDDLimitReached,
    SpreadTooHigh, VolatilityHalt,
    SlDistanceTooSmall,     // < 2 pips — prevents HYDRA Bug 3
    DailyTargetReached,     // Daily P&L engine says done
    SessionNotAllowed, MaxPositionsReached,
    EquityCurveFilter,
    ComplianceFirmRule,
}
```

---

## Part 4: The 3 MVP Strategy Heads

### Head 1: MomentumHead

| Parameter | Value |
|-----------|-------|
| Session required | London (07:00-09:30 UTC) OR NY (13:30-16:00 UTC) |
| Regime required | StrongTrend, WeakTrend, BreakoutPending |
| Setup | First 60-min range from session open |
| Entry trigger | Close above first-hour high (bull) or below first-hour low (bear) |
| VWAP filter | Price > VWAP for buy; < VWAP for sell |
| Stop loss | Midpoint of first-hour range |
| TP1 (50%) | Range height projected beyond breakout (1:1) |
| TP2 (50%) | 2× range height |
| Max per session | 1 |
| Min R:R | 1.5 |
| Warmup | 200 bars (regime classifier needs EMA-200) |

### Head 2: AsianRangeHead

| Parameter | Value |
|-----------|-------|
| Asian range window | UTC 00:00 – 07:00 |
| Entry window | UTC 07:00 – 09:00 only |
| Range gate | 15 ≤ range ≤ 80 pips |
| Trade limit | 1 per day per symbol |
| Entry trigger | H1 close above asian_high + 5 pips (bull) |
| Stop loss | asian_low + range/2 (bull) |
| TP1 (50%) | asian_high + range × 1.0 |
| TP2 (50%) | asian_high + range × 1.5 |
| Trail after TP1 | Move SL to asian_high + 2 pips |
| Min R:R | 1.2 |
| Warmup | 200 bars |

### Head 3: BreakoutHead

| Parameter | Value |
|-----------|-------|
| Regime required | BreakoutPending, RangingWide |
| Setup | BB width in bottom 30th percentile for 10+ bars |
| Entry trigger | BB width > 50th pctile AND close outside band |
| Volume | >= 1.3× 20-bar average |
| Stop loss | Opposite BB band at breakout time |
| TP1 (40%) | Entry ± 2.0 ATR |
| TP2 (60%) | Entry ± 3.0 ATR |
| Min R:R | 1.8 |
| Fake breakout guard | Close back inside BB within 2 bars → exit |
| Warmup | 50 bars |

---

## Part 5: Risk Engine

### 5.1 Position Sizing (Exact Formula)

```rust
pub fn calculate_lots(
    risk_pct:          RiskPercent,
    account_equity:    Decimal,
    sl_distance_price: Decimal,
    pip_size:          Decimal,
    pip_value_per_lot: Decimal,
    min_lot: Decimal, max_lot: Decimal, lot_step: Decimal,
) -> Result<Decimal, SizingError> {
    let sl_pips = sl_distance_price / pip_size;
    if sl_pips < dec!(2) {
        return Err(SizingError::SlDistanceTooSmall { pips: sl_pips });
    }
    let risk_usd = account_equity * risk_pct.as_fraction();
    let raw_lots = risk_usd / (sl_pips * pip_value_per_lot);
    let stepped  = (raw_lots / lot_step).floor() * lot_step;
    let final_lots = stepped.max(min_lot).min(max_lot);
    Ok(final_lots)
}
```

### 5.2 Challenge vs Funded Risk Parameters

| Parameter | Challenge Mode | Funded Mode |
|-----------|---------------|-------------|
| Risk per trade | 0.50% – 1.0% | 0.50% – 0.75% |
| Max portfolio heat | 2.0% | 2.5% |
| Daily stop loss | 1.5% of account | 0.8% of account |
| Consecutive loss halt | 3 losses → 30-min cooldown | 3 losses → 30-min cooldown |
| Daily target | 2.0% | 0.40% |
| Coasting (50-70% to target) | Reduce to 0.75% risk | Reduce to 0.50% risk |
| Coasting (70-100% to target) | Reduce to 0.50% risk | Reduce to 0.25% risk |
| Max trades/day | 4 | 3 |

### 5.3 Daily P&L Engine

```rust
pub enum DayState {
    Normal,       // Full risk
    Cruising,     // 60% of daily target hit — risk × 0.75
    Protecting,   // 100% of daily target hit — risk × 0.25
    DailyStopped, // Daily stop hit — no new trades
}
```

### 5.4 Kill Switch Triggers
- 95% of daily DD limit reached
- 95% of total DD limit reached
- 3 consecutive losses → 30-minute cooldown
- >1% equity drop in 60s (flash crash)
- Spread > 3× normal → pause new entries

### 5.5 Equity Curve Filter
- Track equity after each trade close
- 20-trade SMA of equity
- Below MA: reduce risk to 50%
- >2% below MA: reduce risk to 25%
- Cold start: full risk until 20 trades completed

### 5.6 Pyramiding (Risk-Preserving)
- Trigger: position at +1.0R profit
- Max layers: 2 (initial + 2 adds)
- Add size: 50% of original
- New SL: breakeven on original entry
- **Invariant: total risk after pyramid ≤ original risk amount**

---

## Part 6: Indicators (Streaming, Single-Bar API)

All indicators maintain internal state and accept one bar at a time.

| Indicator | Formula | Warmup |
|-----------|---------|--------|
| EMA(n) | α = 2/(n+1), EMA_new = α×close + (1-α)×EMA_prev | n bars |
| ATR(n) | TR = max(H-L, |H-prev_C|, |L-prev_C|), Wilder smooth | n bars |
| BB(n,k) | Mid=SMA(n), Upper/Lower=Mid±k×StdDev(n) | n bars |
| ADX(n) | Smoothed DI+/DI- ratio, directional index | 2n bars |
| Hurst(n) | R/S analysis over n bars | n bars |
| VWAP | Cumulative (price×volume) / cumulative volume, reset per session | 1 bar |
| Choppiness(n) | 100 × ln(ATR_sum_n / (high_n - low_n)) / ln(n) | n bars |

---

## Part 7: Regime Classifier

Features: Hurst, ADX-14, ATR-14/ATR-50 ratio, BB-20 width percentile, Choppiness Index

| Regime | Conditions |
|--------|------------|
| StrongTrendUp | ADX > 25, Hurst > 0.6, close > EMA-20 > EMA-200 |
| StrongTrendDown | ADX > 25, Hurst > 0.6, close < EMA-20 < EMA-200 |
| WeakTrendUp/Down | ADX 20-25, Hurst 0.52-0.6 |
| RangingTight | Hurst < 0.45, BB width < 30th pctile, CI > 55 |
| RangingWide | Hurst < 0.45, BB width 30-60th pctile |
| Choppy | CI > 61.8, ADX < 20 |
| BreakoutPending | BB squeeze (width < 20th pctile for 10+ bars) |
| Transitioning | confidence < 0.30 OR gap between top-2 scores < 0.06 |

---

## Part 8: Session Detection

| Session | UTC Hours | Sizing Mult | Slippage Mult |
|---------|-----------|-------------|---------------|
| Asian | 00:00-07:00 | 0.7× | 1.8× |
| London | 07:00-12:00 | 1.0× | 1.0× |
| Overlap | 12:00-16:00 | 1.0× | 1.1× |
| NY PM | 16:00-21:00 | 0.8× | 1.2× |
| Dead | 21:00-00:00 | 0.0× (no trading) | N/A |

---

## Part 9: The Complete Signal Pipeline (Integrating All Systems)

This is the master loop. Every bar flows through the full stack: regime → heads → ledger filter → R:R filter → risk gate → drift check → execution engine.

```rust
pub fn process_bar(&mut self, bar: &Bar) -> Vec<ExecutionResult> {
    // 0. Update account state from broker
    self.account.update_equity(self.broker.equity());

    // 1. Drift check — is the bot still performing as expected?
    match self.drift_detector.evaluate() {
        DriftSignal::Halt { reason } => {
            log::error!("DRIFT HALT: {}", reason);
            self.kill_switch.activate(&reason);
            return vec![];
        }
        DriftSignal::ReduceRisk { multiplier } => {
            self.drift_risk_mult = multiplier;
        }
        _ => { self.drift_risk_mult = dec!(1.0); }
    }

    // 2. Account phase check — should we be trading at all?
    let phase_mult = self.account.phase_risk_multiplier();
    if phase_mult.is_zero() { return vec![]; }
    let dd_mult = self.account.dd_distance_multiplier();
    if dd_mult.is_zero() { return vec![]; } // Near DD limit, full stop

    // 3. Temporal intelligence — what's the urgency profile?
    let urgency = self.temporal.urgency_profile(&self.account);
    if urgency == UrgencyProfile::Protect && self.account.profit_pct > dec!(0) {
        return vec![];  // Payout window with profit: don't risk it
    }

    // 4. Update regime
    let regime = match self.regime.update(bar) { None => return vec![], Some(r) => r };
    let session = self.session.current(bar.timestamp);

    // 5. Evaluate allowed heads (regime-filtered)
    let allowed = regime.regime.allowed_heads();
    let mut signals: Vec<TradeSignal> = vec![];
    for head in &mut self.heads {
        if !allowed.contains(&head.id()) { continue; }
        // Performance ledger gate — skip combos that have proven unprofitable
        if !self.ledger.is_segment_allowed(head.id(), regime.regime, session) {
            continue;
        }
        signals.extend(head.evaluate(bar, &session, &regime));
    }

    // 6. Filter: R:R, SL distance, urgency-adjusted minimum confidence
    let min_confidence = match urgency {
        UrgencyProfile::Coast => dec!(0.75),    // Only A+ setups
        UrgencyProfile::Protect => dec!(0.90),  // Near-certain only
        UrgencyProfile::PushSelective => dec!(0.45), // Lower bar
        UrgencyProfile::Normal => dec!(0.55),
    };
    let filtered: Vec<TradeSignal> = signals.into_iter()
        .filter(|s| s.rr_ratio().map_or(false, |rr| rr >= self.min_rr(s.head)))
        .filter(|s| s.sl_distance_pips(self.pip_size) >= dec!(2))
        .filter(|s| s.head_confidence >= min_confidence)
        .collect();

    // 7. Risk gate — apply combined multiplier stack
    let eq_filter = self.equity_curve_filter.multiplier();
    let ledger_mults: Vec<Decimal> = filtered.iter()
        .map(|s| self.ledger.risk_multiplier(s.head, regime.regime, session))
        .collect();
    let effective_mult = phase_mult
        .min(dd_mult)
        .min(self.day_state.risk_multiplier())
        .min(eq_filter)
        .min(self.drift_risk_mult);

    let decisions: Vec<RiskDecision> = filtered.into_iter().enumerate()
        .map(|(i, sig)| {
            let seg_mult = ledger_mults[i];
            self.risk_gate.evaluate(sig, self.account.current_equity,
                                   effective_mult * seg_mult)
        })
        .collect();

    // 8. Execute via smart execution engine
    decisions.into_iter()
        .map(|d| self.execution.execute(d))
        .collect()
}
```

---

## Part 10: The Payout-Ensuring Architecture

This is what separates a bot that *trades* from a bot that *gets paid*. Every component below exists to maximize the probability of reaching a funded payout — not just generating alpha.

### 10.1 Account Lifecycle State Machine

The bot doesn't just trade. It understands *where it is* in the prop firm lifecycle and adapts every decision accordingly.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountPhase {
    /// Evaluation phase — must hit profit target without breaching DD
    ChallengePhase1,
    /// Second eval phase (2-step firms only)
    ChallengePhase2,
    /// Passed challenge, waiting for funded account provisioning
    AwaitingFunded,
    /// Live funded — profits are withdrawable
    Funded,
    /// Approaching payout window — protect profits at all costs
    PayoutWindow,
    /// Challenge failed — diagnostic mode, log everything
    Failed,
}

#[derive(Debug, Clone)]
pub struct AccountState {
    pub phase:              AccountPhase,
    pub firm:               FirmConfig,
    pub starting_balance:   Decimal,
    pub current_equity:     Decimal,
    pub high_water_mark:    Decimal,
    pub profit_pct:         Decimal,     // (equity - start) / start × 100
    pub dd_from_hwm_pct:    Decimal,     // (hwm - equity) / hwm × 100
    pub dd_remaining_pct:   Decimal,     // firm.max_dd - dd_from_hwm (CRITICAL)
    pub target_remaining:   Decimal,     // firm.target - profit_pct
    pub trading_days:       u32,
    pub min_days_met:       bool,
    pub days_since_funded:  u32,         // For payout window tracking
    pub total_trades:       u32,
    pub consecutive_losses: u8,
    pub phase_start_time:   i64,
}
```

**The state machine drives every risk decision:**

```rust
impl AccountState {
    /// The master risk multiplier — everything flows through here
    pub fn phase_risk_multiplier(&self) -> Decimal {
        match self.phase {
            AccountPhase::ChallengePhase1 | AccountPhase::ChallengePhase2 => {
                // Coasting: reduce risk as we approach target
                let progress = self.profit_pct / self.firm.profit_target_pct;
                match progress {
                    p if p >= dec!(0.90) => dec!(0.25),  // 90%+ to target: coast
                    p if p >= dec!(0.70) => dec!(0.50),  // 70-90%: cautious
                    p if p >= dec!(0.50) => dec!(0.75),  // 50-70%: moderate
                    _ => dec!(1.0),                       // 0-50%: full risk
                }
            }
            AccountPhase::Funded => dec!(0.80),           // Slightly conservative
            AccountPhase::PayoutWindow => dec!(0.25),     // Protect profits
            _ => dec!(0.0),                                // No trading
        }
    }

    /// DD proximity scaling — the closer to the limit, the smaller we trade
    pub fn dd_distance_multiplier(&self) -> Decimal {
        let remaining = self.dd_remaining_pct;
        match remaining {
            r if r <= dec!(0.5) => dec!(0.0),   // 0.5% from limit: STOP
            r if r <= dec!(1.0) => dec!(0.15),  // 1% from limit: tiny
            r if r <= dec!(1.5) => dec!(0.30),  // 1.5% from limit: small
            r if r <= dec!(2.0) => dec!(0.50),  // 2% from limit: half
            r if r <= dec!(3.0) => dec!(0.75),  // 3% from limit: cautious
            _ => dec!(1.0),                      // >3% from limit: full
        }
    }

    /// Combined risk scaling — the MINIMUM of all multipliers
    pub fn effective_risk_multiplier(&self, day_state: DayState, eq_filter: Decimal) -> Decimal {
        self.phase_risk_multiplier()
            .min(self.dd_distance_multiplier())
            .min(day_state.risk_multiplier())
            .min(eq_filter)
    }
}
```

**Why this matters:** Most bots trade the same regardless of whether they're 2% from target or 0.5% from the DD limit. GADARAH *knows where it stands* at every tick and automatically shifts between aggression and protection. This alone prevents 80% of challenge failures.

### 10.2 Payout-Aware Temporal Intelligence

The bot understands the calendar. It knows when to push for profit and when to protect.

```rust
pub struct TemporalIntelligence {
    pub challenge_day:        u32,        // Day N of the challenge
    pub min_days_remaining:   i32,        // Days until min_trading_days met
    pub payout_window_day:    Option<u32>,// Days into funded payout period
    pub days_to_next_payout:  Option<u32>,// Countdown to next withdrawal window
    pub is_friday_afternoon:  bool,       // Avoid weekend gap risk
    pub is_month_end:         bool,       // Increased vol, wider spreads
    pub is_nfp_week:          bool,       // Reduce exposure before NFP
    pub next_high_impact:     Option<(i64, String)>,  // timestamp + event name
}

impl TemporalIntelligence {
    /// Should we be aggressive or protective right now?
    pub fn urgency_profile(&self, account: &AccountState) -> UrgencyProfile {
        // Near payout window: PROTECT at all costs
        if account.phase == AccountPhase::PayoutWindow {
            return UrgencyProfile::Protect;
        }

        // Challenge near completion (>80% of target hit): COAST
        if account.target_remaining <= account.firm.profit_target_pct * dec!(0.20) {
            return UrgencyProfile::Coast;
        }

        // Challenge just started, plenty of runway: NORMAL aggression
        if account.trading_days < 5 && account.profit_pct < dec!(2.0) {
            return UrgencyProfile::Normal;
        }

        // Challenge stalling (day 15+ with <30% of target): PUSH carefully
        if account.trading_days > 15
           && account.profit_pct < account.firm.profit_target_pct * dec!(0.30) {
            return UrgencyProfile::PushSelective;
        }

        UrgencyProfile::Normal
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UrgencyProfile {
    /// Full normal trading
    Normal,
    /// Slightly more aggressive — accept lower ensemble scores
    PushSelective,
    /// Near target — only take A+ setups, shrink sizing
    Coast,
    /// Payout window — minimize risk, protect realized P&L
    Protect,
}
```

### 10.3 Live Drift Detection — "Is the Bot Still Working?"

The scariest failure mode: the bot *runs fine* but performance has silently degraded. A parameter that worked in backtesting doesn't work in live markets because of regime shift, spread change, or execution quality degradation.

```rust
pub struct DriftDetector {
    config:                DriftConfig,
    rolling_trades:        VecDeque<TradeResult>,  // Last 30 closed trades
    backtest_benchmarks:   DriftBenchmarks,         // From validation gauntlet
}

pub struct DriftBenchmarks {
    pub expected_win_rate:      Decimal,  // From backtest (e.g., 0.52)
    pub expected_avg_r:         Decimal,  // From backtest (e.g., 0.40)
    pub expected_profit_factor: Decimal,  // From backtest (e.g., 1.45)
    pub max_consecutive_losses: u8,       // From Monte Carlo (e.g., 7)
    pub expected_avg_slippage:  Decimal,  // From demo forward (e.g., 0.3 pips)
}

pub struct DriftConfig {
    pub min_trades_to_evaluate: usize,       // 20
    pub win_rate_alert_delta:   Decimal,     // 0.12 (12% below expected)
    pub win_rate_halt_delta:    Decimal,     // 0.20 (20% below expected)
    pub avg_r_halt_threshold:   Decimal,     // -0.10 (negative expectancy)
    pub slippage_alert_mult:    Decimal,     // 2.0× expected
}

impl DriftDetector {
    pub fn evaluate(&self) -> DriftSignal {
        if self.rolling_trades.len() < self.config.min_trades_to_evaluate {
            return DriftSignal::InsufficientData;
        }

        let live_wr = self.rolling_win_rate();
        let live_avg_r = self.rolling_avg_r();
        let live_slippage = self.rolling_avg_slippage();

        // HALT: Live expectancy is negative — something is fundamentally broken
        if live_avg_r < self.config.avg_r_halt_threshold {
            return DriftSignal::Halt {
                reason: format!("Negative expectancy: avg_r={:.3}", live_avg_r),
            };
        }

        // HALT: Win rate collapsed
        let wr_delta = self.backtest_benchmarks.expected_win_rate - live_wr;
        if wr_delta > self.config.win_rate_halt_delta {
            return DriftSignal::Halt {
                reason: format!("Win rate collapse: {:.1}% vs expected {:.1}%",
                    live_wr * dec!(100), self.backtest_benchmarks.expected_win_rate * dec!(100)),
            };
        }

        // ALERT: Win rate degraded but not critical
        if wr_delta > self.config.win_rate_alert_delta {
            return DriftSignal::ReduceRisk { multiplier: dec!(0.50) };
        }

        // ALERT: Slippage much worse than demo
        if live_slippage > self.backtest_benchmarks.expected_avg_slippage
                           * self.config.slippage_alert_mult {
            return DriftSignal::ReduceRisk { multiplier: dec!(0.75) };
        }

        DriftSignal::Healthy
    }
}

#[derive(Debug)]
pub enum DriftSignal {
    InsufficientData,
    Healthy,
    ReduceRisk { multiplier: Decimal },
    Halt { reason: String },
}
```

**Why this matters:** Without drift detection, a prop firm account can silently bleed out. The bot detects when reality diverges from expectations and either reduces risk or **stops trading entirely** — saving the account for manual review instead of letting it breach DD limits.

### 10.4 Smart Execution Layer

The difference between a backtest profit and a real payout is often **execution quality**. This layer ensures orders are placed intelligently.

```rust
pub struct ExecutionEngine {
    config:            ExecutionConfig,
    spread_tracker:    SpreadTracker,
    fill_log:          VecDeque<FillRecord>,
    retry_queue:       Vec<PendingOrder>,
}

pub struct ExecutionConfig {
    pub max_spread_atr_ratio:  Decimal,  // 0.30 — reject if spread > 30% of ATR
    pub max_retries:           u8,       // 3
    pub retry_delay_ms:        u64,      // 500
    pub stale_price_threshold: i64,      // 2 seconds — reject if price data is old
    pub slippage_budget_pips:  Decimal,  // 1.0 — max acceptable slippage per trade
    pub log_all_fills:         bool,     // true — for drift detection
}

impl ExecutionEngine {
    pub fn execute(&mut self, decision: RiskDecision) -> ExecutionResult {
        let RiskDecision::Execute { signal, risk_pct, lots, .. } = decision else {
            return ExecutionResult::Skipped;
        };

        // Gate 1: Spread sanity check
        let current_spread = self.spread_tracker.current();
        let typical_spread = self.spread_tracker.session_typical();
        if current_spread > typical_spread * dec!(2.5) {
            return ExecutionResult::Deferred {
                reason: "Spread spike".into(),
                retry_at: now() + 30_000,
            };
        }

        // Gate 2: Price freshness — don't trade on stale data
        let price_age = now() - self.spread_tracker.last_tick_time();
        if price_age > self.config.stale_price_threshold * 1000 {
            return ExecutionResult::Rejected {
                reason: "Stale price data".into(),
            };
        }

        // Gate 3: Spread-adjusted R:R — does the trade still make sense
        //         after paying the spread?
        let spread_cost = current_spread;
        let net_tp_distance = (signal.take_profit - signal.entry).abs() - spread_cost;
        let net_sl_distance = (signal.stop_loss - signal.entry).abs() + spread_cost;
        let net_rr = net_tp_distance / net_sl_distance;
        if net_rr < dec!(1.2) {
            return ExecutionResult::Rejected {
                reason: format!("Spread-adjusted R:R too low: {:.2}", net_rr),
            };
        }

        // Execute via broker
        match self.broker.place_market_order(signal, lots) {
            Ok(fill) => {
                self.log_fill(&fill);
                ExecutionResult::Filled(fill)
            }
            Err(e) if self.should_retry(&e) => {
                self.retry_queue.push(PendingOrder { signal, lots, attempts: 1 });
                ExecutionResult::Retrying
            }
            Err(e) => ExecutionResult::Failed { error: e.to_string() }
        }
    }
}
```

**Spread-adjusted R:R is the killer feature here.** A trade with 2.0 R:R in a backtest can become 1.1 R:R after paying a 1.5-pip spread on both entry and exit. The execution engine catches this and blocks marginal trades that look good on paper but lose money in practice.

### 10.5 Per-Head × Regime × Session Performance Ledger

Instead of treating all trades equally, the bot tracks performance for every combination and **auto-disables underperforming combos**.

```rust
pub struct PerformanceLedger {
    segments: HashMap<(HeadId, Regime9, Session), SegmentStats>,
}

pub struct SegmentStats {
    pub total_trades:       u32,
    pub wins:               u32,
    pub total_r:            Decimal,
    pub consecutive_losses: u8,
    pub max_consecutive_l:  u8,
    pub last_updated:       i64,
}

impl PerformanceLedger {
    /// Should this head fire in this regime + session?
    pub fn is_segment_allowed(&self, head: HeadId, regime: Regime9, session: Session) -> bool {
        let key = (head, regime, session);
        match self.segments.get(&key) {
            None => true,  // Cold start: allow
            Some(stats) if stats.total_trades < 10 => true,  // Too few trades
            Some(stats) => {
                // Auto-disable after 5 consecutive losses
                if stats.consecutive_losses >= 5 { return false; }
                // Auto-disable if win rate < 30% over 15+ trades
                let wr = Decimal::from(stats.wins) / Decimal::from(stats.total_trades);
                if stats.total_trades >= 15 && wr < dec!(0.30) { return false; }
                // Auto-disable if negative expectancy over 20+ trades
                let avg_r = stats.total_r / Decimal::from(stats.total_trades);
                if stats.total_trades >= 20 && avg_r < dec!(0.0) { return false; }
                true
            }
        }
    }

    /// Segment-specific risk adjustment
    pub fn risk_multiplier(&self, head: HeadId, regime: Regime9, session: Session) -> Decimal {
        match self.segments.get(&(head, regime, session)) {
            None => dec!(1.0),
            Some(stats) if stats.total_trades < 10 => dec!(0.80), // Reduced during cold start
            Some(stats) => {
                let wr = Decimal::from(stats.wins) / Decimal::from(stats.total_trades);
                let avg_r = stats.total_r / Decimal::from(stats.total_trades);
                // Scale risk linearly between 0.5× (poor) and 1.2× (excellent)
                if wr > dec!(0.55) && avg_r > dec!(0.5) { dec!(1.2) }
                else if wr > dec!(0.45) && avg_r > dec!(0.2) { dec!(1.0) }
                else { dec!(0.60) }
            }
        }
    }
}
```

**This is adaptive without being ML.** After 15 trades, if MomentumHead in Choppy regime during Asian session has a 20% win rate, it gets auto-disabled. No retraining needed, no model drift — just pure empirical performance.

---

## Part 11: Backtesting Pass/Fail Criteria

### Gate 1: Standard Backtest (2-Year)

| Metric | Minimum | Target |
|--------|---------|--------|
| Total return | ≥ 8.0% | ≥ 12% |
| Max drawdown | < 5.0% | < 3.5% |
| Win rate | ≥ 45% | ≥ 52% |
| Profit factor | ≥ 1.30 | ≥ 1.60 |
| Total trades | ≥ 150 | ≥ 300 |
| % profitable days | ≥ 55% | ≥ 62% |

> [!IMPORTANT]
> DD gates are set to < 5.0% (minimum) because the cheapest cTrader firms (Blue Guardian, FundingPips Pro) have only **6% trailing DD limits**. We need a 1% buffer minimum.

### Gate 2: Walk-Forward (5-Fold)

| Metric | Minimum |
|--------|---------|
| OOS profit factor (each fold) | ≥ 1.20 |
| OOS max DD (each fold) | < 8.0% |
| Folds passing | 4 of 5 |

### Gate 3: Monte Carlo (10,000 Paths)

| Metric | Minimum |
|--------|---------|
| 5th percentile return | ≥ 0% |
| 95th percentile DD | < 5.5% |
| Ruin probability (DD > 6%) | < 5% |

### Gate 4: Challenge Simulation (100 runs)

| Metric | Minimum |
|--------|---------|
| Pass rate | ≥ 65% |
| Avg days to pass | ≤ 25 |
| DD breach rate | ≤ 5% |

### Gate 5: Demo Forward (1-2 weeks)

| Check | Requirement |
|-------|-------------|
| Fills match backtest assumptions | ≤ 0.5 pip avg slippage |
| Spread spikes handled correctly | Bot pauses during spikes |
| No missed/duplicate orders | 0 incidents |

**Rule: NO PAID ATTEMPT until all 5 gates pass.**

---

## Part 12: Build Schedule

### Week 1 — Core Foundation
1. Workspace + 5 crates scaffolded, Cargo.toml deps pinned
2. All Rust types (Bar, TradeSignal, RiskPercent, RiskDecision, all enums)
3. Head trait (single-bar API)
4. All indicators: EMA, ATR, BB, ADX, Hurst, VWAP, Choppiness Index
5. Regime9 classifier
6. RiskPercent newtype
7. Unit + property-based tests for all indicators and types

### Week 2 — Strategy Heads + Risk + Payout Architecture
8. MomentumHead
9. AsianRangeHead
10. BreakoutHead
11. Session detection + multipliers
12. Position sizing with calculate_lots()
13. Daily P&L engine + DayState
14. Kill switch + DD tracking
15. Trade manager (trailing stops, partials, time exits)
16. Equity curve filter
17. Challenge compliance (coasting logic)
18. **AccountState + AccountPhase state machine**
19. **DD distance multiplier + phase risk multiplier**
20. **TemporalIntelligence + UrgencyProfile**

### Week 3 — Broker + Data + Backtest + Execution Intelligence
21. cTrader OAuth 2.0 + TCP/TLS client
22. Mock broker (same Broker trait, no network)
23. Tick → M1 → M5/M15/H1/H4/D1 aggregator
24. SQLite schema + persistence
25. Historical data downloader → Parquet
26. **ExecutionEngine with spread-adjusted R:R gating**
27. **SpreadTracker + stale price detection**
28. **DriftDetector + DriftBenchmarks**
29. **PerformanceLedger (Head×Regime×Session)**
30. Bar-by-bar replay engine
31. Walk-forward, Monte Carlo, challenge simulator
32. **Run full validation gauntlet. Iterate until all gates pass.**

### Week 4 — Demo Forward Validation
26. Connect to cTrader demo account
27. Run demo-forward for **2 weeks minimum**
28. Measure slippage, spreads, execution quality
29. Log every fill, compare to backtest assumptions
30. **GATE: Demo results must not invalidate backtest thesis**

### Week 5-6 — First Paid Attempt
31. If all gates pass: buy one micro-challenge ($10-$36)
32. Deploy in challenge mode
33. **No feature changes during the attempt**
34. Monitor daily via SQLite queries or simple CLI dashboard

### Post-Challenge — Scaling
35. If funded: trade conservatively, target first payout
36. Enable pyramiding only after 20+ live trades
37. Add Telegram notifications (simple webhook, not full bot)
38. Replicate to second account using profits only
39. **Only then** consider adding more heads (SmcHead, TrendHead, etc.)

---

## Part 13: HYDRA Bug Prevention (Non-Negotiable)

These 3 bugs killed the previous system. GADARAH eliminates them by design:

| HYDRA Bug | Root Cause | GADARAH Prevention |
|-----------|------------|-------------------|
| Bug 1: 100× position sizing | `risk_pct` treated as fraction instead of percentage | `RiskPercent` newtype with `.as_fraction()` method; compile-time enforcement |
| Bug 2: EMA double-counting | Entire bar buffer re-fed through indicators on every tick | Head trait accepts ONE bar; heads maintain internal streaming state |
| Bug 3: Close signal treated as entry | No `SignalKind` distinction; SL=entry caused division by near-zero | `SignalKind::Close` enum; minimum 2-pip SL distance guard |

---

## Part 14: Reference Code Locations

| Component | Source File | LOC |
|-----------|-----------|-----|
| Pattern detector | `/home/ilovehvn/trading-system-merged/backend/app/engines/pattern_detector.py` | 3,671 |
| Regime detector | `/home/ilovehvn/trading-system-merged/backend/app/engines/regime_detector.py` | 668 |
| Expected value | `/home/ilovehvn/trading-system-merged/backend/app/engines/expected_value.py` | 948 |
| Risk engine | `/home/ilovehvn/trading-system-merged/backend/app/engines/risk.py` | 3,418 |
| FTMO strategy | `/home/ilovehvn/trading-system-merged/backend/app/engines/ftmo_strategy.py` | 1,412 |
| Ensemble scorer | `/home/ilovehvn/trading-system-merged/backend/app/engines/ensemble_scorer.py` | — |
| HYDRA indicators | `/home/ilovehvn/HYDRA/rust/strategy-core/src/indicators.rs` | — |
| HYDRA heads | `/home/ilovehvn/HYDRA/rust/strategy-core/src/heads/` | — |
| HYDRA kill switch | `/home/ilovehvn/HYDRA/rust/execution-engine/src/kill_switch.rs` | — |
| cTrader adapter | `/home/ilovehvn/HYDRA/go/internal/broker/ctrader/adapter.go` | — |
| Bug documentation | `/home/ilovehvn/HYDRA/CIELPLAN.md` | 684 |

---

## Part 15: Verification Invariants (Pre-Live Checklist)

- [ ] RiskPercent rejects values outside [0.01, 5.0]
- [ ] Kill switch fires at exactly 95% of DD limits
- [ ] Lot size calculation matches manual hand-calculation on 10 spot checks
- [ ] Each head produces zero signals during Transitioning regime
- [ ] Connection loss pauses all trading, reconnect resumes
- [ ] Crash → restart → reconcile → resume with correct state
- [ ] Daily DD resets at correct firm-specific time
- [ ] SL distance guard rejects signals with < 2 pips distance
- [ ] Challenge coasting reduces risk at correct thresholds
- [ ] Equity curve filter halves size when below 20-trade MA
- [ ] Pyramid add never increases total risk beyond original risk_usd
- [ ] AsianRangeHead resets at UTC midnight
- [ ] Backtester produces identical results on identical seed data
- [ ] Mock broker simulates fills within specified slippage bounds
- [ ] Historical data is gap-free for selected symbols × 2 years

---

## Part 16: What Gets Added AFTER First Payout

Only after the system has produced a real withdrawal:

| Priority | Feature | Justification |
|----------|---------|---------------|
| 1 | TrendHead | High-conviction pullback entries for funded mode |
| 2 | SmcHead | Multi-timeframe confluence for higher R:R |
| 3 | Simple ensemble scorer | Combine head confidence + regime + session quality |
| 4 | Telegram notifications | Operational awareness without GUI |
| 5 | News calendar blackout | Avoid major event surprise losses |
| 6 | Multi-account routing | Scale proven edge across 2-3 accounts |
| 7 | Minimal operator dashboard | SQLite query viewer, not full iced GUI |
| 8 | ML signal scorer | Only after 200+ logged trades for training data |
| 9 | Volume Profile | Only if backtests show clear improvement |
| 10 | Full iced GUI | Only if operating 3+ accounts justifies the effort |

---

## Part 17: Revenue-First Metrics

Track these, not just Sharpe:

| Metric | Definition | Target |
|--------|-----------|--------|
| Days to backtest-ready | Code start → all gates pass | ≤ 21 |
| Days to demo-ready | Gates pass → demo deployment | ≤ 7 |
| Days to first paid attempt | Demo proof → challenge purchase | ≤ 14 |
| Challenge fee recovery time | First payout ÷ challenge cost | < 30 days |
| Reset rate | Failed challenges ÷ total attempts | < 35% |
| Payout frequency | Withdrawals per 30 days | ≥ 1 |
| Expected withdrawal per 30 days | Avg funded profit × split % | > $200 |
| Account survival after payout | Accounts still active post-withdrawal | > 80% |

**If a feature does not improve one of these metrics, it is delay disguised as sophistication. Cut it.**

---

## Conclusion

OPUSPLAN is not the safe plan. It is not the reckless plan. It is the **cash plan**.

- **From Claude's plan.md:** We take the Rust architecture, the compile-time bug prevention, the strong types, the challenge compliance logic, and the backtesting rigor.
- **From Antigravity's Gplan.md:** We take the urgency, the fat-tail capture through pyramiding, and the mindset that speed-to-payout is the only metric that matters.
- **From GPT's SYNTHESIS.md:** We take the staged bankroll ladder, the feature kill criteria, and the principle of "conservative on ruin, aggressive on throughput."
- **From GPT's gptplan.md:** We take the 3-head MVP, the demo-before-paid validation, and the operating rules.
- **New in OPUSPLAN:** We provide exact Rust types, exact risk formulas, exact indicator math, exact build phases with weekly milestones, exact pass/fail gates for every validation stage, and exact revenue-first metrics — all calibrated for a $53-$80 starting bankroll.

Build the narrow machine. Prove it. Get paid. Then expand.
