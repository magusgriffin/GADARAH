# Project Readiness Review

Date: 2026-04-16

## Executive View

The repo is technically healthy enough to keep building on: the Rust workspace
tests currently pass, the default target (`The5ers Hyper Growth`) is still live,
and the cTrader/Open API path is implemented.

The main readiness risk is not code quality. It is target drift. Several firm
profiles and plan documents were written against stale assumptions. For a U.S.
trader running a custom cTrader bot, the current support matrix is:

| Firm / Program | U.S. access | cTrader | Bot fit | Status for this repo |
| --- | --- | --- | --- | --- |
| The5ers Hyper Growth | Yes | Yes | Clean | Primary live target |
| FTMO 2-Step | Yes | Yes | Clean | Secondary target |
| FTMO 1-Step | Yes | Yes | Clean, but rule-model nuance remains | Backtest/research target; live use only after more firm-specific risk modeling |
| FundingPips 1-Step / Zero | Appears yes | Yes | Conditional | Comparison only |
| Blue Guardian | U.S. restricted away from cTrader | No for this workflow | No | Exclude |
| Alpha One | Yes | Yes | No on cTrader | Exclude |

## Verified Firm Findings

### Clean Fits

- **The5ers Hyper Growth** remains the strongest current match for this project. Official help articles show Hyper Growth on `cTrader`, no minimum days on level 1, a `3%` daily pause, a `6%` stopout, and explicit EA allowance subject to bans on HFT/arbitrage/emulators/signal copying.
- **FTMO 2-Step** remains a valid U.S. `cTrader` target and FTMO explicitly allows algorithmic trading / EAs as long as trading is legitimate and replicable.
- **FTMO 1-Step** is also valid for U.S. `cTrader` bot use, but the repo had stale rules. Official FTMO materials now show `3%` max daily loss, `10%` max loss with end-of-day trailing logic, and a `50%` Best Day Rule. The simulator in this repo has been updated to reflect that.

### Conditional Fits

- **FundingPips** currently advertises `cTrader` and states service is offered in `195+` countries, with homepage examples that include the United States. Its legal page restricts certain jurisdictions, but not the United States.
- FundingPips is **not a clean bot-first fit**. Its terms prohibit many behaviors this repo must avoid anyway: HFT, latency arbitrage, hedging, tick scalping, opposite-account trading, and purposely trading news. More importantly, it only explicitly allows **third-party** EAs when used as trade/risk managers. A private/custom bot may still be usable, but that is an inference, not an explicit allowance. Treat FundingPips as comparison-only unless you are comfortable with tighter compliance risk.

### Exclusions

- **Blue Guardian** is no longer a U.S. `cTrader` candidate. Its help center says `cTrader` was discontinued for new/existing account switches and U.S. clients are limited to Match Trader and TradeLocker. Even though Blue Guardian allows EAs generally, that does not help this repo's `cTrader` path.
- **Alpha One** still exists and is available on `cTrader` for U.S. residents, but Alpha explicitly says there is currently **no EA functionality for cTrader, DX Trade, and TradeLocker**. That makes it unusable for this project as a cTrader bot target.

## Repo Readiness

### What Is Ready

- Workspace tests pass (`cargo test --workspace`).
- The codebase already defaults to The5ers Hyper Growth, which is still the best-aligned live deployment target.
- cTrader client/auth plumbing exists locally, so the brokerage side is not vaporware.
- The project already has some firm-specific compliance logic, especially for FundingPips and The5ers.

### What Is Not Ready

- Older planning docs still treat Blue Guardian / Alpha / FundingPips comparables too casually.
- The flat `config/firms/*.toml` schema cannot express every modern prop rule nuance. FTMO 1-Step is the clearest example: the simulator now handles its Best Day Rule and end-of-day trailing max loss, but the generic live `FirmConfig` schema still does not encode every one of those mechanics directly.
- FundingPips remains economically tempting, but policy risk is materially higher than with The5ers or FTMO for a bot-driven workflow.

## Financial Viability

The project is **conditionally viable**, but only if it stays narrow.

- **Best probability-adjusted path:** keep The5ers Hyper Growth as the default proving ground. It is the cleanest mix of U.S. availability, cTrader support, bot-friendliness, and repo alignment.
- **Secondary path:** use FTMO 2-Step as the next target once the strategy is validated and you want a second clean venue.
- **Do not optimize around the cheapest fee alone.** FundingPips may look cheaper or faster on paper, but stricter terms and ambiguous bot treatment make the expected-value case worse for this specific repo.
- **Do not spend capital on a broad multi-firm rollout yet.** The project is not ready for that. It is ready for a narrow, disciplined The5ers-first path with FTMO as a follow-on venue.

## Sources

- The5ers Hyper Growth: https://help.the5ers.com/how-does-the-hyper-growth-program-work/
- The5ers EA policy: https://help.the5ers.com/can-i-use-an-ea-expert-advisor-can-i-set-a-stealth-mode-stop-loss/
- The5ers eligibility: https://help.the5ers.com/who-can-join-the5ers/
- FTMO 1-Step product page: https://ftmo.com/en/1-step-challenge/
- FTMO Trading Objectives: https://ftmo.com/en/trading-objectives/
- FTMO 1-Step minimum duration / Best Day Rule context: https://ftmo.com/en/faq/what-is-the-minimum-time-required-to-pass-ftmo-challenge-1-step/
- FTMO platform availability: https://ftmo.com/en/faq/which-platforms-can-i-use-for-trading/
- FTMO cTrader login: https://ftmo.com/en/faq/how-do-i-log-in-to-ctrader/
- FTMO strategy / EA policy: https://ftmo.com/en/faq/which-instruments-can-i-trade-and-what-strategies-am-i-allowed-to-use/
- FTMO eligibility: https://ftmo.com/en/faq/who-can-join-ftmo/
- FundingPips homepage: https://fundingpips.com/
- FundingPips terms: https://fundingpips.com/legal/terms-and-conditions
- Blue Guardian platform policy: https://help.blueguardian.com/en/articles/9661525-can-i-change-platform
- Blue Guardian EA policy: https://help.blueguardian.com/en/articles/9661396-are-eas-trade-copiers-allowed
- Alpha platform availability for U.S. residents: https://help.alphacapitalgroup.uk/en/articles/6933883-what-trading-platforms-are-available-for-use
- Alpha EA policy: https://help.alphacapitalgroup.uk/en/articles/6934236-can-i-use-an-expert-advisor-ea
- Alpha One overview: https://help.alphacapitalgroup.uk/en/articles/10097421-alpha-one
