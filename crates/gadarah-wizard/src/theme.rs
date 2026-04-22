//! Minimal forge palette + frame builders, mirrored from `gadarah-gui::theme`
//! so the wizard stays a self-contained crate without pulling the full GUI
//! stack.

use eframe::egui::{self, Color32, CornerRadius, Margin, Stroke};

pub const BG: Color32 = Color32::from_rgb(8, 12, 18);
pub const CARD: Color32 = Color32::from_rgb(16, 21, 30);
pub const BORDER: Color32 = Color32::from_rgb(38, 46, 56);
pub const TEXT: Color32 = Color32::from_rgb(230, 237, 244);
pub const MUTED: Color32 = Color32::from_rgb(120, 132, 148);
pub const DIM: Color32 = Color32::from_rgb(55, 65, 78);
pub const ACCENT: Color32 = Color32::from_rgb(0, 210, 160);
pub const GREEN: Color32 = Color32::from_rgb(46, 196, 82);
pub const RED: Color32 = Color32::from_rgb(248, 68, 58);
pub const YELLOW: Color32 = Color32::from_rgb(220, 170, 30);

// Forge tones for gothic trim.
pub const FORGE_BG: Color32 = Color32::from_rgb(18, 8, 6);
pub const FORGE_GOLD: Color32 = Color32::from_rgb(212, 168, 71);
pub const FORGE_GOLD_DIM: Color32 = Color32::from_rgb(139, 111, 46);
pub const FORGE_CRIMSON: Color32 = Color32::from_rgb(139, 26, 26);
pub const FORGE_CHROME: Color32 = Color32::from_rgb(138, 139, 147);
pub const FORGE_OBSIDIAN: Color32 = Color32::from_rgb(30, 31, 46);
pub const FORGE_PARCHMENT: Color32 = Color32::from_rgb(54, 38, 22);

pub fn setup(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    v.panel_fill = BG;
    v.window_fill = CARD;
    v.window_stroke = Stroke::new(1.0, BORDER);
    v.extreme_bg_color = Color32::from_rgb(5, 8, 14);
    v.hyperlink_color = ACCENT;
    v.widgets.noninteractive.bg_fill = CARD;
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, MUTED);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
    v.widgets.inactive.bg_fill = Color32::from_rgb(24, 30, 40);
    v.widgets.inactive.weak_bg_fill = CARD;
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);
    v.widgets.hovered.bg_fill = Color32::from_rgb(30, 38, 50);
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, TEXT);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT);
    v.widgets.active.bg_fill = Color32::from_rgb(20, 80, 60);
    v.widgets.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    ctx.set_visuals(v);

    let mut s = (*ctx.style()).clone();
    s.text_styles = [
        (egui::TextStyle::Small, egui::FontId::proportional(11.0)),
        (egui::TextStyle::Body, egui::FontId::proportional(13.5)),
        (egui::TextStyle::Button, egui::FontId::proportional(13.5)),
        (egui::TextStyle::Heading, egui::FontId::proportional(22.0)),
        (egui::TextStyle::Monospace, egui::FontId::monospace(12.5)),
    ]
    .into();
    s.spacing.item_spacing = egui::vec2(8.0, 6.0);
    s.spacing.button_padding = egui::vec2(14.0, 8.0);
    ctx.set_style(s);
}

pub fn card() -> egui::Frame {
    egui::Frame::new()
        .fill(CARD)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(10u8)
        .inner_margin(Margin::same(18))
}

pub fn breathing_glow(time_secs: f32, base: Color32, freq_hz: f32) -> Color32 {
    let phase = (time_secs * freq_hz * std::f32::consts::TAU).sin();
    let factor = 0.85 + 0.15 * phase;
    base.linear_multiply(factor.clamp(0.7, 1.0))
}

pub fn pill(ui: &mut egui::Ui, text: &str, bg: Color32, fg: Color32) {
    egui::Frame::new()
        .fill(bg)
        .stroke(Stroke::new(1.0, fg.linear_multiply(0.6)))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin {
            left: 10,
            right: 10,
            top: 4,
            bottom: 4,
        })
        .show(ui, |ui| {
            ui.label(egui::RichText::new(text).color(fg).size(11.0).strong());
        });
}
