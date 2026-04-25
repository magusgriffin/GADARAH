use std::path::Path;

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

/// Update-mode welcome card. We have already detected an existing install
/// (`install_dir`) — the user just needs to confirm they want to refresh it
/// in place. License is skipped, components are read-only, the install step
/// preserves their `.env` and `config/` files.
pub fn show_update(ui: &mut egui::Ui, install_dir: Option<&Path>) {
    theme::card().show(ui, |ui| {
        ui.label(
            RichText::new("Update GADARAH")
                .heading()
                .color(theme::FORGE_GOLD)
                .strong(),
        );
        ui.add_space(8.0);
        ui.label(
            RichText::new(
                "A newer version of GADARAH is available. The wizard will refresh the binaries \
                 in place — your firm profiles, broker credentials, and saved settings are \
                 preserved.",
            )
            .color(theme::TEXT)
            .size(13.5),
        );
        ui.add_space(12.0);

        ui.columns(2, |cols| {
            theme::card().show(&mut cols[0], |ui| {
                ui.label(
                    RichText::new("WHAT WE'LL DO")
                        .size(11.0)
                        .color(theme::MUTED)
                        .strong(),
                );
                ui.add_space(6.0);
                for item in [
                    "• Stop any running GADARAH processes",
                    "• Replace gadarah-gui.exe and gadarah.exe",
                    "• Refresh shortcuts and registry entry",
                    "• Keep your config/, .env, and oracle_config.json",
                ] {
                    ui.label(RichText::new(item).color(theme::TEXT));
                }
            });

            theme::card().show(&mut cols[1], |ui| {
                ui.label(
                    RichText::new("EXISTING INSTALL")
                        .size(11.0)
                        .color(theme::MUTED)
                        .strong(),
                );
                ui.add_space(6.0);
                let location = install_dir
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "<not detected>".to_string());
                ui.label(
                    RichText::new(location)
                        .monospace()
                        .size(11.5)
                        .color(theme::TEXT),
                );
                ui.add_space(6.0);
                ui.label(
                    RichText::new(
                        "Close GADARAH before continuing — the updater will force-stop it \
                         otherwise.",
                    )
                    .italics()
                    .size(11.0)
                    .color(theme::MUTED),
                );
            });
        });

        ui.add_space(8.0);
        ui.label(
            RichText::new("Click Next to confirm the components, then Install to apply.")
                .italics()
                .color(theme::MUTED),
        );
    });
}

/// Uninstall-mode welcome card. The destructive nature of the operation is
/// front-and-center; the components tab will require a tick-box before the
/// Install button enables.
pub fn show_uninstall(ui: &mut egui::Ui, install_dir: Option<&Path>) {
    theme::card().show(ui, |ui| {
        ui.label(
            RichText::new("Uninstall GADARAH")
                .heading()
                .color(theme::FORGE_GOLD)
                .strong(),
        );
        ui.add_space(8.0);
        ui.label(
            RichText::new(
                "This wizard will remove GADARAH from your system. By default, your firm \
                 profiles and broker credentials are preserved — you can opt to wipe them on \
                 the next step.",
            )
            .color(theme::TEXT)
            .size(13.5),
        );
        ui.add_space(12.0);

        ui.columns(2, |cols| {
            theme::card().show(&mut cols[0], |ui| {
                ui.label(
                    RichText::new("WHAT WE'LL REMOVE")
                        .size(11.0)
                        .color(theme::MUTED)
                        .strong(),
                );
                ui.add_space(6.0);
                for item in [
                    "• gadarah-gui.exe + gadarah.exe + libraries",
                    "• Start Menu folder \"GADARAH\"",
                    "• Desktop shortcut (if any)",
                    "• Add/Remove Programs registry entry",
                ] {
                    ui.label(RichText::new(item).color(theme::TEXT));
                }
            });

            theme::card().show(&mut cols[1], |ui| {
                ui.label(
                    RichText::new("DETECTED INSTALL")
                        .size(11.0)
                        .color(theme::MUTED)
                        .strong(),
                );
                ui.add_space(6.0);
                let location = install_dir
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "<not detected>".to_string());
                ui.label(
                    RichText::new(location)
                        .monospace()
                        .size(11.5)
                        .color(theme::TEXT),
                );
                ui.add_space(6.0);
                ui.label(
                    RichText::new("Close GADARAH before continuing.")
                        .italics()
                        .size(11.0)
                        .color(theme::MUTED),
                );
            });
        });

        ui.add_space(8.0);
        ui.label(
            RichText::new("Click Next to confirm and choose whether to keep your user data.")
                .italics()
                .color(theme::MUTED),
        );
    });
}
