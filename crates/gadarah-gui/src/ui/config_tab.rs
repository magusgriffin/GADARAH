//! Configuration panel — Connect your broker, set risk limits, choose your firm

use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::thread;

use eframe::egui;
use egui::RichText;

use gadarah_broker::auth::{run_oauth_flow, save_credentials, AuthResult, SavedCredentials, TradingAccount};

use crate::config::{FirmConfig, GadarahConfig};
use crate::notifications::{self, NotificationSettings, WebhookKind};
use crate::state::{AlertSeverity, AppState, ConnectionStatus, LogLevel, SharedState};
use crate::theme;

/// Events emitted by the OAuth worker thread.
enum AuthEvent {
    Log(String),
    /// Completed with an AuthResult — user must now pick an account.
    NeedAccountPick(AuthResult, String, String),
    Failed(String),
}

pub struct ConfigPanel {
    base_risk_pct: String,
    daily_stop_pct: String,
    max_portfolio_heat: String,
    daily_dd_trigger: String,
    total_dd_trigger: String,
    consecutive_loss_limit: String,
    cooldown_minutes: String,
    pending_save: bool,
    synced: bool,
    // Broker OAuth flow state
    setup_open: bool,
    broker_client_id: String,
    broker_client_secret: String,
    auth_rx: Option<Receiver<AuthEvent>>,
    auth_busy: bool,
    /// After OAuth returns, the user picks which ctid account to bind.
    pending_accounts: Option<(AuthResult, String, String)>,
    pending_account_idx: usize,
}

impl ConfigPanel {
    pub fn new() -> Self {
        Self {
            base_risk_pct: "0.74".to_string(),
            daily_stop_pct: "1.5".to_string(),
            max_portfolio_heat: "2.0".to_string(),
            daily_dd_trigger: "95.0".to_string(),
            total_dd_trigger: "95.0".to_string(),
            consecutive_loss_limit: "3".to_string(),
            cooldown_minutes: "30".to_string(),
            pending_save: false,
            synced: false,
            setup_open: false,
            broker_client_id: String::new(),
            broker_client_secret: String::new(),
            auth_rx: None,
            auth_busy: false,
            pending_accounts: None,
            pending_account_idx: 0,
        }
    }

    /// Drain any pending OAuth worker events. Call from `show()` every frame.
    fn pump_auth(&mut self, app_state: &AppState) {
        let Some(rx) = self.auth_rx.as_ref() else {
            return;
        };
        loop {
            match rx.try_recv() {
                Ok(AuthEvent::Log(msg)) => {
                    app_state
                        .lock()
                        .unwrap()
                        .add_log(LogLevel::Info, msg);
                }
                Ok(AuthEvent::NeedAccountPick(result, cid, csec)) => {
                    self.auth_busy = false;
                    self.pending_account_idx = 0;
                    self.pending_accounts = Some((result, cid, csec));
                    app_state
                        .lock()
                        .unwrap()
                        .add_log(LogLevel::Info, "OAuth complete — pick an account below.");
                }
                Ok(AuthEvent::Failed(err)) => {
                    self.auth_busy = false;
                    app_state
                        .lock()
                        .unwrap()
                        .add_log(LogLevel::Error, format!("OAuth failed: {err}"));
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.auth_rx = None;
                    break;
                }
            }
        }
    }

    fn sync_from(&mut self, s: &SharedState) {
        self.base_risk_pct = s.config.risk.base_risk_pct.to_string();
        self.daily_stop_pct = s.config.risk.daily_stop_pct.to_string();
        self.max_portfolio_heat = s.config.risk.max_portfolio_heat.to_string();
        self.daily_dd_trigger = s.config.kill_switch.daily_dd_trigger_pct.to_string();
        self.total_dd_trigger = s.config.kill_switch.total_dd_trigger_pct.to_string();
        self.consecutive_loss_limit = s.config.kill_switch.consecutive_loss_limit.to_string();
        self.cooldown_minutes = s.config.kill_switch.cooldown_minutes.to_string();
        self.synced = true;
    }

    pub fn show(&mut self, ui: &mut egui::Ui, app_state: &AppState) {
        // Drain any pending events from the OAuth worker thread before rendering.
        self.pump_auth(app_state);

        let (conn_status, daily_target, selected_firm, available_firms, firm_config) = {
            let g = app_state.lock().unwrap();
            if !self.synced {
                self.sync_from(&g);
            }
            (
                g.connection_status,
                g.config.risk.daily_target_pct,
                g.selected_firm.clone(),
                g.available_firms.clone(),
                g.firm_config.clone(),
            )
        };

        theme::heading(ui, "Settings");
        ui.label(
            RichText::new("Configure your broker connection, risk limits, and prop firm profile.")
                .color(theme::MUTED)
                .size(12.5),
        );
        ui.add_space(14.0);

        // ── Connection ────────────────────────────────────────────────────────
        theme::card().show(ui, |ui| {
            theme::section_label(ui, "BROKER CONNECTION");
            ui.add_space(8.0);

            let connected = matches!(
                conn_status,
                ConnectionStatus::ConnectedLive | ConnectionStatus::ConnectedDemo
            );

            ui.horizontal(|ui| {
                let (dot, status_text, status_color) = match conn_status {
                    ConnectionStatus::ConnectedLive => {
                        ("●", "Connected (Live Account)", theme::GREEN)
                    }
                    ConnectionStatus::ConnectedDemo => {
                        ("●", "Connected (Demo Account)", theme::YELLOW)
                    }
                    ConnectionStatus::Connecting => ("◌", "Connecting…", theme::BLUE),
                    ConnectionStatus::Disconnected => ("○", "Not Connected", theme::RED),
                };
                ui.label(RichText::new(dot).color(status_color).size(14.0));
                ui.label(
                    RichText::new(status_text)
                        .color(status_color)
                        .size(13.5)
                        .strong(),
                );
            });

            ui.add_space(4.0);
            ui.label(
                RichText::new("Broker: cTrader Open API  —  demo.ctraderapi.com:5035")
                    .color(theme::MUTED)
                    .size(12.0),
            );
            ui.add_space(8.0);

            ui.horizontal(|ui| {
                if !connected {
                    let label = if self.setup_open { "Hide Setup" } else { "Broker Setup" };
                    if ui
                        .add_sized(
                            [140.0, 36.0],
                            egui::Button::new(
                                RichText::new(label).color(egui::Color32::WHITE).strong(),
                            )
                            .fill(egui::Color32::from_rgb(0, 130, 95)),
                        )
                        .clicked()
                    {
                        self.setup_open = !self.setup_open;
                    }
                    ui.label(
                        RichText::new(
                            "Open the setup card to enter your cTrader client ID / secret.",
                        )
                        .color(theme::MUTED)
                        .size(12.0),
                    );
                } else if ui
                    .add_sized(
                        [140.0, 36.0],
                        egui::Button::new(
                            RichText::new("Disconnect").color(egui::Color32::WHITE),
                        )
                        .fill(egui::Color32::from_rgb(120, 40, 40)),
                    )
                    .clicked()
                {
                    let mut g = app_state.lock().unwrap();
                    g.connection_status = ConnectionStatus::Disconnected;
                    g.add_log(LogLevel::Info, "Disconnected from broker");
                }
            });

            if self.setup_open && !connected {
                ui.add_space(10.0);
                ui.separator();
                ui.add_space(8.0);
                self.show_broker_setup(ui, app_state);
            }
        });

        ui.add_space(12.0);

        // ── Risk settings ─────────────────────────────────────────────────────
        theme::card().show(ui, |ui| {
            theme::section_label(ui, "RISK SETTINGS — How much the bot risks per trade");
            ui.add_space(8.0);

            egui::Grid::new("risk_grid")
                .num_columns(4)
                .spacing([12.0, 10.0])
                .show(ui, |ui| {
                    ui.label(
                        RichText::new("Risk per trade (% of balance):")
                            .color(theme::TEXT)
                            .size(13.0),
                    );
                    if ui
                        .add_sized(
                            [110.0, 28.0],
                            egui::TextEdit::singleline(&mut self.base_risk_pct)
                                .hint_text("e.g. 0.74"),
                        )
                        .changed()
                    {
                        self.pending_save = true;
                    }
                    ui.label(
                        RichText::new("Max daily loss (%):")
                            .color(theme::TEXT)
                            .size(13.0),
                    );
                    if ui
                        .add_sized(
                            [110.0, 28.0],
                            egui::TextEdit::singleline(&mut self.daily_stop_pct)
                                .hint_text("e.g. 1.5"),
                        )
                        .changed()
                    {
                        self.pending_save = true;
                    }
                    ui.end_row();

                    ui.label(
                        RichText::new("Max simultaneous risk (%):")
                            .color(theme::TEXT)
                            .size(13.0),
                    );
                    if ui
                        .add_sized(
                            [110.0, 28.0],
                            egui::TextEdit::singleline(&mut self.max_portfolio_heat)
                                .hint_text("e.g. 2.0"),
                        )
                        .changed()
                    {
                        self.pending_save = true;
                    }
                    ui.label(
                        RichText::new("Daily profit target (%):")
                            .color(theme::TEXT)
                            .size(13.0),
                    );
                    ui.label(
                        RichText::new(format!("{}", daily_target))
                            .monospace()
                            .color(theme::ACCENT)
                            .size(13.0),
                    );
                    ui.end_row();
                });
        });

        ui.add_space(12.0);

        // ── Auto-stop settings ────────────────────────────────────────────────
        theme::card().show(ui, |ui| {
            theme::section_label(ui, "AUTO-STOP RULES — When the bot stops automatically");
            ui.add_space(4.0);
            ui.label(
                RichText::new("The bot will pause trading if these thresholds are crossed.")
                    .color(theme::MUTED)
                    .size(12.0),
            );
            ui.add_space(8.0);

            egui::Grid::new("ks_grid")
                .num_columns(4)
                .spacing([12.0, 10.0])
                .show(ui, |ui| {
                    ui.label(
                        RichText::new("Stop after daily loss reaches (%):")
                            .color(theme::TEXT)
                            .size(13.0),
                    );
                    if ui
                        .add_sized(
                            [110.0, 28.0],
                            egui::TextEdit::singleline(&mut self.daily_dd_trigger)
                                .hint_text("e.g. 4.0"),
                        )
                        .changed()
                    {
                        self.pending_save = true;
                    }
                    ui.label(
                        RichText::new("Stop after total loss reaches (%):")
                            .color(theme::TEXT)
                            .size(13.0),
                    );
                    if ui
                        .add_sized(
                            [110.0, 28.0],
                            egui::TextEdit::singleline(&mut self.total_dd_trigger)
                                .hint_text("e.g. 8.0"),
                        )
                        .changed()
                    {
                        self.pending_save = true;
                    }
                    ui.end_row();

                    ui.label(
                        RichText::new("Stop after consecutive losses:")
                            .color(theme::TEXT)
                            .size(13.0),
                    );
                    if ui
                        .add_sized(
                            [110.0, 28.0],
                            egui::TextEdit::singleline(&mut self.consecutive_loss_limit)
                                .hint_text("e.g. 3"),
                        )
                        .changed()
                    {
                        self.pending_save = true;
                    }
                    ui.label(
                        RichText::new("Pause duration after stop (minutes):")
                            .color(theme::TEXT)
                            .size(13.0),
                    );
                    if ui
                        .add_sized(
                            [110.0, 28.0],
                            egui::TextEdit::singleline(&mut self.cooldown_minutes)
                                .hint_text("e.g. 30"),
                        )
                        .changed()
                    {
                        self.pending_save = true;
                    }
                    ui.end_row();
                });

            ui.add_space(10.0);
            let risk_ack = app_state.lock().unwrap().risk_acknowledged;
            ui.horizontal(|ui| {
                if risk_ack {
                    ui.label(
                        RichText::new("Reviewed ✓")
                            .color(theme::GREEN)
                            .strong()
                            .size(13.0),
                    );
                    ui.label(
                        RichText::new("Required for Demo and LIVE modes. Clears on save.")
                            .color(theme::MUTED)
                            .size(11.5)
                            .italics(),
                    );
                } else if ui
                    .add_sized(
                        [220.0, 30.0],
                        egui::Button::new(
                            RichText::new("I've reviewed these limits")
                                .color(egui::Color32::WHITE),
                        )
                        .fill(theme::FORGE_GOLD_DIM),
                    )
                    .clicked()
                {
                    let mut g = app_state.lock().unwrap();
                    g.risk_acknowledged = true;
                    g.add_log(
                        LogLevel::Info,
                        "Risk limits acknowledged — Demo/LIVE preflight cleared.",
                    );
                }
            });
        });

        ui.add_space(12.0);

        // ── Prop firm profile ─────────────────────────────────────────────────
        theme::card().show(ui, |ui| {
            theme::section_label(ui, "PROP FIRM CHALLENGE");
            ui.add_space(4.0);
            ui.label(
                RichText::new(
                    "Select your challenge to load its rules and automatically adapt risk limits.",
                )
                .color(theme::MUTED)
                .size(12.0),
            );
            ui.add_space(8.0);

            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Active challenge:")
                        .color(theme::TEXT)
                        .size(13.0),
                );
                let mut sel = selected_firm.clone().unwrap_or_default();
                let prev = sel.clone();
                egui::ComboBox::from_id_salt("cfg_firm")
                    .width(220.0)
                    .selected_text(if sel.is_empty() {
                        "— Select challenge —"
                    } else {
                        sel.as_str()
                    })
                    .show_ui(ui, |ui| {
                        for firm in &available_firms {
                            ui.selectable_value(&mut sel, firm.clone(), firm);
                        }
                    });
                if sel != prev && !sel.is_empty() {
                    self.select_firm(app_state, &sel);
                }
            });

            if let Some(firm) = &firm_config {
                ui.add_space(10.0);
                egui::Grid::new("firm_details")
                    .num_columns(4)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        ui.label(RichText::new("Firm:").color(theme::MUTED).size(12.0));
                        ui.label(RichText::new(&firm.firm.name).strong().color(theme::TEXT));
                        ui.label(RichText::new("Type:").color(theme::MUTED).size(12.0));
                        ui.label(RichText::new(&firm.firm.challenge_type).color(theme::TEXT));
                        ui.end_row();

                        ui.label(
                            RichText::new("Profit target:")
                                .color(theme::MUTED)
                                .size(12.0),
                        );
                        ui.label(
                            RichText::new(format!("{}%", firm.firm.profit_target_pct))
                                .color(theme::GREEN)
                                .strong(),
                        );
                        ui.label(
                            RichText::new("Daily loss limit:")
                                .color(theme::MUTED)
                                .size(12.0),
                        );
                        ui.label(
                            RichText::new(format!("{}%", firm.firm.daily_dd_limit_pct))
                                .color(theme::YELLOW)
                                .strong(),
                        );
                        ui.end_row();

                        ui.label(
                            RichText::new("Max drawdown:")
                                .color(theme::MUTED)
                                .size(12.0),
                        );
                        ui.label(
                            RichText::new(format!("{}%", firm.firm.max_dd_limit_pct))
                                .color(theme::RED)
                                .strong(),
                        );
                        ui.label(
                            RichText::new("Profit split:")
                                .color(theme::MUTED)
                                .size(12.0),
                        );
                        ui.label(
                            RichText::new(format!("{}%", firm.firm.profit_split_pct))
                                .color(theme::ACCENT)
                                .strong(),
                        );
                        ui.end_row();
                    });
            }
        });

        ui.add_space(14.0);

        // ── Notifications ─────────────────────────────────────────────────────
        self.show_notifications_card(ui, app_state);

        ui.add_space(14.0);

        // ── Save / Reload buttons ─────────────────────────────────────────────
        ui.horizontal(|ui| {
            let save_fill = if self.pending_save {
                egui::Color32::from_rgb(0, 130, 95)
            } else {
                egui::Color32::from_rgb(35, 43, 55)
            };
            if ui
                .add_sized(
                    [160.0, 38.0],
                    egui::Button::new(
                        RichText::new(if self.pending_save {
                            "Save Changes"
                        } else {
                            "Saved"
                        })
                        .color(egui::Color32::WHITE)
                        .strong(),
                    )
                    .fill(save_fill),
                )
                .clicked()
            {
                self.save_config(app_state);
            }

            ui.add_space(8.0);

            if ui
                .add_sized(
                    [130.0, 38.0],
                    egui::Button::new(RichText::new("Reload from File").color(theme::TEXT))
                        .fill(egui::Color32::from_rgb(28, 35, 45)),
                )
                .clicked()
            {
                self.reload_config(app_state);
            }

            if self.pending_save {
                ui.add_space(12.0);
                ui.label(
                    RichText::new("You have unsaved changes.")
                        .color(theme::YELLOW)
                        .size(12.5),
                );
            }
        });
    }

    fn show_notifications_card(&mut self, ui: &mut egui::Ui, app_state: &AppState) {
        // Pull a working copy out of state so the user can edit without
        // holding the mutex across the whole UI block.
        let mut settings = app_state.lock().unwrap().notification_settings.clone();
        let original = settings.clone();

        theme::card().show(ui, |ui| {
            theme::section_label(
                ui,
                "NOTIFICATIONS — OS toasts and outbound webhooks",
            );
            ui.add_space(6.0);
            ui.label(
                RichText::new(
                    "Surface alerts beyond the in-app banner. Critical events (kill-switch \
                     trips, vol halts, daily-stop) ping you even when GADARAH is minimised.",
                )
                .color(theme::MUTED)
                .size(11.5),
            );
            ui.add_space(10.0);

            // Master OS toggle.
            ui.horizontal(|ui| {
                ui.checkbox(&mut settings.os_enabled, "Send OS notifications");
                ui.add_space(12.0);
                ui.label(
                    RichText::new("Threshold:")
                        .color(theme::MUTED)
                        .size(11.5),
                );
                egui::ComboBox::from_id_salt("notify-min-severity")
                    .selected_text(severity_label(settings.min_severity))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut settings.min_severity,
                            AlertSeverity::Info,
                            "Info and above",
                        );
                        ui.selectable_value(
                            &mut settings.min_severity,
                            AlertSeverity::Warning,
                            "Warning and above",
                        );
                        ui.selectable_value(
                            &mut settings.min_severity,
                            AlertSeverity::Danger,
                            "Danger only",
                        );
                    });
            });

            ui.add_space(10.0);
            theme::section_label(ui, "WEBHOOK (Discord / Slack / Generic)");
            ui.add_space(4.0);
            ui.label(
                RichText::new(
                    "Paste an incoming-webhook URL. Leave blank to disable. Discord and \
                     Slack URLs are auto-detected by format; pick \"Generic\" to POST a \
                     plain JSON body to a self-hosted receiver.",
                )
                .color(theme::MUTED)
                .size(11.0),
            );
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Kind:").color(theme::MUTED).size(11.5));
                egui::ComboBox::from_id_salt("notify-webhook-kind")
                    .selected_text(webhook_kind_label(settings.webhook_kind))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut settings.webhook_kind,
                            WebhookKind::Discord,
                            "Discord",
                        );
                        ui.selectable_value(
                            &mut settings.webhook_kind,
                            WebhookKind::Slack,
                            "Slack",
                        );
                        ui.selectable_value(
                            &mut settings.webhook_kind,
                            WebhookKind::Generic,
                            "Generic JSON",
                        );
                    });
            });
            ui.add(
                egui::TextEdit::singleline(&mut settings.webhook_url)
                    .desired_width(f32::INFINITY)
                    .hint_text("https://discord.com/api/webhooks/... or https://hooks.slack.com/... "),
            );

            ui.add_space(10.0);
            ui.horizontal(|ui| {
                let send_test_enabled =
                    settings.os_enabled || !settings.webhook_url.trim().is_empty();
                if ui
                    .add_enabled(
                        send_test_enabled,
                        egui::Button::new(
                            RichText::new("Send test notification").color(theme::TEXT),
                        )
                        .fill(egui::Color32::from_rgb(28, 35, 45)),
                    )
                    .clicked()
                {
                    notifications::send_test(&settings);
                    app_state.lock().unwrap().add_log(
                        LogLevel::Info,
                        "Test notification dispatched.",
                    );
                }
                if !send_test_enabled {
                    ui.label(
                        RichText::new("Enable OS toasts or paste a webhook URL first.")
                            .italics()
                            .color(theme::DIM)
                            .size(11.0),
                    );
                }
            });
        });

        // Persist + push back into state on any change. Save is cheap (small
        // JSON file, off-render-thread risk is acceptable here for now).
        if settings_changed(&original, &settings) {
            settings.save();
            app_state.lock().unwrap().notification_settings = settings;
        }
    }

    fn show_broker_setup(&mut self, ui: &mut egui::Ui, app_state: &AppState) {
        theme::section_label(ui, "BROKER SETUP (cTrader OAuth)");
        ui.add_space(6.0);

        if let Some((result, _cid, _csec)) = &self.pending_accounts {
            // Account picker — shown after OAuth exchange returns the account list.
            ui.label(
                RichText::new("Pick the trading account to bind to GADARAH:")
                    .color(theme::TEXT)
                    .size(12.5),
            );
            ui.add_space(4.0);
            let accounts = result.accounts.clone();
            let summary = |a: &TradingAccount| -> String {
                format!(
                    "#{}  {}  {}",
                    a.ctid_trader_account_id,
                    if a.is_live { "LIVE" } else { "DEMO" },
                    a.broker_name.clone().unwrap_or_else(|| "—".into()),
                )
            };
            egui::ComboBox::from_id_salt("broker_acct_pick")
                .width(420.0)
                .selected_text(
                    accounts
                        .get(self.pending_account_idx)
                        .map(summary)
                        .unwrap_or_else(|| "— select —".into()),
                )
                .show_ui(ui, |ui| {
                    for (i, acct) in accounts.iter().enumerate() {
                        ui.selectable_value(&mut self.pending_account_idx, i, summary(acct));
                    }
                });
            ui.add_space(8.0);
            if ui
                .add_sized(
                    [150.0, 32.0],
                    egui::Button::new(
                        RichText::new("Save + Connect")
                            .color(egui::Color32::WHITE)
                            .strong(),
                    )
                    .fill(egui::Color32::from_rgb(0, 130, 95)),
                )
                .clicked()
            {
                self.finalize_auth(app_state);
            }
            return;
        }

        ui.label(
            RichText::new(
                "Paste the client ID and secret for your Spotware OAuth app. \
                 The button below opens your browser to authorize GADARAH; the \
                 callback is caught on http://localhost:5555.",
            )
            .color(theme::MUTED)
            .size(12.0),
        );
        ui.add_space(8.0);

        egui::Grid::new("broker_creds_grid")
            .num_columns(2)
            .spacing([10.0, 8.0])
            .show(ui, |ui| {
                ui.label(RichText::new("Client ID:").color(theme::TEXT));
                ui.add_sized(
                    [340.0, 26.0],
                    egui::TextEdit::singleline(&mut self.broker_client_id),
                );
                ui.end_row();
                ui.label(RichText::new("Client Secret:").color(theme::TEXT));
                ui.add_sized(
                    [340.0, 26.0],
                    egui::TextEdit::singleline(&mut self.broker_client_secret).password(true),
                );
                ui.end_row();
            });

        ui.add_space(8.0);
        let can_start = !self.broker_client_id.trim().is_empty()
            && !self.broker_client_secret.trim().is_empty()
            && !self.auth_busy;
        ui.horizontal(|ui| {
            let label = if self.auth_busy {
                "Waiting for browser…"
            } else {
                "Start OAuth"
            };
            if ui
                .add_enabled(
                    can_start,
                    egui::Button::new(
                        RichText::new(label).color(egui::Color32::WHITE).strong(),
                    )
                    .fill(egui::Color32::from_rgb(0, 130, 95))
                    .min_size(egui::vec2(150.0, 32.0)),
                )
                .clicked()
            {
                self.start_auth(app_state);
            }
            if self.auth_busy {
                ui.label(
                    RichText::new("Check your browser and approve the request.")
                        .color(theme::YELLOW)
                        .size(12.0),
                );
            }
        });
    }

    fn start_auth(&mut self, app_state: &AppState) {
        self.auth_busy = true;
        let (tx, rx) = channel();
        self.auth_rx = Some(rx);
        let cid = self.broker_client_id.trim().to_string();
        let csec = self.broker_client_secret.trim().to_string();
        // Clear the secret field immediately — the worker thread has its own copy.
        self.broker_client_secret.clear();
        app_state
            .lock()
            .unwrap()
            .add_log(LogLevel::Info, "Starting OAuth flow; browser will open.");
        let _ = thread::Builder::new()
            .name("gadarah-oauth".into())
            .spawn(move || {
                let _ = tx.send(AuthEvent::Log(
                    "Opening browser for Spotware authorization…".into(),
                ));
                match run_oauth_flow(&cid, &csec) {
                    Ok(result) => {
                        let _ = tx.send(AuthEvent::Log(format!(
                            "Authorization complete — {} account(s) available",
                            result.accounts.len()
                        )));
                        let _ = tx.send(AuthEvent::NeedAccountPick(result, cid, csec));
                    }
                    Err(e) => {
                        let _ = tx.send(AuthEvent::Failed(e.to_string()));
                    }
                }
            });
    }

    fn finalize_auth(&mut self, app_state: &AppState) {
        let Some((result, cid, csec)) = self.pending_accounts.take() else {
            return;
        };
        let idx = self.pending_account_idx.min(result.accounts.len().saturating_sub(1));
        let Some(selected) = result.accounts.get(idx).cloned() else {
            app_state
                .lock()
                .unwrap()
                .add_log(LogLevel::Error, "No accounts available to save.");
            return;
        };
        let env_file = if selected.is_live {
            ".env.live"
        } else {
            ".env.demo"
        };
        let creds = SavedCredentials {
            client_id: cid,
            client_secret: csec,
            access_token: result.access_token.clone(),
            refresh_token: result.refresh_token.clone(),
            ctid_account_id: selected.ctid_trader_account_id,
            is_live: selected.is_live,
        };
        match save_credentials(env_file, &creds) {
            Ok(()) => {
                // Hydrate this process's env so the live loop sees the new creds
                // without a restart.
                let _ = dotenvy::from_filename_override(env_file);
                let status = if selected.is_live {
                    ConnectionStatus::ConnectedLive
                } else {
                    ConnectionStatus::ConnectedDemo
                };
                let mut g = app_state.lock().unwrap();
                g.connection_status = status;
                g.add_log(
                    LogLevel::Info,
                    format!(
                        "Credentials saved to {env_file}; bound ctid {}",
                        selected.ctid_trader_account_id
                    ),
                );
            }
            Err(e) => {
                app_state
                    .lock()
                    .unwrap()
                    .add_log(LogLevel::Error, format!("Failed to save credentials: {e}"));
                // Put the accounts back so the user can retry.
                self.pending_accounts = Some((result, creds.client_id, creds.client_secret));
            }
        }
    }

    fn select_firm(&self, app_state: &AppState, firm_name: &str) {
        let mut g = app_state.lock().unwrap();
        let path = PathBuf::from(format!("config/firms/{}.toml", firm_name));
        match FirmConfig::load(&path) {
            Ok(firm) => {
                let name = firm.firm.name.clone();
                g.firm_config = Some(firm);
                g.selected_firm = Some(firm_name.to_string());
                g.add_log(
                    LogLevel::Info,
                    format!("Challenge profile loaded: {}", name),
                );
            }
            Err(e) => g.add_log(
                LogLevel::Error,
                format!("Failed to load {}: {}", firm_name, e),
            ),
        }
    }

    fn save_config(&mut self, app_state: &AppState) {
        let mut g = app_state.lock().unwrap();
        if let Ok(v) = self.base_risk_pct.parse() {
            g.config.risk.base_risk_pct = v;
        }
        if let Ok(v) = self.daily_stop_pct.parse() {
            g.config.risk.daily_stop_pct = v;
        }
        if let Ok(v) = self.max_portfolio_heat.parse() {
            g.config.risk.max_portfolio_heat = v;
        }
        if let Ok(v) = self.daily_dd_trigger.parse() {
            g.config.kill_switch.daily_dd_trigger_pct = v;
        }
        if let Ok(v) = self.total_dd_trigger.parse() {
            g.config.kill_switch.total_dd_trigger_pct = v;
        }
        if let Ok(v) = self.consecutive_loss_limit.parse::<u8>() {
            g.config.kill_switch.consecutive_loss_limit = v;
        }
        if let Ok(v) = self.cooldown_minutes.parse::<u32>() {
            g.config.kill_switch.cooldown_minutes = v;
        }

        match g.config.save(&PathBuf::from("config/gadarah.toml")) {
            Ok(_) => {
                self.pending_save = false;
                // A fresh save changes the limits under the user's feet — force
                // a re-acknowledgement before Demo/LIVE can run again.
                g.risk_acknowledged = false;
                g.add_log(LogLevel::Info, "Settings saved to config/gadarah.toml");
            }
            Err(e) => g.add_log(LogLevel::Error, format!("Save failed: {}", e)),
        }
    }

    fn reload_config(&mut self, app_state: &AppState) {
        let mut g = app_state.lock().unwrap();
        match GadarahConfig::load(&PathBuf::from("config/gadarah.toml")) {
            Ok(cfg) => {
                g.config = cfg;
                self.sync_from(&g);
                self.pending_save = false;
                g.add_log(LogLevel::Info, "Settings reloaded from file");
            }
            Err(e) => g.add_log(LogLevel::Error, format!("Reload failed: {}", e)),
        }
    }
}

fn severity_label(s: AlertSeverity) -> &'static str {
    match s {
        AlertSeverity::Info => "Info and above",
        AlertSeverity::Warning => "Warning and above",
        AlertSeverity::Danger => "Danger only",
    }
}

fn webhook_kind_label(k: WebhookKind) -> &'static str {
    match k {
        WebhookKind::Discord => "Discord",
        WebhookKind::Slack => "Slack",
        WebhookKind::Generic => "Generic JSON",
    }
}

fn settings_changed(a: &NotificationSettings, b: &NotificationSettings) -> bool {
    a.os_enabled != b.os_enabled
        || a.min_severity != b.min_severity
        || a.webhook_kind != b.webhook_kind
        || a.webhook_url != b.webhook_url
}
