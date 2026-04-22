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
