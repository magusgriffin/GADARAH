//! Price chart — OHLC candlestick chart with trade markers and SL/TP lines

use eframe::egui;
use egui::RichText;
use egui_plot::{BoxElem, BoxPlot, BoxSpread, HLine, Plot, PlotPoints, Points};

use crate::state::{AppState, PriceBar, TradeMarkerKind};
use crate::theme;

pub struct PriceChartPanel;

impl PriceChartPanel {
    pub fn show(ui: &mut egui::Ui, state: &AppState) {
        let g = state.lock().unwrap();

        let symbol = if g.chart_symbol.is_empty() {
            g.positions
                .first()
                .map(|p| p.symbol.clone())
                .or_else(|| g.regime_by_symbol.keys().next().cloned())
                .unwrap_or_default()
        } else {
            g.chart_symbol.clone()
        };

        let bars = g.price_bars.clone();
        let positions = g.positions.clone();
        let markers = g.trade_markers.clone();

        drop(g);

        // Header
        ui.horizontal(|ui| {
            theme::heading(ui, "Price Chart");
            if !symbol.is_empty() {
                ui.add_space(8.0);
                theme::pill(
                    ui,
                    &format!(" {} ", symbol),
                    egui::Color32::from_rgb(10, 30, 45),
                    theme::BLUE,
                );
            }
            if !bars.is_empty() {
                ui.add_space(8.0);
                ui.label(
                    RichText::new(format!("{} bars", bars.len()))
                        .color(theme::MUTED)
                        .size(12.0),
                );
            }
        });
        ui.add_space(4.0);
        ui.label(
            RichText::new("Live price action with trade entries, stop losses, and take profits.")
                .color(theme::MUTED)
                .size(12.5),
        );
        ui.add_space(12.0);

        if bars.is_empty() {
            theme::card().show(ui, |ui| {
                theme::empty_state(
                    ui,
                    "🕯",
                    "No Price Data",
                    "Connect to a broker or use --state-file to stream live candles from the CLI.",
                );
            });
            return;
        }

        // Current price info bar
        if let Some(last) = bars.last() {
            let prev_close = if bars.len() >= 2 {
                bars[bars.len() - 2].close
            } else {
                last.open
            };
            let change = last.close - prev_close;
            let change_pct = if prev_close != 0.0 {
                change / prev_close * 100.0
            } else {
                0.0
            };
            let bullish = last.close >= last.open;

            theme::card_sm().show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("{:.5}", last.close))
                            .size(22.0)
                            .color(if bullish { theme::GREEN } else { theme::RED })
                            .strong()
                            .monospace(),
                    );
                    ui.add_space(12.0);
                    let sign = if change >= 0.0 { "+" } else { "" };
                    ui.label(
                        RichText::new(format!(
                            "{}{:.5} ({}{:.2}%)",
                            sign, change, sign, change_pct
                        ))
                        .size(13.0)
                        .color(if change >= 0.0 {
                            theme::GREEN
                        } else {
                            theme::RED
                        })
                        .monospace(),
                    );
                    ui.add_space(20.0);
                    for (label, val) in [
                        ("O", last.open),
                        ("H", last.high),
                        ("L", last.low),
                        ("C", last.close),
                    ] {
                        ui.label(
                            RichText::new(format!("{}: {:.5}", label, val))
                                .size(11.5)
                                .color(theme::MUTED)
                                .monospace(),
                        );
                        ui.add_space(6.0);
                    }
                });
            });
            ui.add_space(8.0);
        }

        // Candlestick chart
        theme::card().show(ui, |ui| {
            theme::section_label(ui, "OHLC CANDLESTICK CHART");
            ui.add_space(6.0);

            let candles = build_candles(&bars);
            let volume_bars = build_volume_bars(&bars);

            // Main price chart
            Plot::new("price_chart_main")
                .height(360.0)
                .show_axes([true, true])
                .x_axis_label("Bar")
                .y_axis_label("Price")
                .label_formatter(move |_name, value| {
                    format!("Bar {:.0}\nPrice: {:.5}", value.x, value.y)
                })
                .show(ui, |plot_ui| {
                    // Bullish candles
                    let bull_candles: Vec<BoxElem> = candles
                        .iter()
                        .filter(|c| c.bullish)
                        .map(|c| c.to_box_elem())
                        .collect();
                    if !bull_candles.is_empty() {
                        plot_ui.box_plot(
                            BoxPlot::new(bull_candles)
                                .name("Bullish")
                                .color(theme::GREEN),
                        );
                    }

                    // Bearish candles
                    let bear_candles: Vec<BoxElem> = candles
                        .iter()
                        .filter(|c| !c.bullish)
                        .map(|c| c.to_box_elem())
                        .collect();
                    if !bear_candles.is_empty() {
                        plot_ui
                            .box_plot(BoxPlot::new(bear_candles).name("Bearish").color(theme::RED));
                    }

                    // SL/TP lines from open positions
                    for pos in &positions {
                        if pos.symbol != symbol && !symbol.is_empty() {
                            continue;
                        }
                        let entry_f = pos.entry_price.to_string().parse::<f64>().unwrap_or(0.0);
                        plot_ui.hline(
                            HLine::new(entry_f)
                                .name(format!("Entry {}", pos.symbol))
                                .color(theme::BLUE)
                                .width(1.5),
                        );
                        if let Some(sl) = pos.stop_loss {
                            let sl_f = sl.to_string().parse::<f64>().unwrap_or(0.0);
                            plot_ui.hline(
                                HLine::new(sl_f)
                                    .name("Stop Loss")
                                    .color(theme::RED)
                                    .width(1.0)
                                    .style(egui_plot::LineStyle::dashed_dense()),
                            );
                        }
                        if let Some(tp) = pos.take_profit {
                            let tp_f = tp.to_string().parse::<f64>().unwrap_or(0.0);
                            plot_ui.hline(
                                HLine::new(tp_f)
                                    .name("Take Profit")
                                    .color(theme::GREEN)
                                    .width(1.0)
                                    .style(egui_plot::LineStyle::dashed_dense()),
                            );
                        }
                    }

                    // Trade entry/exit markers
                    let entry_points: Vec<[f64; 2]> = markers
                        .iter()
                        .filter(|m| m.kind == TradeMarkerKind::Entry)
                        .filter_map(|m| {
                            bar_index_for_timestamp(&bars, m.timestamp)
                                .map(|idx| [idx as f64, m.price])
                        })
                        .collect();
                    if !entry_points.is_empty() {
                        plot_ui.points(
                            Points::new(PlotPoints::new(entry_points))
                                .name("Entries")
                                .shape(egui_plot::MarkerShape::Up)
                                .radius(6.0)
                                .color(theme::ACCENT),
                        );
                    }

                    let tp_points: Vec<[f64; 2]> = markers
                        .iter()
                        .filter(|m| m.kind == TradeMarkerKind::TakeProfit)
                        .filter_map(|m| {
                            bar_index_for_timestamp(&bars, m.timestamp)
                                .map(|idx| [idx as f64, m.price])
                        })
                        .collect();
                    if !tp_points.is_empty() {
                        plot_ui.points(
                            Points::new(PlotPoints::new(tp_points))
                                .name("Take Profits")
                                .shape(egui_plot::MarkerShape::Diamond)
                                .radius(5.0)
                                .color(theme::GREEN),
                        );
                    }

                    let sl_points: Vec<[f64; 2]> = markers
                        .iter()
                        .filter(|m| m.kind == TradeMarkerKind::StopLoss)
                        .filter_map(|m| {
                            bar_index_for_timestamp(&bars, m.timestamp)
                                .map(|idx| [idx as f64, m.price])
                        })
                        .collect();
                    if !sl_points.is_empty() {
                        plot_ui.points(
                            Points::new(PlotPoints::new(sl_points))
                                .name("Stop Losses")
                                .shape(egui_plot::MarkerShape::Cross)
                                .radius(5.0)
                                .color(theme::RED),
                        );
                    }
                });

            ui.add_space(8.0);

            // Volume sub-chart
            theme::section_label(ui, "VOLUME");
            Plot::new("volume_chart")
                .height(80.0)
                .show_axes([false, true])
                .y_axis_label("Vol")
                .link_axis("price_chart_main", true)
                .link_cursor("price_chart_main", true)
                .show(ui, |plot_ui| {
                    if !volume_bars.is_empty() {
                        plot_ui.bar_chart(
                            egui_plot::BarChart::new(volume_bars)
                                .name("Volume")
                                .color(egui::Color32::from_rgba_premultiplied(80, 160, 255, 80)),
                        );
                    }
                });
        });

        ui.add_space(12.0);

        // Price summary card
        if bars.len() >= 2 {
            theme::card_sm().show(ui, |ui| {
                let highs: f64 = bars
                    .iter()
                    .map(|b| b.high)
                    .fold(f64::NEG_INFINITY, f64::max);
                let lows: f64 = bars.iter().map(|b| b.low).fold(f64::INFINITY, f64::min);
                let avg_vol: f64 =
                    bars.iter().map(|b| b.volume as f64).sum::<f64>() / bars.len() as f64;
                let bullish_count = bars.iter().filter(|b| b.close >= b.open).count();
                let bear_count = bars.len() - bullish_count;

                ui.horizontal(|ui| {
                    for (label, value) in [
                        ("Period High", format!("{:.5}", highs)),
                        ("Period Low", format!("{:.5}", lows)),
                        ("Range", format!("{:.5}", highs - lows)),
                        ("Avg Volume", format!("{:.0}", avg_vol)),
                        ("Bull/Bear", format!("{}/{}", bullish_count, bear_count)),
                    ] {
                        ui.vertical(|ui| {
                            ui.label(RichText::new(label).size(10.5).color(theme::MUTED).strong());
                            ui.label(
                                RichText::new(value)
                                    .size(13.0)
                                    .color(theme::TEXT)
                                    .monospace(),
                            );
                        });
                        ui.add_space(20.0);
                    }
                });
            });
        }
    }
}

struct CandleData {
    index: f64,
    open: f64,
    high: f64,
    low: f64,
    close: f64,
    bullish: bool,
}

impl CandleData {
    fn to_box_elem(&self) -> BoxElem {
        let body_lo = self.open.min(self.close);
        let body_hi = self.open.max(self.close);
        let mid = (self.open + self.close) / 2.0;
        let color = if self.bullish {
            theme::GREEN
        } else {
            theme::RED
        };

        BoxElem::new(
            self.index,
            BoxSpread {
                lower_whisker: self.low,
                quartile1: body_lo,
                median: mid,
                quartile3: body_hi,
                upper_whisker: self.high,
            },
        )
        .box_width(0.6)
        .whisker_width(0.3)
        .fill(color)
        .stroke(egui::Stroke::new(1.0, color))
    }
}

fn build_candles(bars: &[PriceBar]) -> Vec<CandleData> {
    bars.iter()
        .enumerate()
        .map(|(i, b)| CandleData {
            index: i as f64,
            open: b.open,
            high: b.high,
            low: b.low,
            close: b.close,
            bullish: b.close >= b.open,
        })
        .collect()
}

fn build_volume_bars(bars: &[PriceBar]) -> Vec<egui_plot::Bar> {
    bars.iter()
        .enumerate()
        .map(|(i, b)| {
            let color = if b.close >= b.open {
                egui::Color32::from_rgba_premultiplied(56, 182, 74, 120)
            } else {
                egui::Color32::from_rgba_premultiplied(245, 78, 70, 120)
            };
            egui_plot::Bar::new(i as f64, b.volume as f64)
                .fill(color)
                .width(0.6)
        })
        .collect()
}

fn bar_index_for_timestamp(bars: &[PriceBar], ts: i64) -> Option<usize> {
    bars.iter().position(|b| b.timestamp >= ts)
}
