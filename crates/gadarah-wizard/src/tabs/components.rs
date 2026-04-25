use std::path::Path;

use eframe::egui::{self, RichText};

use crate::theme;

#[derive(Debug, Clone)]
pub struct ComponentSelection {
    pub gui: bool,
    pub cli_daemon: bool,
    pub install_ollama: bool,
    pub create_desktop_shortcut: bool,
    pub install_path: String,
}

impl Default for ComponentSelection {
    fn default() -> Self {
        Self {
            gui: true,
            cli_daemon: true,
            install_ollama: false,
            create_desktop_shortcut: true,
            install_path: "%LOCALAPPDATA%\\GADARAH".to_string(),
        }
    }
}

pub fn show(ui: &mut egui::Ui, sel: &mut ComponentSelection) {
    theme::card().show(ui, |ui| {
        ui.label(
            RichText::new("Choose Components")
                .heading()
                .color(theme::FORGE_GOLD),
        );
        ui.add_space(4.0);
        ui.label(
            RichText::new(
                "Required components cannot be unchecked. Optional components can be added later \
                 through Windows Programs & Features.",
            )
            .color(theme::MUTED)
            .size(12.0),
        );
        ui.add_space(12.0);

        component_row(
            ui,
            "GADARAH GUI",
            "Desktop application with dashboard, charts, and Oracle.",
            "~45 MB",
            &mut sel.gui,
            true,
        );
        component_row(
            ui,
            "CLI Daemon",
            "Headless runner that feeds the GUI via JSON snapshots.",
            "~25 MB",
            &mut sel.cli_daemon,
            false,
        );
        component_row(
            ui,
            "Oracle — DeepSeek R1 1.5B",
            "Local LLM advisor. Downloaded post-install via Ollama.",
            "~1.1 GB",
            &mut sel.install_ollama,
            false,
        );
        component_row(
            ui,
            "Desktop Shortcut",
            "Create a desktop shortcut in addition to the Start Menu entry.",
            "—",
            &mut sel.create_desktop_shortcut,
            false,
        );

        ui.add_space(14.0);
        ui.label(
            RichText::new("INSTALL LOCATION")
                .size(11.0)
                .color(theme::MUTED)
                .strong(),
        );
        ui.add(egui::TextEdit::singleline(&mut sel.install_path).desired_width(f32::INFINITY));
        ui.label(
            RichText::new(
                "Per-user install — no admin rights required. Per-machine installs are planned for v2.",
            )
            .italics()
            .size(11.0)
            .color(theme::DIM),
        );
    });
}

/// Update mode: show the same component grid but read-only — the user
/// can't change install path or component selection at this point. The
/// payload extraction will preserve their `.env` and `config/`.
pub fn show_update(ui: &mut egui::Ui, sel: &ComponentSelection, install_dir: Option<&Path>) {
    theme::card().show(ui, |ui| {
        ui.label(
            RichText::new("Confirm Update")
                .heading()
                .color(theme::FORGE_GOLD),
        );
        ui.add_space(4.0);
        ui.label(
            RichText::new(
                "Components are locked during an update — the wizard refreshes whatever is \
                 already installed. To add or remove components, uninstall first and run a \
                 fresh install.",
            )
            .color(theme::MUTED)
            .size(12.0),
        );
        ui.add_space(12.0);

        readonly_row(ui, "GADARAH GUI", "Refreshed in place.", "~45 MB", sel.gui);
        readonly_row(
            ui,
            "CLI Daemon",
            "Refreshed in place.",
            "~25 MB",
            sel.cli_daemon,
        );
        readonly_row(
            ui,
            "Oracle — DeepSeek R1 1.5B",
            "Untouched (already installed via Ollama if present).",
            "~1.1 GB",
            sel.install_ollama,
        );
        readonly_row(
            ui,
            "Desktop Shortcut",
            "Refreshed if present.",
            "—",
            sel.create_desktop_shortcut,
        );

        ui.add_space(14.0);
        ui.label(
            RichText::new("INSTALL LOCATION")
                .size(11.0)
                .color(theme::MUTED)
                .strong(),
        );
        let location = install_dir
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| sel.install_path.clone());
        ui.label(
            RichText::new(location)
                .monospace()
                .size(12.0)
                .color(theme::TEXT),
        );
        ui.add_space(6.0);
        ui.label(
            RichText::new(
                "Your firm profiles, broker credentials, and Oracle settings will be preserved.",
            )
            .italics()
            .size(11.0)
            .color(theme::DIM),
        );
    });
}

/// Uninstall mode: show what's about to be removed and require an explicit
/// confirmation tick-box before the action button enables. A second
/// checkbox controls whether `.env*` and `config/` are wiped or kept.
pub fn show_uninstall(
    ui: &mut egui::Ui,
    install_dir: Option<&Path>,
    confirmed: &mut bool,
    keep_user_data: &mut bool,
) {
    theme::card().show(ui, |ui| {
        ui.label(
            RichText::new("Confirm Uninstall")
                .heading()
                .color(theme::FORGE_CRIMSON),
        );
        ui.add_space(4.0);
        ui.label(
            RichText::new(
                "Closing this wizard before completion will leave the install in a half-removed \
                 state. Make sure GADARAH is not running.",
            )
            .color(theme::MUTED)
            .size(12.0),
        );
        ui.add_space(12.0);

        ui.label(
            RichText::new("WILL BE REMOVED FROM")
                .size(11.0)
                .color(theme::MUTED)
                .strong(),
        );
        let location = install_dir
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "<no install detected>".to_string());
        ui.label(
            RichText::new(location)
                .monospace()
                .size(12.0)
                .color(theme::TEXT),
        );
        ui.add_space(12.0);

        ui.label(
            RichText::new("ITEMS REMOVED")
                .size(11.0)
                .color(theme::MUTED)
                .strong(),
        );
        for item in [
            "• gadarah-gui.exe, gadarah.exe, gadarah-wizard.exe",
            "• Start Menu folder \"GADARAH\"",
            "• Desktop shortcut",
            "• Add/Remove Programs registry entry",
        ] {
            ui.label(RichText::new(item).color(theme::TEXT));
        }
        ui.add_space(12.0);

        ui.checkbox(
            keep_user_data,
            "Keep my firm profiles, broker credentials, and saved settings",
        );
        ui.label(
            RichText::new(
                "Untick to also wipe `.env` files and the `config/` directory. Use this if you \
                 want a fully clean reinstall.",
            )
            .italics()
            .size(11.0)
            .color(theme::DIM),
        );
        ui.add_space(10.0);

        ui.checkbox(
            confirmed,
            "I understand this will remove GADARAH from my system.",
        );
    });
}

fn readonly_row(ui: &mut egui::Ui, title: &str, subtitle: &str, size: &str, on: bool) {
    egui::Frame::new()
        .fill(theme::CARD)
        .stroke(egui::Stroke::new(1.0, theme::BORDER))
        .corner_radius(6u8)
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let dot = if on { "●" } else { "○" };
                let dot_color = if on { theme::GREEN } else { theme::MUTED };
                ui.label(RichText::new(dot).color(dot_color).size(14.0));
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new(title)
                            .size(13.5)
                            .color(theme::TEXT)
                            .strong(),
                    );
                    ui.label(RichText::new(subtitle).size(11.5).color(theme::MUTED));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new(size)
                            .monospace()
                            .size(11.5)
                            .color(theme::MUTED),
                    );
                });
            });
        });
    ui.add_space(6.0);
}

fn component_row(
    ui: &mut egui::Ui,
    title: &str,
    subtitle: &str,
    size: &str,
    value: &mut bool,
    required: bool,
) {
    egui::Frame::new()
        .fill(theme::CARD)
        .stroke(egui::Stroke::new(1.0, theme::BORDER))
        .corner_radius(6u8)
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let mut cb = *value;
                let resp = ui.add_enabled(!required, egui::Checkbox::new(&mut cb, ""));
                if !required && resp.changed() {
                    *value = cb;
                }
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(title)
                                .size(13.5)
                                .color(theme::TEXT)
                                .strong(),
                        );
                        if required {
                            theme::pill(
                                ui,
                                "REQUIRED",
                                egui::Color32::from_rgb(10, 38, 20),
                                theme::GREEN,
                            );
                        }
                    });
                    ui.label(RichText::new(subtitle).size(11.5).color(theme::MUTED));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new(size)
                            .monospace()
                            .size(11.5)
                            .color(theme::MUTED),
                    );
                });
            });
        });
    ui.add_space(6.0);
}
