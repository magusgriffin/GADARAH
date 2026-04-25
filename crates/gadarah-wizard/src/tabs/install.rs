use eframe::egui::{self, RichText, ScrollArea};

use crate::install::InstallState;
use crate::theme;
use crate::uninstall::UninstallState;

pub fn show(ui: &mut egui::Ui, state: &mut InstallState, title: &str) {
    progress_body(
        ui,
        title,
        state.progress,
        &state.current_step,
        state.eta().map(|d| d.as_secs_f32()),
        &state.log,
        state.error.as_deref(),
    );
}

pub fn show_uninstall(ui: &mut egui::Ui, state: &mut UninstallState) {
    progress_body(
        ui,
        "Uninstalling GADARAH",
        state.progress,
        &state.current_step,
        state.eta().map(|d| d.as_secs_f32()),
        &state.log,
        state.error.as_deref(),
    );
}

fn progress_body(
    ui: &mut egui::Ui,
    title: &str,
    progress: f32,
    current_step: &str,
    eta_secs: Option<f32>,
    log: &[String],
    error: Option<&str>,
) {
    theme::card().show(ui, |ui| {
        ui.label(RichText::new(title).heading().color(theme::FORGE_GOLD));
        ui.add_space(6.0);

        ui.add(
            egui::ProgressBar::new(progress)
                .desired_width(f32::INFINITY)
                .fill(theme::ACCENT)
                .text(format!("{:>3.0}%", progress * 100.0)),
        );
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(current_step)
                    .size(12.5)
                    .color(theme::TEXT),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(eta) = eta_secs {
                    ui.label(
                        RichText::new(format!("~{:.0}s remaining", eta))
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
                        for line in log {
                            ui.label(
                                RichText::new(line)
                                    .monospace()
                                    .size(11.0)
                                    .color(theme::MUTED),
                            );
                        }
                    });
            });

        if let Some(err) = error {
            ui.add_space(10.0);
            ui.label(
                RichText::new(format!("Error: {err}"))
                    .color(theme::RED)
                    .strong(),
            );
        }
    });
}
