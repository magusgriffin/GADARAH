# GPTPLAN — Cash-First Execution Plan For GADARAH

**Date:** 2026-03-27

## Core Thesis

The goal is not to build the most sophisticated trading engine.

The goal is:

**Get from a tiny bankroll to first real withdrawal as fast as possible without relying on account bans, one-shot luck, or self-destructive risk.**

This plan assumes the starting bankroll is only **$53-$80**.

That changes everything.

With capital this small:

- every paid reset hurts,
- every extra feature delays money,
- every execution mistake matters,
- and every fantasy about "one perfect 30-second trade" is expensive.

So the correct strategy is:

**software discipline + narrow edge + low-cost validation + one focused paid attempt + replication only after proof**

---

## What This Plan Accepts

- Rust-only architecture is correct.
- Strong type safety is mandatory.
- Risk controls matter more than aesthetic completeness.
- Speed of delivery matters more than total feature count.
- Pyramiding is useful only if total account risk does not expand.
- Challenge mode and funded mode must behave differently.

## What This Plan Rejects

- toxic-flow or broker-lag exploitation,
- anti-fingerprinting or obfuscation tactics,
- 30% to 100% risk-per-trade logic,
- offshore burner-account strategy as the main business model,
- RL / Transformer / deep ML before there is a proven base edge,
- a giant 9-head MVP,
- building the GUI before the bot earns money.

---

## Actual Objective

### Primary Objective

Reach first funded payout quickly enough to recover the initial bankroll and create a base for replication.

### Secondary Objective

Use realized profits, not personal cash, to expand into additional accounts.

### Non-Objective

Do not optimize for theoretical maximum CAGR at the start.

Early-stage survival and payout frequency matter more.

---

## Product Definition

GADARAH should start as a **small prop-compatible Rust execution engine**, not a full platform.

### Phase-1 product

Build only:

- `gadarah-core`
- `gadarah-risk`
- `gadarah-broker`
- `gadarah-data`
- `gadarah-backtest`

Delay:

- `gadarah-gui`
- advanced ML
- sentiment NLP
- heavy macro scraping
- elaborate multi-account orchestration
- experimental low-latency infrastructure

---

## MVP Strategy Scope

The MVP gets **3 heads maximum**.

### Head 1: MomentumHead

Purpose:
- capture clean session expansion and continuation

Why it stays:
- simple to test
- good for fast target progress when volatility is real

### Head 2: AsianRangeHead

Purpose:
- capture structured breakout/reclaim setups from well-defined ranges

Why it stays:
- highly mechanical
- easier to validate
- better suited for consistent challenge behavior

### Head 3: BreakoutHead or SmcHead

Choose only one initially.

Selection rule:
- whichever produces cleaner backtests,
- fewer moving parts,
- lower live interpretation risk

Do not launch both unless the data clearly justifies it.

### Excluded from MVP

- GridHead
- M1 scalp logic
- news-only sniper mode
- latency-arb logic
- pairs trading
- advanced volume profile unless it clearly helps one chosen head

---

## Market Style

The bot should trade:

- London open
- New York open
- London/NY overlap

The bot should avoid:

- dead Asian drift unless using AsianRange setup
- random mid-session chop
- latency-dependent macro release entries
- low-volatility garbage conditions

The trading style should be:

**selective, session-driven, eventful, and inactive when edge quality is poor**

The edge should come from:

- clean structure,
- volatility expansion,
- limited number of high-quality trades,
- disciplined risk,
- and fast replication after proof.

---

## Risk Model

## Challenge Mode

Goal:
- pass without churn

Rules:

- risk per initial trade: **0.30% to 0.50%**
- max portfolio heat: **1.5% to 2.0%**
- daily stop: **1.25% to 1.75%**
- hard stop after **3 consecutive losses**
- no discretionary revenge trading
- no major-news trading until execution data proves it is viable
- pyramids allowed only if total net risk does not exceed original risk

Challenge mode should feel restrained.

That is correct.

The fastest way to stay poor is repeatedly rebuying evaluations.

## Funded Mode

Goal:
- maximize real withdrawals without violating consistency behavior

Rules:

- risk per initial trade: **0.50% to 0.75%**
- max portfolio heat: **2.0% to 3.0%**
- daily protection state after a strong positive day
- consistency-aware throttling
- automatic coasting near payout windows
- controlled pyramiding only on proven runners

Funded mode should scale intelligently, not emotionally.

---

## Bankroll Ladder

Because the bankroll is only `$53-$80`, deployment must follow a strict ladder.

### Stage 0: Free Validation

Spend no money yet.

Do:

- historical replay
- walk-forward testing
- challenge simulation
- demo forward execution logging

Do not buy anything until there is one narrow setup that survives all of the above.

### Stage 1: Single Paid Attempt

Choose exactly one:

- one low-cost micro-challenge
- or one tiny live micro account for execution validation

Preferred route:

- if backtests and demo behavior are clean, choose the micro-challenge
- if execution quality is still unclear, use the tiny live account first

Do not split the bankroll across multiple experiments.

### Stage 2: First Withdrawal

After first funded proof:

- protect the payout window
- reduce unnecessary aggression
- prioritize cash extraction over target-chasing ego

The first withdrawal changes the game.

It converts the project from theory into a capital machine.

### Stage 3: Controlled Replication

Only after first real withdrawal:

- open a second account
- keep the exact same playbook
- avoid introducing new heads during scaling
- recycle profits into expansion

### Stage 4: Portfolio Replication

Only after repeated proof:

- scale to 3-5 accounts
- keep correlated exposure capped
- retain cash reserves outside the trading stack

---

## Build Order

### Phase 0: Freeze Scope

Before writing broad implementation:

- cap the MVP at 3 heads
- remove GUI from critical path
- remove deep ML from critical path
- remove toxic-flow concepts from critical path
- remove multi-broker ambitions from critical path

### Phase 1: Core Contracts

Implement first:

- exact shared types
- signal types
- risk percent newtype
- head trait
- account mode types
- reject reasons and risk decisions

This layer prevents dumb losses.

### Phase 2: Data And Execution

Implement:

- broker client
- market data ingestion
- tick/bar aggregation
- symbol metadata resolution
- spread tracking
- order placement and order state handling

This layer determines whether the system can actually trade.

### Phase 3: Risk Engine

Implement:

- position sizing
- daily P&L engine
- drawdown tracking
- challenge compliance
- trade manager
- kill switch
- simple equity curve filter

This layer protects the bankroll from software optimism.

### Phase 4: Cash Heads

Implement:

- MomentumHead
- AsianRangeHead
- one of BreakoutHead or SmcHead

Test them independently first.

Do not fuse everything immediately.

### Phase 5: Backtest Harness

Implement:

- replay engine
- walk-forward test
- Monte Carlo simulation
- challenge simulator

Promotion rule:

A head does not graduate to live testing unless it survives all four.

### Phase 6: Demo Forward

Run the exact strategy in demo mode and log:

- slippage
- spread spikes
- missed fills
- rejected orders
- stop/TP behavior
- time-of-day quality

This phase exists to kill illusions.

### Phase 7: First Paid Deployment

Deploy:

- one account only
- challenge-mode risk only
- no discretionary overrides unless explicitly logged
- no new features during the attempt

### Phase 8: First Payout Optimization

After funded proof:

- enable controlled pyramiding
- tighten payout-window protection
- replicate to a second account
- build a small operator dashboard only if it saves time

---

## Validation Gates

Before any paid deployment, the system must pass:

### Gate 1: Standard Backtest

- multiple market regimes
- no obvious overfit behavior
- stable expectancy by head

### Gate 2: Walk-Forward

- edge must persist out of sample

### Gate 3: Monte Carlo

- acceptable drawdown and ruin probability

### Gate 4: Challenge Simulation

- realistic pass/fail behavior under prop rules

### Gate 5: Demo Forward Validation

- fills, spreads, and execution behavior must not invalidate the backtest thesis

If a setup fails a gate, it does not go live.

---

## Metrics That Matter

Do not obsess over only win rate or Sharpe.

Track:

- days to backtest-ready
- days to demo-ready
- days to first paid attempt
- challenge fee recovery time
- average days to target
- payout frequency
- payout variance
- reset rate
- survival after first payout
- expected withdrawal per 30 days

If a feature does not improve one of these metrics, it is probably delay disguised as sophistication.

---

## Operating Rules

### Rule 1

No feature enters the MVP because it sounds powerful.

It enters only if it helps:

- time to first payout,
- payout reliability,
- or reduction in reset risk.

### Rule 2

Do not add complexity during a live paid attempt.

### Rule 3

Do not scale account count before proving one-account profitability.

### Rule 4

Do not risk the whole bankroll to avoid feeling slow.

Slow and compounding beats dramatic and dead.

### Rule 5

Withdraw early.

External cash reserves are part of the strategy.

---

## Final Recommendation

If the goal is quick money, do this:

1. Build the Rust core, broker path, and risk engine first.
2. Launch with only 3 strategy heads.
3. Prove the heads in replay, walk-forward, Monte Carlo, challenge simulation, and demo.
4. Spend the tiny bankroll on only one paid path once proof exists.
5. Trade challenge mode conservatively enough to avoid fee churn.
6. Switch to controlled funded-mode scaling only after first payout.
7. Replicate the proven playbook using realized gains.
8. Delay GUI and advanced ML until the bot earns the right to become more complicated.

This is the plan I would actually follow.
