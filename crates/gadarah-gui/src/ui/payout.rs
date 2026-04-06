//! Payout panel — Maps expected payouts across prop firm challenge phases

use eframe::egui;
use egui::RichText;
use egui_plot::{Bar, BarChart, Line, Plot, PlotPoints};

use crate::config::FirmConfig;
use crate::state::AppState;
use crate::theme;

/// Account sizes offered by common prop firms
const ACCOUNT_SIZES: &[f64] = &[5_000.0, 10_000.0, 25_000.0, 50_000.0, 100_000.0, 200_000.0];

pub struct PayoutPanel {
    selected_account_size: f64,
    monthly_return_pct: f64,
    months_to_project: usize,
}

impl Default for PayoutPanel {
    fn default() -> Self {
        Self {
            selected_account_size: 100_000.0,
            monthly_return_pct: 5.0,
            months_to_project: 12,
        }
    }
}

impl PayoutPanel {
    pub fn show(&mut self, ui: &mut egui::Ui, state: &AppState) {
        let g = state.lock().unwrap();
        let selected_firm = g.selected_firm.clone();
        let available_firms = g.available_firms.clone();
        let current_balance = g.balance;
        let current_equity = g.equity;
        let total_pnl = g.total_pnl;
        let total_pnl_pct = g.total_pnl_pct;
        let starting_balance = g.starting_balance;
        drop(g);

        theme::heading(ui, "Payout Projections");
        ui.label(
            RichText::new("See your expected payouts based on challenge rules and projected performance.")
                .color(theme::MUTED)
                .size(12.5),
        );
        ui.add_space(12.0);

        // Configuration card
        theme::card().show(ui, |ui| {
            theme::section_label(ui, "PROJECTION SETTINGS");
            ui.add_space(8.0);

            ui.horizontal(|ui| {
                ui.label(RichText::new("Account Size:").color(theme::TEXT).size(13.0));
                ui.add_space(4.0);
                for &size in ACCOUNT_SIZES {
                    let label = if size >= 1000.0 {
                        format!("${}k", size as u64 / 1000)
                    } else {
                        format!("${}", size as u64)
                    };
                    let selected = (self.selected_account_size - size).abs() < 0.01;
                    let btn = ui.add(
                        egui::Button::new(
                            RichText::new(&label)
                                .size(12.0)
                                .color(if selected { theme::ACCENT } else { theme::MUTED }),
                        )
                        .fill(if selected {
                            egui::Color32::from_rgb(0, 40, 30)
                        } else {
                            theme::CARD2
                        }),
                    );
                    if btn.clicked() {
                        self.selected_account_size = size;
                    }
                }
            });

            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Expected Monthly Return:").color(theme::TEXT).size(13.0));
                ui.add(egui::Slider::new(&mut self.monthly_return_pct, 1.0..=20.0).suffix("%"));
            });

            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Projection Period:").color(theme::TEXT).size(13.0));
                ui.add(egui::Slider::new(&mut self.months_to_project, 3..=24).suffix(" months"));
            });
        });

        ui.add_space(12.0);

        // Current account status
        let balance_f: f64 = current_balance.to_string().parse().unwrap_or(0.0);
        let equity_f: f64 = current_equity.to_string().parse().unwrap_or(0.0);
        let starting_f: f64 = starting_balance.to_string().parse().unwrap_or(0.0);
        let pnl_f: f64 = total_pnl.to_string().parse().unwrap_or(0.0);
        let pnl_pct_f: f64 = total_pnl_pct.to_string().parse().unwrap_or(0.0);

        if balance_f > 0.0 {
            theme::card().show(ui, |ui| {
                theme::section_label(ui, "CURRENT ACCOUNT STATUS");
                ui.add_space(8.0);
                let card_w = (ui.available_width() - 40.0) / 4.0;
                ui.horizontal(|ui| {
                    for (label, value, color) in [
                        ("Balance", format!("${:.2}", balance_f), theme::TEXT),
                        ("Equity", format!("${:.2}", equity_f), if equity_f >= balance_f { theme::GREEN } else { theme::RED }),
                        ("Total P&L", format!("${:.2} ({:+.1}%)", pnl_f, pnl_pct_f), if pnl_f >= 0.0 { theme::GREEN } else { theme::RED }),
                        ("Starting", format!("${:.2}", starting_f), theme::MUTED),
                    ] {
                        theme::stat_card(ui, label, &value, color, card_w);
                        ui.add_space(8.0);
                    }
                });
            });
            ui.add_space(12.0);
        }

        // Load all firm configs and show payout table
        let firms = load_firm_configs(&available_firms);

        if firms.is_empty() {
            theme::card().show(ui, |ui| {
                theme::empty_state(ui, "🏢", "No Firm Configs", "Add TOML files to config/firms/ to see payout projections for different prop firm challenges.");
            });
            return;
        }

        // Payout comparison table
        theme::card().show(ui, |ui| {
            theme::section_label(ui, "CHALLENGE PAYOUT COMPARISON");
            ui.add_space(4.0);
            ui.label(
                RichText::new(format!(
                    "Based on ${:.0}k account with {:.1}% monthly return",
                    self.selected_account_size / 1000.0,
                    self.monthly_return_pct
                ))
                .color(theme::MUTED)
                .size(12.0),
            );
            ui.add_space(8.0);

            egui::ScrollArea::horizontal().show(ui, |ui| {
                egui::Grid::new("payout_grid")
                    .num_columns(9)
                    .spacing([12.0, 8.0])
                    .striped(true)
                    .show(ui, |ui| {
                        for h in [
                            "Firm",
                            "Type",
                            "Target",
                            "Daily DD",
                            "Max DD",
                            "Min Days",
                            "Split",
                            "Payout/Mo",
                            "Annual",
                        ] {
                            ui.label(RichText::new(h).color(theme::MUTED).size(11.5).strong());
                        }
                        ui.end_row();

                        for fc in &firms {
                            let f = &fc.firm;
                            let target_f: f64 =
                                f.profit_target_pct.to_string().parse().unwrap_or(0.0);
                            let daily_dd: f64 =
                                f.daily_dd_limit_pct.to_string().parse().unwrap_or(0.0);
                            let max_dd: f64 =
                                f.max_dd_limit_pct.to_string().parse().unwrap_or(0.0);
                            let split: f64 =
                                f.profit_split_pct.to_string().parse().unwrap_or(0.0);

                            // Monthly profit = account * monthly_return_pct%
                            let monthly_profit =
                                self.selected_account_size * self.monthly_return_pct / 100.0;
                            // Your share = profit * split%
                            let monthly_payout = monthly_profit * split / 100.0;
                            let annual_payout = monthly_payout * 12.0;

                            // Can we hit the target each month?
                            let can_hit_target = self.monthly_return_pct >= target_f;
                            // Is risk within limits?
                            let risk_ok = self.monthly_return_pct * 0.3 < daily_dd;

                            let row_color = if can_hit_target && risk_ok {
                                theme::TEXT
                            } else {
                                theme::DIM
                            };

                            let is_selected = selected_firm.as_deref() == Some(&f.name);
                            let name_color = if is_selected { theme::ACCENT } else { row_color };

                            ui.label(
                                RichText::new(&f.name).size(12.5).color(name_color).strong(),
                            );
                            ui.label(
                                RichText::new(&f.challenge_type)
                                    .size(12.0)
                                    .color(theme::MUTED),
                            );
                            ui.label(
                                RichText::new(format!("{:.1}%", target_f))
                                    .monospace()
                                    .color(if can_hit_target {
                                        theme::GREEN
                                    } else {
                                        theme::RED
                                    })
                                    .size(12.0),
                            );
                            ui.label(
                                RichText::new(format!("{:.1}%", daily_dd))
                                    .monospace()
                                    .color(row_color)
                                    .size(12.0),
                            );
                            ui.label(
                                RichText::new(format!("{:.1}%", max_dd))
                                    .monospace()
                                    .color(row_color)
                                    .size(12.0),
                            );
                            ui.label(
                                RichText::new(format!("{}", f.min_trading_days))
                                    .monospace()
                                    .color(row_color)
                                    .size(12.0),
                            );
                            ui.label(
                                RichText::new(format!("{:.0}%", split))
                                    .monospace()
                                    .color(theme::ACCENT)
                                    .size(12.0),
                            );
                            ui.label(
                                RichText::new(format!("${:.0}", monthly_payout))
                                    .monospace()
                                    .color(if can_hit_target {
                                        theme::GREEN
                                    } else {
                                        theme::DIM
                                    })
                                    .size(12.5)
                                    .strong(),
                            );
                            ui.label(
                                RichText::new(format!("${:.0}", annual_payout))
                                    .monospace()
                                    .color(if can_hit_target {
                                        theme::GREEN
                                    } else {
                                        theme::DIM
                                    })
                                    .size(12.5)
                                    .strong(),
                            );
                            ui.end_row();
                        }
                    });
            });
        });

        ui.add_space(12.0);

        // Monthly payout bar chart + cumulative line
        theme::card().show(ui, |ui| {
            theme::section_label(ui, "PROJECTED MONTHLY PAYOUTS");
            ui.add_space(6.0);

            // Get the best firm's split for projection
            let best_split: f64 = firms
                .iter()
                .map(|f| {
                    f.firm
                        .profit_split_pct
                        .to_string()
                        .parse::<f64>()
                        .unwrap_or(0.0)
                })
                .fold(0.0f64, f64::max);
            let split_frac = best_split / 100.0;

            let mut cumulative = 0.0;
            let mut bars_data = Vec::new();
            let mut cum_points = Vec::new();

            for month in 0..self.months_to_project {
                let monthly_profit =
                    self.selected_account_size * self.monthly_return_pct / 100.0;
                let payout = monthly_profit * split_frac;
                cumulative += payout;

                bars_data.push(
                    Bar::new(month as f64, payout)
                        .fill(theme::ACCENT)
                        .width(0.6),
                );
                cum_points.push([month as f64, cumulative]);
            }

            let total_annual = cumulative;

            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!(
                        "Best split: {:.0}%  |  Projected {}-month total: ${:.0}",
                        best_split, self.months_to_project, total_annual
                    ))
                    .color(theme::ACCENT)
                    .size(13.0)
                    .strong(),
                );
            });
            ui.add_space(6.0);

            Plot::new("payout_bars")
                .height(200.0)
                .x_axis_label("Month")
                .y_axis_label("Payout ($)")
                .show_axes([true, true])
                .label_formatter(|_name, value| {
                    format!("Month {:.0}: ${:.0}", value.x + 1.0, value.y)
                })
                .show(ui, |plot_ui| {
                    plot_ui.bar_chart(BarChart::new(bars_data).name("Monthly Payout"));
                    plot_ui.line(
                        Line::new(PlotPoints::new(cum_points))
                            .name("Cumulative")
                            .color(theme::YELLOW)
                            .width(2.0),
                    );
                });
        });

        ui.add_space(12.0);

        // Scaling plan
        theme::card().show(ui, |ui| {
            theme::section_label(ui, "SCALING ROADMAP — Growing from challenge to funded");
            ui.add_space(8.0);

            let phases = build_scaling_phases(
                self.selected_account_size,
                self.monthly_return_pct,
                &firms,
            );

            egui::Grid::new("scaling_grid")
                .num_columns(6)
                .spacing([14.0, 8.0])
                .striped(true)
                .show(ui, |ui| {
                    for h in ["Phase", "Account", "Monthly Profit", "Your Payout", "Time to Pass", "Cumulative Earned"] {
                        ui.label(RichText::new(h).color(theme::MUTED).size(11.5).strong());
                    }
                    ui.end_row();

                    let mut total_earned = 0.0;
                    for (i, phase) in phases.iter().enumerate() {
                        total_earned += phase.monthly_payout * phase.months_to_pass as f64;

                        let phase_color = match i {
                            0 => theme::YELLOW,
                            1 => theme::BLUE,
                            _ => theme::GREEN,
                        };

                        ui.label(
                            RichText::new(&phase.name)
                                .size(12.5)
                                .color(phase_color)
                                .strong(),
                        );
                        ui.label(
                            RichText::new(format!("${:.0}k", phase.account_size / 1000.0))
                                .monospace()
                                .color(theme::TEXT)
                                .size(12.0),
                        );
                        ui.label(
                            RichText::new(format!("${:.0}", phase.monthly_profit))
                                .monospace()
                                .color(theme::TEXT)
                                .size(12.0),
                        );
                        ui.label(
                            RichText::new(format!("${:.0}", phase.monthly_payout))
                                .monospace()
                                .color(theme::GREEN)
                                .size(12.5)
                                .strong(),
                        );
                        ui.label(
                            RichText::new(format!(
                                "~{} month{}",
                                phase.months_to_pass,
                                if phase.months_to_pass != 1 { "s" } else { "" }
                            ))
                            .color(theme::MUTED)
                            .size(12.0),
                        );
                        ui.label(
                            RichText::new(format!("${:.0}", total_earned))
                                .monospace()
                                .color(theme::ACCENT)
                                .size(12.0),
                        );
                        ui.end_row();
                    }
                });
        });
    }
}

struct ScalingPhase {
    name: String,
    account_size: f64,
    monthly_profit: f64,
    monthly_payout: f64,
    months_to_pass: usize,
}

fn build_scaling_phases(
    account_size: f64,
    monthly_return_pct: f64,
    firms: &[FirmConfig],
) -> Vec<ScalingPhase> {
    // Find the firm with the best split
    let best_firm = firms
        .iter()
        .max_by(|a, b| {
            let sa: f64 = a.firm.profit_split_pct.to_string().parse().unwrap_or(0.0);
            let sb: f64 = b.firm.profit_split_pct.to_string().parse().unwrap_or(0.0);
            sa.partial_cmp(&sb).unwrap_or(std::cmp::Ordering::Equal)
        });

    let (split_pct, target_pct, challenge_type) = match best_firm {
        Some(f) => (
            f.firm.profit_split_pct.to_string().parse::<f64>().unwrap_or(80.0),
            f.firm.profit_target_pct.to_string().parse::<f64>().unwrap_or(8.0),
            f.firm.challenge_type.clone(),
        ),
        None => (80.0, 8.0, "2step".to_string()),
    };

    let split_frac = split_pct / 100.0;
    let monthly_profit = account_size * monthly_return_pct / 100.0;

    // How many months to hit the challenge target?
    let months_to_target = (target_pct / monthly_return_pct).ceil().max(1.0) as usize;

    let mut phases = Vec::new();

    // Challenge phase(s)
    match challenge_type.as_str() {
        "2step" => {
            phases.push(ScalingPhase {
                name: "Phase 1 — Challenge".to_string(),
                account_size,
                monthly_profit,
                monthly_payout: 0.0, // no payouts during challenge
                months_to_pass: months_to_target,
            });
            phases.push(ScalingPhase {
                name: "Phase 2 — Verification".to_string(),
                account_size,
                monthly_profit,
                monthly_payout: 0.0,
                months_to_pass: months_to_target,
            });
        }
        "1step" => {
            phases.push(ScalingPhase {
                name: "Challenge".to_string(),
                account_size,
                monthly_profit,
                monthly_payout: 0.0,
                months_to_pass: months_to_target,
            });
        }
        _ => {
            phases.push(ScalingPhase {
                name: "Evaluation".to_string(),
                account_size,
                monthly_profit,
                monthly_payout: 0.0,
                months_to_pass: months_to_target,
            });
        }
    }

    // Funded phase — now getting payouts
    phases.push(ScalingPhase {
        name: "Funded — Base".to_string(),
        account_size,
        monthly_profit,
        monthly_payout: monthly_profit * split_frac,
        months_to_pass: 6,
    });

    // Scaled up phase (most firms offer scaling after consistent performance)
    let scaled_size = account_size * 2.0;
    let scaled_profit = scaled_size * monthly_return_pct / 100.0;
    phases.push(ScalingPhase {
        name: "Funded — Scaled (2x)".to_string(),
        account_size: scaled_size,
        monthly_profit: scaled_profit,
        monthly_payout: scaled_profit * split_frac,
        months_to_pass: 6,
    });

    // Max scale
    let max_size = account_size * 4.0;
    let max_profit = max_size * monthly_return_pct / 100.0;
    phases.push(ScalingPhase {
        name: "Funded — Max (4x)".to_string(),
        account_size: max_size,
        monthly_profit: max_profit,
        monthly_payout: max_profit * split_frac,
        months_to_pass: 12,
    });

    phases
}

fn load_firm_configs(available_firms: &[String]) -> Vec<FirmConfig> {
    let mut configs = Vec::new();
    for name in available_firms {
        let path = std::path::PathBuf::from(format!("config/firms/{}.toml", name));
        if let Ok(fc) = FirmConfig::load(&path) {
            configs.push(fc);
        }
    }
    configs
}
