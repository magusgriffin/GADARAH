# GADARAH — The Ultimate Plan

**Date:** 2026-03-27
**Starting Capital:** $53 – $80
**Language:** Rust (single binary, zero IPC, zero serialization bugs)
**Target:** First funded payout in weeks, not months. Then compound via replication.

> **2026-04-16 due-diligence note:** This plan contains legacy firm comparisons
> that are no longer all valid for a U.S. `cTrader` bot workflow. Use
> [PROJECT_READINESS_2026-04-16.md](/home/ilovehvn/GADARAH/PROJECT_READINESS_2026-04-16.md)
> as the current source of truth for target selection and bot-friendliness.

---

## Preamble: What This Plan Absorbs

This document supersedes all prior plans. It takes the best from each and discards the rest.

| Document | What We Take | What We Reject |
|----------|-------------|----------------|
| **plan.md** (2139 lines, Claude) | Full 9-head specs with exact params, SMC engine, Volume Profile, Macro Filter, 20-feature ML, 28 patterns, Bayesian ensemble, GARCH, Kelly, Daily P&L engine, consistency tracker, equity curve filter, re-entry, pyramiding, complete Rust types, SQLite schema, config schema, verification invariants | 6-week cathedral build before any trading; assumes €2,000 capital; GUI on critical path; 7-account orchestration from day one; 2.0-2.5% challenge risk |
| **Gplan.md** (121 lines, Antigravity) | Urgency mindset, fat-tail capture through pyramiding, speed as product requirement, pre-allocated buffers for hot paths, capital velocity | Toxic flow, latency arb, 30% risk per trade, offshore burner accounts, anti-fingerprinting, account ban as business model, deep RL before proven edge |
| **gptplan.md** (479 lines, GPT-4o) | 3-head MVP, staged bankroll ladder, revenue-first metrics, feature kill criteria, operating rules, demo-before-paid validation | Too cautious at 0.30% risk; no Rust types; no indicator math; no implementation detail |
| **SYNTHESIS.md** (586 lines, GPT-o3) | "Conservative on ruin, aggressive on throughput" thesis, ruthless scope reduction | Advisory only — zero implementation detail |
| **OPUSPLAN.md** (1175 lines, Opus) | AccountPhase state machine, DD distance multiplier, TemporalIntelligence + UrgencyProfile, DriftDetector, ExecutionEngine with spread-adjusted R:R gating, PerformanceLedger (Head x Regime x Session), verified prop firm research, weekly milestones | Some overlap with plan.md types; missing full 9-head specs, SMC, VP, macro filter, ML features |

### The Core Thesis

> **Conservative on ruin. Aggressive on throughput. Ruthless on scope.**

The plan is organized in two phases:
- **Phase 1 (MVP):** 3 heads, 5 crates, no GUI, no ML, no sentiment. Goal: first paid payout.
- **Phase 2 (Expansion):** Remaining 7 heads, SMC engine, Volume Profile, Macro Filter, ML, sentiment, GUI, multi-account. Goal: compound and scale.

Phase 2 specs are included in full so nothing is lost — but they are clearly marked as POST-PAYOUT work.

---

## Part 1: The $53-$80 Bankroll Strategy

### Why This Changes Everything

At $80, every dollar spent on a failed challenge is 50-100% of your bankroll. You cannot absorb repeated failure. The software must compensate for the capital you don't have.

At $80, you MUST:
- Validate exhaustively (replay, walk-forward, Monte Carlo, challenge sim, demo) before spending a cent
- Treat the first paid attempt as precious, not disposable
- Use software quality and discipline to compensate for lack of capital

At $80, you CANNOT:
- Risk 30% of the account on one trade (Gplan.md fantasy)
- Buy 7 prop firm challenges simultaneously (plan.md fantasy)
- Absorb multiple challenge resets at $40-55 each
- Afford execution bugs, spread spikes, or software errors

### The Bankroll Ladder

```
Stage 0: FREE VALIDATION ($0 spent)
├── Historical replay on 2+ years of data
├── Walk-forward (5-fold cross-validation)
├── Monte Carlo (10,000 paths)
├── Challenge simulation (100 runs on The5ers Hyper Growth rules)
├── Demo-forward on cTrader demo account (1-2 weeks)
└── GATE: All 5 validation stages pass before ANY money is spent

Stage 1: SINGLE PAID ATTEMPT ($260+ spent)
├── One The5ers Hyper Growth $5k challenge
├── Challenge-mode risk only
├── No feature changes during the attempt
├── No new heads, no parameter tweaks, no "quick fixes"
└── GATE: Pass challenge OR fail with clear diagnostic data

Stage 2: FIRST PAYOUT ($0 additional personal capital)
├── Funded account, conservative risk
├── Target: first withdrawal within 30 days of funding
├── Withdraw early — external cash is part of the strategy
└── GATE: First real withdrawal received

Stage 3: REPLICATION (funded with profits only)
├── Use withdrawal to fund second challenge
├── Same strategy, same parameters, same symbols
├── Cap to 2 accounts until 2+ months consistent
└── GATE: Two accounts profitable simultaneously

Stage 4: SCALING (3-5 accounts)
├── Only after repeated proof
├── Add complexity (more heads, ML, GUI) only if measurably improves payout
├── Retain cash reserves outside the trading stack
└── Consider SmcHead, TrendHead, Volume Profile, ML scorer
```

### Challenge Target (Verified April 2026: The5ers Hyper Growth Rules)

| Firm | Account Size | Cost | Challenge Type | Target | Daily Guardrail | Max DD | Min Days | cTrader | EAs on cTrader |
|------|-------------|------|---------------|--------|----------|--------|----------|--------|----------------|
| **The5ers** | $5,000 | $260 | Hyper Growth | 10% | 3% daily pause | 6% static stopout | 0 | Y | Y |

> **The5ers Hyper Growth is now the primary optimization target for this repo.**
> - Strategy validation, challenge simulation, drawdown handling, and defaults are aligned to Hyper Growth.
> - It is cTrader-accessible and The5ers explicitly allows EAs, subject to the normal bans on HFT, arbitrage, emulators, and copy-trading of other people’s signals.

> **Hyper Growth constraints carried into the codebase:**
> - One-step / instant-style challenge with 10% target
> - 3% daily pause anchored to the higher of start-of-day balance or equity
> - 6% stopout below the initial account size
> - No minimum trading days
> - First payout 14 days after funded, then every 2 weeks

> **Selectable instant-style profiles in the repo:**
> - `config/firms/the5ers_hypergrowth.toml` — default, policy-clean target
> - `config/firms/fundingpips_1step.toml` — faster-payout candidate, now guarded by a FundingPips compliance manager
> - `config/firms/fundingpips_zero.toml` — funded-style trailing profile, now guarded by a FundingPips compliance manager
> - `config/firms/blueguardian.toml` — legacy instant profile already retained for comparison
> - `config/firms/alphaone.toml` — rules-only profile, not cTrader-EA compatible

> **FundingPips-specific compliance layer now expected in code:**
> - Reject scalping/news-style heads on FundingPips profiles
> - Block opposite-direction exposure on the same symbol
> - Enforce entry pacing / anti-HFT guardrails
> - Detect abnormal lot/risk jumps vs recent baseline behavior
> - Enforce FundingPips Zero same-trade-idea risk cap
> - Load blackout windows from local config so scheduled news windows are enforced in replay and live mode

**Recommendation:** Use **The5ers Hyper Growth $5k ($260)** as the default proving tier. Treat the other instant profiles as optional comparison modes, not the primary deployment target.

---

## Part 2: HYDRA Bug Prevention (Non-Negotiable)

These 3 bugs killed the previous system. GADARAH eliminates them by design. Every architectural decision flows from this.

| HYDRA Bug | Root Cause | GADARAH Prevention |
|-----------|------------|-------------------|
| **Bug 1: 100x position sizing** | `risk_pct` treated as fraction instead of percentage across Go-Rust boundary | `RiskPercent` newtype with `.as_fraction()` method; compile-time enforcement; no cross-language boundary |
| **Bug 2: EMA double-counting** | Entire bar buffer re-fed through indicators on every tick due to Go passing slices | `Head` trait accepts ONE bar; heads maintain internal streaming state; caller NEVER passes a buffer |
| **Bug 3: Close signal treated as entry** | No `SignalKind` distinction; SL=entry caused division by near-zero | `SignalKind::Close` enum variant; minimum 2-pip SL distance guard in `calculate_lots()` |

**Architecture consequence:** Single language (Rust), single binary, in-process function calls. No gRPC, no protobuf between internal components, no cross-language serialization. The only protobuf is for the cTrader external API.

---

## Part 3: Architecture

### Workspace Structure (MVP — Phase 1)

```
gadarah/
├── Cargo.toml                    # Workspace root
├── proto/                        # Spotware's official .proto files
├── config/
│   ├── gadarah.toml              # Master config
│   └── firms/
│       ├── blueguardian.toml
│       └── fundingpips.toml
├── crates/
│   ├── gadarah-core/             # Types, indicators, regime, 3 heads, session
│   ├── gadarah-risk/             # Kill switch, DD, sizing, daily P&L, trade manager,
│   │                             # equity curve filter, pyramiding, account state machine,
│   │                             # drift detector, performance ledger, consistency
│   ├── gadarah-broker/           # cTrader TCP/SSL, mock broker, execution engine
│   ├── gadarah-data/             # Tick->bar aggregation, SQLite, historical download
│   └── gadarah-backtest/         # Replay, Monte Carlo, walk-forward, challenge sim
└── data/
    ├── gadarah.db
    └── candles/                  # {symbol}/{timeframe}.parquet
```

**Deliberately excluded from Phase 1 MVP:**
- `gadarah-gui` (no GUI until 3+ funded accounts)
- `gadarah-notify` (simple webhook only, not full notification system)
- SMC engine (OB, FVG, BOS/ChoCH, liquidity)
- Volume Profile (VPOC, VAH/VAL, HVN/LVN)
- Macro Filter (DXY/VIX/yields)
- ML scorer (LightGBM/ONNX)
- Sentiment engine (FinBERT)
- Bayesian ensemble scorer
- GARCH volatility
- S/R confluence engine
- 28 candlestick patterns
- News calendar (beyond simple blackout)
- Correlation matrix / portfolio VaR
- Kelly criterion (use fixed risk sizing)
- TrendHead, GridHead, SmcHead, NewsHead, ScalpM1, ScalpM5, VolumeProfileHead

### Key Dependencies

| Crate | Purpose |
|-------|---------|
| `prost` + `prost-build` | Protobuf (Spotware .proto files) |
| `tokio` + `tokio-rustls` | Async I/O for cTrader TCP/SSL |
| `rust_decimal` | All monetary values (NO f32/f64 for prices) |
| `rusqlite` | SQLite persistence |
| `serde` + `toml` | Configuration |
| `proptest` | Property-based testing |
| `chrono` | UTC time handling |

---

## Part 4: Exact Rust Type Definitions

All types defined here before any implementation begins. This is the contract every crate must honor.

### 4.1 Bar & Timeframe

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bar {
    pub open:      Decimal,
    pub high:      Decimal,
    pub low:       Decimal,
    pub close:     Decimal,
    pub volume:    u64,        // tick count (forex tick volume)
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

### 4.2 TradeSignal

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HeadId {
    // Phase 1 MVP (3 heads)
    Momentum, AsianRange, Breakout,
    // Phase 2 expansion (7 heads — added AFTER first payout)
    Trend, Grid, Smc, News, ScalpM1, ScalpM5, VolProfile,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Direction { Buy, Sell }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SignalKind {
    Open,        // New position entry
    Close,       // Close existing position (HYDRA Bug 3 prevention)
    AddPyramid,  // Add to existing winning position
    ReEntry,     // Re-entry after temporary blocker cleared
    Adjust,      // Move SL/TP on existing position
}

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
    pub pyramid_level:   u8,             // 0=initial, 1/2=pyramid adds
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

### 4.3 RiskPercent Newtype (Eliminates HYDRA Bug 1)

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
    /// 1.5% -> 0.015. This is the ONLY way to get the fraction.
    pub fn as_fraction(&self) -> Decimal { self.0 / dec!(100) }
}
```

### 4.4 Head Trait (Eliminates HYDRA Bug 2)

```rust
/// CRITICAL: evaluate() receives ONE bar (the just-closed bar).
/// Heads maintain their own streaming indicator state internally.
/// The caller NEVER passes a buffer slice — this makes HYDRA's
/// indicator double-counting bug impossible by construction.
pub trait Head: Send + Sync {
    fn id(&self) -> HeadId;

    /// Process one new closed bar. Returns zero or more trade signals.
    /// INVARIANT: Must be called exactly once per closed bar, in order.
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

### 4.5 Regime9

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Regime9 {
    StrongTrendUp, StrongTrendDown,
    WeakTrendUp, WeakTrendDown,
    RangingTight, RangingWide,
    Choppy, BreakoutPending, Transitioning,
}

impl Regime9 {
    /// Phase 1 MVP: only 3 heads
    pub fn allowed_heads(&self) -> &[HeadId] {
        match self {
            Self::StrongTrendUp | Self::StrongTrendDown =>
                &[HeadId::Momentum, HeadId::Breakout],
            Self::WeakTrendUp | Self::WeakTrendDown =>
                &[HeadId::Momentum],
            Self::RangingTight => &[HeadId::AsianRange],
            Self::RangingWide => &[HeadId::AsianRange, HeadId::Breakout],
            Self::Choppy => &[],                    // No trading in chop
            Self::BreakoutPending => &[HeadId::Breakout],
            Self::Transitioning => &[],             // No new trades during uncertainty
        }
    }

    pub fn is_trending(&self) -> bool {
        matches!(self,
            Self::StrongTrendUp | Self::StrongTrendDown |
            Self::WeakTrendUp | Self::WeakTrendDown)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegimeSignal9 {
    pub regime:           Regime9,
    pub confidence:       Decimal,     // [0.0, 1.0]
    pub adx:              Decimal,
    pub hurst:            Decimal,
    pub atr_ratio:        Decimal,     // ATR14 / ATR50
    pub bb_width_pctile:  Decimal,     // percentile in [0,1]
    pub choppiness_index: Decimal,
    pub computed_at:      i64,
}
```

### 4.6 RiskDecision

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
    KillSwitchActive,
    DailyDDLimitReached,
    TotalDDLimitReached,
    SpreadTooHigh,
    VolatilityHalt,
    SlDistanceTooSmall,     // < 2 pips — prevents HYDRA Bug 3
    DailyTargetReached,     // Daily P&L engine says done
    SessionNotAllowed,
    MaxPositionsReached,
    EquityCurveFilter,      // Below 20-trade equity MA
    ComplianceFirmRule,
    ConsecutiveLossHalt,    // 3 losses -> cooldown
    DriftDetectorHalt,      // Live performance diverged from backtest
    PerformanceLedgerBlock, // This head+regime+session combo has negative history
    RrTooLowAfterSpread,   // Spread-adjusted R:R below minimum
    StalePriceData,        // Price data older than 2 seconds
}
```

### 4.7 Session

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Session {
    Asian,    // UTC 00:00-07:00
    London,   // UTC 07:00-12:00
    Overlap,  // UTC 12:00-16:00
    NyPm,     // UTC 16:00-21:00
    Dead,     // UTC 21:00-00:00
}

#[derive(Debug, Clone)]
pub struct SessionProfile {
    pub session:         Session,
    pub sizing_mult:     Decimal,    // Asian 0.7, London 1.0, Overlap 1.0, NyPm 0.8, Dead 0.0
    pub slippage_mult:   Decimal,    // Asian 1.8, London 1.0, Overlap 1.1, NyPm 1.2
    pub min_rr_override: Option<Decimal>,
}

impl Session {
    pub fn from_utc_hour(h: u8) -> Self {
        match h {
            0..=6   => Self::Asian,
            7..=11  => Self::London,
            12..=15 => Self::Overlap,
            16..=20 => Self::NyPm,
            _       => Self::Dead,
        }
    }

    pub fn sizing_multiplier(&self) -> Decimal {
        match self {
            Self::Asian   => dec!(0.7),
            Self::London  => dec!(1.0),
            Self::Overlap => dec!(1.0),
            Self::NyPm    => dec!(0.8),
            Self::Dead    => dec!(0.0),
        }
    }

    pub fn slippage_multiplier(&self) -> Decimal {
        match self {
            Self::Asian   => dec!(1.8),
            Self::London  => dec!(1.0),
            Self::Overlap => dec!(1.1),
            Self::NyPm    => dec!(1.2),
            Self::Dead    => dec!(2.0),
        }
    }
}
```

### 4.8 Account Lifecycle State Machine

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountPhase {
    ChallengePhase1,
    ChallengePhase2,      // 2-step firms only
    AwaitingFunded,
    Funded,
    PayoutWindow,         // Approaching payout — protect at all costs
    Failed,               // Diagnostic mode, log everything
}

#[derive(Debug, Clone)]
pub struct AccountState {
    pub phase:              AccountPhase,
    pub firm:               FirmConfig,
    pub starting_balance:   Decimal,
    pub current_equity:     Decimal,
    pub high_water_mark:    Decimal,
    pub profit_pct:         Decimal,     // (equity - start) / start * 100
    pub dd_from_hwm_pct:    Decimal,     // (hwm - equity) / hwm * 100
    pub dd_remaining_pct:   Decimal,     // firm.max_dd - dd_from_hwm (CRITICAL)
    pub target_remaining:   Decimal,     // firm.target - profit_pct
    pub trading_days:       u32,
    pub min_days_met:       bool,
    pub days_since_funded:  u32,
    pub total_trades:       u32,
    pub consecutive_losses: u8,
    pub phase_start_time:   i64,
}

impl AccountState {
    pub fn update_equity(&mut self, equity: Decimal) {
        self.current_equity = equity;
        if equity > self.high_water_mark {
            self.high_water_mark = equity;
        }
        self.profit_pct = (equity - self.starting_balance) / self.starting_balance * dec!(100);
        self.dd_from_hwm_pct = (self.high_water_mark - equity) / self.high_water_mark * dec!(100);
        self.dd_remaining_pct = self.firm.max_dd_limit_pct - self.dd_from_hwm_pct;
        self.target_remaining = self.firm.profit_target_pct - self.profit_pct;
    }

    /// The master risk multiplier from account phase / challenge progress
    pub fn phase_risk_multiplier(&self) -> Decimal {
        match self.phase {
            AccountPhase::ChallengePhase1 | AccountPhase::ChallengePhase2 => {
                let progress = self.profit_pct / self.firm.profit_target_pct;
                match progress {
                    p if p >= dec!(0.90) => dec!(0.25),  // 90%+ to target: coast
                    p if p >= dec!(0.70) => dec!(0.50),  // 70-90%: cautious
                    p if p >= dec!(0.50) => dec!(0.75),  // 50-70%: moderate
                    _ => dec!(1.0),                       // 0-50%: full risk
                }
            }
            AccountPhase::Funded => dec!(0.80),
            AccountPhase::PayoutWindow => dec!(0.25),
            _ => dec!(0.0),  // AwaitingFunded, Failed: no trading
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

    /// Combined effective risk — MINIMUM of all multipliers
    pub fn effective_risk_multiplier(
        &self,
        day_state: DayState,
        eq_filter: Decimal,
        drift_mult: Decimal,
    ) -> Decimal {
        self.phase_risk_multiplier()
            .min(self.dd_distance_multiplier())
            .min(day_state.risk_multiplier())
            .min(eq_filter)
            .min(drift_mult)
    }
}
```

---

## Part 5: Indicators (Streaming, Single-Bar API)

All indicators maintain internal state and accept one bar at a time. No re-computation over buffers.

### 5.1 EMA

```rust
pub struct EMA {
    period: usize,
    alpha:  Decimal,
    value:  Option<Decimal>,
    count:  usize,
    sum:    Decimal,  // For SMA seed during warmup
}

impl EMA {
    pub fn new(period: usize) -> Self {
        let alpha = dec!(2) / (Decimal::from(period) + dec!(1));
        Self { period, alpha, value: None, count: 0, sum: Decimal::ZERO }
    }

    pub fn update(&mut self, close: Decimal) -> Option<Decimal> {
        self.count += 1;
        match self.value {
            None if self.count < self.period => {
                self.sum += close;
                None
            }
            None => {
                self.sum += close;
                let sma = self.sum / Decimal::from(self.period);
                self.value = Some(sma);
                Some(sma)
            }
            Some(prev) => {
                let ema = self.alpha * close + (dec!(1) - self.alpha) * prev;
                self.value = Some(ema);
                Some(ema)
            }
        }
    }

    pub fn value(&self) -> Option<Decimal> { self.value }
}
```

### 5.2 ATR (Wilder Smoothing)

```rust
pub struct ATR {
    period:    usize,
    value:     Option<Decimal>,
    prev_close: Option<Decimal>,
    count:     usize,
    tr_sum:    Decimal,
}

impl ATR {
    pub fn update(&mut self, bar: &Bar) -> Option<Decimal> {
        let tr = match self.prev_close {
            None => bar.high - bar.low,
            Some(pc) => (bar.high - bar.low)
                .max((bar.high - pc).abs())
                .max((bar.low - pc).abs()),
        };
        self.prev_close = Some(bar.close);
        self.count += 1;

        match self.value {
            None if self.count < self.period => {
                self.tr_sum += tr;
                None
            }
            None => {
                self.tr_sum += tr;
                let atr = self.tr_sum / Decimal::from(self.period);
                self.value = Some(atr);
                Some(atr)
            }
            Some(prev) => {
                let n = Decimal::from(self.period);
                let atr = (prev * (n - dec!(1)) + tr) / n;
                self.value = Some(atr);
                Some(atr)
            }
        }
    }
}
```

### 5.3 Bollinger Bands

```rust
pub struct BollingerBands {
    period:  usize,
    k:       Decimal,     // Standard: 2.0
    prices:  VecDeque<Decimal>,
    cached:  Option<BBValues>,
}

pub struct BBValues {
    pub upper: Decimal,
    pub mid:   Decimal,
    pub lower: Decimal,
    pub width: Decimal,   // (upper - lower) / mid
}

impl BollingerBands {
    pub fn update(&mut self, close: Decimal) -> Option<&BBValues> {
        self.prices.push_back(close);
        if self.prices.len() > self.period { self.prices.pop_front(); }
        if self.prices.len() < self.period { return None; }

        let n = Decimal::from(self.period);
        let sum: Decimal = self.prices.iter().sum();
        let mid = sum / n;
        let variance: Decimal = self.prices.iter()
            .map(|p| (*p - mid) * (*p - mid))
            .sum::<Decimal>() / n;
        // Decimal sqrt approximation via Newton's method
        let std_dev = decimal_sqrt(variance);
        let upper = mid + self.k * std_dev;
        let lower = mid - self.k * std_dev;
        let width = if mid.is_zero() { Decimal::ZERO } else { (upper - lower) / mid };

        self.cached = Some(BBValues { upper, mid, lower, width });
        self.cached.as_ref()
    }
}
```

### 5.4 ADX

```rust
pub struct ADX {
    period:     usize,
    plus_di:    WilderSmooth,
    minus_di:   WilderSmooth,
    adx_smooth: WilderSmooth,
    prev_bar:   Option<(Decimal, Decimal, Decimal)>,  // (high, low, close)
    count:      usize,
}

impl ADX {
    pub fn update(&mut self, bar: &Bar) -> Option<Decimal> {
        if let Some((ph, pl, _pc)) = self.prev_bar {
            let up_move = bar.high - ph;
            let down_move = pl - bar.low;
            let plus_dm  = if up_move > down_move && up_move > Decimal::ZERO { up_move } else { Decimal::ZERO };
            let minus_dm = if down_move > up_move && down_move > Decimal::ZERO { down_move } else { Decimal::ZERO };

            self.plus_di.update(plus_dm);
            self.minus_di.update(minus_dm);

            self.count += 1;
            if self.count >= self.period {
                let pdi = self.plus_di.value().unwrap_or(Decimal::ZERO);
                let mdi = self.minus_di.value().unwrap_or(Decimal::ZERO);
                let sum = pdi + mdi;
                let dx = if sum.is_zero() { Decimal::ZERO } else { (pdi - mdi).abs() / sum * dec!(100) };
                self.adx_smooth.update(dx);
                self.prev_bar = Some((bar.high, bar.low, bar.close));
                return self.adx_smooth.value();
            }
        }
        self.prev_bar = Some((bar.high, bar.low, bar.close));
        None
    }
}
```

### 5.5 Hurst Exponent (R/S Analysis)

```rust
pub struct HurstExponent {
    period: usize,
    prices: VecDeque<Decimal>,
}

impl HurstExponent {
    pub fn update(&mut self, close: Decimal) -> Option<Decimal> {
        self.prices.push_back(close);
        if self.prices.len() > self.period { self.prices.pop_front(); }
        if self.prices.len() < self.period { return None; }

        // R/S analysis
        let returns: Vec<Decimal> = self.prices.iter()
            .zip(self.prices.iter().skip(1))
            .map(|(a, b)| *b - *a)
            .collect();
        let n = returns.len();
        let mean = returns.iter().sum::<Decimal>() / Decimal::from(n);
        let deviations: Vec<Decimal> = returns.iter().map(|r| *r - mean).collect();

        // Cumulative deviations
        let mut cumdev = Vec::with_capacity(n);
        let mut running = Decimal::ZERO;
        for d in &deviations {
            running += *d;
            cumdev.push(running);
        }

        let range = cumdev.iter().copied().fold(Decimal::MIN, Decimal::max)
                  - cumdev.iter().copied().fold(Decimal::MAX, Decimal::min);
        let std_dev = decimal_sqrt(
            deviations.iter().map(|d| *d * *d).sum::<Decimal>() / Decimal::from(n)
        );

        if std_dev.is_zero() { return Some(dec!(0.5)); }
        let rs = range / std_dev;
        // H = ln(R/S) / ln(n)
        let h = decimal_ln(rs) / decimal_ln(Decimal::from(n));
        Some(h.max(dec!(0)).min(dec!(1)))
    }
}
```

### 5.6 VWAP

```rust
pub struct VWAP {
    cum_price_vol: Decimal,
    cum_volume:    Decimal,
    session_start: Option<i64>,
}

impl VWAP {
    pub fn update(&mut self, bar: &Bar, session_changed: bool) -> Decimal {
        if session_changed {
            self.cum_price_vol = Decimal::ZERO;
            self.cum_volume = Decimal::ZERO;
        }
        let typical = (bar.high + bar.low + bar.close) / dec!(3);
        let vol = Decimal::from(bar.volume);
        self.cum_price_vol += typical * vol;
        self.cum_volume += vol;
        if self.cum_volume.is_zero() { return bar.close; }
        self.cum_price_vol / self.cum_volume
    }
}
```

### 5.7 Choppiness Index

```rust
pub struct ChoppinessIndex {
    period:   usize,
    atr_vals: VecDeque<Decimal>,
    highs:    VecDeque<Decimal>,
    lows:     VecDeque<Decimal>,
    atr:      ATR,
}

impl ChoppinessIndex {
    /// CI = 100 * ln(sum(ATR_i, n) / (highest_high - lowest_low)) / ln(n)
    pub fn update(&mut self, bar: &Bar) -> Option<Decimal> {
        if let Some(atr_val) = self.atr.update(bar) {
            self.atr_vals.push_back(atr_val);
            self.highs.push_back(bar.high);
            self.lows.push_back(bar.low);
            if self.atr_vals.len() > self.period { self.atr_vals.pop_front(); }
            if self.highs.len() > self.period { self.highs.pop_front(); }
            if self.lows.len() > self.period { self.lows.pop_front(); }

            if self.atr_vals.len() == self.period {
                let atr_sum: Decimal = self.atr_vals.iter().sum();
                let hh = self.highs.iter().copied().fold(Decimal::MIN, Decimal::max);
                let ll = self.lows.iter().copied().fold(Decimal::MAX, Decimal::min);
                let range = hh - ll;
                if range.is_zero() { return Some(dec!(50)); }
                let ci = dec!(100) * decimal_ln(atr_sum / range) / decimal_ln(Decimal::from(self.period));
                return Some(ci);
            }
        }
        None
    }
}
```

### 5.8 BB Width Percentile Tracker

```rust
pub struct BBWidthPercentile {
    window: usize,       // 100
    history: VecDeque<Decimal>,
}

impl BBWidthPercentile {
    pub fn update(&mut self, bb_width: Decimal) -> Decimal {
        self.history.push_back(bb_width);
        if self.history.len() > self.window { self.history.pop_front(); }
        let below = self.history.iter().filter(|w| **w <= bb_width).count();
        Decimal::from(below) / Decimal::from(self.history.len())
    }
}
```

---

## Part 6: Regime Classifier

Features: Hurst, ADX-14, ATR-14/ATR-50 ratio, BB-20 width percentile, Choppiness Index

| Regime | Conditions |
|--------|------------|
| StrongTrendUp | ADX > 25, Hurst > 0.6, close > EMA-20 > EMA-200 |
| StrongTrendDown | ADX > 25, Hurst > 0.6, close < EMA-20 < EMA-200 |
| WeakTrendUp | ADX 20-25, Hurst 0.52-0.6, close > EMA-200 |
| WeakTrendDown | ADX 20-25, Hurst 0.52-0.6, close < EMA-200 |
| RangingTight | Hurst < 0.45, BB width < 30th pctile, CI > 55 |
| RangingWide | Hurst < 0.45, BB width 30-60th pctile |
| Choppy | CI > 61.8, ADX < 20 |
| BreakoutPending | BB squeeze: width < 20th pctile for 10+ bars |
| Transitioning | confidence < 0.30 OR gap between top-2 regime scores < 0.06 |

```rust
pub struct RegimeClassifier {
    ema20:      EMA,
    ema200:     EMA,
    atr14:      ATR,
    atr50:      ATR,
    adx:        ADX,
    hurst:      HurstExponent,
    bb:         BollingerBands,
    bb_pctile:  BBWidthPercentile,
    ci:         ChoppinessIndex,
    squeeze_count: u32,   // bars in BB squeeze
}

impl RegimeClassifier {
    pub fn update(&mut self, bar: &Bar) -> Option<RegimeSignal9> {
        // Update all indicators
        let ema20  = self.ema20.update(bar.close)?;
        let ema200 = self.ema200.update(bar.close)?;
        let atr14  = self.atr14.update(bar)?;
        let atr50  = self.atr50.update(bar)?;
        let adx    = self.adx.update(bar)?;
        let hurst  = self.hurst.update(bar.close)?;
        let bb     = self.bb.update(bar.close)?;
        let ci     = self.ci.update(bar)?;

        let atr_ratio = if atr50.is_zero() { dec!(1) } else { atr14 / atr50 };
        let bb_pctile = self.bb_pctile.update(bb.width);

        // Track BB squeeze duration
        if bb_pctile < dec!(0.20) {
            self.squeeze_count += 1;
        } else {
            self.squeeze_count = 0;
        }

        // Classification with confidence scoring
        let (regime, confidence) = self.classify(
            bar.close, ema20, ema200, adx, hurst, atr_ratio, bb_pctile, ci
        );

        Some(RegimeSignal9 {
            regime, confidence, adx, hurst, atr_ratio,
            bb_width_pctile: bb_pctile, choppiness_index: ci,
            computed_at: bar.timestamp,
        })
    }

    fn classify(&self, close: Decimal, ema20: Decimal, ema200: Decimal,
                adx: Decimal, hurst: Decimal, atr_ratio: Decimal,
                bb_pctile: Decimal, ci: Decimal) -> (Regime9, Decimal) {
        // BreakoutPending: BB squeeze for 10+ bars
        if self.squeeze_count >= 10 {
            return (Regime9::BreakoutPending, dec!(0.75));
        }

        // Choppy: CI > 61.8 and ADX < 20
        if ci > dec!(61.8) && adx < dec!(20) {
            return (Regime9::Choppy, dec!(0.70));
        }

        // Strong trends
        if adx > dec!(25) && hurst > dec!(0.6) {
            if close > ema20 && ema20 > ema200 {
                return (Regime9::StrongTrendUp, dec!(0.80));
            }
            if close < ema20 && ema20 < ema200 {
                return (Regime9::StrongTrendDown, dec!(0.80));
            }
        }

        // Weak trends
        if adx >= dec!(20) && adx <= dec!(25) && hurst >= dec!(0.52) && hurst <= dec!(0.6) {
            if close > ema200 {
                return (Regime9::WeakTrendUp, dec!(0.55));
            } else {
                return (Regime9::WeakTrendDown, dec!(0.55));
            }
        }

        // Ranging
        if hurst < dec!(0.45) {
            if bb_pctile < dec!(0.30) && ci > dec!(55) {
                return (Regime9::RangingTight, dec!(0.65));
            }
            if bb_pctile < dec!(0.60) {
                return (Regime9::RangingWide, dec!(0.60));
            }
        }

        // Default: Transitioning (low confidence catchall)
        (Regime9::Transitioning, dec!(0.25))
    }
}
```

---

## Part 7: Session Detection

| Session | UTC Hours | Sizing Mult | Slippage Mult | Notes |
|---------|-----------|-------------|---------------|-------|
| Asian | 00:00-07:00 | 0.7x | 1.8x | Only AsianRangeHead building range |
| London | 07:00-12:00 | 1.0x | 1.0x | Primary trading window |
| Overlap | 12:00-16:00 | 1.0x | 1.1x | Best liquidity, all heads active |
| NY PM | 16:00-21:00 | 0.8x | 1.2x | Reduced activity |
| Dead | 21:00-00:00 | 0.0x (no trading) | N/A | Zero new entries |

**Spread expectations (GBPUSD typical):**
```
00-07 (Asian):        1.5-2.5 pips
07-08 (London open):  0.8-1.5 pips
08-12 (London):       0.5-0.8 pips (tightest)
12-16 (Overlap):      0.6-0.9 pips
16-21 (NY PM):        0.8-1.2 pips
21-24 (Dead):         2.0-5.0 pips
```

---

## Part 8: The 3 MVP Strategy Heads

### Head 1: MomentumHead

**Purpose:** Capture session-open continuation and clean expansion. Good for fast target progress when volatility is real.

| Parameter | Value |
|-----------|-------|
| Session required | London (07:00-09:30 UTC) OR NY (13:30-16:00 UTC) |
| Regime required | StrongTrend, WeakTrend, BreakoutPending |
| Setup | First 60-min range from session open (tracked per session) |
| Entry trigger | Close above first-hour high (bull) or below first-hour low (bear) |
| VWAP filter | Price > VWAP from session open for buy; < VWAP for sell |
| Stop loss | Midpoint of first-hour range |
| TP1 (50%) | Range height projected beyond breakout level (1:1 projection) |
| TP2 (50%) | 2x range height |
| Max per session | 1 signal |
| Min R:R | 1.5 |
| Breakeven | At +1.0R |
| Trail after TP1 | Move SL to breakout level + 2 pips |
| Time exit | 24h if < +0.5R |
| Regime exit | If Ranging/Choppy/Transitioning -> close |
| Warmup | 200 bars (regime classifier needs EMA-200) |

```rust
pub struct MomentumHead {
    config:            MomentumConfig,
    regime_classifier: RegimeClassifier,
    vwap:              VWAP,
    session_high:      Option<Decimal>,
    session_low:       Option<Decimal>,
    range_formed:      bool,
    bars_since_open:   u32,
    current_session:   Option<Session>,
    trade_taken:       bool,
}

impl MomentumHead {
    fn update_session_range(&mut self, bar: &Bar, session: &SessionProfile) {
        let is_session_start = self.current_session != Some(session.session);
        if is_session_start {
            self.session_high = None;
            self.session_low = None;
            self.range_formed = false;
            self.bars_since_open = 0;
            self.trade_taken = false;
            self.current_session = Some(session.session);
        }

        self.bars_since_open += 1;

        // Build first-hour range (4 M15 bars = 60 min)
        if self.bars_since_open <= 4 && bar.timeframe == Timeframe::M15 {
            self.session_high = Some(self.session_high.unwrap_or(bar.high).max(bar.high));
            self.session_low = Some(self.session_low.unwrap_or(bar.low).min(bar.low));
            if self.bars_since_open == 4 { self.range_formed = true; }
        }
    }
}
```

### Head 2: AsianRangeHead

**Purpose:** Highly mechanical, easy to test, well-suited to challenge consistency. Captures structured breakout from the overnight range.

| Parameter | Value |
|-----------|-------|
| Asian range window | UTC 00:00-07:00 |
| Entry window | UTC 07:00-09:00 only |
| Range gate | 15 <= range <= 80 pips |
| Trade limit | 1 per day per symbol |
| Entry trigger | H1 close above asian_high + 5 pips (bull) |
| Stop loss | asian_low + range/2 (bull) |
| TP1 (50%) | asian_high + range x 1.0 |
| TP2 (50%) | asian_high + range x 1.5 |
| Trail after TP1 | Move SL to asian_high + 2 pips |
| Min R:R | 1.2 |
| Daily reset | UTC midnight: reset asian_high, asian_low, trade_taken |
| Warmup | 200 bars |

```rust
pub struct AsianRangeHead {
    config:     AsianRangeConfig,
    state:      AsianRangeState,
    pip_size:   Decimal,
}

pub struct AsianRangeConfig {
    pub asian_start_utc:    u8,      // 0
    pub asian_end_utc:      u8,      // 7
    pub entry_window_end:   u8,      // 9
    pub min_range_pips:     Decimal, // 15.0
    pub max_range_pips:     Decimal, // 80.0
    pub sl_buffer_pips:     Decimal, // 5.0
    pub tp1_multiplier:     Decimal, // 1.0
    pub tp2_multiplier:     Decimal, // 1.5
    pub min_rr:             Decimal, // 1.2
    pub max_trades_per_day: u8,      // 1
}

pub struct AsianRangeState {
    pub asian_high:        Option<Decimal>,
    pub asian_low:         Option<Decimal>,
    pub trade_taken_today: bool,
    pub current_day:       i64,
}

impl AsianRangeHead {
    fn update_asian_range(&mut self, bar: &Bar) {
        let h = utc_hour(bar.timestamp);

        // Daily reset at UTC midnight
        let day = bar.timestamp / 86400;
        if day != self.state.current_day {
            self.state.asian_high = None;
            self.state.asian_low = None;
            self.state.trade_taken_today = false;
            self.state.current_day = day;
        }

        // Build range during Asian session
        if h >= self.config.asian_start_utc && h < self.config.asian_end_utc {
            self.state.asian_high = Some(
                self.state.asian_high.unwrap_or(bar.high).max(bar.high)
            );
            self.state.asian_low = Some(
                self.state.asian_low.unwrap_or(bar.low).min(bar.low)
            );
        }
    }
}
```

### Head 3: BreakoutHead

**Purpose:** Capture volatility expansion after compression. Good for strong trending starts and regime shifts.

| Parameter | Value |
|-----------|-------|
| Regime required | BreakoutPending, RangingWide |
| Setup | BB width in bottom 30th percentile for 10+ bars (squeeze) |
| Entry trigger | BB width > 50th pctile AND close outside BB band |
| Volume | >= 1.3x 20-bar average |
| Stop loss | Opposite BB band at breakout time |
| TP1 (40%) | Entry +/- 2.0 ATR |
| TP2 (60%) | Entry +/- 3.0 ATR |
| Min R:R | 1.8 |
| Fake breakout guard | Close back inside BB within 2 bars -> exit immediately |
| Trail after TP1 | Move SL to entry + 0.5 ATR in breakout direction |
| Warmup | 50 bars |

```rust
pub struct BreakoutHead {
    config:        BreakoutConfig,
    bb:            BollingerBands,
    bb_pctile:     BBWidthPercentile,
    atr:           ATR,
    vol_avg:       VecDeque<u64>,   // 20-bar volume average
    squeeze_bars:  u32,
    breakout_bar:  Option<i64>,     // timestamp of breakout for fake-out detection
    breakout_dir:  Option<Direction>,
    bars_since_bo: u32,
}

pub struct BreakoutConfig {
    pub squeeze_pctile:    Decimal, // 0.30
    pub expansion_pctile:  Decimal, // 0.50
    pub min_squeeze_bars:  u32,     // 10
    pub volume_mult:       Decimal, // 1.3
    pub tp1_atr_mult:      Decimal, // 2.0
    pub tp2_atr_mult:      Decimal, // 3.0
    pub min_rr:            Decimal, // 1.8
    pub fakeout_bars:      u32,     // 2
}
```

---

## Part 9: Risk Engine

### 9.1 Position Sizing (Exact Formula)

```rust
pub fn calculate_lots(
    risk_pct:          RiskPercent,
    account_equity:    Decimal,
    sl_distance_price: Decimal,  // |entry - sl|
    pip_size:          Decimal,
    pip_value_per_lot: Decimal,  // USD per pip per standard lot
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

    // Sanity: verify rounding didn't add >5% extra risk
    let actual_risk = final_lots * sl_pips * pip_value_per_lot;
    let actual_risk_pct = actual_risk / account_equity * dec!(100);
    if actual_risk_pct > risk_pct.inner() * dec!(1.05) {
        return Err(SizingError::RoundingExceededRisk { computed: actual_risk_pct });
    }
    Ok(final_lots)
}
```

### 9.2 Challenge vs Funded Risk Parameters

| Parameter | Challenge Mode | Funded Mode |
|-----------|---------------|-------------|
| Risk per trade | 0.50% - 1.0% | 0.50% - 0.75% |
| Max portfolio heat | 2.0% | 2.5% |
| Daily stop loss | 1.5% of account | 0.8% of account |
| Consecutive loss halt | 3 losses -> 30-min cooldown | 3 losses -> 30-min cooldown |
| Daily target | 2.0% | 0.40% |
| Max trades/day | 4 | 3 |
| Coasting (50-70% to target) | Reduce to 0.75% risk | Reduce to 0.50% risk |
| Coasting (70-90% to target) | Reduce to 0.50% risk | Reduce to 0.25% risk |
| Coasting (90%+ to target) | Reduce to 0.25% risk | Reduce to 0.10% risk |

### 9.3 Daily P&L Engine

```rust
pub struct DailyPnlEngine {
    config:          DailyPnlConfig,
    day_open_equity: Decimal,
    day_pnl_usd:     Decimal,
    intraday_peak:   Decimal,
    state:           DayState,
    last_day:        i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DayState {
    Normal,       // Full risk allowed
    Cruising,     // 60% of daily target hit — risk x 0.75
    Protecting,   // 100% of daily target hit — risk x 0.25
    DailyStopped, // Daily stop hit — no new trades
}

impl DayState {
    pub fn risk_multiplier(&self) -> Decimal {
        match self {
            Self::Normal       => dec!(1.0),
            Self::Cruising     => dec!(0.75),
            Self::Protecting   => dec!(0.25),
            Self::DailyStopped => dec!(0.0),
        }
    }
}

pub struct DailyPnlConfig {
    pub daily_target_pct:      Decimal, // Challenge: 2.0%, Funded: 0.40%
    pub cruise_threshold_pct:  Decimal, // 0.60 (60% of daily target)
    pub cruise_risk_mult:      Decimal, // 0.75
    pub protect_threshold_pct: Decimal, // 1.00 (100% of daily target)
    pub protect_risk_mult:     Decimal, // 0.25
    pub daily_stop_pct:        Decimal, // Challenge: 1.5%, Funded: 0.8%
}

impl DailyPnlEngine {
    pub fn update(&mut self, current_equity: Decimal, timestamp: i64) -> DayState {
        // Daily reset
        let day = timestamp / 86400;
        if day != self.last_day {
            self.day_open_equity = current_equity;
            self.intraday_peak = current_equity;
            self.day_pnl_usd = Decimal::ZERO;
            self.state = DayState::Normal;
            self.last_day = day;
        }

        self.day_pnl_usd = current_equity - self.day_open_equity;
        if current_equity > self.intraday_peak {
            self.intraday_peak = current_equity;
        }

        let pnl_pct = self.day_pnl_usd / self.day_open_equity * dec!(100);
        let intraday_dd = (self.intraday_peak - current_equity) / self.day_open_equity * dec!(100);

        // State transitions
        if intraday_dd >= self.config.daily_stop_pct || pnl_pct <= -self.config.daily_stop_pct {
            self.state = DayState::DailyStopped;
        } else if pnl_pct >= self.config.daily_target_pct * self.config.protect_threshold_pct {
            self.state = DayState::Protecting;
        } else if pnl_pct >= self.config.daily_target_pct * self.config.cruise_threshold_pct {
            self.state = DayState::Cruising;
        }
        // Note: never go back from Protecting -> Cruising -> Normal within same day

        self.state
    }

    pub fn can_trade(&self) -> bool { self.state != DayState::DailyStopped }
}
```

### 9.4 Kill Switch

```rust
pub struct KillSwitch {
    active:     bool,
    reason:     Option<String>,
    activated:  Option<i64>,
    cooldown:   Option<i64>,   // Resumes trading after this timestamp
}

impl KillSwitch {
    pub fn check(&mut self, account: &AccountState, timestamp: i64) -> bool {
        // Check cooldown expiry
        if let Some(resume_at) = self.cooldown {
            if timestamp >= resume_at {
                self.cooldown = None;
                self.active = false;
            }
        }

        if self.active { return true; }

        // 95% of daily DD limit
        let daily_dd_trigger = account.firm.daily_dd_limit_pct * dec!(0.95);
        if account.dd_from_hwm_pct >= daily_dd_trigger {
            self.activate("Daily DD 95% trigger");
            return true;
        }

        // 95% of total DD limit
        let total_dd_trigger = account.firm.max_dd_limit_pct * dec!(0.95);
        if account.dd_from_hwm_pct >= total_dd_trigger {
            self.activate("Total DD 95% trigger");
            return true;
        }

        // 3 consecutive losses -> 30-min cooldown
        if account.consecutive_losses >= 3 {
            self.cooldown = Some(timestamp + 1800); // 30 minutes
            self.active = true;
            self.reason = Some("3 consecutive losses — 30-min cooldown".into());
            return true;
        }

        false
    }

    fn activate(&mut self, reason: &str) {
        self.active = true;
        self.reason = Some(reason.into());
        self.activated = Some(chrono::Utc::now().timestamp());
    }
}
```

### 9.5 Equity Curve Filter

```rust
pub struct EquityCurveFilter {
    config:         EquityCurveFilterConfig,
    equity_history: VecDeque<Decimal>,
    equity_ma:      Option<Decimal>,
}

pub struct EquityCurveFilterConfig {
    pub ma_period:          usize,   // 20 closed trades
    pub below_ma_risk_mult: Decimal, // 0.50
    pub deep_below_mult:    Decimal, // 0.25
    pub deep_threshold_pct: Decimal, // 2.0%
}

impl EquityCurveFilter {
    pub fn record_trade_close(&mut self, equity: Decimal) {
        self.equity_history.push_back(equity);
        if self.equity_history.len() > self.config.ma_period {
            self.equity_history.pop_front();
        }
        if self.equity_history.len() == self.config.ma_period {
            let sum: Decimal = self.equity_history.iter().sum();
            self.equity_ma = Some(sum / Decimal::from(self.config.ma_period));
        }
    }

    pub fn multiplier(&self) -> Decimal {
        let ma = match self.equity_ma { None => return dec!(1.0), Some(m) => m };
        let current = match self.equity_history.back() { None => return dec!(1.0), Some(e) => *e };
        if current >= ma { return dec!(1.0); }
        let pct_below = (ma - current) / ma * dec!(100);
        if pct_below >= self.config.deep_threshold_pct {
            self.config.deep_below_mult    // 0.25
        } else {
            self.config.below_ma_risk_mult // 0.50
        }
    }
}
```

### 9.6 Pyramiding (Risk-Preserving)

```rust
pub struct PyramidConfig {
    pub min_r_to_add:        Decimal,  // 1.0R minimum before adding
    pub max_layers:          u8,       // 2 (initial + 2 adds = 3 units max)
    pub add_size_fraction:   Decimal,  // 0.5 (each add = 50% of original)
    pub require_same_regime: bool,     // true
}

pub struct PyramidState {
    pub initial_lots:     Decimal,
    pub initial_entry:    Decimal,
    pub initial_sl:       Decimal,
    pub initial_risk_usd: Decimal,
    pub layers:           Vec<PyramidLayer>,
}

pub struct PyramidLayer {
    pub lots:      Decimal,
    pub entry:     Decimal,
    pub added_at:  i64,
}
```

**Pyramid entry conditions (ALL required):**
1. Position is open and in profit >= 1.0R
2. Layer count < 2
3. Regime has not changed from original trade
4. Daily P&L state is Normal or Cruising (NOT Protecting or DailyStopped)
5. New SL placement (breakeven on original entry) gives R:R >= 1.0 for the pyramid add
6. **INVARIANT: total risk after pyramid <= original risk_usd**

---

## Part 10: Payout-Ensuring Architecture

This is what separates a bot that trades from a bot that gets paid.

### 10.1 Temporal Intelligence

```rust
pub struct TemporalIntelligence {
    pub challenge_day:        u32,
    pub min_days_remaining:   i32,
    pub payout_window_day:    Option<u32>,
    pub days_to_next_payout:  Option<u32>,
    pub is_friday_afternoon:  bool,
    pub is_month_end:         bool,
}

impl TemporalIntelligence {
    pub fn urgency_profile(&self, account: &AccountState) -> UrgencyProfile {
        // Payout window: PROTECT
        if account.phase == AccountPhase::PayoutWindow {
            return UrgencyProfile::Protect;
        }
        // Challenge near completion (>80% of target): COAST
        if account.target_remaining <= account.firm.profit_target_pct * dec!(0.20) {
            return UrgencyProfile::Coast;
        }
        // Challenge stalling (day 15+ with <30% of target): PUSH
        if account.trading_days > 15
           && account.profit_pct < account.firm.profit_target_pct * dec!(0.30) {
            return UrgencyProfile::PushSelective;
        }
        // Friday afternoon: reduce risk (weekend gap)
        if self.is_friday_afternoon {
            return UrgencyProfile::Coast;
        }
        UrgencyProfile::Normal
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UrgencyProfile {
    Normal,         // Full normal trading
    PushSelective,  // Slightly more aggressive — accept lower confidence
    Coast,          // Near target — only A+ setups, shrink sizing
    Protect,        // Payout window — minimize risk, protect P&L
}
```

### 10.2 Drift Detection

Detects when live performance diverges from backtest expectations.

```rust
pub struct DriftDetector {
    config:              DriftConfig,
    rolling_trades:      VecDeque<TradeResult>,
    backtest_benchmarks: DriftBenchmarks,
}

pub struct DriftBenchmarks {
    pub expected_win_rate:      Decimal,  // From backtest
    pub expected_avg_r:         Decimal,  // From backtest
    pub expected_profit_factor: Decimal,
    pub max_consecutive_losses: u8,       // From Monte Carlo
    pub expected_avg_slippage:  Decimal,  // From demo forward
}

pub struct DriftConfig {
    pub min_trades_to_evaluate: usize,       // 20
    pub win_rate_alert_delta:   Decimal,     // 0.12
    pub win_rate_halt_delta:    Decimal,     // 0.20
    pub avg_r_halt_threshold:   Decimal,     // -0.10 (negative expectancy)
    pub slippage_alert_mult:    Decimal,     // 2.0x expected
}

#[derive(Debug)]
pub enum DriftSignal {
    InsufficientData,
    Healthy,
    ReduceRisk { multiplier: Decimal },
    Halt { reason: String },
}

impl DriftDetector {
    pub fn evaluate(&self) -> DriftSignal {
        if self.rolling_trades.len() < self.config.min_trades_to_evaluate {
            return DriftSignal::InsufficientData;
        }

        let live_wr = self.rolling_win_rate();
        let live_avg_r = self.rolling_avg_r();
        let live_slippage = self.rolling_avg_slippage();

        // HALT: negative expectancy
        if live_avg_r < self.config.avg_r_halt_threshold {
            return DriftSignal::Halt {
                reason: format!("Negative expectancy: avg_r={:.3}", live_avg_r),
            };
        }

        // HALT: win rate collapsed
        let wr_delta = self.backtest_benchmarks.expected_win_rate - live_wr;
        if wr_delta > self.config.win_rate_halt_delta {
            return DriftSignal::Halt {
                reason: format!("Win rate collapse: {:.1}% vs expected {:.1}%",
                    live_wr * dec!(100),
                    self.backtest_benchmarks.expected_win_rate * dec!(100)),
            };
        }

        // ALERT: win rate degraded but not critical
        if wr_delta > self.config.win_rate_alert_delta {
            return DriftSignal::ReduceRisk { multiplier: dec!(0.50) };
        }

        // ALERT: slippage much worse than demo
        if live_slippage > self.backtest_benchmarks.expected_avg_slippage
                           * self.config.slippage_alert_mult {
            return DriftSignal::ReduceRisk { multiplier: dec!(0.75) };
        }

        DriftSignal::Healthy
    }
}
```

### 10.3 Smart Execution Engine

```rust
pub struct ExecutionEngine {
    config:         ExecutionConfig,
    spread_tracker: SpreadTracker,
    fill_log:       VecDeque<FillRecord>,
}

pub struct ExecutionConfig {
    pub max_spread_atr_ratio:  Decimal,  // 0.30 — reject if spread > 30% of ATR
    pub max_retries:           u8,       // 3
    pub retry_delay_ms:        u64,      // 500
    pub stale_price_threshold: i64,      // 2 seconds
    pub slippage_budget_pips:  Decimal,  // 1.0
}

impl ExecutionEngine {
    pub fn execute(&mut self, decision: RiskDecision) -> ExecutionResult {
        let RiskDecision::Execute { signal, risk_pct, lots, .. } = decision else {
            return ExecutionResult::Skipped;
        };

        // Gate 1: Spread spike check
        let current_spread = self.spread_tracker.current();
        let typical_spread = self.spread_tracker.session_typical();
        if current_spread > typical_spread * dec!(2.5) {
            return ExecutionResult::Deferred {
                reason: "Spread spike".into(),
                retry_at: now() + 30_000,
            };
        }

        // Gate 2: Price freshness
        let price_age = now() - self.spread_tracker.last_tick_time();
        if price_age > self.config.stale_price_threshold * 1000 {
            return ExecutionResult::Rejected {
                reason: "Stale price data".into(),
            };
        }

        // Gate 3: Spread-adjusted R:R
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
            Err(e) => ExecutionResult::Failed { error: e.to_string() }
        }
    }
}
```

### 10.4 Performance Ledger (Head x Regime x Session)

Auto-disables underperforming combos without ML.

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
    pub fn is_segment_allowed(&self, head: HeadId, regime: Regime9, session: Session) -> bool {
        match self.segments.get(&(head, regime, session)) {
            None => true,
            Some(stats) if stats.total_trades < 10 => true,
            Some(stats) => {
                if stats.consecutive_losses >= 5 { return false; }
                let wr = Decimal::from(stats.wins) / Decimal::from(stats.total_trades);
                if stats.total_trades >= 15 && wr < dec!(0.30) { return false; }
                let avg_r = stats.total_r / Decimal::from(stats.total_trades);
                if stats.total_trades >= 20 && avg_r < dec!(0.0) { return false; }
                true
            }
        }
    }

    pub fn risk_multiplier(&self, head: HeadId, regime: Regime9, session: Session) -> Decimal {
        match self.segments.get(&(head, regime, session)) {
            None => dec!(1.0),
            Some(stats) if stats.total_trades < 10 => dec!(0.80),
            Some(stats) => {
                let wr = Decimal::from(stats.wins) / Decimal::from(stats.total_trades);
                let avg_r = stats.total_r / Decimal::from(stats.total_trades);
                if wr > dec!(0.55) && avg_r > dec!(0.5) { dec!(1.2) }
                else if wr > dec!(0.45) && avg_r > dec!(0.2) { dec!(1.0) }
                else { dec!(0.60) }
            }
        }
    }

    pub fn record_trade(&mut self, head: HeadId, regime: Regime9, session: Session,
                        won: bool, r_multiple: Decimal) {
        let key = (head, regime, session);
        let stats = self.segments.entry(key).or_insert(SegmentStats::default());
        stats.total_trades += 1;
        if won { stats.wins += 1; stats.consecutive_losses = 0; }
        else { stats.consecutive_losses += 1; }
        if stats.consecutive_losses > stats.max_consecutive_l {
            stats.max_consecutive_l = stats.consecutive_losses;
        }
        stats.total_r += r_multiple;
    }
}
```

### 10.5 Consistency Tracker

```rust
pub struct ConsistencyTracker {
    pub daily_pnl_history:     VecDeque<(i64, Decimal)>,
    pub streak_losing_days:    u8,
    pub total_profitable_days: u32,
    pub total_trading_days:    u32,
    pub max_single_day_pct:    Decimal,  // Track for consistency alerts
}

impl ConsistencyTracker {
    pub fn record_day(&mut self, timestamp: i64, pnl: Decimal) {
        self.daily_pnl_history.push_back((timestamp, pnl));
        if self.daily_pnl_history.len() > 30 { self.daily_pnl_history.pop_front(); }
        self.total_trading_days += 1;
        if pnl > dec!(0) {
            self.total_profitable_days += 1;
            self.streak_losing_days = 0;
        } else {
            self.streak_losing_days += 1;
        }
    }

    pub fn is_paused_for_consistency(&self) -> bool {
        self.streak_losing_days >= 3
    }

    pub fn profitable_day_rate(&self) -> Decimal {
        if self.total_trading_days == 0 { return dec!(0); }
        Decimal::from(self.total_profitable_days) / Decimal::from(self.total_trading_days)
    }
}
```

---

## Part 11: Trade Management

```rust
pub struct TradeManager {
    config: TradeManagerConfig,
}

pub struct TradeManagerConfig {
    pub breakeven_at_r:     Decimal, // 1.0R -> move SL to entry
    pub partial1_at_r:      Decimal, // 1.5R -> close 50%
    pub partial1_pct:       Decimal, // 0.50
    pub trail_after_partial: bool,   // true
    pub trail_atr_mult:     Decimal, // 1.0 ATR per bar
    pub time_exit_hours:    u32,     // 48h if < +0.5R
    pub time_exit_min_r:    Decimal, // 0.5R
    pub adverse_retrace_pct: Decimal, // 0.50 (exit if retraces >50% of MFE)
}
```

**Management flow per open position per bar:**
1. Check time exit: if position age > 48h AND current R < 0.5R -> close
2. Check adverse retrace: if (MFE - current_profit) / MFE > 0.50 -> close
3. Check breakeven: if current R >= 1.0R AND SL not yet at entry -> move SL to entry
4. Check partial: if current R >= 1.5R AND no partial taken -> close 50%, move SL to entry
5. Check trailing: if partial taken, trail SL by 1.0 ATR per bar in profit direction
6. Check regime exit: if regime changed to Choppy/Transitioning -> close

---

## Part 12: Complete Signal Pipeline

```rust
pub fn process_bar(&mut self, bar: &Bar) -> Vec<ExecutionResult> {
    // 0. Update account state from broker
    self.account.update_equity(self.broker.equity());

    // 1. Kill switch check
    if self.kill_switch.check(&self.account, bar.timestamp) {
        return vec![];
    }

    // 2. Drift detection — is the bot still working as expected?
    let drift_mult = match self.drift_detector.evaluate() {
        DriftSignal::Halt { reason } => {
            log::error!("DRIFT HALT: {}", reason);
            self.kill_switch.activate(&reason);
            return vec![];
        }
        DriftSignal::ReduceRisk { multiplier } => multiplier,
        _ => dec!(1.0),
    };

    // 3. Account phase check — should we be trading at all?
    let phase_mult = self.account.phase_risk_multiplier();
    if phase_mult.is_zero() { return vec![]; }
    let dd_mult = self.account.dd_distance_multiplier();
    if dd_mult.is_zero() { return vec![]; }

    // 4. Daily P&L check
    let day_state = self.daily_pnl.update(self.account.current_equity, bar.timestamp);
    if !self.daily_pnl.can_trade() { return vec![]; }

    // 5. Temporal intelligence — urgency profile
    let urgency = self.temporal.urgency_profile(&self.account);
    if urgency == UrgencyProfile::Protect && self.account.profit_pct > dec!(0) {
        return vec![]; // Payout window with profit: sit on it
    }

    // 6. Session check
    let session = Session::from_utc_hour(utc_hour(bar.timestamp));
    let session_profile = SessionProfile::from(session);
    if session_profile.sizing_mult.is_zero() { return vec![]; } // Dead session

    // 7. Update regime
    let regime = match self.regime.update(bar) {
        None => return vec![], // Warming up
        Some(r) => r,
    };

    // 8. Evaluate allowed heads (regime-filtered)
    let allowed = regime.regime.allowed_heads();
    let mut signals: Vec<TradeSignal> = vec![];
    for head in &mut self.heads {
        if !allowed.contains(&head.id()) { continue; }
        // Performance ledger gate
        if !self.ledger.is_segment_allowed(head.id(), regime.regime, session) {
            continue;
        }
        signals.extend(head.evaluate(bar, &session_profile, &regime));
    }

    // 9. Filter: R:R, SL distance, urgency-adjusted minimum confidence
    let min_confidence = match urgency {
        UrgencyProfile::Protect       => dec!(0.90),
        UrgencyProfile::Coast         => dec!(0.75),
        UrgencyProfile::Normal        => dec!(0.55),
        UrgencyProfile::PushSelective => dec!(0.45),
    };
    let filtered: Vec<TradeSignal> = signals.into_iter()
        .filter(|s| s.rr_ratio().map_or(false, |rr| rr >= self.min_rr(s.head)))
        .filter(|s| s.sl_distance_pips(self.pip_size) >= dec!(2))
        .filter(|s| s.head_confidence >= min_confidence)
        .collect();

    // 10. Risk gate — apply combined multiplier stack
    let eq_filter = self.equity_curve_filter.multiplier();
    let effective_mult = self.account.effective_risk_multiplier(
        day_state, eq_filter, drift_mult
    );

    let decisions: Vec<RiskDecision> = filtered.into_iter()
        .map(|sig| {
            let seg_mult = self.ledger.risk_multiplier(sig.head, regime.regime, session);
            let final_mult = effective_mult * seg_mult * session_profile.sizing_mult;
            let base_risk = self.base_risk_pct();
            let adjusted = RiskPercent::clamped(base_risk.inner() * final_mult);
            match calculate_lots(adjusted, self.account.current_equity,
                                (sig.entry - sig.stop_loss).abs(),
                                self.pip_size, self.pip_value_per_lot,
                                self.min_lot, self.max_lot, self.lot_step) {
                Ok(lots) => RiskDecision::Execute {
                    signal: sig, risk_pct: adjusted, lots, is_pyramid: false,
                },
                Err(_) => RiskDecision::Reject {
                    signal: sig, reason: RejectReason::SlDistanceTooSmall,
                },
            }
        })
        .collect();

    // 11. Execute via smart execution engine
    decisions.into_iter()
        .map(|d| self.execution.execute(d))
        .collect()
}
```

---

## Part 13: Backtesting Pass/Fail Criteria

### Gate 1: Standard Backtest (2-Year)

| Metric | Minimum | Target |
|--------|---------|--------|
| Total return | >= 10.0% | >= 14% |
| Max drawdown | < 2.5% | < 2.0% |
| Win rate | >= 45% | >= 52% |
| Profit factor | >= 1.30 | >= 1.60 |
| Total trades | >= 150 | >= 300 |
| % profitable days | >= 55% | >= 62% |

> DD gates are set below Hyper Growth's 3% daily pause and 6% static stopout. We keep replay drawdown under 2.5% so daily pause noise, spread spikes, and open-equity carry do not invalidate the account.

### Gate 2: Walk-Forward (5-Fold)

| Metric | Minimum |
|--------|---------|
| OOS profit factor (each fold) | >= 1.20 |
| OOS max DD (each fold) | < 8.0% |
| Folds passing | 4 of 5 |

### Gate 3: Monte Carlo (10,000 Paths)

| Metric | Minimum |
|--------|---------|
| 5th percentile return | >= 0% |
| 95th percentile DD | < 5.5% |
| Ruin probability (DD > 6%) | < 5% |

### Gate 4: Challenge Simulation (100 Runs)

| Metric | Minimum |
|--------|---------|
| Pass rate | >= 65% |
| Avg days to pass | <= 25 |
| DD breach rate | <= 5% |

### Gate 5: Demo Forward (1-2 Weeks)

| Check | Requirement |
|-------|-------------|
| Avg slippage | <= 0.5 pip |
| Spread spikes handled | Bot pauses during spikes |
| No missed/duplicate orders | 0 incidents |

**RULE: NO PAID ATTEMPT until all 5 gates pass.**

---

## Part 14: Build Schedule

### Week 1 — Core Foundation
1. Workspace + 5 crates scaffolded, Cargo.toml deps pinned
2. All Rust types (Bar, TradeSignal, RiskPercent, RiskDecision, Session, Regime9, AccountState, all enums)
3. Head trait (single-bar API — HYDRA Bug 2 eliminated by signature)
4. All indicators: EMA, ATR, BB, ADX, Hurst, VWAP, Choppiness Index (streaming, single-bar)
5. Regime9 classifier
6. RiskPercent newtype (compile-time bounds — HYDRA Bug 1 eliminated)
7. Unit + property-based tests for all indicators and types

### Week 2 — Strategy Heads + Risk + Payout Architecture
8. MomentumHead
9. AsianRangeHead
10. BreakoutHead
11. Session detection + multipliers
12. Position sizing with `calculate_lots()`
13. Daily P&L engine + DayState
14. Kill switch + DD tracking
15. Trade manager (trailing stops, partials, time exits)
16. Equity curve filter
17. Challenge compliance (coasting logic + firm-specific enforcement for FundingPips profiles)
18. AccountState + AccountPhase state machine
19. DD distance multiplier + phase risk multiplier
20. TemporalIntelligence + UrgencyProfile
21. ConsistencyTracker
22. Pyramiding logic

### Week 3 — Broker + Data + Backtest + Execution Intelligence
23. cTrader OAuth 2.0 + TCP/TLS client
24. Mock broker (same Broker trait, no network)
25. Tick -> M1 -> M5/M15/H1/H4/D1 aggregator
26. SQLite schema + persistence
27. Historical data downloader -> Parquet
28. ExecutionEngine with spread-adjusted R:R gating
29. SpreadTracker + stale price detection
30. DriftDetector + DriftBenchmarks
31. PerformanceLedger (Head x Regime x Session)
32. Bar-by-bar replay engine
33. Walk-forward, Monte Carlo, challenge simulator
34. **Run full validation gauntlet. Iterate until all gates pass.**

### Week 4 — Demo Forward Validation
35. Connect to cTrader demo account
36. Run demo-forward for 1-2 weeks minimum
37. Measure slippage, spreads, execution quality
38. Log every fill, compare to backtest assumptions
39. Populate DriftBenchmarks from demo data
40. **GATE: Demo results must not invalidate backtest thesis**

### Week 5-6 — First Paid Attempt
41. If all gates pass: buy one The5ers Hyper Growth challenge ($260 for 5k)
42. Deploy in challenge mode
43. **No feature changes during the attempt**
44. Monitor daily via SQLite queries or simple CLI dashboard

### Post-Challenge — Scaling
45. If funded: trade conservatively, target first payout
46. Enable pyramiding only after 20+ live trades
47. Replicate to second account using profits only
48. **Only then** consider adding Phase 2 features

---

## Part 15: SQLite Schema

```sql
CREATE TABLE accounts (
    id INTEGER PRIMARY KEY,
    firm_name TEXT NOT NULL,
    broker_account_id INTEGER NOT NULL UNIQUE,
    phase TEXT NOT NULL,
    balance REAL NOT NULL,
    equity REAL NOT NULL,
    high_water_mark REAL NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE firm_symbols (
    firm_name TEXT NOT NULL,
    our_symbol TEXT NOT NULL,
    broker_symbol_id INTEGER NOT NULL,
    pip_size REAL NOT NULL,
    lot_size REAL NOT NULL,
    pip_value_per_lot REAL NOT NULL,
    swap_long REAL,
    swap_short REAL,
    typical_spread_pips REAL,
    commission_per_lot REAL,
    PRIMARY KEY (firm_name, our_symbol)
);

CREATE TABLE trades (
    id INTEGER PRIMARY KEY,
    account_id INTEGER NOT NULL,
    symbol TEXT NOT NULL,
    direction TEXT NOT NULL,
    head TEXT NOT NULL,
    regime TEXT NOT NULL,
    session TEXT NOT NULL,
    entry_price REAL NOT NULL,
    sl_price REAL NOT NULL,
    tp_price REAL NOT NULL,
    lots REAL NOT NULL,
    risk_pct REAL NOT NULL,
    pyramid_level INTEGER NOT NULL DEFAULT 0,
    opened_at INTEGER NOT NULL,
    closed_at INTEGER,
    close_price REAL,
    pnl_usd REAL,
    r_multiple REAL,
    close_reason TEXT,
    slippage_pips REAL
);

CREATE TABLE equity_snapshots (
    id INTEGER PRIMARY KEY,
    account_id INTEGER NOT NULL,
    balance REAL NOT NULL,
    equity REAL NOT NULL,
    daily_pnl_usd REAL NOT NULL,
    daily_dd_pct REAL NOT NULL,
    total_dd_pct REAL NOT NULL,
    day_state TEXT NOT NULL,
    snapshotted_at INTEGER NOT NULL
);

CREATE TABLE drift_log (
    id INTEGER PRIMARY KEY,
    signal TEXT NOT NULL,
    live_win_rate REAL,
    expected_win_rate REAL,
    live_avg_r REAL,
    live_avg_slippage REAL,
    action_taken TEXT NOT NULL,
    logged_at INTEGER NOT NULL
);
```

---

## Part 16: Config Schema

**`config/gadarah.toml`:**
```toml
[engine]
mode = "challenge"  # "challenge" or "funded"
symbols = ["GBPUSD", "EURUSD", "XAUUSD"]
log_level = "info"
db_path = "data/gadarah.db"

[risk]
base_risk_pct = 0.75        # Challenge: 0.50-1.0, Funded: 0.50-0.75
max_portfolio_heat = 2.0
daily_stop_pct = 1.5
daily_target_pct = 2.0

[kill_switch]
daily_dd_trigger_pct = 95.0
total_dd_trigger_pct = 95.0
consecutive_loss_limit = 3
cooldown_minutes = 30

[equity_curve]
ma_period = 20
below_ma_mult = 0.50
deep_below_mult = 0.25
deep_threshold_pct = 2.0

[pyramid]
enabled = false             # Enable after 20+ live trades
min_r_to_add = 1.0
max_layers = 2
add_size_fraction = 0.5

[drift]
min_trades = 20
win_rate_alert_delta = 0.12
win_rate_halt_delta = 0.20
avg_r_halt = -0.10
slippage_alert_mult = 2.0

[execution]
max_spread_atr_ratio = 0.30
stale_price_seconds = 2
min_net_rr = 1.2

[compliance.fundingpips]
# Ignored for non-FundingPips firm profiles.
blackout_file = "compliance/fundingpips_blackouts.toml"
```

**`config/compliance/fundingpips_blackouts.toml`:**
```toml
# Unix timestamps in UTC seconds.
# [[blackout_windows]]
# starts_at = 1775146200
# ends_at = 1775148000
# label = "USD NFP"

blackout_windows = []
```

**`config/firms/the5ers_hypergrowth.toml`:**
```toml
[firm]
name = "The5ers - Hyper Growth"
challenge_type = "hyper_growth"
profit_target_pct = 10.0
daily_dd_limit_pct = 3.0
max_dd_limit_pct = 6.0
dd_mode = "static"
min_trading_days = 0
news_trading_allowed = true
max_positions = 5
profit_split_pct = 80.0

[broker]
host = "live.ctraderapi.com"
port = 5035
client_id_env = "GADARAH_THE5ERS_CLIENT_ID"
client_secret_env = "GADARAH_THE5ERS_CLIENT_SECRET"
```

---

## Part 17: Verification Invariants (Pre-Live Checklist)

All must pass before any live deployment:

- [ ] RiskPercent rejects values outside [0.01, 5.0]
- [ ] Kill switch fires at exactly 95% of DD limits
- [ ] Lot size calculation matches manual hand-calculation on 10 spot checks
- [ ] Each head produces zero signals during Transitioning regime
- [ ] Each head produces zero signals during Dead session
- [ ] Connection loss pauses all trading, reconnect resumes
- [ ] Crash -> restart -> reconcile -> resume with correct state
- [ ] Daily DD resets at correct firm-specific time
- [ ] SL distance guard rejects signals with < 2 pips distance
- [ ] Challenge coasting reduces risk at correct thresholds (50/70/90%)
- [ ] Equity curve filter halves size when below 20-trade MA
- [ ] Pyramid add never increases total risk beyond original risk_usd
- [ ] AsianRangeHead resets correctly at UTC midnight
- [ ] Backtester produces identical results on identical seed data (deterministic)
- [ ] Mock broker simulates fills within specified slippage bounds
- [ ] Historical data is gap-free for selected symbols x 2 years
- [ ] DD distance multiplier returns 0.0 when within 0.5% of limit
- [ ] Phase risk multiplier returns 0.0 for AwaitingFunded and Failed
- [ ] Spread-adjusted R:R correctly rejects marginal trades
- [ ] Drift detector halts trading on negative expectancy
- [ ] Performance ledger disables combos with 5 consecutive losses
- [ ] DayState never regresses within same day (Protecting never goes back to Normal)

---

## Part 18: Revenue-First Metrics

| Metric | Definition | Target |
|--------|-----------|--------|
| Days to backtest-ready | Code start -> all gates pass | <= 21 |
| Days to demo-ready | Gates pass -> demo deployment | <= 7 |
| Days to first paid attempt | Demo proof -> challenge purchase | <= 14 |
| Challenge fee recovery time | First payout / challenge cost | < 30 days |
| Reset rate | Failed challenges / total attempts | < 35% |
| Payout frequency | Withdrawals per 30 days | >= 1 |
| Expected withdrawal per 30 days | Avg funded profit x split % | > $200 |
| Account survival after payout | Accounts still active post-withdrawal | > 80% |

**If a feature does not improve one of these metrics, it is delay disguised as sophistication. Cut it.**

---

## Part 19: Operating Rules

### Rule 1: Feature Admission
No feature enters the MVP because it sounds powerful. It enters only if it helps time to first payout, payout reliability, or reduction in reset risk.

### Rule 2: No Changes During Live Attempts
Do not add complexity during a live paid attempt. No new heads, no parameter tweaks, no "quick improvements."

### Rule 3: One Account Before Many
Do not scale account count before proving one-account profitability.

### Rule 4: Patience Over Drama
Do not risk the whole bankroll to avoid feeling slow. Slow and compounding beats dramatic and dead.

### Rule 5: Withdraw Early
External cash reserves are part of the strategy. First withdrawal changes the project from theory into a capital machine.

### Rule 6: Feature Kill Criteria
Every module must earn its place. If a head or filter does not improve pass rate, payout consistency, or materially delays deployment — cut it.

---

## Part 20: Phase 2 Expansion (AFTER First Payout Only)

The following are fully specified in `plan.md` and should be added in priority order only after the MVP produces a real withdrawal:

| Priority | Feature | Source | Justification |
|----------|---------|--------|---------------|
| 1 | TrendHead | plan.md Head 1 | High-conviction pullback entries for funded mode |
| 2 | SmcHead | plan.md Head 6 | Multi-TF confluence for higher R:R (requires SMC engine) |
| 3 | SMC engine | plan.md Part 3 Layer 1 | OB, FVG, BOS/ChoCH, liquidity detection |
| 4 | Simple news blackout | plan.md Head 5 Mode B | Avoid surprise losses during major events |
| 5 | Telegram notifications | plan.md Part 14 | Operational awareness without GUI |
| 6 | Multi-account routing | plan.md Part 15 | Scale proven edge across 2-3 accounts |
| 7 | GridHead | plan.md Head 3 | Range-trading in sideways markets |
| 8 | ScalpM5 | plan.md Head 8 | Quick trend-following scalps |
| 9 | Macro Filter | plan.md Part 6 | DXY/VIX/yields alignment (daily HTTP fetch) |
| 10 | Volume Profile | plan.md Part 4 | VPOC, VAH/VAL, HVN/LVN |
| 11 | ML signal scorer | plan.md Part 7 | 20-feature LightGBM->ONNX (needs 200+ trade logs) |
| 12 | Bayesian ensemble | plan.md Part 3 Layer 4 | 25-signal log-odds fusion |
| 13 | Sentiment engine | plan.md Part 3 4a | FinBERT via ONNX |
| 14 | Full iced GUI | plan.md Part 15 | Only if operating 3+ accounts justifies effort |
| 15 | NewsHead straddle | plan.md Head 5 | Pre/post-news trading (high execution risk) |
| 16 | ScalpM1 | plan.md Head 7 | M1 micro-breakouts (needs proven execution quality) |
| 17 | VolumeProfileHead | plan.md Head 10 | VP-specific entries (needs VP engine first) |
| 18 | Correlation matrix | plan.md Part 3 5e | Portfolio VaR, only matters at 3+ positions |
| 19 | Kelly criterion | plan.md Part 3 5b | Dynamic sizing (needs statistical data) |
| 20 | GARCH volatility | plan.md Part 3 | Conditional variance forecasting |

**Full specifications for all Phase 2 components exist in `plan.md` (2138 lines). Do not re-specify them here. Read `plan.md` when implementing each Phase 2 feature.**

---

## Part 21: Reference Code Locations

| Component | Source File |
|-----------|-----------|
| Pattern detector | `/home/ilovehvn/trading-system-merged/backend/app/engines/pattern_detector.py` |
| Regime detector | `/home/ilovehvn/trading-system-merged/backend/app/engines/regime_detector.py` |
| Expected value | `/home/ilovehvn/trading-system-merged/backend/app/engines/expected_value.py` |
| Risk engine | `/home/ilovehvn/trading-system-merged/backend/app/engines/risk.py` |
| FTMO strategy | `/home/ilovehvn/trading-system-merged/backend/app/engines/ftmo_strategy.py` |
| Ensemble scorer | `/home/ilovehvn/trading-system-merged/backend/app/engines/ensemble_scorer.py` |
| HYDRA indicators | `/home/ilovehvn/HYDRA/rust/strategy-core/src/indicators.rs` |
| HYDRA heads | `/home/ilovehvn/HYDRA/rust/strategy-core/src/heads/` |
| HYDRA kill switch | `/home/ilovehvn/HYDRA/rust/execution-engine/src/kill_switch.rs` |
| cTrader adapter | `/home/ilovehvn/HYDRA/go/internal/broker/ctrader/adapter.go` |
| Bug documentation | `/home/ilovehvn/HYDRA/CIELPLAN.md` |
| Session detection | `/home/ilovehvn/HYDRA/go/internal/orchestrator/session.go` |
| Trade manager | `/home/ilovehvn/HYDRA/go/internal/orchestrator/trademanager.go` |
| Performance weighting | `/home/ilovehvn/HYDRA/go/internal/orchestrator/performance.go` |

---

## Conclusion

This plan synthesizes 5 documents and 4,500+ lines of prior work into one actionable blueprint.

**From plan.md:** We take the Rust architecture, compile-time bug prevention, strong types, 9 strategy heads (as Phase 2 expansion), challenge compliance, daily P&L engine, equity curve filter, consistency tracker, pyramiding, backtesting rigor, SQLite schema, and config schema.

**From Gplan.md:** We take the urgency, the fat-tail capture through pyramiding, and the mindset that speed-to-payout is the only metric that matters early on.

**From gptplan.md:** We take the 3-head MVP, the staged bankroll ladder, the demo-before-paid validation, the operating rules, and the feature kill criteria.

**From SYNTHESIS.md:** We take the thesis — "conservative on ruin, aggressive on throughput" — and the principle that ruthless scope reduction is the real edge.

**From OPUSPLAN.md:** We take the AccountPhase state machine, DD distance multiplier, TemporalIntelligence with UrgencyProfile, DriftDetector, ExecutionEngine with spread-adjusted R:R, PerformanceLedger, verified prop firm research, and revenue-first metrics with concrete targets.

**New in ULTIMATE:** All of the above unified into a single document with no contradictions, clear MVP/expansion boundary, complete Rust types covering all systems (both MVP and payout-ensuring architecture), exact indicator implementations, and a build schedule that reaches first paid attempt in 5-6 weeks starting from $53-$80.

Build the narrow machine. Prove it. Get paid. Then expand.
