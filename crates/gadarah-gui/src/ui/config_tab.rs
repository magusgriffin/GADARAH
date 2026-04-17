//! Configuration panel — Connect your broker, set risk limits, choose your firm

use std::path::PathBuf;

use eframe::egui;
use egui::RichText;

use crate::config::{FirmConfig, GadarahConfig};
use crate::state::{AppState, ConnectionStatus, LogLevel, SharedState};
use crate::theme;

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
                    if ui
                        .add_sized(
                            [140.0, 36.0],
                            egui::Button::new(
                                RichText::new("Connect")
                                    .color(egui::Color32::WHITE)
                                    .strong(),
                            )
                            .fill(egui::Color32::from_rgb(0, 130, 95)),
                        )
                        .clicked()
                    {
                        let mut g = app_state.lock().unwrap();
                        g.connection_status = ConnectionStatus::ConnectedDemo;
                        g.add_log(LogLevel::Info, "Connected to demo account 5772124");
                    }
                    ui.label(
                        RichText::new("Connect to start receiving live market data.")
                            .color(theme::MUTED)
                            .size(12.0),
                    );
                } else {
                    if ui
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
                }
            });
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
