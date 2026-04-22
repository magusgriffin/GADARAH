use eframe::egui::{self, RichText};

use crate::theme;

pub fn show(ui: &mut egui::Ui) {
    theme::card().show(ui, |ui| {
        ui.label(
            RichText::new("Welcome to the GADARAH Installation Wizard")
                .heading()
                .color(theme::FORGE_GOLD)
                .strong(),
        );
        ui.add_space(8.0);
        ui.label(
            RichText::new(
                "GADARAH is a Rust trading agent for prop-firm challenges. This wizard will \
                 install the desktop GUI, the CLI daemon, and optionally the local LLM stack \
                 used by the Oracle.",
            )
            .color(theme::TEXT)
            .size(13.5),
        );
        ui.add_space(12.0);

        ui.columns(2, |cols| {
            theme::card().show(&mut cols[0], |ui| {
                ui.label(
                    RichText::new("WHAT YOU GET")
                        .size(11.0)
                        .color(theme::MUTED)
                        .strong(),
                );
                ui.add_space(6.0);
                for item in [
                    "• Desktop GUI with live dashboard",
                    "• CLI daemon for headless runs",
                    "• Firm profile presets",
                    "• Optional local Oracle (DeepSeek R1 1.5B)",
                ] {
                    ui.label(RichText::new(item).color(theme::TEXT));
                }
            });

            theme::card().show(&mut cols[1], |ui| {
                ui.label(
                    RichText::new("BEFORE YOU BEGIN")
                        .size(11.0)
                        .color(theme::MUTED)
                        .strong(),
                );
                ui.add_space(6.0);
                for item in [
                    "• ~250 MB free disk (app only)",
                    "• ~1.1 GB extra for the Oracle model",
                    "• Windows 10/11 64-bit",
                    "• Broker credentials for live trading",
                ] {
                    ui.label(RichText::new(item).color(theme::TEXT));
                }
            });
        });

        ui.add_space(8.0);
        ui.label(
            RichText::new(
                "Click Next to review the license. You can return to any earlier step before \
                 installation begins.",
            )
            .italics()
            .color(theme::MUTED),
        );
    });
}
