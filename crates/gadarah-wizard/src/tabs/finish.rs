use eframe::egui::{self, RichText};

use crate::theme;
use crate::WizardMode;

pub fn show_for_mode(
    ui: &mut egui::Ui,
    mode: WizardMode,
    launch_requested: &mut bool,
    close_requested: &mut bool,
) {
    let (heading, heading_color, intro) = match mode {
        WizardMode::Install => (
            "Installation Complete",
            theme::GREEN,
            "GADARAH has been installed. You can launch it now or find it in your Start Menu \
             under GADARAH.",
        ),
        WizardMode::Update => (
            "Update Complete",
            theme::GREEN,
            "GADARAH has been refreshed in place. Your firm profiles, broker credentials, and \
             saved settings were preserved.",
        ),
        WizardMode::Uninstall => (
            "Uninstall Complete",
            theme::FORGE_GOLD,
            "GADARAH has been removed from your system. Thank you for trying it — the wizard \
             will close itself shortly.",
        ),
    };

    theme::card().show(ui, |ui| {
        ui.label(RichText::new(heading).heading().color(heading_color));
        ui.add_space(10.0);
        ui.label(RichText::new(intro).color(theme::TEXT).size(13.5));
        ui.add_space(12.0);

        match mode {
            WizardMode::Install | WizardMode::Update => {
                ui.columns(2, |cols| {
                    theme::card().show(&mut cols[0], |ui| {
                        ui.label(
                            RichText::new("NEXT STEPS")
                                .size(11.0)
                                .color(theme::MUTED)
                                .strong(),
                        );
                        ui.add_space(6.0);
                        let items = if mode == WizardMode::Install {
                            [
                                "1. Pick your firm profile in the Config tab",
                                "2. Configure broker credentials",
                                "3. Run the demo until you trust the flow",
                                "4. Review Oracle settings if enabled",
                            ]
                        } else {
                            [
                                "1. Verify your firm profile still loads",
                                "2. Re-test the broker connection",
                                "3. Smoke-test a Dry Run before going live",
                                "4. Check the Logs tab for any warnings",
                            ]
                        };
                        for item in items {
                            ui.label(RichText::new(item).color(theme::TEXT));
                        }
                    });

                    theme::card().show(&mut cols[1], |ui| {
                        ui.label(
                            RichText::new("RESOURCES")
                                .size(11.0)
                                .color(theme::MUTED)
                                .strong(),
                        );
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new("• README.md in the install directory")
                                .color(theme::TEXT),
                        );
                        ui.label(
                            RichText::new("• config/firms/ for firm presets").color(theme::TEXT),
                        );
                        ui.label(
                            RichText::new("• Logs tab for runtime diagnostics").color(theme::TEXT),
                        );
                        ui.label(
                            RichText::new("• Oracle tab for the LLM advisor").color(theme::TEXT),
                        );
                    });
                });
            }
            WizardMode::Uninstall => {
                theme::card().show(ui, |ui| {
                    ui.label(
                        RichText::new("WHAT'S LEFT")
                            .size(11.0)
                            .color(theme::MUTED)
                            .strong(),
                    );
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(
                            "If you ticked \"Keep my data\", your firm profiles and broker \
                             credentials are still on disk under the install directory. You \
                             can wipe them by hand or run the uninstaller again with the \
                             checkbox unticked.",
                        )
                        .color(theme::TEXT),
                    );
                });
            }
        }

        ui.add_space(14.0);
        ui.horizontal(|ui| {
            if matches!(mode, WizardMode::Install | WizardMode::Update) {
                let label = if mode == WizardMode::Install {
                    "Launch GADARAH"
                } else {
                    "Relaunch GADARAH"
                };
                if ui
                    .add(
                        egui::Button::new(RichText::new(label).color(egui::Color32::WHITE))
                            .fill(theme::FORGE_GOLD_DIM),
                    )
                    .clicked()
                {
                    *launch_requested = true;
                }
                ui.add_space(8.0);
            }
            if ui
                .button(RichText::new("Close").color(theme::TEXT))
                .clicked()
            {
                *close_requested = true;
            }
        });
    });
}
