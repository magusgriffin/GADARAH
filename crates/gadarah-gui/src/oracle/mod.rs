//! Oracle — the LLM advisor layer.
//!
//! # Non-negotiables
//!
//! 1. The Oracle **cannot** place orders. `OracleAdvice` is a sealed newtype
//!    around a `String` with no broker or risk trait implementations. Code
//!    that wants to act on advice must extract the string and go through the
//!    normal user-confirmed flow.
//!
//! 2. All model calls run on a background thread. The UI never blocks.
//!
//! 3. The default model is DeepSeek R1 1.5B via a local Ollama instance.
//!    Users can install heavier GGUFs, or point at a remote OpenAI-compatible
//!    endpoint (Moonshot / OpenAI / Anthropic-compat gateways).

pub mod client;
pub mod config;
pub mod model;
pub mod prompt;
pub mod task;

pub use config::{OracleConfig, RemoteEndpoint};
pub use model::{ModelKind, ModelRegistry, ModelSpec};
pub use prompt::{
    default_system_preprompt, OracleContextSelection, OracleContextSnapshot,
    DEFAULT_SYSTEM_PREPROMPT,
};
pub use task::{OracleHandle, OracleReply, OracleRequest};

/// Sealed advice from the Oracle. Deliberately opaque: the outside world can
/// display the string and store it, but cannot downcast, extend, or feed it
/// into an order path. The only way to obtain an `OracleAdvice` is through
/// [`crate::oracle::task::OracleHandle`].
#[derive(Debug, Clone)]
pub struct OracleAdvice {
    body: String,
    /// Short tag: "analysis", "debrief", "chat", …
    tag: &'static str,
}

impl OracleAdvice {
    /// Constructor is crate-private — external callers can only receive
    /// `OracleAdvice` via the task channel.
    pub(crate) fn new(body: String, tag: &'static str) -> Self {
        Self { body, tag }
    }

    pub fn body(&self) -> &str {
        &self.body
    }

    pub fn tag(&self) -> &'static str {
        self.tag
    }
}
