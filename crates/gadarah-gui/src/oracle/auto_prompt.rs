//! Auto-prompt — fires the Oracle automatically on critical events.
//!
//! Two trigger sources:
//!   1. **Severity** — every alert pushed at Warning+ severity gets an
//!      Oracle take attached as a follow-up Info alert.
//!   2. **Regime flip** — when `regime_by_symbol` changes for a symbol with
//!      an open position, fire the Oracle to assess whether the trend the
//!      user is riding just broke.
//!
//! Both paths are debounced: at most one auto-prompt every
//! `AUTO_PROMPT_DEBOUNCE_SECS` per trigger key (per-title for alerts,
//! per-symbol for regimes).
//!
//! ## Why no worker thread
//!
//! The Oracle's reply channel (`OracleHandle.rx`) is single-owner — the
//! main UI loop drains it once per frame. A separate worker thread that
//! also `recv()`s on it would steal replies destined for user-driven
//! queries. Instead, we run the auto-prompt logic *on* the UI loop:
//!
//!   - `AutoPromptWatcher::tick` is called once per frame; it diffs state
//!     against the previous frame, applies debounce, and fires
//!     `OracleRequest::Analyze` with `tag: AUTO_TAG`.
//!   - The existing `pump_oracle` in `main.rs` checks `advice.tag()`; if
//!     it equals `AUTO_TAG`, the reply is converted into a follow-up
//!     `Alert` (with `oracle_advice: Some(...)` and
//!     `suppress_os_notification: true`) and pushed into `SharedState`.

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::Sender;
use std::time::Instant;

use crate::oracle::prompt::{OracleContextSelection, OracleContextSnapshot};
use crate::oracle::OracleRequest;
use crate::state::{Alert, AlertSeverity, AppState};

const AUTO_PROMPT_DEBOUNCE_SECS: u64 = 60;
pub const AUTO_TAG: &str = "auto";

/// What caused the auto-prompt to fire. Used both for the question text we
/// send to the model and for debounce keying.
#[derive(Debug, Clone)]
enum AutoPromptTrigger {
    AlertSurfaced {
        alert_title: String,
        alert_body: String,
        severity: AlertSeverity,
    },
    RegimeFlipped {
        symbol: String,
        old_regime: String,
        new_regime: String,
    },
}

impl AutoPromptTrigger {
    fn debounce_key(&self) -> String {
        match self {
            Self::AlertSurfaced { alert_title, .. } => format!("alert::{alert_title}"),
            Self::RegimeFlipped { symbol, .. } => format!("regime::{symbol}"),
        }
    }

    fn question(&self) -> String {
        match self {
            Self::AlertSurfaced {
                alert_title,
                alert_body,
                severity,
            } => format!(
                "GADARAH just surfaced a {severity:?}-severity alert.\n\n\
                 Title: {alert_title}\n\
                 Body: {alert_body}\n\n\
                 Assess what this means for the current session, what risks it implies, \
                 and what the operator should review next."
            ),
            Self::RegimeFlipped {
                symbol,
                old_regime,
                new_regime,
            } => format!(
                "Regime for {symbol} just flipped from {old_regime} to {new_regime}, and we \
                 hold an open position in that symbol.\n\n\
                 Assess whether the structure that justified the entry is still intact, \
                 what the new regime implies for the existing stop and target, and what \
                 evidence should drive a hold-vs-trim decision."
            ),
        }
    }

    fn follow_up_title(&self) -> String {
        match self {
            Self::AlertSurfaced { alert_title, .. } => format!("Oracle on \"{alert_title}\""),
            Self::RegimeFlipped { symbol, .. } => format!("Oracle on regime flip — {symbol}"),
        }
    }
}

/// Tracks per-key debounce + per-trigger context so the matching reply can
/// be reconstructed into a follow-up alert when the Oracle answers.
pub struct AutoPromptWatcher {
    last_alert_ts_seen: i64,
    last_regime_by_symbol: HashMap<String, String>,
    last_fired: HashMap<String, Instant>,
    /// FIFO of in-flight follow-up titles. `pump_oracle` pops from the
    /// front when a `tag == AUTO_TAG` reply arrives — order of replies
    /// matches the order of `Analyze` requests since the Oracle worker
    /// processes them serially.
    pending_titles: std::collections::VecDeque<String>,
}

impl Default for AutoPromptWatcher {
    fn default() -> Self {
        Self {
            last_alert_ts_seen: 0,
            last_regime_by_symbol: HashMap::new(),
            last_fired: HashMap::new(),
            pending_titles: std::collections::VecDeque::new(),
        }
    }
}

impl AutoPromptWatcher {
    /// Diff state against the last frame; for any new triggers that pass
    /// the debounce check, send `Analyze` requests on `oracle_tx`. Caller
    /// runs this once per frame from the UI loop.
    pub fn tick(&mut self, state: &AppState, oracle_tx: &Sender<OracleRequest>) {
        let triggers = self.collect_triggers(state);
        for trigger in triggers {
            let key = trigger.debounce_key();
            let now = Instant::now();
            if let Some(last) = self.last_fired.get(&key) {
                if now.duration_since(*last).as_secs() < AUTO_PROMPT_DEBOUNCE_SECS {
                    continue;
                }
            }

            let snapshot = {
                let g = state.lock().unwrap();
                OracleContextSnapshot::from_shared_state(&g)
            };
            let selection = OracleContextSelection {
                account_risk: true,
                market_session: true,
                recent_warnings: true,
                recent_journal: matches!(trigger, AutoPromptTrigger::RegimeFlipped { .. }),
                gate_rejections: matches!(trigger, AutoPromptTrigger::AlertSurfaced { .. }),
            };

            if oracle_tx
                .send(OracleRequest::Analyze {
                    question: trigger.question(),
                    context_snapshot: Box::new(snapshot),
                    context_selection: selection,
                    tag: AUTO_TAG,
                })
                .is_ok()
            {
                self.last_fired.insert(key, now);
                self.pending_titles.push_back(trigger.follow_up_title());
            }
        }
    }

    /// Called from `pump_oracle` when an `OracleAdvice` with
    /// `tag() == AUTO_TAG` arrives. Returns the follow-up title that was
    /// associated with the trigger that generated this reply, so the
    /// caller can build the alert.
    pub fn take_pending_title(&mut self) -> Option<String> {
        self.pending_titles.pop_front()
    }

    fn collect_triggers(&mut self, state: &AppState) -> Vec<AutoPromptTrigger> {
        let g = state.lock().unwrap();
        let mut triggers = Vec::new();

        // ── Severity-driven triggers ──────────────────────────────────────
        let mut newest_seen = self.last_alert_ts_seen;
        for alert in g.alerts.iter() {
            if alert.timestamp <= self.last_alert_ts_seen {
                continue;
            }
            newest_seen = newest_seen.max(alert.timestamp);
            if alert.oracle_advice.is_some() {
                // Don't recurse on Oracle-generated alerts.
                continue;
            }
            if !matches!(
                alert.severity,
                AlertSeverity::Warning | AlertSeverity::Danger
            ) {
                continue;
            }
            triggers.push(AutoPromptTrigger::AlertSurfaced {
                alert_title: alert.title.clone(),
                alert_body: alert.body.clone(),
                severity: alert.severity,
            });
        }
        self.last_alert_ts_seen = newest_seen;

        // ── Regime-flip triggers ──────────────────────────────────────────
        let open_position_symbols: HashSet<String> =
            g.positions.iter().map(|p| p.symbol.clone()).collect();
        let mut new_regimes: HashMap<String, String> = HashMap::new();
        for (symbol, regime) in g.regime_by_symbol.iter() {
            let regime_str = format!("{regime:?}");
            new_regimes.insert(symbol.clone(), regime_str.clone());
            if !open_position_symbols.contains(symbol) {
                continue;
            }
            if let Some(prev) = self.last_regime_by_symbol.get(symbol) {
                if prev != &regime_str {
                    triggers.push(AutoPromptTrigger::RegimeFlipped {
                        symbol: symbol.clone(),
                        old_regime: prev.clone(),
                        new_regime: regime_str.clone(),
                    });
                }
            }
        }
        self.last_regime_by_symbol = new_regimes;

        triggers
    }
}

/// Build a follow-up `Alert` from an Oracle reply that was fired by an
/// auto-prompt. Caller already popped the matching title from
/// `take_pending_title`.
pub fn build_follow_up_alert(title: String, full_advice: String) -> Alert {
    let truncated: String = full_advice.chars().take(280).collect();
    Alert {
        timestamp: chrono::Utc::now().timestamp(),
        severity: AlertSeverity::Info,
        title,
        body: truncated,
        dismissed: false,
        action_url: None,
        action_update_wizard: false,
        oracle_advice: Some(full_advice),
        suppress_os_notification: true,
    }
}
