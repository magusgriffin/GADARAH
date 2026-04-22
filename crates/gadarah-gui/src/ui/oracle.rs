//! Oracle tab — Ask-the-Oracle chat, model picker, Ollama status, and
//! remote-endpoint management. The Oracle itself cannot place orders; every
//! response is rendered as display text only.

use eframe::egui::{self, Color32, RichText, ScrollArea};

use crate::oracle::{
    config::RemoteEndpoint,
    model::{ModelKind, ModelSpec},
    OracleAdvice, OracleConfig, OracleRequest,
};
use crate::theme;
use crate::widgets::ornaments;

/// Tab-local state (transcript, input buffer, new-endpoint fields).
pub struct OraclePanel {
    pub transcript: Vec<(TranscriptRole, String)>,
    pub input: String,
    pub awaiting: bool,
    pub last_status_alive: Option<bool>,
    pub new_endpoint: RemoteEndpoint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscriptRole {
    User,
    Oracle,
    System,
}

impl Default for OraclePanel {
    fn default() -> Self {
        Self {
            transcript: Vec::new(),
            input: String::new(),
            awaiting: false,
            last_status_alive: None,
            new_endpoint: RemoteEndpoint {
                label: String::new(),
                base_url: "https://api.moonshot.cn".to_string(),
                model_id: "moonshot-v1-32k".to_string(),
                api_key_env: "MOONSHOT_API_KEY".to_string(),
            },
        }
    }
}

impl OraclePanel {
    /// Called from the main loop when new replies arrive. Only the oracle
    /// task pushes messages, so the transcript remains append-only.
    pub fn record_advice(&mut self, advice: &OracleAdvice) {
        self.awaiting = false;
        self.transcript
            .push((TranscriptRole::Oracle, advice.body().to_string()));
    }

    pub fn record_error(&mut self, msg: String) {
        self.awaiting = false;
        self.transcript.push((TranscriptRole::System, msg));
    }

    pub fn record_status(&mut self, alive: bool) {
        self.last_status_alive = Some(alive);
        self.transcript.push((
            TranscriptRole::System,
            if alive {
                "Ollama reachable.".to_string()
            } else {
                "Ollama not reachable.".to_string()
            },
        ));
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        cfg: &mut OracleConfig,
        tx: Option<&std::sync::mpsc::Sender<OracleRequest>>,
    ) {
        // Header ─────────────────────────────────────────────────────────────
        ornaments::stone_tablet().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("THE ORACLE")
                        .size(18.0)
                        .color(theme::FORGE_GOLD)
                        .strong(),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let (pill_text, pill_bg, pill_fg) = match self.last_status_alive {
                        Some(true) => ("Ollama ready", Color32::from_rgb(10, 38, 20), theme::GREEN),
                        Some(false) => ("Ollama offline", Color32::from_rgb(40, 10, 10), theme::RED),
                        None => ("Status unknown", Color32::from_rgb(20, 24, 30), theme::MUTED),
                    };
                    theme::pill(ui, pill_text, pill_bg, pill_fg);
                    if ui.small_button("Ping").clicked() {
                        if let Some(tx) = tx {
                            let _ = tx.send(OracleRequest::Ping);
                        }
                    }
                });
            });
            ui.add_space(4.0);
            ui.label(
                RichText::new("The Oracle analyses and advises. It cannot place orders; every answer is text only, for your eyes alone.")
                    .color(theme::MUTED)
                    .size(11.5),
            );
        });

        ui.add_space(12.0);

        ui.columns(2, |cols| {
            // ── Left: transcript + input ─────────────────────────────────────
            let left = &mut cols[0];
            theme::card().show(left, |ui| {
                ui.set_min_height(400.0);
                ui.label(
                    RichText::new("ASK THE ORACLE")
                        .size(11.0)
                        .color(theme::MUTED)
                        .strong(),
                );
                ui.add_space(4.0);
                ScrollArea::vertical()
                    .auto_shrink([false; 2])
                    .max_height(320.0)
                    .show(ui, |ui| {
                        if self.transcript.is_empty() {
                            theme::empty_state(
                                ui,
                                "✴",
                                "No questions asked yet",
                                "Pose a question about the current session or journal",
                            );
                        }
                        for (role, body) in &self.transcript {
                            render_bubble(ui, *role, body);
                            ui.add_space(6.0);
                        }
                    });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    let response = ui.add_sized(
                        egui::vec2(ui.available_width() - 100.0, 28.0),
                        egui::TextEdit::singleline(&mut self.input)
                            .hint_text("Pose a question…"),
                    );
                    let send = ui.add(
                        egui::Button::new(RichText::new("Ask").color(Color32::WHITE))
                            .fill(theme::FORGE_GOLD_DIM),
                    );
                    let do_send = send.clicked()
                        || (response.lost_focus()
                            && ui.input(|i| i.key_pressed(egui::Key::Enter)));
                    if do_send && !self.input.trim().is_empty() && !self.awaiting {
                        if let Some(tx) = tx {
                            let user_msg = self.input.trim().to_string();
                            self.transcript.push((TranscriptRole::User, user_msg.clone()));
                            self.awaiting = true;
                            let _ = tx.send(OracleRequest::Analyze {
                                system: "You are a disciplined trading analyst. Answer concisely. Never recommend specific orders — only observations, risks, and questions.".to_string(),
                                user: user_msg,
                                tag: "chat",
                            });
                            self.input.clear();
                        }
                    }
                });
                if self.awaiting {
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new("…the Oracle considers…")
                            .italics()
                            .color(theme::MUTED),
                    );
                }
            });

            // ── Right: settings ──────────────────────────────────────────────
            let right = &mut cols[1];
            settings_block(right, cfg, &mut self.new_endpoint, tx);
        });
    }
}

fn render_bubble(ui: &mut egui::Ui, role: TranscriptRole, body: &str) {
    let (label, bg, fg) = match role {
        TranscriptRole::User => ("YOU", Color32::from_rgb(14, 24, 38), theme::BLUE),
        TranscriptRole::Oracle => ("ORACLE", theme::FORGE_OBSIDIAN, theme::FORGE_GOLD),
        TranscriptRole::System => ("SYSTEM", Color32::from_rgb(24, 22, 6), theme::YELLOW),
    };
    egui::Frame::new()
        .fill(bg)
        .stroke(egui::Stroke::new(1.0, fg.linear_multiply(0.4)))
        .corner_radius(6u8)
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            ui.label(RichText::new(label).size(10.0).color(fg).strong());
            ui.add_space(2.0);
            ui.label(RichText::new(body).color(theme::TEXT).size(12.5));
        });
}

fn settings_block(
    ui: &mut egui::Ui,
    cfg: &mut OracleConfig,
    new_endpoint: &mut RemoteEndpoint,
    tx: Option<&std::sync::mpsc::Sender<OracleRequest>>,
) {
    theme::card().show(ui, |ui| {
        ui.set_min_height(400.0);
        ui.label(
            RichText::new("INTEGRATIONS & MODEL")
                .size(11.0)
                .color(theme::MUTED)
                .strong(),
        );
        ui.add_space(6.0);

        // Enable toggle
        ui.horizontal(|ui| {
            ui.checkbox(&mut cfg.enabled, "Oracle enabled");
            ui.add_space(8.0);
            if ui.small_button("Save").clicked() {
                let _ = cfg.save();
                if let Some(tx) = tx {
                    let _ = tx.send(OracleRequest::UpdateConfig(Box::new(cfg.clone())));
                }
            }
        });

        ui.add_space(6.0);

        // Ollama URL
        ui.label(RichText::new("Ollama URL").size(11.0).color(theme::MUTED));
        ui.add(egui::TextEdit::singleline(&mut cfg.ollama_url).desired_width(f32::INFINITY));

        ui.add_space(8.0);

        // Model picker
        ui.label(RichText::new("Model").size(11.0).color(theme::MUTED));
        let current_label = cfg
            .selected_spec()
            .map(|s| s.label.clone())
            .unwrap_or_else(|| "—".to_string());
        egui::ComboBox::from_id_salt("oracle_model_picker")
            .width(ui.available_width())
            .selected_text(current_label)
            .show_ui(ui, |ui| {
                for (i, entry) in cfg.registry.entries.iter().enumerate() {
                    ui.selectable_value(&mut cfg.selected, i, &entry.label);
                }
            });
        if let Some(spec) = cfg.selected_spec() {
            if let Some(gb) = spec.kind.approx_disk_gb() {
                if gb > 0.0 {
                    ui.label(
                        RichText::new(format!("~{:.1} GB disk", gb))
                            .size(10.5)
                            .color(theme::MUTED)
                            .italics(),
                    );
                }
            }
        }

        ui.add_space(10.0);

        // Remote endpoints
        ui.label(
            RichText::new("Remote endpoints (OpenAI-compatible)")
                .size(11.0)
                .color(theme::MUTED),
        );
        if cfg.remotes.is_empty() {
            ui.label(
                RichText::new("None configured. Add one below to use Kimi K2, OpenAI, etc.")
                    .italics()
                    .color(theme::DIM),
            );
        }
        let mut remove_idx: Option<usize> = None;
        for (i, r) in cfg.remotes.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(RichText::new(&r.label).strong());
                ui.label(RichText::new(&r.base_url).color(theme::MUTED));
                if ui.small_button("×").clicked() {
                    remove_idx = Some(i);
                }
            });
        }
        if let Some(i) = remove_idx {
            // Also drop any registry entry with the same label, so the
            // picker doesn't point at a dead endpoint.
            let label = cfg.remotes[i].label.clone();
            cfg.remotes.remove(i);
            cfg.registry
                .entries
                .retain(|e| !(e.kind == ModelKind::RemoteOpenAI && e.label == label));
        }

        ui.add_space(6.0);
        ui.collapsing("Add endpoint", |ui| {
            ui.label(RichText::new("Label").size(10.5).color(theme::MUTED));
            ui.text_edit_singleline(&mut new_endpoint.label);
            ui.label(RichText::new("Base URL").size(10.5).color(theme::MUTED));
            ui.text_edit_singleline(&mut new_endpoint.base_url);
            ui.label(RichText::new("Model ID").size(10.5).color(theme::MUTED));
            ui.text_edit_singleline(&mut new_endpoint.model_id);
            ui.label(
                RichText::new("API key env var")
                    .size(10.5)
                    .color(theme::MUTED),
            );
            ui.text_edit_singleline(&mut new_endpoint.api_key_env);
            ui.add_space(4.0);
            if ui.button("Register").clicked() && !new_endpoint.label.trim().is_empty() {
                cfg.remotes.push(new_endpoint.clone());
                cfg.registry.entries.push(ModelSpec {
                    kind: ModelKind::RemoteOpenAI,
                    ollama_tag: None,
                    label: new_endpoint.label.clone(),
                });
                *new_endpoint = RemoteEndpoint {
                    label: String::new(),
                    base_url: "https://".to_string(),
                    model_id: String::new(),
                    api_key_env: String::new(),
                };
            }
        });
    });
}
