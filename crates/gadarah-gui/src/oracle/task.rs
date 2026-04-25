//! Background worker — keeps the Oracle off the UI thread.
//!
//! The handle owns two channels: one for outgoing requests, one for inbound
//! replies. A single worker thread drains requests serially; if the user
//! queues a second request while the first is in-flight the first completes
//! first, then the second. Good enough for v1.

use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread;

use super::client::{self, OracleError};
use super::config::OracleConfig;
use super::prompt::{build_prompt, OracleContextSelection, OracleContextSnapshot};
use super::OracleAdvice;

pub enum OracleRequest {
    /// Analyse a free-form text blob (trade journal excerpt, current
    /// positions, etc.) and return a short assessment.
    Analyze {
        question: String,
        context_snapshot: Box<OracleContextSnapshot>,
        context_selection: OracleContextSelection,
        tag: &'static str,
    },
    /// Refresh status (e.g. ping Ollama). Reply is always `Status`.
    Ping,
    /// Replace the config. Used by the Integrations panel.
    UpdateConfig(Box<OracleConfig>),
    /// Ask the worker to exit cleanly.
    Shutdown,
}

pub enum OracleReply {
    Ready(OracleAdvice),
    Error(String),
    Status { ollama_alive: bool },
}

pub struct OracleHandle {
    pub tx: Sender<OracleRequest>,
    pub rx: Receiver<OracleReply>,
}

impl OracleHandle {
    pub fn spawn(initial_cfg: OracleConfig) -> Self {
        let (req_tx, req_rx) = channel::<OracleRequest>();
        let (rep_tx, rep_rx) = channel::<OracleReply>();
        thread::Builder::new()
            .name("gadarah-oracle".into())
            .spawn(move || worker(initial_cfg, req_rx, rep_tx))
            .expect("spawn oracle worker");
        Self {
            tx: req_tx,
            rx: rep_rx,
        }
    }

    /// Drain any pending replies without blocking. The UI loop calls this
    /// once per frame.
    pub fn drain(&self) -> Vec<OracleReply> {
        self.rx.try_iter().collect()
    }
}

fn worker(mut cfg: OracleConfig, req_rx: Receiver<OracleRequest>, rep_tx: Sender<OracleReply>) {
    while let Ok(req) = req_rx.recv() {
        match req {
            OracleRequest::Shutdown => return,
            OracleRequest::UpdateConfig(new_cfg) => {
                cfg = *new_cfg;
            }
            OracleRequest::Ping => {
                let alive = client::ollama_alive(&cfg);
                let _ = rep_tx.send(OracleReply::Status {
                    ollama_alive: alive,
                });
            }
            OracleRequest::Analyze {
                question,
                context_snapshot,
                context_selection,
                tag,
            } => {
                let prompt = build_prompt(
                    &cfg.system_preprompt,
                    &question,
                    &context_selection,
                    &context_snapshot,
                );
                match client::generate(&cfg, &prompt.system, &prompt.user) {
                    Ok(body) => {
                        let advice = OracleAdvice::new(body, tag);
                        let _ = rep_tx.send(OracleReply::Ready(advice));
                    }
                    Err(e) => {
                        let _ = rep_tx.send(OracleReply::Error(format_err(e)));
                    }
                }
            }
        }
    }
}

fn format_err(e: OracleError) -> String {
    match e {
        OracleError::Disabled => "Oracle is disabled in settings.".to_string(),
        OracleError::NoModel => "No model selected.".to_string(),
        OracleError::MissingRemote(l) => format!("Remote endpoint '{l}' is not configured."),
        OracleError::MissingApiKey(k) => format!("Environment variable '{k}' is not set."),
        OracleError::EmptyResponse => "Model returned no content.".to_string(),
        OracleError::Http(err) => format!("HTTP error: {err}"),
    }
}
