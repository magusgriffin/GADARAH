//! Blocking HTTP client for Ollama `/api/generate` and for OpenAI-compatible
//! `/v1/chat/completions`. Kept simple on purpose — non-streaming in v1, a
//! proper SSE stream reader lands later.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::config::{OracleConfig, RemoteEndpoint};
use super::model::{ModelKind, ModelSpec};

#[derive(Debug, thiserror::Error)]
pub enum OracleError {
    #[error("oracle is disabled in config")]
    Disabled,
    #[error("no model selected")]
    NoModel,
    #[error("remote endpoint {0} is not configured")]
    MissingRemote(String),
    #[error("environment variable {0} is not set")]
    MissingApiKey(String),
    #[error(transparent)]
    Http(#[from] reqwest::Error),
    #[error("model returned no content")]
    EmptyResponse,
}

/// Dispatch a prompt to whatever model is currently selected.
pub fn generate(cfg: &OracleConfig, system: &str, user: &str) -> Result<String, OracleError> {
    if !cfg.enabled {
        return Err(OracleError::Disabled);
    }
    let spec = cfg.selected_spec().ok_or(OracleError::NoModel)?;
    match spec.kind {
        ModelKind::DeepSeekR1_1_5B | ModelKind::DeepSeekR1_7B | ModelKind::CustomGguf => {
            call_ollama(cfg, spec, system, user)
        }
        ModelKind::RemoteOpenAI => {
            let remote = cfg
                .remotes
                .iter()
                .find(|r| r.label == spec.label)
                .ok_or_else(|| OracleError::MissingRemote(spec.label.clone()))?;
            call_openai_compat(cfg, remote, system, user)
        }
    }
}

// ── Ollama ──────────────────────────────────────────────────────────────────
#[derive(Serialize)]
struct OllamaRequest<'a> {
    model: &'a str,
    prompt: String,
    stream: bool,
    options: OllamaOptions,
}

#[derive(Serialize)]
struct OllamaOptions {
    temperature: f32,
    num_predict: u32,
}

#[derive(Deserialize)]
struct OllamaResponse {
    response: String,
}

fn call_ollama(
    cfg: &OracleConfig,
    spec: &ModelSpec,
    system: &str,
    user: &str,
) -> Result<String, OracleError> {
    let tag = spec
        .ollama_tag
        .as_deref()
        .ok_or(OracleError::NoModel)?;
    let prompt = format!("<|system|>\n{system}\n<|user|>\n{user}\n<|assistant|>\n");
    let body = OllamaRequest {
        model: tag,
        prompt,
        stream: false,
        options: OllamaOptions {
            temperature: cfg.temperature,
            num_predict: cfg.max_tokens,
        },
    };
    let url = format!("{}/api/generate", cfg.ollama_url.trim_end_matches('/'));
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()?;
    let resp: OllamaResponse = client.post(url).json(&body).send()?.error_for_status()?.json()?;
    if resp.response.is_empty() {
        return Err(OracleError::EmptyResponse);
    }
    Ok(resp.response)
}

/// Best-effort ping. `true` means we got a 200; `false` could be offline or
/// the URL misconfigured.
pub fn ollama_alive(cfg: &OracleConfig) -> bool {
    let url = format!("{}/api/tags", cfg.ollama_url.trim_end_matches('/'));
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .ok()
        .and_then(|c| c.get(&url).send().ok())
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}

// ── OpenAI-compatible ───────────────────────────────────────────────────────
#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: Vec<ChatMessage<'a>>,
    temperature: f32,
    max_tokens: u32,
    stream: bool,
}

#[derive(Serialize)]
struct ChatMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatContent,
}

#[derive(Deserialize)]
struct ChatContent {
    content: String,
}

fn call_openai_compat(
    cfg: &OracleConfig,
    remote: &RemoteEndpoint,
    system: &str,
    user: &str,
) -> Result<String, OracleError> {
    let api_key = std::env::var(&remote.api_key_env)
        .map_err(|_| OracleError::MissingApiKey(remote.api_key_env.clone()))?;
    let url = format!(
        "{}/v1/chat/completions",
        remote.base_url.trim_end_matches('/')
    );
    let body = ChatRequest {
        model: &remote.model_id,
        messages: vec![
            ChatMessage {
                role: "system",
                content: system,
            },
            ChatMessage {
                role: "user",
                content: user,
            },
        ],
        temperature: cfg.temperature,
        max_tokens: cfg.max_tokens,
        stream: false,
    };
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(90))
        .build()?;
    let resp: ChatResponse = client
        .post(url)
        .bearer_auth(api_key)
        .json(&body)
        .send()?
        .error_for_status()?
        .json()?;
    let content = resp
        .choices
        .into_iter()
        .next()
        .map(|c| c.message.content)
        .ok_or(OracleError::EmptyResponse)?;
    if content.is_empty() {
        return Err(OracleError::EmptyResponse);
    }
    Ok(content)
}
