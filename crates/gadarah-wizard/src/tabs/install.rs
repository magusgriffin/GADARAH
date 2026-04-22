use eframe::egui::{self, RichText, ScrollArea};

use crate::install::InstallState;
use crate::theme;

pub fn show(ui: &mut egui::Ui, state: &mut InstallState) {
    theme::card().show(ui, |ui| {
        ui.label(
            RichText::new("Installing GADARAH")
                .heading()
                .color(theme::FORGE_GOLD),
        );
        ui.add_space(6.0);

        // Big progress bar
        let p = state.progress;
        ui.add(
            egui::ProgressBar::new(p)
                .desired_width(f32::INFINITY)
                .fill(theme::ACCENT)
                .text(format!("{:>3.0}%", p * 100.0)),
        );
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(&state.current_step)
                    .size(12.5)
                    .color(theme::TEXT),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(eta) = state.eta() {
                    ui.label(
                        RichText::new(format!("~{:.0}s remaining", eta.as_secs_f32()))
                            .monospace()
                            .color(theme::MUTED)
                            .size(11.5),
                    );
                }
            });
        });

        ui.add_space(10.0);
        ui.label(
            RichText::new("LOG")
                .size(11.0)
                .color(theme::MUTED)
                .strong(),
        );
        egui::Frame::new()
            .fill(egui::Color32::from_rgb(6, 8, 12))
            .stroke(egui::Stroke::new(1.0, theme::BORDER))
            .corner_radius(4u8)
            .inner_margin(egui::Margin::same(8))
            .show(ui, |ui| {
                ScrollArea::vertical()
                    .auto_shrink([false; 2])
                    .max_height(140.0)
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        for line in &state.log {
                            ui.label(
                                RichText::new(line)
                                    .monospace()
                                    .size(11.0)
                                    .color(theme::MUTED),
                            );
                        }
                    });
            });

        if let Some(err) = &state.error {
            ui.add_space(10.0);
            ui.label(
                RichText::new(format!("Error: {err}"))
                    .color(theme::RED)
                    .strong(),
            );
        }
    });
}
