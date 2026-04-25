//! Oracle tab — Ask-the-Oracle chat, visible context packs, and runtime
//! configuration. The Oracle advises only; it cannot place orders.

use eframe::egui::{self, Color32, RichText, ScrollArea};

use crate::oracle::{
    config::RemoteEndpoint,
    default_system_preprompt,
    model::{ModelKind, ModelSpec},
    OracleAdvice, OracleConfig, OracleContextSelection, OracleContextSnapshot, OracleRequest,
};
use crate::state::AppState;
use crate::theme;
use crate::widgets::ornaments;

/// Tab-local state (transcript, input buffer, context toggles, new-endpoint
/// fields).
pub struct OraclePanel {
    pub transcript: Vec<(TranscriptRole, String)>,
    pub input: String,
    pub awaiting: bool,
    pub last_status_alive: Option<bool>,
    pub new_endpoint: RemoteEndpoint,
    pub context_selection: OracleContextSelection,
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
            context_selection: OracleContextSelection::default(),
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
        app_state: &AppState,
        tx: Option<&std::sync::mpsc::Sender<OracleRequest>>,
    ) {
        let snapshot = {
            let g = app_state.lock().unwrap();
            OracleContextSnapshot::from_shared_state(&g)
        };

        ornaments::stone_tablet().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new("THE ORACLE")
                            .size(18.0)
                            .color(theme::FORGE_GOLD)
                            .strong(),
                    );
                    ui.label(
                        RichText::new(
                            "Hybrid analyst mode: thematic framing, plain risk-first substance, no order placement.",
                        )
                        .color(theme::MUTED)
                        .size(11.5),
                    );
                });
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
        });

        ui.add_space(12.0);

        if ui.available_width() > 980.0 {
            ui.columns(2, |cols| {
                self.primary_column(&mut cols[0], cfg, &snapshot, tx);
                self.settings_column(&mut cols[1], cfg, tx);
            });
        } else {
            self.primary_column(ui, cfg, &snapshot, tx);
            ui.add_space(12.0);
            self.settings_column(ui, cfg, tx);
        }
    }

    fn primary_column(
        &mut self,
        ui: &mut egui::Ui,
        cfg: &OracleConfig,
        snapshot: &OracleContextSnapshot,
        tx: Option<&std::sync::mpsc::Sender<OracleRequest>>,
    ) {
        theme::card().show(ui, |ui| {
            ui.set_min_height(460.0);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("ASK THE ORACLE")
                        .size(11.0)
                        .color(theme::MUTED)
                        .strong(),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new("Ctrl+Enter to ask")
                            .size(10.5)
                            .color(theme::DIM),
                    );
                });
            });
            ui.add_space(6.0);

            context_pack_picker(ui, &mut self.context_selection);
            ui.add_space(8.0);
            attached_context_strip(ui, snapshot, &self.context_selection);
            ui.add_space(10.0);

            ScrollArea::vertical()
                .auto_shrink([false; 2])
                .max_height(290.0)
                .show(ui, |ui| {
                    if self.transcript.is_empty() {
                        theme::empty_state(
                            ui,
                            "✴",
                            "No questions asked yet",
                            "Attach context packs, then ask about risk, state, journal, or warnings.",
                        );
                    }
                    for (role, body) in &self.transcript {
                        render_bubble(ui, *role, body);
                        ui.add_space(6.0);
                    }
                });

            ui.add_space(10.0);
            let disable_reason = send_disable_reason(cfg, self.awaiting);
            let response = ui.add_sized(
                egui::vec2(ui.available_width(), 78.0),
                egui::TextEdit::multiline(&mut self.input)
                    .hint_text("Ask for a risk review, state assessment, or journal debrief...")
                    .desired_rows(3),
            );
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let ask = ui.add_enabled(
                    disable_reason.is_none(),
                    egui::Button::new(RichText::new("Ask").color(Color32::WHITE).strong())
                        .fill(theme::FORGE_GOLD_DIM)
                        .min_size(egui::vec2(96.0, 34.0)),
                );
                if let Some(reason) = &disable_reason {
                    ui.label(RichText::new(*reason).size(11.0).color(theme::YELLOW));
                } else {
                    ui.label(
                        RichText::new("The prompt builder will attach the selected context only.")
                            .size(11.0)
                            .color(theme::DIM),
                    );
                }

                let submit_via_keyboard = response.has_focus()
                    && ui.input(|i| i.key_pressed(egui::Key::Enter) && i.modifiers.command);
                if (ask.clicked() || submit_via_keyboard) && disable_reason.is_none() {
                    self.send_question(snapshot, tx);
                }
            });

            if self.awaiting {
                ui.add_space(4.0);
                ui.label(
                    RichText::new("...the Oracle considers...")
                        .italics()
                        .color(theme::MUTED),
                );
            }
        });
    }

    fn send_question(
        &mut self,
        snapshot: &OracleContextSnapshot,
        tx: Option<&std::sync::mpsc::Sender<OracleRequest>>,
    ) {
        if self.input.trim().is_empty() || self.awaiting {
            return;
        }
        let Some(tx) = tx else {
            return;
        };

        let question = self.input.trim().to_string();
        self.transcript
            .push((TranscriptRole::User, question.clone()));
        self.awaiting = true;
        let _ = tx.send(OracleRequest::Analyze {
            question,
            context_snapshot: Box::new(snapshot.clone()),
            context_selection: self.context_selection,
            tag: "chat",
        });
        self.input.clear();
    }

    fn settings_column(
        &mut self,
        ui: &mut egui::Ui,
        cfg: &mut OracleConfig,
        tx: Option<&std::sync::mpsc::Sender<OracleRequest>>,
    ) {
        behavior_block(ui, cfg, tx);
        ui.add_space(10.0);
        model_runtime_block(ui, cfg, tx);
        ui.add_space(10.0);
        remote_endpoints_block(ui, cfg, &mut self.new_endpoint, tx);
    }
}

fn context_pack_picker(ui: &mut egui::Ui, selection: &mut OracleContextSelection) {
    ui.label(
        RichText::new("CONTEXT PACKS")
            .size(10.5)
            .color(theme::MUTED)
            .strong(),
    );
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        pack_toggle(ui, &mut selection.account_risk, "Account & Risk");
        pack_toggle(ui, &mut selection.market_session, "Market & Session");
        pack_toggle(ui, &mut selection.recent_warnings, "Recent Warnings");
        pack_toggle(ui, &mut selection.recent_journal, "Recent Journal");
        pack_toggle(ui, &mut selection.gate_rejections, "Gate Rejections");
    });
}

fn pack_toggle(ui: &mut egui::Ui, enabled: &mut bool, label: &str) {
    let (bg, fg) = if *enabled {
        (theme::FORGE_PARCHMENT, theme::FORGE_GOLD)
    } else {
        (theme::CARD2, theme::MUTED)
    };
    if ui
        .add(
            egui::Button::new(RichText::new(label).color(fg).size(11.0))
                .fill(bg)
                .stroke(egui::Stroke::new(
                    1.0,
                    if *enabled {
                        theme::FORGE_GOLD_DIM
                    } else {
                        theme::BORDER
                    },
                )),
        )
        .clicked()
    {
        *enabled = !*enabled;
    }
}

fn attached_context_strip(
    ui: &mut egui::Ui,
    snapshot: &OracleContextSnapshot,
    selection: &OracleContextSelection,
) {
    theme::card_sm().show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new("ATTACHED CONTEXT")
                    .size(10.5)
                    .color(theme::MUTED)
                    .strong(),
            );
            let items = snapshot.summary_items(selection);
            if items.is_empty() {
                ui.label(
                    RichText::new("No context packs enabled.")
                        .size(11.0)
                        .color(theme::DIM)
                        .italics(),
                );
            } else {
                for item in items {
                    theme::pill(ui, &item, Color32::from_rgb(24, 28, 36), theme::FORGE_GOLD);
                }
            }
        });
    });
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

fn behavior_block(
    ui: &mut egui::Ui,
    cfg: &mut OracleConfig,
    tx: Option<&std::sync::mpsc::Sender<OracleRequest>>,
) {
    theme::card().show(ui, |ui| {
        ui.label(
            RichText::new("BEHAVIOR")
                .size(11.0)
                .color(theme::MUTED)
                .strong(),
        );
        ui.add_space(6.0);
        ui.checkbox(&mut cfg.enabled, "Oracle enabled");
        ui.add_space(6.0);
        ui.label(
            RichText::new("System preprompt")
                .size(11.0)
                .color(theme::MUTED),
        );
        ui.add_sized(
            egui::vec2(ui.available_width(), 160.0),
            egui::TextEdit::multiline(&mut cfg.system_preprompt).desired_rows(8),
        );
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui.small_button("Reset to default").clicked() {
                cfg.system_preprompt = default_system_preprompt();
            }
            if ui.small_button("Save behavior").clicked() {
                save_config(cfg, tx);
            }
        });
    });
}

fn model_runtime_block(
    ui: &mut egui::Ui,
    cfg: &mut OracleConfig,
    tx: Option<&std::sync::mpsc::Sender<OracleRequest>>,
) {
    theme::card().show(ui, |ui| {
        ui.label(
            RichText::new("MODEL & RUNTIME")
                .size(11.0)
                .color(theme::MUTED)
                .strong(),
        );
        ui.add_space(6.0);

        ui.label(RichText::new("Ollama URL").size(11.0).color(theme::MUTED));
        ui.add(egui::TextEdit::singleline(&mut cfg.ollama_url).desired_width(f32::INFINITY));
        ui.add_space(8.0);

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
            let locality = if spec.kind.is_local() {
                "Local"
            } else {
                "Remote"
            };
            ui.label(
                RichText::new(format!("{locality} model"))
                    .size(10.5)
                    .color(theme::DIM),
            );
            if let Some(gb) = spec.kind.approx_disk_gb() {
                if gb > 0.0 {
                    ui.label(
                        RichText::new(format!("~{gb:.1} GB disk"))
                            .size(10.5)
                            .color(theme::MUTED)
                            .italics(),
                    );
                }
            }
        }

        ui.add_space(8.0);
        ui.label(RichText::new("Temperature").size(11.0).color(theme::MUTED));
        ui.add(egui::Slider::new(&mut cfg.temperature, 0.0..=1.0).show_value(true));
        ui.label(RichText::new("Max tokens").size(11.0).color(theme::MUTED));
        ui.add(egui::Slider::new(&mut cfg.max_tokens, 128..=2048).logarithmic(true));
        ui.add_space(6.0);
        if ui.small_button("Save runtime").clicked() {
            save_config(cfg, tx);
        }
    });
}

fn remote_endpoints_block(
    ui: &mut egui::Ui,
    cfg: &mut OracleConfig,
    new_endpoint: &mut RemoteEndpoint,
    tx: Option<&std::sync::mpsc::Sender<OracleRequest>>,
) {
    theme::card().show(ui, |ui| {
        ui.label(
            RichText::new("REMOTE ENDPOINTS")
                .size(11.0)
                .color(theme::MUTED)
                .strong(),
        );
        ui.add_space(6.0);

        if cfg.remotes.is_empty() {
            ui.label(
                RichText::new("None configured. Add one below to use Kimi K2, OpenAI, or another OpenAI-compatible endpoint.")
                    .italics()
                    .color(theme::DIM),
            );
        }

        let mut remove_idx = None;
        for (i, endpoint) in cfg.remotes.iter().enumerate() {
            theme::card_sm().show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new(&endpoint.label).strong());
                        ui.label(
                            RichText::new(format!(
                                "{}  |  {}  |  env:{}",
                                endpoint.base_url, endpoint.model_id, endpoint.api_key_env
                            ))
                            .size(10.8)
                            .color(theme::MUTED),
                        );
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("Remove").clicked() {
                            remove_idx = Some(i);
                        }
                    });
                });
            });
            ui.add_space(6.0);
        }

        if let Some(i) = remove_idx {
            let label = cfg.remotes[i].label.clone();
            cfg.remotes.remove(i);
            cfg.registry
                .entries
                .retain(|entry| !(entry.kind == ModelKind::RemoteOpenAI && entry.label == label));
            cfg.selected = cfg
                .selected
                .min(cfg.registry.entries.len().saturating_sub(1));
            save_config(cfg, tx);
        }

        ui.separator();
        ui.add_space(6.0);
        ui.label(RichText::new("Add endpoint").size(11.0).color(theme::FORGE_GOLD));
        ui.label(RichText::new("Label").size(10.5).color(theme::MUTED));
        ui.text_edit_singleline(&mut new_endpoint.label);
        ui.label(RichText::new("Base URL").size(10.5).color(theme::MUTED));
        ui.text_edit_singleline(&mut new_endpoint.base_url);
        ui.label(RichText::new("Model ID").size(10.5).color(theme::MUTED));
        ui.text_edit_singleline(&mut new_endpoint.model_id);
        ui.label(RichText::new("API key env var").size(10.5).color(theme::MUTED));
        ui.text_edit_singleline(&mut new_endpoint.api_key_env);
        ui.add_space(4.0);
        if ui.button("Register endpoint").clicked() && !new_endpoint.label.trim().is_empty() {
            cfg.remotes.push(new_endpoint.clone());
            cfg.registry.entries.push(ModelSpec {
                kind: ModelKind::RemoteOpenAI,
                ollama_tag: None,
                label: new_endpoint.label.clone(),
            });
            cfg.selected = cfg.registry.entries.len().saturating_sub(1);
            *new_endpoint = RemoteEndpoint {
                label: String::new(),
                base_url: "https://".to_string(),
                model_id: String::new(),
                api_key_env: String::new(),
            };
            save_config(cfg, tx);
        }
    });
}

fn save_config(cfg: &OracleConfig, tx: Option<&std::sync::mpsc::Sender<OracleRequest>>) {
    let _ = cfg.save();
    if let Some(tx) = tx {
        let _ = tx.send(OracleRequest::UpdateConfig(Box::new(cfg.clone())));
    }
}

fn send_disable_reason(cfg: &OracleConfig, awaiting: bool) -> Option<&'static str> {
    if awaiting {
        return Some("The current request is still running.");
    }
    if !cfg.enabled {
        return Some("Enable the Oracle in Behavior before sending.");
    }
    if cfg.selected_spec().is_none() {
        return Some("Select a model before sending.");
    }
    None
}
