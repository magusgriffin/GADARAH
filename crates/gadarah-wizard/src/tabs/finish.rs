use eframe::egui::{self, RichText};

use crate::theme;

pub fn show(ui: &mut egui::Ui, launch_requested: &mut bool, close_requested: &mut bool) {
    theme::card().show(ui, |ui| {
        ui.label(
            RichText::new("Installation Complete")
                .heading()
                .color(theme::GREEN),
        );
        ui.add_space(10.0);
        ui.label(
            RichText::new(
                "GADARAH has been installed. You can launch it now or find it in your Start \
                 Menu under GADARAH.",
            )
            .color(theme::TEXT)
            .size(13.5),
        );
        ui.add_space(12.0);

        ui.columns(2, |cols| {
            theme::card().show(&mut cols[0], |ui| {
                ui.label(
                    RichText::new("NEXT STEPS")
                        .size(11.0)
                        .color(theme::MUTED)
                        .strong(),
                );
                ui.add_space(6.0);
                for item in [
                    "1. Pick your firm profile in the Config tab",
                    "2. Configure broker credentials",
                    "3. Run the demo until you trust the flow",
                    "4. Review Oracle settings if enabled",
                ] {
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
                ui.label(RichText::new("• README.md in the install directory").color(theme::TEXT));
                ui.label(RichText::new("• config/firms/ for firm presets").color(theme::TEXT));
                ui.label(
                    RichText::new("• Logs tab for runtime diagnostics").color(theme::TEXT),
                );
                ui.label(
                    RichText::new("• Oracle tab for the LLM advisor").color(theme::TEXT),
                );
            });
        });

        ui.add_space(14.0);
        ui.horizontal(|ui| {
            if ui
                .add(
                    egui::Button::new(
                        RichText::new("Launch GADARAH").color(egui::Color32::WHITE),
                    )
                    .fill(theme::FORGE_GOLD_DIM),
                )
                .clicked()
            {
                *launch_requested = true;
            }
            ui.add_space(8.0);
            if ui
                .button(RichText::new("Close").color(theme::TEXT))
                .clicked()
            {
                *close_requested = true;
            }
        });
    });
}
