//! GADARAH Installation Wizard entry point.
//!
//! A linear 5-step wizard (Welcome → License → Components → Install → Finish)
//! with a procedurally-animated Binder assistant on the right. The wizard is
//! a bootstrapper: the MSI payload still does the real install on Windows.
//! On other platforms the wizard runs with a simulated install driver so the
//! flow can be reviewed without a Windows box.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod assistant;
mod install;
mod tabs;
mod theme;

use std::time::Instant;

use eframe::egui::{self, CornerRadius, RichText, Stroke};

use crate::assistant::AssistantMood;
use crate::install::InstallState;
use crate::tabs::components::ComponentSelection;
use crate::tabs::WizardTab;

struct WizardApp {
    current_tab: WizardTab,
    reached: WizardTab,
    components: ComponentSelection,
    install_state: InstallState,
    license_accepted: bool,
    launch_requested: bool,
    close_requested: bool,
    boot: Instant,
}

impl Default for WizardApp {
    fn default() -> Self {
        Self {
            current_tab: WizardTab::Welcome,
            reached: WizardTab::Welcome,
            components: ComponentSelection::default(),
            install_state: InstallState::default(),
            license_accepted: false,
            launch_requested: false,
            close_requested: false,
            boot: Instant::now(),
        }
    }
}

impl WizardApp {
    fn time_secs(&self) -> f32 {
        self.boot.elapsed().as_secs_f32()
    }

    fn assistant_mood(&self) -> AssistantMood {
        match self.current_tab {
            WizardTab::Welcome => AssistantMood::Greeting,
            WizardTab::License => AssistantMood::Explaining,
            WizardTab::Components => AssistantMood::Explaining,
            WizardTab::Install => {
                if self.install_state.error.is_some() {
                    AssistantMood::Worry
                } else if self.install_state.finished {
                    AssistantMood::Triumph
                } else {
                    AssistantMood::Working
                }
            }
            WizardTab::Finish => AssistantMood::Triumph,
        }
    }

    fn assistant_speech(&self) -> &'static str {
        match self.current_tab {
            WizardTab::Welcome => {
                "Hail, operator. I am the Binder — I'll walk you through the \
                 sealing of GADARAH to your machine."
            }
            WizardTab::License => {
                "Read the covenant carefully. Dual-licensed MIT or Apache — \
                 generous terms, but the disclaimer on trading risk stands."
            }
            WizardTab::Components => {
                "The GUI and the daemon are the essential runes. The Oracle \
                 is optional — it pulls a 1.1 GB model and can be added later."
            }
            WizardTab::Install => {
                if self.install_state.finished {
                    "The seal is complete. Your system is bound to GADARAH."
                } else if self.install_state.error.is_some() {
                    "A rune has fractured. Review the log below."
                } else {
                    "Etching the runes. Do not interrupt the binding."
                }
            }
            WizardTab::Finish => {
                "The work is done. Launch the application or close this wizard \
                 — GADARAH will be waiting in your Start Menu."
            }
        }
    }

    fn can_advance(&self) -> bool {
        match self.current_tab {
            WizardTab::Welcome => true,
            WizardTab::License => self.license_accepted,
            WizardTab::Components => !self.components.install_path.trim().is_empty(),
            WizardTab::Install => self.install_state.finished,
            WizardTab::Finish => false,
        }
    }

    fn advance(&mut self) {
        if let Some(next) = self.current_tab.next() {
            self.current_tab = next;
            if step_index(next) > step_index(self.reached) {
                self.reached = next;
            }
            if next == WizardTab::Install {
                self.install_state.start(&self.components);
            }
        }
    }

    fn retreat(&mut self) {
        if self.current_tab == WizardTab::Install && self.install_state.started_at.is_some() {
            return; // can't go back once installation has started
        }
        if let Some(prev) = self.current_tab.prev() {
            self.current_tab = prev;
        }
    }

    fn breadcrumb(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.add_space(4.0);
            for (i, tab) in WizardTab::ORDER.iter().enumerate() {
                let is_current = *tab == self.current_tab;
                let is_reached = step_index(*tab) <= step_index(self.reached);
                let (bg, fg) = if is_current {
                    (theme::FORGE_GOLD_DIM, theme::TEXT)
                } else if is_reached {
                    (theme::CARD, theme::TEXT)
                } else {
                    (theme::BG, theme::DIM)
                };
                egui::Frame::new()
                    .fill(bg)
                    .stroke(Stroke::new(
                        1.0,
                        if is_current { theme::FORGE_GOLD } else { theme::BORDER },
                    ))
                    .corner_radius(CornerRadius::same(14))
                    .inner_margin(egui::Margin {
                        left: 12,
                        right: 12,
                        top: 6,
                        bottom: 6,
                    })
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(format!("{}. {}", tab.step_number(), tab.label()))
                                .color(fg)
                                .size(12.0)
                                .strong(),
                        );
                    });
                if i + 1 < WizardTab::ORDER.len() {
                    ui.label(
                        RichText::new("›")
                            .color(theme::MUTED)
                            .size(14.0),
                    );
                }
            }
        });
    }

    fn tab_body(&mut self, ui: &mut egui::Ui) {
        match self.current_tab {
            WizardTab::Welcome => tabs::welcome::show(ui),
            WizardTab::License => tabs::license::show(ui, &mut self.license_accepted),
            WizardTab::Components => tabs::components::show(ui, &mut self.components),
            WizardTab::Install => {
                self.install_state.tick();
                tabs::install::show(ui, &mut self.install_state);
                if self.install_state.finished && self.current_tab == WizardTab::Install {
                    // Auto-advance to Finish one frame after completion so the
                    // user sees the 100% bar briefly.
                    self.advance();
                }
            }
            WizardTab::Finish => tabs::finish::show(
                ui,
                &mut self.launch_requested,
                &mut self.close_requested,
            ),
        }
    }

    fn action_bar(&mut self, ui: &mut egui::Ui) -> Option<Action> {
        let mut action = None;
        ui.horizontal(|ui| {
            let can_back = self.current_tab.prev().is_some()
                && !(self.current_tab == WizardTab::Install
                    && self.install_state.started_at.is_some())
                && self.current_tab != WizardTab::Finish;
            if ui
                .add_enabled(can_back, egui::Button::new(RichText::new("Back").color(theme::TEXT)))
                .clicked()
            {
                action = Some(Action::Back);
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                match self.current_tab {
                    WizardTab::Finish => {
                        // Finish tab has its own buttons; action bar is empty here.
                    }
                    WizardTab::Install => {
                        let label = if self.install_state.finished {
                            "Continue"
                        } else {
                            "Installing…"
                        };
                        if ui
                            .add_enabled(
                                self.install_state.finished,
                                egui::Button::new(
                                    RichText::new(label).color(egui::Color32::WHITE),
                                )
                                .fill(theme::FORGE_GOLD_DIM),
                            )
                            .clicked()
                        {
                            action = Some(Action::Next);
                        }
                    }
                    _ => {
                        let label = if self.current_tab == WizardTab::Components {
                            "Install"
                        } else {
                            "Next"
                        };
                        if ui
                            .add_enabled(
                                self.can_advance(),
                                egui::Button::new(
                                    RichText::new(label).color(egui::Color32::WHITE),
                                )
                                .fill(theme::FORGE_GOLD_DIM),
                            )
                            .clicked()
                        {
                            action = Some(Action::Next);
                        }
                    }
                }
                ui.add_space(6.0);
                if self.current_tab != WizardTab::Finish
                    && ui
                        .button(RichText::new("Cancel").color(theme::MUTED))
                        .clicked()
                {
                    action = Some(Action::Cancel);
                }
            });
        });
        action
    }
}

enum Action {
    Next,
    Back,
    Cancel,
}

fn step_index(tab: WizardTab) -> usize {
    WizardTab::ORDER
        .iter()
        .position(|t| *t == tab)
        .unwrap_or(0)
}

impl eframe::App for WizardApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // While installing, request continuous repaints so the progress bar
        // and assistant animate smoothly.
        if matches!(self.current_tab, WizardTab::Install) && !self.install_state.finished {
            ctx.request_repaint();
        } else {
            // Still animate the assistant at a gentle cadence.
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }

        egui::TopBottomPanel::top("wizard-header")
            .exact_height(48.0)
            .frame(
                egui::Frame::new()
                    .fill(theme::FORGE_OBSIDIAN)
                    .inner_margin(egui::Margin::symmetric(12, 8)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("GADARAH · Installation Wizard")
                            .size(15.0)
                            .color(theme::FORGE_GOLD)
                            .strong(),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("Step {} of 5", self.current_tab.step_number()))
                                .color(theme::MUTED)
                                .size(11.5),
                        );
                    });
                });
            });

        egui::TopBottomPanel::top("wizard-breadcrumb")
            .exact_height(46.0)
            .frame(
                egui::Frame::new()
                    .fill(theme::BG)
                    .inner_margin(egui::Margin::symmetric(12, 8)),
            )
            .show(ctx, |ui| self.breadcrumb(ui));

        egui::TopBottomPanel::bottom("wizard-actions")
            .exact_height(56.0)
            .frame(
                egui::Frame::new()
                    .fill(theme::CARD)
                    .inner_margin(egui::Margin::symmetric(16, 10))
                    .stroke(Stroke::new(1.0, theme::BORDER)),
            )
            .show(ctx, |ui| {
                if let Some(a) = self.action_bar(ui) {
                    match a {
                        Action::Next => self.advance(),
                        Action::Back => self.retreat(),
                        Action::Cancel => self.close_requested = true,
                    }
                }
            });

        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(theme::BG)
                    .inner_margin(egui::Margin::same(14)),
            )
            .show(ctx, |ui| {
                let avail = ui.available_rect_before_wrap();
                let split_x = avail.left() + avail.width() * 0.60;

                let left_rect = egui::Rect::from_min_max(
                    avail.min,
                    egui::pos2(split_x - 7.0, avail.bottom()),
                );
                let right_rect = egui::Rect::from_min_max(
                    egui::pos2(split_x + 7.0, avail.top()),
                    avail.max,
                );

                let mut left_ui = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(left_rect)
                        .layout(egui::Layout::top_down(egui::Align::Min)),
                );
                egui::ScrollArea::vertical()
                    .auto_shrink([false; 2])
                    .show(&mut left_ui, |ui| self.tab_body(ui));

                let mut right_ui = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(right_rect)
                        .layout(egui::Layout::top_down(egui::Align::Min)),
                );
                assistant::show(
                    &mut right_ui,
                    self.assistant_mood(),
                    self.time_secs(),
                    self.assistant_speech(),
                );
            });

        if self.close_requested {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("GADARAH · Installation Wizard")
            .with_inner_size([760.0, 540.0])
            .with_min_inner_size([720.0, 500.0])
            .with_resizable(false),
        ..Default::default()
    };
    eframe::run_native(
        "GADARAH Installation Wizard",
        options,
        Box::new(|cc| {
            theme::setup(&cc.egui_ctx);
            Ok(Box::new(WizardApp::default()))
        }),
    )
}
