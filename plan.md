# GADARAH — Ultra-Detailed Full Stack Development Plan v2

**Date:** 2026-03-27

## Context

Previous bots (HYDRA, Ganimead) were polyglot systems (Rust+Go+Python over gRPC) designed for slow, conservative trading over weeks/months. HYDRA has 3 critical account-killing bugs (100x position sizing from unit mismatch, EMA double-counting, signal type confusion) stemming from cross-language serialization and buffer-passing patterns. GADARAH eliminates these bug classes by design: single language (Rust), single binary, in-process function calls, single-bar indicator API.

**The goal:** A fully automated Rust trading system that beats prop firm 1-step and multi-step challenges in ≤3 trading days, then produces consistent daily profit on funded accounts at 8-10% monthly — managing up to 7 accounts simultaneously with a tacticool native GUI.

**New in v2:** 9 strategy heads (up from 6), Daily P&L Management Engine, Volume Profile Analysis, Intermarket Macro Filter, Pyramiding Logic, Re-Entry Logic, Equity Curve Filter, 20-feature ML model, exact Rust type definitions, quantified backtesting pass/fail criteria, and daily consistency mechanics.

---

## Part 1: Prop Firm Strategy

### Recommended Firms (all allow bots on cTrader)

| Firm | Challenge | Target | Daily/Max DD | Profit Split | Payouts | Account Sizes | Price (100k) |
|------|-----------|--------|-------------|-------------|---------|---------------|-------------|
| **FTMO** | 1-step | 10% | 5%/10% | 80-90% | Monthly | 10k-200k+ | €540 |
| **E8 Markets** | 1-step | Customizable | Customizable | Up to 100% | Fast/automated | 5k-500k | Varies |
| **BrightFunded** | 2-phase | 8%/5% | 5%/10% static | 80-100% | Weekly (24hr) | Custom, scales 30%/4mo | $55-$975 |
| **The5ers** | 2-step | 10%/5% | 5%/10% | 80-100% | Bi-weekly | Up to 100k→4M | $495 |
| **FTUK** | 1/2-step | 10% | 4%/8% | N/A | N/A | 5k→6.4M | N/A |

**Selected firms (initial):**
1. **FTMO** — 1-step, 10% target, 5%/10% DD, 80-90% profit split, monthly payouts, most established
2. **BrightFunded** — 2-phase (8%/5% targets), 5%/10% DD, 80-100% profit split, weekly payouts (24hr guaranteed), scales 30% every 4 months

Start with 2-3 accounts, scale to 7 after system proves consistent. BrightFunded's weekly payouts provide steady cash flow; FTMO provides higher per-payout amounts.

Capital allocation:
- 2× FTMO $100K challenges = €1,080
- 1× BrightFunded $100K = ~$975
- **Total initial: ~$2,055**
- Expected first funded month revenue: $8,000-16,000 per account at 80% split

---

## Part 2: Exact Rust Type Definitions

All types defined here before any implementation begins. This is the contract every crate must honor.

### 2.1 Bar

```rust
// gadarah-core/src/types.rs

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bar {
    pub open:      Decimal,
    pub high:      Decimal,
    pub low:       Decimal,
    pub close:     Decimal,
    pub volume:    u64,        // tick count (forex tick volume — no real lot volume available)
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

### 2.2 TradeSignal

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HeadId {
    Trend, Breakout, Grid, Momentum, News, Smc,
    ScalpM1, ScalpM5, AsianRange, VolProfile,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Direction { Buy, Sell }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SignalKind {
    Open,        // New position entry
    Close,       // Close existing position
    AddPyramid,  // Add to existing winning position
    ReEntry,     // Re-entry after missed signal cleared
    Adjust,      // Move SL/TP on existing position
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeSignal {
    pub symbol:           String,
    pub direction:        Direction,
    pub kind:             SignalKind,
    pub entry:            Decimal,          // 0 = market order
    pub stop_loss:        Decimal,
    pub take_profit:      Decimal,          // Primary TP
    pub take_profit2:     Option<Decimal>,  // Second TP for partial
    pub take_profit3:     Option<Decimal>,  // Third TP for final close
    pub head:             HeadId,
    pub head_confidence:  Decimal,          // [0.0, 1.0] from head
    pub regime:           Regime9,
    pub session:          Session,
    pub smc_confluence:   u8,               // 0-5 count of aligned SMC factors
    pub pyramid_level:    u8,               // 0=initial, 1/2=pyramid adds
    pub comment:          String,
    pub generated_at:     i64,
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

### 2.3 RiskPercent Newtype — Eliminates HYDRA Bug 1 at Compile Time

```rust
// gadarah-risk/src/types.rs

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct RiskPercent(Decimal);

impl RiskPercent {
    pub const MIN: Decimal = dec!(0.01);
    pub const MAX: Decimal = dec!(10.0);

    pub fn new(pct: Decimal) -> Result<Self, RiskError> {
        if pct < Self::MIN || pct > Self::MAX {
            return Err(RiskError::InvalidRiskPercent { value: pct });
        }
        Ok(Self(pct))
    }

    /// Clamp to valid range — used in sizer output after multipliers applied.
    pub fn clamped(pct: Decimal) -> Self {
        Self(pct.max(Self::MIN).min(Self::MAX))
    }

    pub fn inner(&self) -> Decimal { self.0 }
    /// Convert to fractional for lot size math: 1.5% → 0.015
    pub fn as_fraction(&self) -> Decimal { self.0 / dec!(100) }
}
```

### 2.4 Head Trait — Eliminates HYDRA Bug 2 by Type Signature

```rust
// gadarah-core/src/heads/mod.rs

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
        smc:     &SmcContext,
        session: &SessionProfile,
        regime:  &RegimeSignal9,
    ) -> Vec<TradeSignal>;

    /// Reset on session start or regime flip.
    fn reset(&mut self);

    /// Bars required before this head can produce valid signals.
    fn warmup_bars(&self) -> usize;

    /// Whether this head is permitted in the given regime.
    fn regime_allowed(&self, regime: &RegimeSignal9) -> bool;
}
```

### 2.5 SmcContext

```rust
// gadarah-core/src/smc/mod.rs

#[derive(Debug, Clone)]
pub struct SmcContext {
    pub d1_bias:              Option<Direction>,
    pub h4_bos_direction:     Option<Direction>,
    pub h1_order_blocks:      Vec<OrderBlock>,
    pub h1_fvgs:              Vec<FairValueGap>,
    pub m15_order_blocks:     Vec<OrderBlock>,
    pub m15_fvgs:             Vec<FairValueGap>,
    pub m15_last_bos:         Option<BreakOfStructure>,
    pub liquidity_levels:     Vec<LiquidityLevel>,
    pub sweep_detected:       bool,
    pub vpoc:                 Option<Decimal>,
    pub vah:                  Option<Decimal>,
    pub val:                  Option<Decimal>,
    pub bullish_factor_count: u8,
    pub bearish_factor_count: u8,
}

#[derive(Debug, Clone)]
pub struct OrderBlock {
    pub id:          u64,
    pub direction:   Direction,
    pub top:         Decimal,
    pub bottom:      Decimal,
    pub formed_at:   i64,
    pub strength:    Decimal,    // ATR-normalized impulse size
    pub touch_count: u8,         // Invalidate after >= 2
    pub bars_age:    u32,        // Invalidate after >= 200
    pub fvg_overlap: bool,
    pub mitigated:   bool,       // body-closed through = dead
}

#[derive(Debug, Clone)]
pub struct FairValueGap {
    pub id:          u64,
    pub direction:   Direction,
    pub top:         Decimal,
    pub bottom:      Decimal,
    pub formed_at:   i64,
    pub fill_pct:    Decimal,    // 0.0-1.0
    pub decay_factor: Decimal,   // exp(-0.01 * bars_since_formed)
    pub mitigated:   bool,       // >50% filled = dead
}

#[derive(Debug, Clone)]
pub struct BreakOfStructure {
    pub direction:    Direction,
    pub broken_level: Decimal,
    pub formed_at:    i64,
    pub is_choch:     bool,  // Change of Character vs true BOS
}

#[derive(Debug, Clone)]
pub struct LiquidityLevel {
    pub price:      Decimal,
    pub level_type: LiquidityType,
    pub strength:   u8,  // 1-5 times tested
    pub swept:      bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiquidityType {
    EqualHighs, EqualLows, PreviousDayHigh, PreviousDayLow,
    RoundNumber, SwingHigh, SwingLow,
}
```

### 2.6 FusionContext

```rust
// gadarah-core/src/fusion/mod.rs

#[derive(Debug, Clone)]
pub struct FusionContext {
    pub sentiment_score:    Decimal,    // [-1.0, +1.0]
    pub sentiment_aligned:  bool,
    pub mins_to_next_news:  u32,
    pub news_blackout:      bool,
    pub ml_score:           Decimal,    // [0.0, 1.0]
    pub ensemble_score:     Decimal,    // [0.0, 1.0]
    pub ev_grade:           EvGrade,
    pub garch_vol_forecast: Decimal,
    pub garch_low_vol:      bool,
    pub macro_filter:       MacroFilterResult,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvGrade {
    Excellent,   // EV > +0.5R
    Good,        // EV > +0.3R
    Acceptable,  // EV > +0.15R
    Marginal,    // EV > +0.05R
    Negative,    // EV <= 0 — BLOCKED
}
```

### 2.7 RiskDecision

```rust
// gadarah-risk/src/types.rs

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
    Defer {
        signal:   TradeSignal,
        reason:   DeferReason,
        retry_at: i64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RejectReason {
    KillSwitchActive, DailyDDLimitReached, TotalDDLimitReached,
    NewsBlackout, SpreadTooHigh, VolatilityHalt, EvNegative,
    EnsembleScoreTooLow, MlScoreTooLow, CorrelationGroupFull,
    PortfolioHeatMax, MaxPositionsReached,
    SlDistanceTooSmall,     // < 2 pips — prevents HYDRA Bug 3 class
    EquityCurveFilter,      // Below 20-trade equity MA
    MacroFilterBlocked,     // Intermarket macro says no
    DailyTargetReached,     // Daily P&L engine says done for the day
    SessionNotAllowed, HeadDisabledByAdaptive, ComplianceFirmRule,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeferReason {
    SpreadTemporallyHigh, NewsEventApproaching, WaitingForFill,
}
```

### 2.8 Regime9 — Full 9-State Enum

```rust
// gadarah-core/src/regime.rs

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
                &[HeadId::Trend, HeadId::Momentum, HeadId::Smc, HeadId::ScalpM5],
            Self::WeakTrendUp | Self::WeakTrendDown =>
                &[HeadId::Trend, HeadId::Smc, HeadId::ScalpM5],
            Self::RangingTight =>
                &[HeadId::Grid, HeadId::AsianRange, HeadId::VolProfile],
            Self::RangingWide =>
                &[HeadId::Grid, HeadId::Breakout, HeadId::AsianRange, HeadId::VolProfile],
            Self::Choppy =>
                &[HeadId::Grid],
            Self::BreakoutPending =>
                &[HeadId::Breakout, HeadId::ScalpM1, HeadId::VolProfile],
            Self::Transitioning =>
                &[],  // No new trades during regime uncertainty
        }
    }

    pub fn is_trending(&self) -> bool {
        matches!(self, Self::StrongTrendUp | Self::StrongTrendDown | Self::WeakTrendUp | Self::WeakTrendDown)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegimeSignal9 {
    pub regime:           Regime9,
    pub confidence:       Decimal,
    pub adx:              Decimal,
    pub hurst:            Decimal,
    pub atr_ratio:        Decimal,       // ATR14 / ATR50
    pub bb_width_pctile:  Decimal,       // percentile in [0,1]
    pub choppiness_index: Decimal,
    pub computed_at:      i64,
}
```

---

## Part 3: Trading Strategy — The Intelligence Layer

### Philosophy: Confluence-Driven, Multi-Layer Decision Engine
No single indicator or strategy decides a trade. Every signal must pass through a **5-layer confluence pipeline** before execution.

```
Layer 1: Market Structure (SMC)    → WHERE to trade (OBs, FVGs, liquidity zones, VPOC)
Layer 2: Regime Classification     → WHAT type of market (9-state: trending/ranging/volatile)
Layer 3: Strategy Heads            → Directional signals with entry/SL/TP (9 heads)
Layer 4: Intelligence Fusion       → Score via sentiment, news, ML, macro, ensemble
Layer 5: Risk Gate                 → Compliance, sizing, kill switch, correlation, daily P&L
```

---

### Layer 1: Smart Money Concepts (SMC) Engine — `gadarah-core/src/smc/`
**Ported from:** `/home/ilovehvn/trading-system-merged/backend/app/engines/`

- **Order Block Detection:** Last opposing candle before an impulsive BOS move. Zone tracked with: top/bottom, ATR-normalized strength, touch count (expires at 2), age decay (expires at 200 bars), FVG overlap flag. Invalidated when body closes through.
- **Fair Value Gap Detection:** 3-candle imbalance (bullish: candle[i+1].low > candle[i-1].high). Tracks fill percentage, exponential decay (λ=0.01/bar), mitigated at 50% fill.
- **Break of Structure (BOS):** Swing point tracking with pivot strength=3. Bullish BOS = breaks previous swing high; bearish = breaks swing low. Tracks ChoCH (internal swing BOS) separately.
- **Liquidity Sweep Detection:** Price pierces key S/R level then reverses sharply. Identifies: equal highs/lows, previous day high/low, round numbers.
- **Multi-Timeframe Hierarchy:** D1 bias → H4 BOS/ChoCH → H1 OB+FVG zones → M15 entry confirmation.

**Output:** `SmcContext` (see Part 2).

---

### Layer 2: Regime Classifier — `gadarah-core/src/regime.rs`
**Port from:** `/home/ilovehvn/trading-system-merged/backend/app/engines/regime_detector.py` (668 LOC) — 9-state version

Features fed into classifier:
- Hurst exponent (trending >0.5, mean-reverting <0.5)
- ADX-14 (strength)
- ATR-14 / ATR-50 ratio (current vs long-term volatility)
- BB-20 width percentile (0=squeeze, 1=expansion)
- Choppiness Index: `100 × ln(ATR_sum_14 / (high_14 - low_14)) / ln(14)` (≥61.8=choppy, ≤38.2=trending)

Classification rules:
- `StrongTrendUp`: ADX > 25, Hurst > 0.6, close > EMA-20 > EMA-200
- `StrongTrendDown`: ADX > 25, Hurst > 0.6, close < EMA-20 < EMA-200
- `WeakTrendUp/Down`: ADX 20-25, Hurst 0.52-0.6
- `RangingTight`: Hurst < 0.45, BB width < 30th pctile, CI > 55
- `RangingWide`: Hurst < 0.45, BB width 30-60th pctile
- `Choppy`: CI > 61.8, ADX < 20
- `BreakoutPending`: BB squeeze (width < 20th pctile for 10+ bars) about to expand
- `Transitioning`: confidence < 0.30 OR gap between top-2 scores < 0.06

---

### Layer 3: Strategy Heads — All 9 Heads

**Single-bar API enforced on all heads.** All implement `Head` trait.

#### Head 1: TrendHead — `src/heads/trend.rs`
**Port from:** HYDRA TrendHead, bug-fixed

| Parameter | Value |
|---|---|
| EMA periods | 20 / 200 |
| Regime required | StrongTrendUp/Down, WeakTrendUp/Down |
| Entry trigger | Price pulls back to within 1.0 ATR of EMA-20 |
| Confirmation | Rejection candle (wick > 1.5× body, close in favorable half) |
| SMC alignment | D1 bias must match direction OR be neutral |
| SMC block | No active H1 OB between price and TP |
| Stop loss | Recent 5-bar swing low/high OR EMA-20 ± 1.5 ATR (whichever tighter) |
| TP1 (50% close) | Entry ± 2.0 ATR |
| TP2 (30% close) | Entry ± 3.5 ATR |
| TP3 (20% close) | Next H4 OB or S/R level |
| Min R:R | 1.5 |
| Breakeven | At +1.0R |
| Trail after TP1 | Trail by EMA-20 value each bar |
| Pyramid | At TP1, add 50% of original (see Part 5) |
| Time exit | 72h if < +0.5R |
| Regime exit | If Ranging/Choppy/Transitioning → close |
| Warmup | 200 bars |

#### Head 2: BreakoutHead — `src/heads/breakout.rs`
**Port from:** HYDRA BreakoutHead, bug-fixed

| Parameter | Value |
|---|---|
| Regime required | BreakoutPending, RangingWide |
| Setup | BB width in bottom 30th pctile for 10+ bars (squeeze) |
| Entry trigger | Current bar BB width > 50th pctile (expansion) AND closes outside BB band |
| Volume | >= 1.3× 20-bar average |
| SMC | Active M15 FVG in breakout direction |
| Stop loss | Inside the squeeze (opposite band at breakout time) |
| TP1 (40%) | Entry ± 2.0 ATR |
| TP2 (60%) | Entry ± 3.0 ATR |
| Min R:R | 1.8 |
| Fake breakout guard | If price closes back inside BB within 2 bars → exit immediately |
| Warmup | 50 bars |

#### Head 3: GridHead — `src/heads/grid.rs`
**Port from:** HYDRA GridHead

| Parameter | Value |
|---|---|
| Regime required | RangingTight (primary), RangingWide (secondary) |
| Range boundaries | 20-bar high/low + Fibonacci 38.2/61.8 of last swing |
| Grid spacing | 1.0 ATR per level |
| Grid levels | 3 buy below mid, 3 sell above mid |
| Entry trigger | Price touches grid level (within 0.2 ATR) |
| Volume scaling | 1.0× at L1, 1.3× at L2, 1.3²× at L3 (geometric) |
| Max positions | 6 per symbol (3 buy + 3 sell) |
| Basket TP | Range midpoint for buy basket; midpoint for sell basket |
| Hedge guard | If Ranging→Trending: close all losing legs, keep winning |
| Portfolio heat | Grid positions count as 0.5× for heat calculation |
| Choppy guard | If Choppy: level 1 only, no L2/L3 |
| Warmup | 50 bars |

#### Head 4: MomentumHead — `src/heads/momentum.rs`
**NEW**

| Parameter | Value |
|---|---|
| Session required | London (07:00-09:30 UTC) OR NY (13:30-16:00 UTC) |
| Regime required | StrongTrend, WeakTrend, BreakoutPending |
| Setup | First 60-min range from session open (tracked per session) |
| Entry trigger | Close above first-hour high (bull) or below first-hour low (bear) |
| VWAP filter | Price > VWAP from session open for buy; < VWAP for sell |
| BOS required | M15 BOS in breakout direction confirmed in last 30 min |
| SMC preferred | Breakout into area with cleared liquidity (swept equal highs above/below) |
| Stop loss | Midpoint of first-hour range |
| TP1 (50%) | Range height projected beyond breakout level (1:1 projection) |
| TP2 (50%) | 2× range height |
| Max per session | 1 signal per session |
| News guard | No entry within 30 min of major scheduled news |
| Warmup | 80 bars for VWAP |

#### Head 5: NewsHead — `src/heads/news.rs`
**NEW**

**Mode A — Pre-news straddle:**

| Parameter | Value |
|---|---|
| Timing | 30-60 seconds before event |
| Events | NFP, CPI, FOMC, ECB, BoE rate, GDP (high-impact only) |
| Spread gate | Spread ≤ 1.5× normal (if already wide, skip) |
| Orders | Buy stop: price + 15 pips, Sell stop: price - 15 pips |
| TP | 30 pips (both sides) |
| SL | 10 pips from pending order level |
| Cancel unfilled | 60s after event, or when other side fills |
| Firm compliance | Check firm TOML `news_trading_allowed` before firing |
| Max concurrent | 1 straddle per symbol |

**Mode B — Post-news continuation:**

| Parameter | Value |
|---|---|
| Timing window | 5-15 minutes after event |
| Prerequisite | Actual vs Forecast deviation > 1 std dev of historical surprises for this event type |
| Direction | In direction of the initial post-news spike |
| Entry | First M5 pullback (20-50% retrace of spike) to broken S/R |
| Confirmation | M5 rejection candle at pullback zone |
| Stop loss | Below pullback low (bull) + 0.5 ATR |
| TP | 150% of initial spike distance |
| Time limit | No entry after 15 min post-event |
| Spread gate | Spread must return to ≤ 2× normal before entry |

#### Head 6: SmcHead — `src/heads/smc_head.rs`
**NEW — Highest conviction head**

| Parameter | Value |
|---|---|
| Regime | All except Transitioning |
| Minimum confluence | 3 of 5 factors present simultaneously |
| Factor 1 | D1 BOS in trade direction |
| Factor 2 | Price inside or touching valid H1 OB in trade direction |
| Factor 3 | Active H1 or M15 FVG in same zone as OB |
| Factor 4 | Recent M15 BOS (within 20 bars) confirming direction |
| Factor 5 | Liquidity sweep at/before OB zone |
| Entry confirmation | M15 rejection candle inside OB zone |
| Stop loss | Below OB bottom for bull (body-close-through = OB broken → exit) |
| TP1 (40%) | Previous swing high/low (last significant structure) |
| TP2 (30%) | Next H4 FVG midpoint |
| TP3 (30%) | D1 previous structure level |
| Min R:R | 2.0 |
| Breakeven | At TP1 (move SL to OB top for bull) |
| OB invalidation | If OB mitigated: exit immediately regardless of P&L |
| Pyramid | At TP1, add 50% |
| Warmup | 200 bars D1 |

#### Head 7: ScalpM1 — `src/heads/scalp_m1.rs`
**NEW**

Config:
```rust
pub struct ScalpM1Config {
    pub range_bars:        usize,   // 5
    pub atr_period:        usize,   // 14
    pub sl_atr_mult:       Decimal, // 0.5
    pub tp_atr_mult:       Decimal, // 1.0 → 2:1 R:R with 0.5 SL
    pub min_range_pips:    Decimal, // 3.0
    pub max_range_pips:    Decimal, // 15.0
    pub volume_spike_mult: Decimal, // 1.3
    pub min_rr:            Decimal, // 1.5
}
```

| Parameter | Value |
|---|---|
| Regime required | BreakoutPending |
| Session required | London (07-12 UTC) or NY (13-16 UTC) |
| Setup | Last 5 M1 bars form tight consolidation: range < 15 pips |
| Entry trigger | Current bar closes above 5-bar high OR below 5-bar low |
| Volume | >= 1.3× average of consolidation bars |
| H1 SMC | D1 bias aligns OR H1 OB in breakout direction |
| ATR gate | ATR(14) > 3 pips (no scalping flat markets) |
| SL gate | SL distance >= 2 pips |
| Stop loss | Opposite extreme of 5-bar range + 0.5 ATR |
| Take profit | Single TP: entry ± (SL distance × 2.0). No partials. |
| Time exit | 15 M1 bars if not at target |
| Regime exit | If BreakoutPending → Ranging: close immediately |
| Pyramid | None |
| Warmup | 20 bars |

#### Head 8: ScalpM5 — `src/heads/scalp_m5.rs`
**NEW**

Config:
```rust
pub struct ScalpM5Config {
    pub ema_period:            usize,   // 8
    pub vwap_period_bars:      usize,   // 60 (5-hour VWAP)
    pub atr_period:            usize,   // 14
    pub sl_atr_mult:           Decimal, // 0.75
    pub tp1_atr_mult:          Decimal, // 1.5 (60% close)
    pub tp2_atr_mult:          Decimal, // 2.5 (40% close)
    pub min_rr:                Decimal, // 1.8
    pub max_entry_per_session: u8,      // 2
    pub pullback_depth_min:    Decimal, // 0.20 (min 20% retrace of last impulse)
    pub pullback_depth_max:    Decimal, // 0.50 (max 50% retrace)
}
```

| Parameter | Value |
|---|---|
| Regime required | StrongTrendUp/Down |
| H1 context | TrendHead: EMA-20 above/below EMA-200 |
| Setup | M5 price pulled back to within 0.75 ATR of EMA-8 on M5 |
| Pullback depth | 20-50% of last M5 impulse swing |
| Confirmation | Rejection candle: body >= 60% of range, close in favorable half |
| VWAP filter | VWAP(60) below entry for buys, above for sells |
| SMC gate | No active H1 OB between entry and TP in opposing direction |
| Session | London or NY (not Asian) |
| Max per session | 2 |
| Stop loss | Low of rejection bar - 0.5 ATR (bull) / High + 0.5 ATR (bear) |
| TP1 (60%) | Entry ± 1.5 ATR |
| TP2 (40%) | Entry ± 2.5 ATR |
| Trail after TP1 | Move SL to breakeven. Trail by EMA-8. |
| Pyramid | Add 1 unit at TP1 |
| Warmup | 80 bars |

#### Head 9: AsianRangeHead — `src/heads/asian_range.rs`
**NEW — High daily consistency impact**

Config:
```rust
pub struct AsianRangeConfig {
    pub asian_start_utc:    u8,      // 0
    pub asian_end_utc:      u8,      // 7
    pub entry_window_end:   u8,      // 9
    pub min_range_pips:     Decimal, // 15.0
    pub max_range_pips:     Decimal, // 80.0
    pub sl_buffer_pips:     Decimal, // 5.0
    pub tp_multiplier:      Decimal, // 1.5
    pub min_rr:             Decimal, // 1.2
    pub require_bos:        bool,    // true
    pub volume_confirm:     bool,    // true
    pub max_trades_per_day: u8,      // 1
}

pub struct AsianRangeState {
    pub asian_high:        Option<Decimal>,
    pub asian_low:         Option<Decimal>,
    pub trade_taken_today: bool,
    pub signal_expired:    bool,
    pub current_day:       i64,
}
```

| Parameter | Value |
|---|---|
| Entry window | UTC 07:00-09:00 only |
| Asian range gate | 15 ≤ range ≤ 80 pips, high/low both set |
| Trade limit | 1 per day per symbol |
| Entry trigger | H1 bar closes above `asian_high + sl_buffer` (bull) OR below `asian_low - sl_buffer` (bear) |
| BOS required | H1 SMC m15_last_bos in breakout direction |
| Volume required | Breakout bar >= 1.2× Asian session average |
| D1 bias | Must align with breakout OR neutral |
| News guard | No entry within 30 min (UK news at London open) |
| Entry type | Market order (DO NOT use limits — breakouts gap) |
| Stop loss | `asian_low + range/2` (bull) OR tighter of range-based vs low-5pips |
| TP1 (50%) | `asian_high + range × 1.0` (100% projection) |
| TP2 (remaining) | `asian_high + range × 1.5` (150% projection) |
| Trail after TP1 | Move SL to `asian_high + 2 pips` (broken range acts as support) |
| Daily reset | UTC midnight: reset asian_high, asian_low, trade_taken_today |
| Warmup | 200 bars (range building requires history) |

**Asian range builder:**
```rust
fn update_asian_range(&mut self, bar: &Bar) {
    let h = utc_hour(bar.timestamp);
    if h >= self.config.asian_start_utc && h < self.config.asian_end_utc {
        self.state.asian_high = Some(self.state.asian_high.unwrap_or(bar.high).max(bar.high));
        self.state.asian_low  = Some(self.state.asian_low.unwrap_or(bar.low).min(bar.low));
    }
}
```

---

### Layer 4: Intelligence Fusion — `gadarah-core/src/fusion/`

#### 4a. Sentiment Engine — `fusion/sentiment.rs`
**Reference:** `/home/ilovehvn/HYDRA/python/news/sentiment.py`

- FinBERT via ONNX Runtime (`ort` crate). No Python dependency. <10ms per batch.
- Sources: ForexFactory (15-min scrape), FXStreet RSS, DailyFX RSS, Reuters forex
- Rolling 4-hour weighted average per currency pair → [-1.0, +1.0]
- If aligned: +15% ensemble confidence. If opposes: -20%. If < -0.5: block signal.

#### 4b. News Proximity Scoring — `fusion/news_calendar.rs`
**Reference:** `/home/ilovehvn/HYDRA/python/news/economic_calendar.py`

- Pre-loaded calendar: 9 central bank event types + NFP, CPI, GDP
- Rules:
  - >120 min → no adjustment
  - 30-120 min → -10% confidence
  - 5-30 min → -30% confidence (unless NewsHead)
  - <5 min → block all non-NewsHead signals
  - NFP Friday → 50% position sizing reduction (unless NewsHead)

#### 4c. ML Signal Quality Scorer — `fusion/ml_scorer.rs`
LightGBM → ONNX. 20-feature input vector (see Part 7). Output: P(profitable) ∈ [0.0, 1.0]. Threshold: 0.55.

#### 4d. Ensemble Bayesian Scorer — `fusion/ensemble.rs`
**Reference:** `/home/ilovehvn/trading-system-merged/backend/app/engines/ensemble_scorer.py`

Log-odds combination with Beta(α,β) per signal source. 25 signal sources:

| Category | Signals |
|----------|---------|
| **Pattern (11)** | double_top/bottom, engulfing, pin_bar, morning/evening_star, hammer/shooting_star, soldiers/crows, doji_reversal |
| **Structure (2)** | bos_alignment (0.8), choch_reversal (0.7) |
| **Zone (2)** | order_block_proximity, fvg_zone_entry (0.85 if confluent) |
| **Market (5)** | mtf_alignment, session_quality, regime_confidence, volume_confirmation, liquidity_sweep |
| **Intelligence (4)** | sentiment_alignment, news_proximity, ml_quality_score, ev_grade |
| **Volatility (1)** | garch_low_vol |

Cold-start heuristic: With <30 trades, boost score by `n_signals × 0.08 × avg_strength`.
Minimum ensemble score: 0.45 (challenge), 0.55 (funded).

#### 4e. Intermarket Macro Filter — `fusion/macro_filter.rs`
**(See Part 6 for full specification)**

---

### Layer 5: Risk Gate — `gadarah-risk/`

#### 5a. Position Sizing (Exact Formula)
**Reference:** `/home/ilovehvn/trading-system-merged/backend/app/engines/expected_value.py`

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
    let actual_risk_pct = final_lots * sl_pips * pip_value_per_lot / account_equity * dec!(100);
    if actual_risk_pct > risk_pct.inner() * dec!(1.05) {
        return Err(SizingError::RoundingExceededRisk { computed: actual_risk_pct });
    }
    Ok(final_lots)
}
```

#### 5b. Kelly Criterion + Fractional Kelly
- `Kelly% = W - [(1-W) / R]` with 0.25 fraction default
- Multi-level hierarchy: Pattern+Regime → Pattern → Regime → Overall → Default
- Caps: 0.5%-2.5% (challenge) / 0.5%-1.0% (funded)

#### 5c. EV Tracking — `risk/expected_value.rs`
- Per-segment: overall, per-pattern, per-regime, per-symbol, per-pattern×regime
- EV grades: EXCELLENT (>0.5R), GOOD (>0.3R), ACCEPTABLE (>0.15R), MARGINAL (>0.05R), NEGATIVE (blocked)
- Minimum 30 trades for statistical validity; Bonferroni correction applied
- Cost-adjusted: includes spread + commission + slippage (default 5% of risk)

#### 5d. Kill Switch — `risk/kill_switch.rs`
- 95% of daily DD limit reached
- 95% of total DD limit reached
- 5 consecutive losses → 30-minute cooldown
- >1% equity drop in 60s (flash crash)
- Spread >3x normal → pause new entries
- ATR >3x normal → volatility halt, gradual recovery at 25%→50%→75%→100% sizing
- Per-account independence

#### 5e. Portfolio Risk — `risk/correlation.rs`
- Rolling 100-bar Pearson correlation on log returns
- Correlation groups: USD pairs, risk-on, safe haven, JPY pairs
- Max 2 concurrent positions per correlation group
- Portfolio VaR₉₉ capped at 80% of daily DD limit
- Max portfolio heat: 3% of account equity

#### 5f. Coasting Logic — `risk/challenge_tracker.rs`

| Progress to Target | Risk per Trade |
|---|---|
| 0-50% | Full risk (2.0-2.5%) |
| 50-70% | Reduce to 1.5% |
| 70-90% | Reduce to 1.0% |
| 90-100% | Coast at 0.5% |

---

### Challenge Mode vs Funded Mode

| Parameter | Challenge Mode | Funded Mode |
|-----------|---------------|-------------|
| Objective | 10% in ≤3 days | 8-10% monthly |
| Risk per trade | 2.0-2.5% (coasting to 0.5%) | 0.5-1.0% |
| Timeframe primary | M15 | H1 |
| Active heads | All 9 | TrendHead, BreakoutHead, GridHead, SmcHead, AsianRange, ScalpM5 |
| Ensemble threshold | 0.45 | 0.55 |
| Min EV grade | ACCEPTABLE (>0.15R) | GOOD (>0.3R) |
| Session focus | London/NY overlap (12-16 UTC) | All sessions |
| News trading | Aggressive (straddle + continuation) | Defensive (filter only) |
| Symbols | GBPUSD, EURUSD, XAUUSD | Full 6 pairs |
| Partial profits | 50% at 1.5R, SL to BE | 33% at 1R, 33% at 2R |
| Max trades/day | 5 | 3 |
| Daily target | 3.3% | 0.40% |

---

### Candlestick Patterns — `gadarah-core/src/patterns.rs`
**Port from:** `/home/ilovehvn/trading-system-merged/backend/app/engines/pattern_detector.py` (3,671 LOC)

**28 patterns** with ATR-relative thresholds and confidence scoring:
- **Single (4):** Doji, Marubozu, Spinning Top, High-Wave
- **Hammer variants (4):** Hammer, Inverted Hammer, Shooting Star, Hanging Man
- **Two-candle (6):** Pin Bar bull/bear, Engulfing bull/bear, Harami, Tweezer
- **Three-candle (8):** Morning/Evening Star, Three Soldiers/Crows, Kicker bull/bear, Tri-Star
- **Continuation (2):** Tasuki Gap, Kicking
- **Multi (4):** Double Top/Bottom, Head & Shoulders
- S/R proximity bonus (+15%), volume confirmation (120% avg required)
- Trend context analysis (7-15 bar lookback)

### S/R Confluence — `gadarah-core/src/sr_confluence.rs`
**Port from:** `/home/ilovehvn/trading-system-merged/backend/app/engines/sr_confluence.py` (732 LOC)
- S/R from swing points (fractal method)
- S/R from EMA 20/50/200
- S/R from Fibonacci retracements
- Multi-source confluence scoring

### GARCH Volatility — `gadarah-core/src/garch.rs`
**Port from:** `trading-system-merged/backend/app/engines/garch_forecast.py`
- GARCH(1,1) conditional variance model
- Predicts next-bar volatility regime (low/normal/high)
- Low-vol prediction → higher breakout confidence in ensemble

### Session Detection — `gadarah-core/src/session.rs`
**Port from:** `/home/ilovehvn/HYDRA/go/internal/orchestrator/session.go`
- Asian/London/NY session detection with overlap
- Per-session slippage multipliers: Asian 1.8×, London 1.0×, NY 1.1×
- Per-session sizing multipliers: off-session 0.7×

### Adaptive Signal Weighting — `gadarah-core/src/adaptive.rs`
**Port from:** `/home/ilovehvn/HYDRA/go/internal/orchestrator/performance.go`
- Per (Head, Regime, Session) combination:
  - 5+ consecutive losses → weight 0.0 (skip signals)
  - Last 20 trades < 35% win rate → weight 0.5
  - Otherwise → weight 1.0

---

## Part 4: Volume Profile Analysis — `gadarah-core/src/volume_profile.rs`

Volume Profile is NOT a volume bar indicator. It is a horizontal price histogram showing how much volume traded at each price level over a window.

**Key levels:**
- **VPOC (Volume Point of Control):** Highest-volume price bucket. Price has magnetic attraction here — markets return repeatedly.
- **VAH/VAL (Value Area High/Low):** Boundaries of the 70% volume zone. Trades outside value snap back.
- **HVN (High Volume Node):** Buckets with ≥ 1.5× avg bucket volume. Price slows and consolidates here — good TP targets.
- **LVN (Low Volume Node):** Buckets with < 0.5× avg volume. Price accelerates through — good breakout entries.

```rust
pub struct VolumeProfile {
    bucket_size_pips: Decimal,   // 5.0
    bars_window:      usize,     // 200
    bars:             VecDeque<Bar>,
    cached_profile:   Option<ComputedProfile>,
    atr:              ATR,
    pip_size:         Decimal,
}

pub struct ComputedProfile {
    pub vpoc:         Decimal,
    pub vah:          Decimal,
    pub val:          Decimal,
    pub buckets:      Vec<VolumeBucket>,
    pub hvn_levels:   Vec<Decimal>,
    pub lvn_levels:   Vec<Decimal>,
    pub total_volume: u64,
    pub computed_at:  i64,
}

pub struct VolumeBucket {
    pub price_low:  Decimal,
    pub price_high: Decimal,
    pub price_mid:  Decimal,
    pub volume:     u64,
    pub is_hvn:     bool,
    pub is_lvn:     bool,
    pub is_vpoc:    bool,
}

impl VolumeProfile {
    /// Call once per closed bar. Recomputes profile.
    pub fn update(&mut self, bar: &Bar) -> &ComputedProfile { ... }

    /// Distribute bar volume uniformly across pip buckets it spans.
    fn compute(&self) -> ComputedProfile {
        let mut buckets: HashMap<i64, u64> = HashMap::new();
        for bar in &self.bars {
            let low_bucket  = self.bucket_index(bar.low);
            let high_bucket = self.bucket_index(bar.high);
            let n_buckets   = (high_bucket - low_bucket + 1).max(1) as u64;
            let vol_per_bucket = bar.volume / n_buckets;
            for b in low_bucket..=high_bucket {
                *buckets.entry(b).or_insert(0) += vol_per_bucket;
            }
        }
        // Find VPOC = max volume bucket
        // Build value area: expand from VPOC outward, adding larger adjacent bucket
        // until 70% of total volume captured
        ...
    }

    /// Value Area computation: start at VPOC, expand outward adding larger adjacent
    /// bucket at each step until 70% of total volume is inside.
    fn compute_value_area(&self, buckets: &HashMap<i64, u64>, vpoc_idx: i64, total: u64)
        -> (Decimal, Decimal) { ... }
}
```

**VolumeProfileHead entry logic:**

Bull Entry (VAL Rejection):
1. Price approaches VAL from above (within 0.3 ATR)
2. Rejection candle: lower wick > 2× body, close in upper 60%
3. No LVN between VAL and VPOC (else lower TP to VPOC)
4. SMC: bullish OB in same zone
5. Regime: RangingTight/Wide or WeakTrend
6. SL: below VAL - 1.0 ATR
7. TP1 (50%): VPOC
8. TP2: VAH

LVN Breakout:
1. Bar closes through LVN with volume >= 1.5× average
2. No HVN in path for at least 1.5× SL distance
3. Entry: market at bar close
4. SL: other side of LVN + 0.5 ATR
5. TP: next HVN or VPOC

---

## Part 5: Daily P&L Management Engine — `gadarah-risk/src/daily_pnl.rs`

### Why This Is Critical

Without a daily P&L manager, the bot chases losses after a bad morning and gives back profits trying for more. Both behaviors destroy prop firm challenges. This is the single largest missing piece in HYDRA-class systems.

```rust
pub struct DailyPnlEngine {
    config:          DailyPnlConfig,
    day_open_equity: Decimal,    // Set at UTC reset time
    day_pnl_usd:     Decimal,    // Realized + unrealized for the day
    intraday_peak:   Decimal,
    state:           DayState,
    reset_hour_utc:  u8,
    last_day:        i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DayState {
    Normal,       // Full risk allowed
    Cruising,     // 60% of daily target hit — slight reduction
    Protecting,   // 100% of daily target hit — lock profits
    DailyStopped, // Daily stop hit — no new trades
}

pub struct DailyPnlConfig {
    pub daily_target_pct:     Decimal, // Challenge: 3.3%, Funded: 0.40%
    pub cruise_threshold_pct: Decimal, // 0.60: enter Cruising at 60% of target
    pub cruise_risk_mult:     Decimal, // 0.75
    pub protect_threshold_pct: Decimal,// 1.00: enter Protecting at 100% of target
    pub protect_risk_mult:    Decimal, // 0.25 (tiny trades to stay active for min-days)
    pub daily_stop_pct:       Decimal, // Challenge: 1.5%, Funded: 0.8% of account
    pub max_daily_loss_pct:   Decimal, // 1.0% (internal soft stop, before firm's hard limit)
    pub no_trade_hours_utc:   Vec<u8>, // [21, 22, 23] dead zone
    pub enabled:              bool,
}
```

**State transitions:**
```
New Day → state = Normal
  │
  ├── P&L >= 60% of daily_target → Cruising
  │     risk_mult = 0.75, ensemble threshold +0.05 (more selective)
  │
  ├── P&L >= 100% of daily_target → Protecting
  │     risk_mult = 0.25, only SmcHead and TrendHead allowed
  │     No Grid, Scalp, or NewsHead straddles
  │
  ├── Intraday DD from peak >= daily_stop_pct → DailyStopped
  │     No new trades. Alert via Telegram.
  │
  └── P&L < -max_daily_loss_pct → DailyStopped
        Alert: "Daily loss limit reached."
```

**Challenge vs Funded parameters:**

| Parameter | Challenge | Funded |
|---|---|---|
| daily_target_pct | 3.3% | 0.40% |
| cruise_threshold_pct | 0.60 | 0.60 |
| cruise_risk_mult | 0.75 | 0.80 |
| protect_threshold_pct | 1.00 | 1.00 |
| protect_risk_mult | 0.30 | 0.50 |
| daily_stop_pct | 1.5% | 0.8% |
| max_daily_loss_pct | 1.0% | 0.5% |

**API:**
```rust
impl DailyPnlEngine {
    /// Call on every equity update. Returns state with multipliers.
    pub fn update(&mut self, current_equity: Decimal) -> DayStateResult { ... }
    pub fn can_trade(&self) -> bool { self.state != DayState::DailyStopped }
    pub fn risk_multiplier(&self) -> Decimal {
        match self.state {
            DayState::Normal      => dec!(1.0),
            DayState::Cruising    => self.config.cruise_risk_mult,
            DayState::Protecting  => self.config.protect_risk_mult,
            DayState::DailyStopped => dec!(0.0),
        }
    }
}
```

**Session P&L state (supplementary):**
```rust
pub struct SessionPnlState {
    pub session:             Session,
    pub session_open_pnl:    Decimal,
    pub session_pnl:         Decimal,
    pub trades_this_session: u8,
}
```
If London session reaches 60% of daily target, NY session starts in Cruising mode.

---

## Part 6: Intermarket Macro Filter — `gadarah-core/src/fusion/macro_filter.rs`

### Rationale
GBPUSD, EURUSD, USDJPY, XAUUSD are driven by DXY and risk sentiment. Trading GBPUSD long when DXY is in a strong uptrend ignores the macro flow even if the technical setup is valid.

**Data sources** (HTTP daily fetch at 06:00 UTC, before London open):
- **DXY:** Twelve Data free API (8 req/min) or Yahoo Finance scraping
- **VIX:** CBOE VIX daily close
- **US10Y, US2Y:** US Treasury yields (for yield curve spread)
- Refresh: once daily — intraday updates not needed

```rust
pub struct MacroFilter {
    dxy_ema20:   EMA,  // 20-day
    dxy_ema50:   EMA,  // 50-day
    vix_ema10:   EMA,  // 10-day
    yield_spread_history: VecDeque<Decimal>, // 10Y-2Y, 20 days
    latest:      Option<MacroSnapshot>,
}

pub struct MacroSnapshot {
    pub dxy_close:         Decimal,
    pub dxy_ema20:         Decimal,
    pub dxy_ema50:         Decimal,
    pub dxy_trend:         MacroTrend,
    pub vix_level:         Decimal,
    pub vix_regime:        VixRegime,
    pub us10y:             Decimal,
    pub us2y:              Decimal,
    pub yield_curve:       Decimal,    // 10Y - 2Y
    pub yield_curve_trend: MacroTrend,
    pub risk_mode:         RiskMode,
    pub fetched_at:        i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VixRegime {
    Low,       // VIX < 15: complacency
    Normal,    // VIX 15-25
    Elevated,  // VIX 25-35: reduce size
    Extreme,   // VIX > 35: panic, stop non-safe-haven longs
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RiskMode { RiskOn, RiskOff, Neutral }
```

**Filter rules by symbol type:**

```rust
pub fn evaluate(&self, signal: &TradeSignal, snap: &MacroSnapshot) -> MacroFilterResult {
    // USD pairs (EURUSD, GBPUSD, AUDUSD — Buy = USD weakening):
    //   DXY EMA20 > EMA50 AND DXY +0.5% in 3 days → size_mult = 0.5
    //   DXY extreme uptrend (>2% in 3 days) AND VIX < 20 → block
    //
    // USD pairs (Sell = USD strengthening):
    //   DXY EMA20 < EMA50 (weakening DXY) → size_mult = 0.6
    //
    // USDJPY, USDCAD (Buy = USD strengthening):
    //   Reverse of above DXY rules
    //
    // XAUUSD (Gold):
    //   VixRegime::Extreme → gold safe haven, Buy size_mult = 1.5 (ENHANCED)
    //   VixRegime::Low → gold likely underperform, size_mult = 0.7
    //   DXY strong up AND RiskOn → gold headwind, reduce longs to 0.5
    //   10Y yield rising fast (>10bp in 3 days) → reduce gold longs
    //
    // Risk-on pairs (AUDUSD, NZDUSD, GBPUSD):
    //   RiskMode::RiskOff → reduce buys to 0.5, prefer sells
    //   VixRegime::Extreme → block all longs
    //
    // Global blocks:
    //   VixRegime::Extreme → block all EXCEPT XAUUSD buy and USDJPY sell
    //   yield_curve < -0.5% for >5 days → reduce all USD buy trades
}
```

---

## Part 7: 20-Feature ML Input Vector

**Reference:** LightGBM → ONNX. All values normalized before inference.

```rust
pub struct MlFeatures {
    // Volatility Context (3)
    pub atr_percentile:       f32,  // ATR vs 100-bar range [0,1]
    pub garch_vol_norm:       f32,  // Next-bar GARCH forecast / avg ATR [0,2]
    pub bb_width_pctile:      f32,  // BB width percentile [0,1]

    // Regime Quality (3)
    pub regime_confidence:    f32,  // [0,1]
    pub regime_ordinal_norm:  f32,  // 9 states → 0-8 → /8 [0,1]
    pub hurst:                f32,  // [0,1] (0.5=random, >0.5=trending)

    // SMC Structural Quality (3)
    pub smc_confluence_norm:  f32,  // 0-5 factors /5 [0,1]
    pub ob_strength:          f32,  // ATR-normalized OB impulse [0,1]
    pub ob_fresh_entry:       f32,  // 1.0=entering OB from outside, 0.5=already inside

    // Market Sentiment & News (3)
    pub sentiment_score:      f32,  // [-1,+1]
    pub news_proximity_norm:  f32,  // minutes/120, capped at 1.0
    pub macro_alignment:      f32,  // DXY/VIX alignment [-1,+1]

    // Session & Time (3)
    pub session_quality:      f32,  // Overlap=1.0, NY=0.8, London=0.9, Asian=0.4, Dead=0.0
    pub hour_norm:            f32,  // 0-23 / 23
    pub day_of_week_norm:     f32,  // 0=Mon, 4=Fri / 4

    // Trade Quality Context (3)
    pub head_confidence:      f32,  // [0,1]
    pub spread_atr_ratio:     f32,  // spread/ATR [0,1] (high=bad)
    pub vol_profile_position: f32,  // 0=at LVN, 0.5=mid, 1.0=at HVN

    // System State (2)
    pub rolling_win_rate:     f32,  // Rolling 20-trade win rate [0,1]
    pub current_dd_pct_of_max: f32, // Current DD / max allowed [0,1]
}

impl MlFeatures {
    pub fn to_array(&self) -> [f32; 20] {
        [
            self.atr_percentile, self.garch_vol_norm, self.bb_width_pctile,
            self.regime_confidence, self.regime_ordinal_norm, self.hurst,
            self.smc_confluence_norm, self.ob_strength, self.ob_fresh_entry,
            self.sentiment_score, self.news_proximity_norm, self.macro_alignment,
            self.session_quality, self.hour_norm, self.day_of_week_norm,
            self.head_confidence, self.spread_atr_ratio, self.vol_profile_position,
            self.rolling_win_rate, self.current_dd_pct_of_max,
        ]
    }
}
```

**ML training data schema:**
```sql
CREATE TABLE ml_training_data (
    id INTEGER PRIMARY KEY,
    trade_id INTEGER REFERENCES trades(id),
    atr_percentile REAL, garch_vol_norm REAL, bb_width_pctile REAL,
    regime_confidence REAL, regime_ordinal_norm REAL, hurst REAL,
    smc_confluence_norm REAL, ob_strength REAL, ob_fresh_entry REAL,
    sentiment_score REAL, news_proximity_norm REAL, macro_alignment REAL,
    session_quality REAL, hour_norm REAL, day_of_week_norm REAL,
    head_confidence REAL, spread_atr_ratio REAL, vol_profile_position REAL,
    rolling_win_rate REAL, current_dd_pct_of_max REAL,
    was_profitable INTEGER NOT NULL,  -- 1=win, 0=loss
    r_multiple REAL NOT NULL,
    created_at INTEGER NOT NULL
);
```

---

## Part 8: Pyramiding Into Winners — `gadarah-risk/src/pyramid.rs`

**Rule:** The pyramid add must be sized so that even if the combined position stops out at the NEW stop (near breakeven), the total loss cannot exceed the original risk amount. Risk does not increase.

```rust
pub struct PyramidConfig {
    pub min_r_to_add:        Decimal,  // 1.0R minimum before adding
    pub max_layers:          u8,       // 2 (initial + 2 adds = 3 units max)
    pub add_size_fraction:   Decimal,  // 0.5 (each add = 50% of original size)
    pub new_sl_placement:    PyramidSlPlacement,
    pub require_same_regime: bool,     // true
    pub ml_score_threshold:  Decimal,  // 0.55
}

pub enum PyramidSlPlacement {
    Breakeven,                                    // Move to original entry
    RecentSwing { bars: usize },                  // Recent swing (5 bars)
    EntryBarExtreme { buffer_atr: Decimal },       // Below/above pyramid bar
}

pub struct PyramidState {
    pub initial_lots:     Decimal,
    pub initial_entry:    Decimal,
    pub initial_sl:       Decimal,
    pub initial_risk_usd: Decimal,
    pub layers:           Vec<PyramidLayer>,
}
```

**Pyramid entry conditions (ALL required):**
1. Position is open and in profit >= 1.0R
2. Layer count < 2
3. Regime has not changed from original trade
4. ML score >= 0.55 on current bar
5. No news within 30 minutes
6. Daily P&L state is Normal or Cruising (NOT Protecting or DailyStopped)
7. Portfolio heat after add <= 3% max
8. New SL placement gives R:R >= 1.0 for the pyramid add itself

**Heads that support pyramiding:** TrendHead, SmcHead, ScalpM5

---

## Part 9: Re-Entry Logic — `gadarah-core/src/re_entry.rs`

When a valid signal cannot execute due to a temporary blocker (spread spike, news blackout, 5-loss cooldown), the signal is queued for re-evaluation when the blocker clears.

```rust
pub struct PendingReEntry {
    pub original_signal: TradeSignal,
    pub blocked_reason:  DeferReason,
    pub created_at:      i64,
    pub expires_at:      i64,  // created_at + (3 bars × timeframe.seconds())
    pub max_price_drift: Decimal,
}

pub struct ReEntryConfig {
    pub max_wait_bars:   u8,      // 3
    pub max_drift_pips:  Decimal, // 5.0
    pub min_rr:          Decimal, // same as originating head
    pub max_spread_mult: Decimal, // 1.5
}

impl ReEntryEvaluator {
    pub fn evaluate_pending(&mut self, bar: &Bar, spread: Decimal) -> Option<TradeSignal> {
        // 1. Check if expired
        if bar.timestamp > pending.expires_at { discard; return None; }
        // 2. Check price drift
        let drift = (bar.close - pending.entry).abs() / pip_size;
        if drift > max_drift_pips { discard; return None; }
        // 3. Check spread normalized
        if spread > normal_spread * max_spread_mult { return None; } // Keep waiting
        // 4. SL still valid (>= 2 pips)
        // 5. R:R still >= min_rr at current price
        // All clear: emit re-entry with SignalKind::ReEntry
    }
}
```

---

## Part 10: Equity Curve Filter — `gadarah-risk/src/equity_curve_filter.rs`

When the system's equity curve is below its 20-trade moving average, reduce all position sizing. This is a macro-level drawdown guard distinct from the kill switch (hard stop) and daily P&L engine (session-level).

```rust
pub struct EquityCurveFilter {
    config:         EquityCurveFilterConfig,
    equity_history: VecDeque<Decimal>,  // rolling equity after each trade close
    equity_ma:      Option<Decimal>,    // 20-trade SMA
}

pub struct EquityCurveFilterConfig {
    pub ma_period:          usize,   // 20 closed trades
    pub below_ma_risk_mult: Decimal, // 0.50: 50% size when below MA
    pub deep_below_mult:    Decimal, // 0.25: 25% size when >2% below MA
    pub deep_threshold_pct: Decimal, // 2.0%: threshold for deep filter
    pub min_trades:         usize,   // 20: cold start — full size until enough data
}

impl EquityCurveFilter {
    pub fn risk_multiplier(&self, current_equity: Decimal) -> Decimal {
        let ma = match self.equity_ma { None => return dec!(1.0), Some(m) => m };
        if current_equity >= ma { return dec!(1.0); }
        let pct_below = (ma - current_equity) / ma * dec!(100);
        if pct_below >= self.config.deep_threshold_pct {
            self.config.deep_below_mult    // 0.25
        } else {
            self.config.below_ma_risk_mult // 0.50
        }
    }
}
```

---

## Part 11: 5-Layer Pipeline Integration — `gadarah-core/src/signal.rs`

```rust
pub fn process_bar(
    &mut self,
    bar:            &Bar,
    account_id:     u64,
    equity:         Decimal,
    daily_pnl:      Decimal,
    open_positions: &[OpenPosition],
) -> Vec<RiskDecision> {
    // Layer 1: Update SMC + Volume Profile
    self.smc_engine.update(bar);
    self.vol_profile.update(bar);
    let smc = self.smc_engine.context_with_vp(&self.vol_profile);

    // Layer 2: Update regime
    let regime = match self.regime.update(bar) { None => return vec![], Some(r) => r };

    // Layer 3: Evaluate allowed heads
    let allowed = regime.regime.allowed_heads();
    let mut raw_signals: Vec<TradeSignal> = vec![];
    for head in &mut self.heads {
        if allowed.contains(&head.id()) {
            let session = self.session.current(bar.timestamp);
            raw_signals.extend(head.evaluate(bar, &smc, &session, &regime));
        }
    }
    // Check re-entries
    if let Some(re) = self.re_entry_eval.evaluate_pending(bar, self.current_spread) {
        raw_signals.push(re);
    }

    // Layer 4: Intelligence Fusion
    let fusion_ctx = self.fusion.evaluate(bar, &raw_signals, equity);
    let scored: Vec<(TradeSignal, FusionContext)> = raw_signals.into_iter()
        .zip(fusion_ctx)
        .filter(|(_, f)| f.ensemble_score >= self.min_ensemble_score())
        .filter(|(_, f)| f.ml_score >= self.config.ml_score_threshold)
        .filter(|(_, f)| !f.news_blackout)
        .filter(|(_, f)| f.macro_filter.allowed)
        .filter(|(_, f)| f.ev_grade != EvGrade::Negative)
        .collect();

    // Conflict resolution
    let resolved = self.conflict_resolver.resolve(scored);

    // Layer 5: Risk Gate
    resolved.into_iter()
        .map(|(sig, fuse)| self.risk_gate.evaluate(sig, fuse, equity, daily_pnl, open_positions))
        .collect()
}
```

**Signal conflict resolution priority:**
```rust
let priority: HashMap<HeadId, u8> = [
    (HeadId::Smc,        10),  // Highest — pure confluence
    (HeadId::AsianRange,  9),  // Well-defined structural setup
    (HeadId::Momentum,    8),  // Session open = high reliability
    (HeadId::News,        8),  // Scheduled events
    (HeadId::Trend,       6),
    (HeadId::Breakout,    6),
    (HeadId::VolProfile,  5),
    (HeadId::ScalpM5,     4),
    (HeadId::Grid,        3),
    (HeadId::ScalpM1,     2),
].into();
// Same direction → TakeBoth if portfolio heat allows
// Opposing direction → TakeNeither (uncertainty)
// Priority used when both same direction and portfolio heat is full
```

---

## Part 12: Trade Management — `gadarah-risk/src/trade_manager.rs`
**Port from:** `/home/ilovehvn/HYDRA/go/internal/orchestrator/trademanager.go`

- **Trailing to breakeven:** at +1R → move SL to entry
- **Trailing to lock:** at +2R → lock currentR - 1R
- **Partial profits:**
  - Close 50% at +1.5R
  - Close 25% at +3R
- **Time exit:** 48h if < +0.5R (stale trade)
- **Adverse move guard:** exit if price retraces >50% of max favorable excursion
- **Trailing after partial:** activates after first partial, trails by 1.0 ATR per bar
- **Session-specific partials:**
  - Challenge: 50% at 1.5R, SL to BE
  - Funded: 33% at 1R, 33% at 2R

---

## Part 13: Daily Consistency Tracking — `gadarah-risk/src/consistency.rs`

Prop firms track consistency. FTMO flags accounts where one day > 50% of total profit.

```rust
pub struct ConsistencyTracker {
    pub daily_pnl_history:     VecDeque<(i64, Decimal)>,
    pub streak_losing_days:    u8,
    pub total_profitable_days: u32,
    pub total_trading_days:    u32,
    config:                    ConsistencyConfig,
}

pub struct ConsistencyConfig {
    pub max_consecutive_losing_days: u8,      // 3
    pub losing_streak_pause_days:    u8,      // 2 (pause after 3 consecutive losses)
    pub max_single_day_gain_mult:    Decimal, // 3.0× avg daily P&L
    pub profitable_day_threshold:    Decimal, // min $10 to count as "profitable"
    pub history_days:                usize,   // 30
}

impl ConsistencyTracker {
    /// FTMO formula: no single day > 50% of cumulative P&L.
    pub fn ftmo_consistency_score(&self) -> Decimal {
        let total: Decimal = self.daily_pnl_history.iter().map(|(_, p)| p).sum();
        if total <= dec!(0) { return dec!(0); }
        let max_day = self.daily_pnl_history.iter().map(|(_, p)| *p).fold(Decimal::MIN, Decimal::max);
        (dec!(1) - max_day / total).max(dec!(0)) * dec!(100)
    }

    pub fn profitable_day_rate(&self) -> Decimal {
        if self.total_trading_days == 0 { return dec!(0); }
        Decimal::from(self.total_profitable_days) / Decimal::from(self.total_trading_days)
    }

    pub fn is_paused_for_consistency(&self) -> bool {
        self.streak_losing_days >= self.config.max_consecutive_losing_days
    }
}
```

**Target metrics for funded accounts:**

| Metric | Target |
|---|---|
| % profitable trading days | ≥ 60% |
| Largest single-day gain | ≤ 3× average daily P&L |
| Largest single-day loss | ≤ 1.5× average daily P&L |
| FTMO consistency score | ≥ 70% |
| Consecutive losing days max | 3 (triggers 2-day pause) |
| Days with 0 trades | ≤ 20% of calendar days |

---

## Part 14: Programming Stack

### Rust-Primary + iced GUI — Single binary, zero IPC, zero serialization bugs

| Crate | Purpose |
|-------|---------|
| `gadarah-core` | Indicators, regime, 9 heads, SMC, volume profile, fusion, ensemble, patterns |
| `gadarah-risk` | Kill switch, DD tracking, challenge tracker, daily P&L engine, equity curve filter, consistency, pyramid, re-entry, compliance |
| `gadarah-broker` | cTrader TCP/SSL client, OAuth, mock broker, spread model |
| `gadarah-data` | Tick→candle aggregator, SQLite persistence, historical downloader |
| `gadarah-notify` | Telegram (teloxide), Discord (webhook), desktop (notify-rust) |
| `gadarah-gui` | iced GUI, military HUD theme, daemon client |
| `gadarah-backtest` | Bar-by-bar replay, Monte Carlo, walk-forward, stress test, challenge sim |

### Key Dependencies
- `prost` + `prost-build` — protobuf from Spotware's `.proto` files
- `tokio` + `tokio-rustls` — async I/O for cTrader TCP/SSL
- `iced` — native Rust GUI (Elm architecture)
- `ort` — ONNX Runtime (FinBERT + LightGBM signal scorer)
- `rusqlite` — SQLite persistence
- `rust_decimal` — Decimal type for all monetary values (NO f32/f64 for prices)
- `serde` + `toml` — configuration
- `teloxide` — Telegram bot API
- `notify-rust` — Linux desktop notifications
- `proptest` — property-based testing
- `reqwest` + `scraper` — HTTP client + HTML parsing for news/macro
- `tokenizers` — HuggingFace BERT tokenizer (Rust-native)
- `notify` — filesystem watcher for config hot-reload

---

## Part 15: Architecture

### Workspace Structure
```
gadarah/
├── Cargo.toml                      # Workspace root
├── proto/                          # Spotware's official .proto files
├── config/
│   ├── gadarah.toml                # Master config
│   └── firms/
│       ├── ftmo.toml
│       └── brightfunded.toml
├── models/
│   ├── finbert.onnx
│   ├── signal_scorer.onnx
│   └── tokenizer.json
├── crates/
│   ├── gadarah-core/
│   │   └── src/
│   │       ├── types.rs            # Bar, TradeSignal, Direction, Timeframe, enums
│   │       ├── indicators.rs       # EMA, ATR, BB, Hurst, ADX, VWAP (streaming, single-bar)
│   │       ├── regime.rs           # Regime9 + RegimeSignal9 + 9-state classifier
│   │       ├── volume_profile.rs   # VolumeProfile, VPOC, VAH/VAL, HVN/LVN
│   │       ├── smc/
│   │       │   ├── order_blocks.rs
│   │       │   ├── fair_value_gaps.rs
│   │       │   ├── structure.rs    # BOS/ChoCH, swing points
│   │       │   └── liquidity.rs    # Liquidity levels, sweep detection
│   │       ├── heads/
│   │       │   ├── mod.rs          # Head trait
│   │       │   ├── trend.rs
│   │       │   ├── breakout.rs
│   │       │   ├── grid.rs
│   │       │   ├── momentum.rs
│   │       │   ├── news.rs
│   │       │   ├── smc_head.rs
│   │       │   ├── scalp_m1.rs
│   │       │   ├── scalp_m5.rs
│   │       │   ├── asian_range.rs
│   │       │   └── vol_profile.rs
│   │       ├── patterns.rs         # 28 candlestick patterns
│   │       ├── fusion/
│   │       │   ├── mod.rs          # FusionContext, 5-layer pipeline
│   │       │   ├── sentiment.rs    # FinBERT via ONNX, RSS feeds
│   │       │   ├── news_calendar.rs
│   │       │   ├── ml_scorer.rs    # 20-feature ONNX inference
│   │       │   ├── ensemble.rs     # Bayesian log-odds scorer
│   │       │   ├── macro_filter.rs # DXY/VIX/yields macro filter
│   │       │   └── conflict_resolver.rs
│   │       ├── signal.rs           # SignalCombiner, process_bar()
│   │       ├── re_entry.rs         # PendingReEntry evaluator
│   │       ├── adaptive.rs         # Per-head×regime×session performance weighting
│   │       ├── session.rs          # Session detection + slippage/sizing multipliers
│   │       ├── sr_confluence.rs    # Multi-source S/R
│   │       └── garch.rs            # GARCH(1,1)
│   ├── gadarah-risk/
│   │   └── src/
│   │       ├── types.rs            # RiskPercent, RiskDecision, RejectReason
│   │       ├── compliance.rs       # Firm rule enforcement (from firm TOML)
│   │       ├── kill_switch.rs      # Circuit breaker + volatility halt
│   │       ├── drawdown.rs         # Daily/trailing/total DD tracking per firm mode
│   │       ├── challenge_tracker.rs # Phase progress, coasting, min days, lifecycle
│   │       ├── daily_pnl.rs        # DailyPnlEngine, DayState
│   │       ├── equity_curve_filter.rs
│   │       ├── consistency.rs      # FTMO consistency score, losing streaks
│   │       ├── pyramid.rs          # Pyramid add logic
│   │       ├── trade_manager.rs    # Trailing stops, partials, time exits
│   │       ├── correlation.rs      # Dynamic correlation matrix, VaR
│   │       ├── expected_value.rs   # EV tracking per segment
│   │       ├── sizing.rs           # calculate_lots(), Kelly
│   │       └── risk_manager.rs     # Portfolio limits, heat, margin
│   ├── gadarah-broker/
│   │   └── src/
│   │       ├── client.rs           # TCP/TLS, 4-byte length-prefix framing
│   │       ├── auth.rs             # OAuth 2.0, token refresh
│   │       ├── orders.rs           # Market, limit, stop, pending orders
│   │       ├── market_data.rs      # Spot subscriptions, tick streaming
│   │       ├── symbols.rs          # Symbol spec resolver
│   │       ├── spread_model.rs     # Per-hour typical spread, real-time spread tracking
│   │       ├── swap_manager.rs     # Swap cost estimation, overnight avoidance
│   │       └── mock.rs             # MockBroker (same Broker trait, no network)
│   ├── gadarah-data/
│   │   └── src/
│   │       ├── aggregator.rs       # Tick → M1 → M5/M15/H1/H4/D1
│   │       ├── store.rs            # SQLite schema + CRUD
│   │       ├── downloader.rs       # ProtoOAGetTrendbarsReq pager → Parquet
│   │       └── account_manager.rs  # AccountEngine × 7, multi-account routing
│   ├── gadarah-notify/
│   │   └── src/
│   │       ├── telegram.rs
│   │       ├── discord.rs
│   │       └── desktop.rs
│   ├── gadarah-gui/
│   │   └── src/
│   │       ├── main.rs             # Display detection, mode select
│   │       ├── app.rs              # iced Application, message routing
│   │       ├── views/
│   │       │   ├── dashboard.rs    # 7-account grid, DD arc gauges, challenge bars
│   │       │   ├── trade_log.rs    # Trade history table, P&L coloring, R-multiples
│   │       │   ├── strategy.rs     # Regime display, signal queue, OB/FVG zones
│   │       │   ├── news.rs         # Calendar panel, countdown timers, sentiment bars
│   │       │   ├── config.rs       # Settings editor, kill switch buttons
│   │       │   └── system.rs       # Connection, CPU/RAM, uptime, heartbeat
│   │       ├── theme.rs            # Colors + fonts
│   │       └── daemon_client.rs    # Unix socket IPC to engine
│   └── gadarah-backtest/
│       └── src/
│           ├── replayer.rs         # Bar-by-bar replay (same strategy code as live)
│           ├── monte_carlo.rs      # 10,000 paths, ruin probability
│           ├── walk_forward.rs     # 5-fold cross-validation
│           ├── stress_test.rs      # 1.5× losses + 10% win rate reduction
│           └── challenge_sim.rs    # FTMO + BrightFunded rule simulation
└── data/
    ├── gadarah.db
    └── candles/                    # {symbol}/{timeframe}.parquet
```

### GUI Theme (Military HUD / Cyberpunk)
- Background: `#0a0a1a` (near-black)
- Primary accent: `#00ff88` (neon green)
- Profit: `#00ff88`, Loss: `#ff2244` (hot red)
- Neutral: `#4488ff` (electric blue)
- Warning: `#ffaa00` (amber)
- Borders: `#1a2a3a` (dark blue-gray)
- Text: `#c0c0d0` (cool gray)
- Font: JetBrains Mono or Fira Code — monospace everywhere for prices/numbers
- DD meters: arc gauges. Challenge progress: horizontal bars with % markers.
- Open position P&L: live color-pulsing based on profit/loss

### Thread Model
```
Main Thread (iced event loop)
  └── tokio runtime
       ├── ConnectionManager (TCP read/write, heartbeat 10s)
       ├── AccountEngine × 7 (strategy hot path SYNC, not async, <1ms per bar)
       ├── NewsCalendarFetcher (15-min interval)
       ├── SentimentFetcher (RSS scraper, 15-min interval)
       ├── MacroDataFetcher (daily at 06:00 UTC)
       ├── NotificationDispatcher
       ├── PersistenceWriter (batched SQLite)
       └── ConfigWatcher (notify crate, file watcher)
```

### SQLite Schema

```sql
CREATE TABLE accounts (
    id INTEGER PRIMARY KEY,
    firm_name TEXT NOT NULL,
    broker_account_id INTEGER NOT NULL UNIQUE,
    phase TEXT NOT NULL,  -- CHALLENGE_P1, CHALLENGE_P2, AWAITING_FUNDED, FUNDED, FAILED
    balance REAL NOT NULL,
    equity REAL NOT NULL,
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
    ensemble_score REAL NOT NULL,
    ml_score REAL NOT NULL,
    smc_confluence_count INTEGER NOT NULL,
    pyramid_level INTEGER NOT NULL DEFAULT 0,
    opened_at INTEGER NOT NULL,
    closed_at INTEGER,
    close_price REAL,
    pnl_usd REAL,
    r_multiple REAL,
    close_reason TEXT  -- TP, SL, PARTIAL, TIME_EXIT, KILL_SWITCH, OB_INVALIDATED
);

CREATE TABLE equity_snapshots (
    id INTEGER PRIMARY KEY,
    account_id INTEGER NOT NULL,
    balance REAL NOT NULL,
    equity REAL NOT NULL,
    daily_pnl_usd REAL NOT NULL,
    daily_dd_pct REAL NOT NULL,
    total_dd_pct REAL NOT NULL,
    day_state TEXT NOT NULL,  -- Normal, Cruising, Protecting, DailyStopped
    snapshotted_at INTEGER NOT NULL
);

CREATE TABLE news_event_history (
    id INTEGER PRIMARY KEY,
    event_name TEXT NOT NULL,
    event_date INTEGER NOT NULL,
    forecast REAL,
    actual REAL,
    deviation REAL,
    deviation_z_score REAL,
    currency TEXT NOT NULL,
    pair_reaction_pips REAL,
    created_at INTEGER NOT NULL
);

CREATE TABLE ml_training_data (
    id INTEGER PRIMARY KEY,
    trade_id INTEGER REFERENCES trades(id),
    atr_percentile REAL, garch_vol_norm REAL, bb_width_pctile REAL,
    regime_confidence REAL, regime_ordinal_norm REAL, hurst REAL,
    smc_confluence_norm REAL, ob_strength REAL, ob_fresh_entry REAL,
    sentiment_score REAL, news_proximity_norm REAL, macro_alignment REAL,
    session_quality REAL, hour_norm REAL, day_of_week_norm REAL,
    head_confidence REAL, spread_atr_ratio REAL, vol_profile_position REAL,
    rolling_win_rate REAL, current_dd_pct_of_max REAL,
    was_profitable INTEGER NOT NULL,
    r_multiple REAL NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE TABLE config_changes (
    id INTEGER PRIMARY KEY,
    account_id INTEGER,
    key TEXT NOT NULL,
    old_value TEXT,
    new_value TEXT NOT NULL,
    changed_at INTEGER NOT NULL
);
```

### Config Schema

**`config/gadarah.toml`:**
```toml
[engine]
mode = "challenge"
symbols = ["GBPUSD", "EURUSD", "XAUUSD"]
log_level = "info"
db_path = "data/gadarah.db"
models_dir = "models"

[risk]
max_risk_pct = 2.5
min_risk_pct = 0.5
max_portfolio_heat = 3.0
max_correlated_positions = 2
kelly_fraction = 0.25

[ensemble]
min_score_challenge = 0.45
min_score_funded = 0.55
ml_score_threshold = 0.55
cold_start_trades = 30

[kill_switch]
daily_dd_trigger_pct = 95.0
total_dd_trigger_pct = 95.0
consecutive_loss_limit = 5
cooldown_minutes = 30
flash_crash_drop_pct = 1.0
flash_crash_window_seconds = 60
spread_halt_multiplier = 3.0
volatility_halt_atr_multiplier = 3.0

[daily_pnl]
daily_target_pct = 3.3
cruise_threshold_pct = 0.60
cruise_risk_mult = 0.75
protect_threshold_pct = 1.00
protect_risk_mult = 0.30
daily_stop_pct = 1.5
max_daily_loss_pct = 1.0
no_trade_hours_utc = [21, 22, 23]

[session.london]
min_ensemble_score = 0.40
scalp_m5_max_per_session = 3

[session.overlap]
min_ensemble_score = 0.45
scalp_m5_max_per_session = 2

[session.asian]
min_ensemble_score = 0.55
scalp_m1_allowed = false
grid_max_levels = 2

[sentiment]
enabled = true
refresh_interval_minutes = 15
sources = ["forexfactory", "fxstreet", "dailyfx"]

[macro_filter]
enabled = true
refresh_hour_utc = 6
vix_extreme_threshold = 35.0
dxy_strong_trend_pct_3day = 0.5

[notifications]
telegram_enabled = true
discord_enabled = false
desktop_enabled = true
```

**`config/firms/ftmo.toml`:**
```toml
[firm]
name = "FTMO"
challenge_type = "1step"
profit_target_pct = 10.0
daily_dd_limit_pct = 5.0
max_dd_limit_pct = 10.0
min_trading_days = 4
dd_mode = "static"
news_trading_allowed = true
max_positions = 5
profit_split_pct = 80.0

[broker]
host = "live.ctraderapi.com"
port = 5035
client_id_env = "GADARAH_FTMO_CLIENT_ID"
client_secret_env = "GADARAH_FTMO_CLIENT_SECRET"
```

---

## Part 16: Implementation Phases

### Priority order (revised for 9 heads + new components)

```
WEEK 1 — Core Foundation
  1.  Workspace + all 7 crates scaffolded, Cargo.toml dependencies pinned
  2.  All Rust types defined (Bar, TradeSignal, RiskPercent, SmcContext, FusionContext, RiskDecision, all enums)
  3.  Head trait (single-bar API — HYDRA Bug 2 class made impossible by signature)
  4.  All indicators: EMA, ATR, BB, ADX, Hurst, GARCH, VWAP, Choppiness Index (streaming, single-bar)
  5.  Regime9 classifier (9-state)
  6.  RiskPercent newtype (compile-time bounds — HYDRA Bug 1 class eliminated)
  7.  Unit + property-based tests for all indicators and types

WEEK 2 — Strategy Heads + SMC + Volume Profile
  8.  TrendHead, BreakoutHead, GridHead (bug-fixed ports from HYDRA)
  9.  MomentumHead, SmcHead (new)
  10. AsianRangeHead (new — highest daily consistency impact, implement first of new heads)
  11. ScalpM5 (new)
  12. ScalpM1 (new)
  13. VolumeProfile component (VPOC, VAH/VAL, HVN/LVN)
  14. VolumeProfileHead (new)
  15. SMC engine: OB detection, FVG detection, BOS/ChoCH, Liquidity sweep
  16. 28 candlestick patterns (port from trading-system-merged)
  17. S/R confluence engine
  18. Session detection + multipliers
  19. Adaptive signal weighting

WEEK 3 — Intelligence + Risk Systems
  20. Daily P&L Engine (DailyPnlEngine + DayState — implement before fusion)
  21. Equity Curve Filter
  22. Consistency Tracker
  23. Pyramid logic
  24. Re-entry evaluator
  25. Macro Filter (DXY/VIX/yields HTTP daily fetch)
  26. FinBERT sentiment via ONNX (export model, integrate ort crate)
  27. News calendar + economic event scoring
  28. ML signal scorer (20-feature ONNX — use placeholder model initially, train in Phase 6)
  29. Bayesian ensemble scorer (25 signal sources)
  30. Signal conflict resolver
  31. 5-layer SignalCombiner.process_bar()
  32. Risk Gate: compliance, kill switch, DD tracker, sizing, correlation, EV tracker, trade manager

WEEK 4 — Broker + Data
  33. cTrader OAuth 2.0 + TCP/TLS client (port from HYDRA Go adapter)
  34. Mock broker (same Broker trait, no network — for all tests)
  35. Tick → M1 → M5/M15/H1/H4/D1 aggregator
  36. SQLite schema + persistence (all tables)
  37. Historical data downloader (ProtoOAGetTrendbarsReq → Parquet)
  38. AccountManager + multi-account routing + copy trading avoidance
  39. Crash recovery (reconcile on startup)
  40. Config hot-reload (notify crate)
  41. Swap cost manager, spread model

WEEK 5 — Backtesting + Validation
  42. Bar-by-bar replayer (same strategy code path as live)
  43. Challenge simulator (FTMO + BrightFunded rules)
  44. Walk-forward (5-fold cross-validation)
  45. Monte Carlo (10,000 paths)
  46. Stress test (1.5× losses, 10% win rate reduction)
  47. Train LightGBM on backtest data → export ONNX → swap in for placeholder
  48. Run full validation gauntlet. Re-optimize if any stage fails.
  49. 1-week paper trading on cTrader demo

WEEK 6 — GUI + Deployment
  50. iced GUI skeleton + military HUD theme (colors, fonts, grid overlays)
  51. Dashboard view (account cards, DD arc gauges, challenge progress)
  52. Trade log view (P&L coloring, R-multiples, head/regime labels)
  53. Strategy view (regime display, signal queue, OB/FVG overlay)
  54. News calendar panel (countdown timers, sentiment, next high-impact event)
  55. Config editor + kill switch buttons
  56. System health view (connection status, CPU/RAM, uptime, heartbeat)
  57. Daemon mode detection ($DISPLAY/$WAYLAND_DISPLAY) + Unix socket IPC
  58. Telegram + Discord + desktop notifications
  59. systemd user service
  60. Secrets setup: ~/.config/gadarah/secrets.env chmod 600
  61. Fund first accounts (1× FTMO $100K, 1× BrightFunded $100K)
  62. Monitor live, tune via hot-reload, scale after 2+ months consistent profits
```

**Critical reference files:**
- `/home/ilovehvn/HYDRA/rust/strategy-core/src/` — indicators, regime, heads, sizing
- `/home/ilovehvn/HYDRA/rust/execution-engine/src/` — kill switch, compliance
- `/home/ilovehvn/HYDRA/CIELPLAN.md` — bug documentation (DO NOT REPEAT ANY)
- `/home/ilovehvn/HYDRA/go/internal/broker/ctrader/adapter.go` — wire protocol
- `/home/ilovehvn/HYDRA/go/internal/broker/mock/` — mock broker
- `/home/ilovehvn/HYDRA/go/internal/orchestrator/trademanager.go`
- `/home/ilovehvn/HYDRA/go/internal/orchestrator/session.go`
- `/home/ilovehvn/HYDRA/go/internal/orchestrator/performance.go`
- `/home/ilovehvn/HYDRA/python/news/sentiment.py` — FinBERT reference
- `/home/ilovehvn/HYDRA/python/news/economic_calendar.py`
- `/home/ilovehvn/trading-system-merged/backend/app/engines/pattern_detector.py` (3,671 LOC)
- `/home/ilovehvn/trading-system-merged/backend/app/engines/regime_detector.py` (668 LOC)
- `/home/ilovehvn/trading-system-merged/backend/app/engines/expected_value.py` (948 LOC)
- `/home/ilovehvn/trading-system-merged/backend/app/engines/order_blocks.py`
- `/home/ilovehvn/trading-system-merged/backend/app/engines/fair_value_gaps.py`
- `/home/ilovehvn/trading-system-merged/backend/app/engines/market_structure.py`
- `/home/ilovehvn/trading-system-merged/backend/app/engines/ftmo_strategy.py` (1,412 LOC)
- `/home/ilovehvn/trading-system-merged/backend/app/engines/ensemble_scorer.py`
- `/home/ilovehvn/trading-system-merged/backend/app/engines/kelly_criterion.py`
- `/home/ilovehvn/trading-system-merged/backend/app/engines/risk.py` (3,418 LOC)
- `/home/ilovehvn/trading-system-merged/backend/app/engines/correlation_matrix.py`
- `/home/ilovehvn/trading-system-merged/backend/app/engines/sr_confluence.py` (732 LOC)
- `/home/ilovehvn/trading-system-merged/backend/app/engines/garch_forecast.py`
- `/home/ilovehvn/HYDRA/python/backtest/validation_gauntlet.py`

---

## Part 17: Operational Requirements

### 17.1 cTrader Open API Registration
1. Register at openapi.ctrader.com → get `client_id` + `client_secret`
2. Set redirect URI for OAuth flow
3. Spotware approval may take days
4. Store in `~/.config/gadarah/secrets.env` (chmod 600, never in repo)

### 17.2 Historical Data
- Primary: cTrader `ProtoOAGetTrendbarsReq` (5 req/s, ~300 bars/req)
- Alternative: Dukascopy (free, tick-level, bulk download)
- Need: 2+ years M1 data for GBPUSD, EURUSD, XAUUSD, USDJPY, AUDUSD, USDCAD
- Check: `/home/ilovehvn/HYDRA/data/candles/` for reusable bars
- Storage: Parquet, organized `data/{symbol}/{timeframe}.parquet`

### 17.3 Symbol Name Mapping
- On connect: query `ProtoOASymbolsListReq` → runtime symbol→ID map
- Store in SQLite: `firm_symbols(firm_name, our_symbol, broker_symbol_id, pip_size, ...)`
- Canonical internal names (GBPUSD) → resolve to broker IDs at order time

### 17.4 Account Lifecycle
```
CHALLENGE_PHASE_1 → (hit target + min days) → CHALLENGE_PHASE_2 → AWAITING_FUNDED → FUNDED
                  → (breach DD)             → FAILED
```
- Auto-detect transitions, alert via Telegram, switch risk profiles

### 17.5 Copy Trading Avoidance (7 accounts)
- Random 5-60s delay between placing same trade across accounts
- Symbol rotation: assign primary/secondary symbols per account
- Entry variation: limit orders offset 1-3 pips from each other
- Strategy diversification: Account 1 favors Momentum, Account 2 favors SMC, etc.
- Risk variation: 2.0%, 2.1%, 2.2%, 2.3%, 2.4%, 2.5% per account

### 17.6 Secrets Management
- `~/.config/gadarah/secrets.env` chmod 600
- `GADARAH_FTMO_1_ACCESS_TOKEN`, `GADARAH_FTMO_1_ACCOUNT_ID`, etc.
- Token refresh before expiry (not after 401)
- Tokens never in SQLite, never logged

### 17.7 Config Hot-Reload
- `notify` crate watches TOML files
- Atomic swap via `Arc<RwLock<Config>>`
- Hot-reloadable: risk %, thresholds, session params, kill switch thresholds
- NOT hot-reloadable: broker credentials, account IDs (require restart)
- Every change logged to SQLite `config_changes`

### 17.8 Spread Modeling Per Session
```
GBPUSD typical:
  00-07 (Asian):        1.5-2.5 pips
  07-08 (London open):  0.8-1.5 pips
  08-12 (London):       0.5-0.8 pips (tightest)
  12-16 (Overlap):      0.6-0.9 pips
  16-21 (NY PM):        0.8-1.2 pips
  21-24 (Dead):         2.0-5.0 pips
```
ScalpM1 will not fire if spread > 1.0 pip. NewsHead straddle skips if spread > 1.5 pips.

### 17.9 Pre-Market HTF Bias Update (Daily at 06:45 UTC)
Each day before London open, compute and store:
1. D1 regime and bias from just-closed D1 bar
2. H4 last BOS/ChoCH direction
3. DXY direction from overnight
4. Yesterday's VPOC (magnet for today's price)
5. Previous day high/low (PDH/PDL) — key intraday S/R

### 17.10 Latency
- cTrader servers: London LD4/LD5, New York
- M15 strategies: 50-200ms from home is acceptable
- Start from home. Add London VPS (Hetzner €5-10/mo) only if measurable edge loss.
- NewsHead straddle placed 30s before release — home latency irrelevant.

---

## Part 18: Backtesting Pass/Fail Criteria

### Stage 1 — Standard Backtest (2-Year In-Sample)

| Metric | Minimum Pass | Target |
|---|---|---|
| Total return (challenge period) | ≥ 8.0% | ≥ 12% |
| Max drawdown | < 8.0% | < 5.0% |
| Trailing DD breach | 0 | 0 |
| Win rate | ≥ 45% | ≥ 52% |
| Profit factor | ≥ 1.30 | ≥ 1.60 |
| Sharpe ratio (annualized) | ≥ 0.60 | ≥ 1.0 |
| Sortino ratio | ≥ 0.80 | ≥ 1.2 |
| Average R-multiple | ≥ +0.25R | ≥ +0.40R |
| Total trades | ≥ 200 | ≥ 400 |
| Avg trades per day | ≥ 1.0 | 2.0-5.0 |
| Calmar ratio | ≥ 0.8 | ≥ 1.5 |
| % profitable days | ≥ 55% | ≥ 62% |

### Stage 2 — Walk-Forward (5-Fold)

| Metric | Minimum Pass |
|---|---|
| OOS Sharpe (each fold) | ≥ 0.5 |
| OOS profit factor (each fold) | ≥ 1.20 |
| OOS max DD (each fold) | < 9.0% |
| OOS win rate (each fold) | ≥ 42% |
| Folds passing all criteria | 4 of 5 minimum |
| IS/OOS Sharpe ratio | ≤ 2.0 (overfit guard) |
| Trades per OOS fold | ≥ 30 |

### Stage 3 — Monte Carlo (10,000 Paths)

| Metric | Minimum Pass |
|---|---|
| 5th percentile return | ≥ 0% |
| 95th percentile DD | < 9.5% |
| Ruin probability (DD > 10%) | < 5% |
| 50th percentile return | ≥ 6.0% |
| Probability of hitting 8% target | ≥ 60% |

### Stage 4 — Stress Test (1.5× losses + 10% win rate reduction)

| Metric | Minimum Pass |
|---|---|
| Max DD under stress | < 9.8% |
| Account survival rate | ≥ 85% |
| Expected return under stress | > 0% |

### Stage 5 — Challenge Simulation (100 simulations per firm)

| Metric | Minimum Pass |
|---|---|
| FTMO 1-step pass rate | ≥ 70% |
| BrightFunded Phase 1+2 pass rate | ≥ 65% |
| Avg days to pass (winning sims) | ≤ 25 trading days |
| DD limit breach rate | ≤ 5% |
| Daily DD breach rate | ≤ 2% |
| Min trading days always met | 100% of passing sims |

---

## Part 19: Verification Invariants

**All must pass before live deployment:**

- RiskPercent newtype rejects values outside [0.01, 10.0]
- Kill switch fires at exactly 95% of DD limits
- Daily DD resets at correct firm-specific time
- Lot size calculation matches manual hand-calculation on 10 spot checks
- Each account's state is fully independent (kill one, others continue)
- Connection loss pauses all accounts, reconnect resumes them
- Crash → restart → reconcile → resume with correct state
- FinBERT ONNX produces identical scores vs Python HuggingFace on 100 headlines
- OB zones invalidate on body-close-through (not wick)
- FVGs track fill % and decay correctly, mitigate at 50%
- NewsHead straddle places + cancels pending orders within timing constraints
- Ensemble scorer output always in [0.0, 1.0]
- ML scorer ONNX matches Python LightGBM predict_proba within 0.001 tolerance
- 5-layer pipeline: no signal reaches execution without passing all 5 layers
- H4 bias change propagates to M15 head signals on next bar
- Symbol resolver returns correct pip_size and lot_size per firm
- Mock broker simulates fills within specified slippage bounds
- Config hot-reload applies without dropping connections or restarting engine
- Trade staggering produces 5-60s random delays between accounts for same signal
- Account lifecycle auto-transitions on challenge completion
- OAuth token refresh triggers before expiry
- Secrets.env has 600 permissions, tokens never in SQLite, never in logs
- Swap costs correctly factored into EV for multi-day holds
- ML drift detector alerts when rolling accuracy drops below 50%
- Daily P&L engine transitions state correctly at correct thresholds
- DayState.DailyStopped = no new trades, existing positions still managed
- Equity curve filter halves position size when equity is below 20-trade MA
- AsianRangeHead resets correctly at UTC midnight
- Pyramid add never increases total risk beyond original risk_usd
- Re-entry signal abandons after 3 bars or 5-pip price drift
- Volume Profile VPOC is always the maximum-volume bucket in the window
- Value area computation always captures ≥ 70% of total volume
- Regime9 Transitioning allows zero new trades (empty allowed_heads)
- Consistency tracker pauses trading after 3 consecutive losing days
- FTMO consistency score alert fires when score drops below 70%
- Backtester reproduces identical results on identical seed data (deterministic)
- Historical data downloader produces gap-free Parquet for 6 symbols × 2 years
