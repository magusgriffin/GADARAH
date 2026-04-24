//! First-run welcome overlay. A 4-step modal that mirrors the installer
//! wizard's tone — parchment + gold, deliberate copy — so the first-time
//! GADARAH experience feels like one product instead of "installer done,
//! now figure this out yourself". Dismissing the overlay persists a flag
//! so it doesn't re-appear on subsequent launches.

use eframe::egui;
use egui::RichText;

use crate::theme;

/// Requested action when the overlay closes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WelcomeOutcome {
    /// User didn't act yet.
    None,
    /// Dismiss + jump to the Config tab to finish setup.
    GoToConfig,
    /// Dismiss + stay on current tab.
    Dismiss,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    What,
    FirmProfile,
    Broker,
    FirstRun,
}

impl Step {
    const ORDER: [Self; 4] = [Self::What, Self::FirmProfile, Self::Broker, Self::FirstRun];

    fn index(self) -> usize {
        Self::ORDER.iter().position(|s| *s == self).unwrap_or(0)
    }

    fn label(self) -> &'static str {
        match self {
            Self::What => "What GADARAH is",
            Self::FirmProfile => "Pick a firm",
            Self::Broker => "Connect your broker",
            Self::FirstRun => "Try a dry run",
        }
    }

    fn next(self) -> Option<Self> {
        Self::ORDER.get(self.index() + 1).copied()
    }

    fn prev(self) -> Option<Self> {
        if self.index() == 0 {
            None
        } else {
            Self::ORDER.get(self.index() - 1).copied()
        }
    }
}

pub struct WelcomeOverlay {
    pub step: Step,
}

impl Default for WelcomeOverlay {
    fn default() -> Self {
        Self {
            step: Step::What,
        }
    }
}

impl WelcomeOverlay {
    /// Render the overlay. Returns the outcome so the caller can react.
    pub fn show(&mut self, ctx: &egui::Context) -> WelcomeOutcome {
        let mut outcome = WelcomeOutcome::None;

        // Dim the background so the overlay feels modal.
        egui::Area::new("welcome-dim".into())
            .order(egui::Order::Middle)
            .fixed_pos(egui::pos2(0.0, 0.0))
            .show(ctx, |ui| {
                let screen = ctx.screen_rect();
                ui.painter()
                    .rect_filled(screen, 0.0, egui::Color32::from_black_alpha(180));
            });

        egui::Window::new("")
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .frame(
                egui::Frame::new()
                    .fill(egui::Color32::from_rgb(16, 21, 30))
                    .stroke(egui::Stroke::new(2.0, theme::FORGE_GOLD))
                    .corner_radius(10u8)
                    .inner_margin(egui::Margin::same(24)),
            )
            .show(ctx, |ui| {
                ui.set_min_width(520.0);
                ui.set_max_width(560.0);

                // Breadcrumb row.
                ui.horizontal(|ui| {
                    for (i, s) in Step::ORDER.iter().enumerate() {
                        let done = self.step.index() > i;
                        let current = self.step == *s;
                        let (dot, color) = if current {
                            ("●", theme::FORGE_GOLD)
                        } else if done {
                            ("●", theme::GREEN)
                        } else {
                            ("○", theme::MUTED)
                        };
                        ui.label(RichText::new(dot).color(color).size(14.0));
                        ui.label(
                            RichText::new(format!("{}.", i + 1))
                                .color(color)
                                .size(11.0),
                        );
                        ui.label(
                            RichText::new(s.label())
                                .color(if current { theme::TEXT } else { theme::MUTED })
                                .size(11.5)
                                .strong(),
                        );
                        if i + 1 < Step::ORDER.len() {
                            ui.label(RichText::new("›").color(theme::MUTED).size(12.0));
                        }
                    }
                });
                ui.separator();
                ui.add_space(8.0);

                // Step body.
                match self.step {
                    Step::What => self.step_what(ui),
                    Step::FirmProfile => self.step_firm(ui),
                    Step::Broker => self.step_broker(ui),
                    Step::FirstRun => self.step_first_run(ui),
                }

                ui.add_space(14.0);
                ui.separator();
                ui.add_space(8.0);

                // Action row.
                ui.horizontal(|ui| {
                    if let Some(prev) = self.step.prev() {
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new("Back").color(theme::TEXT),
                                )
                                .fill(egui::Color32::from_rgb(24, 28, 36)),
                            )
                            .clicked()
                        {
                            self.step = prev;
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if self.step == Step::FirstRun {
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new("Take me to Config")
                                            .color(egui::Color32::WHITE)
                                            .strong(),
                                    )
                                    .fill(theme::FORGE_GOLD_DIM),
                                )
                                .clicked()
                            {
                                outcome = WelcomeOutcome::GoToConfig;
                            }
                            ui.add_space(6.0);
                            if ui
                                .button(RichText::new("Skip for now").color(theme::MUTED))
                                .clicked()
                            {
                                outcome = WelcomeOutcome::Dismiss;
                            }
                        } else if let Some(next) = self.step.next() {
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new("Next")
                                            .color(egui::Color32::WHITE)
                                            .strong(),
                                    )
                                    .fill(theme::FORGE_GOLD_DIM),
                                )
                                .clicked()
                            {
                                self.step = next;
                            }
                        }
                    });
                });
            });

        outcome
    }

    fn step_what(&self, ui: &mut egui::Ui) {
        ui.label(
            RichText::new("Welcome to GADARAH")
                .color(theme::FORGE_GOLD)
                .strong()
                .size(20.0),
        );
        ui.add_space(6.0);
        ui.label(
            RichText::new(
                "GADARAH is a Rust-native trading agent for prop-firm challenges. Every order \
                 routes through a sealed risk gate that enforces your firm's daily-loss and \
                 total-drawdown rules before an order ever reaches the broker.",
            )
            .color(theme::TEXT)
            .size(13.0),
        );
        ui.add_space(10.0);
        ui.label(
            RichText::new("Three things worth knowing before you start:")
                .color(theme::TEXT)
                .size(13.0)
                .strong(),
        );
        ui.add_space(4.0);
        for (bullet, line) in [
            (
                "•",
                "The Oracle (local DeepSeek R1 model) is an advisor — it cannot place trades.",
            ),
            (
                "•",
                "The kill switch is sealed: once armed, it only clears after a cooldown expires.",
            ),
            (
                "•",
                "Trade dry-run first, then demo, then live. The Trading tab enforces that order.",
            ),
        ] {
            ui.horizontal(|ui| {
                ui.label(RichText::new(bullet).color(theme::FORGE_GOLD).strong());
                ui.label(RichText::new(line).color(theme::TEXT).size(12.5));
            });
        }
    }

    fn step_firm(&self, ui: &mut egui::Ui) {
        ui.label(
            RichText::new("Pick the firm you're trading")
                .color(theme::FORGE_GOLD)
                .strong()
                .size(18.0),
        );
        ui.add_space(6.0);
        ui.label(
            RichText::new(
                "Each firm has its own profit target, daily-loss limit, and max-drawdown rule. \
                 Selecting a profile loads those limits into the risk gate, so orders that would \
                 violate your firm's rules are rejected before they ship.",
            )
            .color(theme::TEXT)
            .size(13.0),
        );
        ui.add_space(8.0);
        ui.label(
            RichText::new("Supported profiles:")
                .color(theme::TEXT)
                .size(12.5)
                .strong(),
        );
        for firm in [
            "The5ers Hyper Growth (primary)",
            "FTMO 1-Step",
            "FTMO 2-Step",
            "FundingPips (1-Step, 2-Step, Zero)",
            "Alpha Capital",
            "Blue Guardian",
        ] {
            ui.label(
                RichText::new(format!("  • {firm}"))
                    .color(theme::MUTED)
                    .size(12.0),
            );
        }
    }

    fn step_broker(&self, ui: &mut egui::Ui) {
        ui.label(
            RichText::new("Connect your cTrader account")
                .color(theme::FORGE_GOLD)
                .strong()
                .size(18.0),
        );
        ui.add_space(6.0);
        ui.label(
            RichText::new(
                "GADARAH talks to your broker via cTrader's Open API. In the Config tab, open \
                 Broker Setup and paste the Client ID and Client Secret from your Spotware app. \
                 The wizard will open your browser to authorize GADARAH — the callback is caught \
                 on http://localhost:5555.",
            )
            .color(theme::TEXT)
            .size(13.0),
        );
        ui.add_space(8.0);
        ui.label(
            RichText::new(
                "Spotware returns every trading account linked to your ID — pick the one you \
                 want to bind. Demo and live accounts appear in the same list; switching between \
                 them is a menu choice, not a reinstall.",
            )
            .italics()
            .color(theme::MUTED)
            .size(12.0),
        );
    }

    fn step_first_run(&self, ui: &mut egui::Ui) {
        ui.label(
            RichText::new("Start with a dry run")
                .color(theme::FORGE_GOLD)
                .strong()
                .size(18.0),
        );
        ui.add_space(6.0);
        ui.label(
            RichText::new(
                "The Trading tab has three modes: Dry Run, Demo Execute, and LIVE Execute. \
                 Dry Run simulates every signal without sending orders — it's safe to leave on \
                 all day. Demo sends real orders to your broker's demo server. LIVE sends real \
                 money.",
            )
            .color(theme::TEXT)
            .size(13.0),
        );
        ui.add_space(8.0);
        ui.label(
            RichText::new(
                "Preflight gates stack: Dry Run unlocks first, then Demo, then LIVE. A LIVE start \
                 fires a confirmation modal that you have to explicitly approve.",
            )
            .color(theme::TEXT)
            .size(12.5),
        );
        ui.add_space(8.0);
        ui.label(
            RichText::new(
                "Click \"Take me to Config\" to finish broker setup, or skip if you just want to \
                 poke around.",
            )
            .italics()
            .color(theme::MUTED)
            .size(12.0),
        );
    }
}
