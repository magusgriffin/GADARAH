//! Oracle configuration — persisted to `$CONFIG/gadarah/oracle.toml`.
//!
//! Remote API keys are **not** embedded here; they live in the
//! OS-appropriate secure store when `keyring` feature lights up (TODO), or
//! in a sibling `secrets.toml` with 0600 perms as a v1 fallback.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::model::{ModelRegistry, ModelSpec};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteEndpoint {
    /// Display name, e.g. "Moonshot Kimi K2".
    pub label: String,
    /// Base URL including protocol, e.g. `https://api.moonshot.cn`.
    pub base_url: String,
    /// Model name the endpoint accepts in the `model` field.
    pub model_id: String,
    /// Environment variable name holding the API key, e.g. `MOONSHOT_API_KEY`.
    /// Keeps secrets off disk.
    pub api_key_env: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OracleConfig {
    /// Ollama base URL. Default `http://127.0.0.1:11434`.
    pub ollama_url: String,
    /// Currently-selected model index into [`ModelRegistry::entries`].
    pub selected: usize,
    /// Extra local/remote models users added at runtime.
    pub registry: ModelRegistry,
    /// Remote endpoints. Index into `registry.entries` with
    /// `ModelKind::RemoteOpenAI` cross-references these by `label`.
    pub remotes: Vec<RemoteEndpoint>,
    /// Temperature for analytical prompts.
    pub temperature: f32,
    /// Soft token cap.
    pub max_tokens: u32,
    /// When false the Oracle is considered "sleeping" and no model calls are
    /// made. Useful for users without Ollama installed.
    pub enabled: bool,
}

impl Default for OracleConfig {
    fn default() -> Self {
        Self {
            ollama_url: "http://127.0.0.1:11434".to_string(),
            selected: 0,
            registry: ModelRegistry::default(),
            remotes: Vec::new(),
            temperature: 0.2,
            max_tokens: 512,
            enabled: true,
        }
    }
}

impl OracleConfig {
    pub fn selected_spec(&self) -> Option<&ModelSpec> {
        self.registry.entries.get(self.selected)
    }

    pub fn config_path() -> Option<PathBuf> {
        dirs::config_dir().map(|p| p.join("gadarah").join("oracle.toml"))
    }

    pub fn load() -> Self {
        let Some(path) = Self::config_path() else {
            return Self::default();
        };
        let Ok(raw) = std::fs::read_to_string(&path) else {
            return Self::default();
        };
        toml::from_str(&raw).unwrap_or_default()
    }

    pub fn save(&self) -> std::io::Result<()> {
        let Some(path) = Self::config_path() else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let body = toml::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
        std::fs::write(path, body)
    }
}
