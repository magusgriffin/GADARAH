//! Prompt policy and context shaping for the Oracle.

use crate::state::{LogLevel, SharedState};

const MAX_WARNINGS: usize = 3;
const MAX_JOURNAL: usize = 3;
const MAX_REJECTIONS: usize = 3;
const MAX_REGIMES: usize = 4;

pub const DEFAULT_SYSTEM_PREPROMPT: &str = r#"You are the GADARAH Oracle, a disciplined trading analyst embedded in a prop-firm trading workstation.

Voice and style:
- Maintain a restrained oracle tone in headings only. The substance must stay plain, practical, and risk-first.
- Be concise, specific, and skeptical.
- Prefer short paragraphs or tight bullets over long exposition.

Response contract:
- Use exactly these sections: Assessment, Risks, Next checks.
- In Assessment, summarize the state or answer the question directly.
- In Risks, call out concrete dangers, constraint breaches, or missing evidence.
- In Next checks, suggest review steps or questions for the operator.

Guardrails:
- Never place trades.
- Never recommend exact live orders, entries, stops, or take-profit levels.
- Never claim to see data that is not present in the supplied context.
- If the context is incomplete, say what is missing.
- Treat all output as advisory text for a human operator inside GADARAH.
"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OracleContextSelection {
    pub account_risk: bool,
    pub market_session: bool,
    pub recent_warnings: bool,
    pub recent_journal: bool,
    pub gate_rejections: bool,
}

impl Default for OracleContextSelection {
    fn default() -> Self {
        Self {
            account_risk: true,
            market_session: true,
            recent_warnings: true,
            recent_journal: false,
            gate_rejections: false,
        }
    }
}

impl OracleContextSelection {
    pub fn enabled_count(&self) -> usize {
        [
            self.account_risk,
            self.market_session,
            self.recent_warnings,
            self.recent_journal,
            self.gate_rejections,
        ]
        .into_iter()
        .filter(|enabled| *enabled)
        .count()
    }
}

#[derive(Debug, Clone)]
pub struct OracleContextSnapshot {
    pub account_risk: String,
    pub market_session: String,
    pub recent_warnings: Vec<String>,
    pub recent_journal: Vec<String>,
    pub gate_rejections: Vec<String>,
}

impl OracleContextSnapshot {
    pub fn from_shared_state(state: &SharedState) -> Self {
        let firm = state.selected_firm.as_deref().unwrap_or("none selected");
        let day_state = state.daily_state.label();
        let account_risk = format!(
            "Connection: {:?}\nFeed healthy: {}\nFeed stale ms: {}\nSelected firm: {}\nDaily state: {}\nBalance: ${:.2}\nEquity: ${:.2}\nDaily PnL: ${:.2} ({:+.2}%)\nTotal PnL: ${:.2} ({:+.2}%)\nDaily DD trigger: {:.2}%\nTotal DD trigger: {:.2}%\nKill switch active: {}\nOpen positions: {}",
            state.connection_status,
            if state.feed_healthy() { "yes" } else { "no" },
            state.stale_ms,
            firm,
            day_state,
            state.balance,
            state.equity,
            state.daily_pnl,
            state.daily_pnl_pct,
            state.total_pnl,
            state.total_pnl_pct,
            state.config.kill_switch.daily_dd_trigger_pct,
            state.config.kill_switch.total_dd_trigger_pct,
            state.kill_switch_active,
            state.positions.len(),
        );

        let heads = if state.active_heads.is_empty() {
            "none".to_string()
        } else {
            state
                .active_heads
                .iter()
                .map(|head| format!("{head:?}"))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let regimes = if state.regime_by_symbol.is_empty() {
            "none".to_string()
        } else {
            state
                .regime_by_symbol
                .iter()
                .take(MAX_REGIMES)
                .map(|(symbol, regime)| format!("{symbol}: {regime:?}"))
                .collect::<Vec<_>>()
                .join(" | ")
        };
        let market_session = format!(
            "Chart symbol: {}\nTracked regimes: {}\nActive heads: {}\nMarkets tracked: {}",
            if state.chart_symbol.is_empty() {
                "n/a"
            } else {
                &state.chart_symbol
            },
            regimes,
            heads,
            state.regime_by_symbol.len(),
        );

        let recent_warnings = state
            .logs
            .iter()
            .rev()
            .filter(|entry| matches!(entry.level, LogLevel::Warn | LogLevel::Error))
            .take(MAX_WARNINGS)
            .map(|entry| format!("[{}] {}", entry.level.as_str(), entry.message))
            .collect::<Vec<_>>();

        let recent_journal = state
            .trade_journal
            .iter()
            .rev()
            .take(MAX_JOURNAL)
            .map(|entry| {
                let note = entry
                    .user_note
                    .as_deref()
                    .filter(|note| !note.trim().is_empty())
                    .map(|note| format!(" note={}", truncate_inline(note, 90)))
                    .unwrap_or_default();
                format!(
                    "{} {:?} {} pnl=${:.2} r={:.2} entry={} exit={}{}",
                    entry.symbol,
                    entry.direction,
                    entry.session,
                    entry.pnl,
                    entry.r_multiple,
                    truncate_inline(&entry.entry_reason, 70),
                    truncate_inline(&entry.exit_reason, 70),
                    note,
                )
            })
            .collect::<Vec<_>>();

        let gate_rejections = state
            .gate_rejections
            .iter()
            .rev()
            .take(MAX_REJECTIONS)
            .map(|reject| {
                format!(
                    "{} {:?}: {}",
                    reject.symbol,
                    reject.head,
                    truncate_inline(&reject.reason, 90)
                )
            })
            .collect::<Vec<_>>();

        Self {
            account_risk,
            market_session,
            recent_warnings,
            recent_journal,
            gate_rejections,
        }
    }

    pub fn summary_items(&self, selection: &OracleContextSelection) -> Vec<String> {
        let mut items = Vec::new();
        if selection.account_risk {
            items.push("Account & Risk".to_string());
        }
        if selection.market_session {
            items.push("Market & Session".to_string());
        }
        if selection.recent_warnings {
            items.push(format!("Recent Warnings ({})", self.recent_warnings.len()));
        }
        if selection.recent_journal {
            items.push(format!("Recent Journal ({})", self.recent_journal.len()));
        }
        if selection.gate_rejections {
            items.push(format!("Gate Rejections ({})", self.gate_rejections.len()));
        }
        items
    }
}

#[derive(Debug, Clone)]
pub struct OraclePrompt {
    pub system: String,
    pub user: String,
}

pub fn default_system_preprompt() -> String {
    DEFAULT_SYSTEM_PREPROMPT.to_string()
}

pub fn normalize_preprompt(preprompt: &str) -> String {
    let trimmed = preprompt.trim();
    if trimmed.is_empty() {
        default_system_preprompt()
    } else {
        trimmed.to_string()
    }
}

pub fn build_prompt(
    system_preprompt: &str,
    question: &str,
    selection: &OracleContextSelection,
    snapshot: &OracleContextSnapshot,
) -> OraclePrompt {
    let mut user = String::from("Operator question:\n");
    user.push_str(question.trim());
    user.push_str("\n\nVisible context:\n");

    if selection.account_risk {
        push_section(&mut user, "Account & Risk", Some(&snapshot.account_risk));
    }
    if selection.market_session {
        push_section(
            &mut user,
            "Market & Session",
            Some(&snapshot.market_session),
        );
    }
    if selection.recent_warnings {
        push_section_list(&mut user, "Recent Warnings", &snapshot.recent_warnings);
    }
    if selection.recent_journal {
        push_section_list(&mut user, "Recent Journal", &snapshot.recent_journal);
    }
    if selection.gate_rejections {
        push_section_list(&mut user, "Gate Rejections", &snapshot.gate_rejections);
    }
    if selection.enabled_count() == 0 {
        user.push_str("- No app context attached.\n");
    }

    OraclePrompt {
        system: normalize_preprompt(system_preprompt),
        user,
    }
}

fn push_section(buf: &mut String, title: &str, body: Option<&str>) {
    buf.push_str(&format!("\n[{title}]\n"));
    match body {
        Some(text) if !text.trim().is_empty() => {
            buf.push_str(text.trim());
            buf.push('\n');
        }
        _ => buf.push_str("- No data available.\n"),
    }
}

fn push_section_list(buf: &mut String, title: &str, items: &[String]) {
    buf.push_str(&format!("\n[{title}]\n"));
    if items.is_empty() {
        buf.push_str("- No recent items.\n");
        return;
    }
    for item in items {
        buf.push_str("- ");
        buf.push_str(item);
        buf.push('\n');
    }
}

fn truncate_inline(text: &str, max_chars: usize) -> String {
    let compact = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out = compact.chars().take(max_chars).collect::<String>();
    if compact.chars().count() > max_chars {
        out.push_str("...");
    }
    out
}

#[cfg(test)]
mod tests {
    use rust_decimal_macros::dec;

    use super::*;
    use crate::state::{ConnectionStatus, GateReject, JournalEntry, SharedState};
    use gadarah_core::{Direction, HeadId};

    #[test]
    fn normalize_empty_preprompt_uses_default() {
        let normalized = normalize_preprompt("   ");
        assert!(normalized.contains("Never place trades"));
        assert!(normalized.contains("Assessment, Risks, Next checks"));
    }

    #[test]
    fn build_prompt_only_includes_selected_sections() {
        let snapshot = OracleContextSnapshot {
            account_risk: "A".into(),
            market_session: "B".into(),
            recent_warnings: vec!["warn".into()],
            recent_journal: vec!["journal".into()],
            gate_rejections: vec!["reject".into()],
        };
        let selection = OracleContextSelection {
            account_risk: true,
            market_session: false,
            recent_warnings: true,
            recent_journal: false,
            gate_rejections: false,
        };
        let prompt = build_prompt("system", "What matters?", &selection, &snapshot);
        assert!(prompt.user.contains("[Account & Risk]"));
        assert!(prompt.user.contains("[Recent Warnings]"));
        assert!(!prompt.user.contains("[Market & Session]"));
        assert!(!prompt.user.contains("[Recent Journal]"));
    }

    #[test]
    fn snapshot_bounds_recent_lists() {
        let mut state = SharedState::default();
        state.connection_status = ConnectionStatus::ConnectedLive;
        state.logs.push_back(crate::state::LogEntry {
            timestamp: 0,
            level: LogLevel::Info,
            message: "ignore".into(),
        });
        for idx in 0..5 {
            state.add_log(LogLevel::Warn, format!("warn-{idx}"));
            state.push_journal(JournalEntry {
                trade_id: idx as u64,
                opened_at: 0,
                closed_at: 0,
                symbol: "EURUSD".into(),
                head: HeadId::Momentum,
                direction: Direction::Buy,
                regime: "Trend".into(),
                session: "London".into(),
                entry_price: dec!(1.1),
                exit_price: dec!(1.2),
                lots: dec!(0.1),
                pnl: dec!(12),
                r_multiple: dec!(1.2),
                slippage_pips: dec!(0.2),
                entry_reason: "entry".into(),
                exit_reason: "exit".into(),
                posterior_p: None,
                user_tag: None,
                user_note: None,
            });
            state.push_gate_rejection(GateReject {
                timestamp: idx as i64,
                symbol: "EURUSD".into(),
                head: HeadId::Momentum,
                reason: format!("reject-{idx}"),
            });
        }

        let snapshot = OracleContextSnapshot::from_shared_state(&state);
        assert_eq!(snapshot.recent_warnings.len(), MAX_WARNINGS);
        assert_eq!(snapshot.recent_journal.len(), MAX_JOURNAL);
        assert_eq!(snapshot.gate_rejections.len(), MAX_REJECTIONS);
        assert!(snapshot.recent_warnings[0].contains("warn-4"));
    }
}
