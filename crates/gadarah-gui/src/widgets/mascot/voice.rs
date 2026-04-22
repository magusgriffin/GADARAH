//! Phrase library for mascot speech.
//!
//! Phrases are short and Warhammer-gothic in cadence without crossing into
//! parody. Deterministic given `(head, topic)` plus a rotation index — no
//! randomness at render time, so a frame that re-renders with the same state
//! produces the same text.

use super::MascotSubsystem;

/// High-level topic the voice should address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceContext {
    FeedStale,
    FeedRecovered,
    DdApproaching,
    DdBreached,
    KillSwitchArmed,
    KillSwitchCleared,
    GateRejected,
    DailyStopReached,
    ProfitTargetReached,
    ChallengePassed,
    OracleReady,
    OracleOffline,
    JournalMilestone,
    Idle,
}

pub fn phrase_for(head: MascotSubsystem, ctx: VoiceContext, rotation: u32) -> String {
    let pool = phrases(head, ctx);
    if pool.is_empty() {
        return String::new();
    }
    let idx = (rotation as usize) % pool.len();
    pool[idx].to_string()
}

fn phrases(head: MascotSubsystem, ctx: VoiceContext) -> &'static [&'static str] {
    use MascotSubsystem as H;
    use VoiceContext as C;
    match (head, ctx) {
        // ── The Herald (feed / broker) ─────────────────────────────────────
        (H::MarketFeed, C::FeedStale) => &[
            "The wires are silent. No word has reached the tower in some time.",
            "Silence from the markets. I cannot swear to what the price is now.",
            "The tickers have ceased. Trust nothing until they breathe again.",
        ],
        (H::MarketFeed, C::FeedRecovered) => &[
            "Couriers returned. The price is known once more.",
            "The feed resumes. Proceed — carefully.",
        ],

        // ── The Warden (risk gate) ─────────────────────────────────────────
        (H::RiskGate, C::GateRejected) => &[
            "The gate is shut. This order did not meet the covenants.",
            "Refused at the threshold. Examine the rejection scroll.",
        ],
        (H::RiskGate, C::KillSwitchArmed) => &[
            "The kill rune is lit. No blade leaves the sheath.",
            "I have sealed the vault. Trading is halted until the rite ends.",
        ],
        (H::RiskGate, C::KillSwitchCleared) => &[
            "The seal is broken. Move, but not recklessly.",
        ],

        // ── The Reckoner (challenge clock) ─────────────────────────────────
        (H::ChallengeClock, C::DdApproaching) => &[
            "The drawdown creeps toward the limit. Tighten the leash.",
            "We are nearing the pyre. Consider resting the account.",
        ],
        (H::ChallengeClock, C::DdBreached) => &[
            "The floor has been struck. The challenge is forfeit.",
            "All is lost. Record what was learned and begin again.",
        ],
        (H::ChallengeClock, C::DailyStopReached) => &[
            "The day is done. No further trades shall open.",
            "The daily rite is closed. Rest until the bells.",
        ],
        (H::ChallengeClock, C::ProfitTargetReached) => &[
            "The target is taken. Cease — do not squander the passage.",
            "We have met the mark. Stand down; the trial is won.",
        ],
        (H::ChallengeClock, C::ChallengePassed) => &[
            "The evaluation bends the knee. You are funded.",
        ],

        // ── The Oracle (AI) ────────────────────────────────────────────────
        (H::Oracle, C::OracleReady) => &[
            "The divination circle is lit. Ask, and I shall answer.",
            "The Oracle attends. Pose the question plainly.",
        ],
        (H::Oracle, C::OracleOffline) => &[
            "The Oracle has withdrawn. The circle waits for its lamp.",
        ],

        // ── The Chronicler (journal / logs) ────────────────────────────────
        (H::Chronicler, C::JournalMilestone) => &[
            "A new entry is engraved in the ledger.",
            "The chronicle has grown. Review it when the day is through.",
        ],

        // ── Idle chatter ───────────────────────────────────────────────────
        (_, C::Idle) => &[
            "All is quiet on the ramparts.",
            "I stand watch.",
            "The furnace hums. Nothing stirs.",
        ],

        // Fallbacks
        _ => &["…"],
    }
}
