//! Model catalogue. DeepSeek R1 1.5B is the default because it runs at 20–40
//! tok/s on modern CPU at Q4, needs ~1.1 GB disk and ~2 GB RAM, and is sane
//! enough for short analytical prompts.
//!
//! The 7B tier is offered as a step up for users with a discrete GPU; the
//! custom GGUF path lets users bring any HuggingFace GGUF (subject to disk
//! guardrails); remote endpoints are the escape hatch for users who want
//! frontier models like Kimi K2 without owning H100-class hardware.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModelKind {
    /// Default. ~1.1 GB. Ollama tag: `deepseek-r1:1.5b`.
    DeepSeekR1_1_5B,
    /// Step up. ~4.7 GB. Ollama tag: `deepseek-r1:7b`. Needs ~8 GB RAM.
    DeepSeekR1_7B,
    /// Arbitrary user-provided GGUF wrapped via `ollama create` from a
    /// Modelfile. Size depends on the file.
    CustomGguf,
    /// OpenAI-compatible `/v1/chat/completions` endpoint. Lets users point at
    /// OpenAI, Moonshot (Kimi K2 hosted), Together, OpenRouter, etc.
    RemoteOpenAI,
}

impl ModelKind {
    pub fn display_name(self) -> &'static str {
        match self {
            Self::DeepSeekR1_1_5B => "DeepSeek R1 1.5B (default, local)",
            Self::DeepSeekR1_7B => "DeepSeek R1 7B (local, needs GPU)",
            Self::CustomGguf => "Custom GGUF (HuggingFace)",
            Self::RemoteOpenAI => "Remote OpenAI-compatible endpoint",
        }
    }

    pub fn is_local(self) -> bool {
        matches!(
            self,
            Self::DeepSeekR1_1_5B | Self::DeepSeekR1_7B | Self::CustomGguf
        )
    }

    /// Approximate disk footprint in GB for guardrail warnings.
    pub fn approx_disk_gb(self) -> Option<f32> {
        match self {
            Self::DeepSeekR1_1_5B => Some(1.1),
            Self::DeepSeekR1_7B => Some(4.7),
            Self::CustomGguf => None,
            Self::RemoteOpenAI => Some(0.0),
        }
    }
}

/// Everything needed to route a prompt to one model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelSpec {
    pub kind: ModelKind,
    /// For local models: the Ollama tag, e.g. `deepseek-r1:1.5b`.
    /// For remote: unused.
    pub ollama_tag: Option<String>,
    /// Display label surfaced in the picker.
    pub label: String,
}

impl ModelSpec {
    pub fn deepseek_1_5b() -> Self {
        Self {
            kind: ModelKind::DeepSeekR1_1_5B,
            ollama_tag: Some("deepseek-r1:1.5b".to_string()),
            label: ModelKind::DeepSeekR1_1_5B.display_name().to_string(),
        }
    }

    pub fn deepseek_7b() -> Self {
        Self {
            kind: ModelKind::DeepSeekR1_7B,
            ollama_tag: Some("deepseek-r1:7b".to_string()),
            label: ModelKind::DeepSeekR1_7B.display_name().to_string(),
        }
    }
}

/// Built-in registry. Users can add `CustomGguf` / `RemoteOpenAI` entries at
/// runtime through the Integrations panel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelRegistry {
    pub entries: Vec<ModelSpec>,
}

impl Default for ModelRegistry {
    fn default() -> Self {
        Self {
            entries: vec![ModelSpec::deepseek_1_5b(), ModelSpec::deepseek_7b()],
        }
    }
}
