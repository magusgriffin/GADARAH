//! Visual design system — colors, frames, helpers

use eframe::egui::{self, Color32, CornerRadius, Margin, Stroke};

// ── Palette ───────────────────────────────────────────────────────────────────
pub const BG:         Color32 = Color32::from_rgb(11,  15,  21);
pub const CARD:       Color32 = Color32::from_rgb(20,  26,  35);
pub const CARD2:      Color32 = Color32::from_rgb(27,  34,  45);
pub const BORDER:     Color32 = Color32::from_rgb(45,  53,  63);
pub const ACCENT:     Color32 = Color32::from_rgb(0,   200, 150);
pub const ACCENT_DIM: Color32 = Color32::from_rgb(0,   90,  68);
pub const TEXT:       Color32 = Color32::from_rgb(228, 235, 242);
pub const MUTED:      Color32 = Color32::from_rgb(130, 140, 152);
pub const DIM:        Color32 = Color32::from_rgb(65,  75,  88);
pub const GREEN:      Color32 = Color32::from_rgb(56,  182, 74);
pub const RED:        Color32 = Color32::from_rgb(245, 78,  70);
pub const YELLOW:     Color32 = Color32::from_rgb(210, 155, 30);
pub const ORANGE:     Color32 = Color32::from_rgb(220, 110, 35);
pub const BLUE:       Color32 = Color32::from_rgb(80,  160, 255);
pub const INPUT_BG:   Color32 = Color32::from_rgb(7,   10,  16);

// ── Initialise ────────────────────────────────────────────────────────────────
pub fn setup(ctx: &egui::Context) {
    let cr6 = CornerRadius::same(6);

    let mut v = egui::Visuals::dark();
    v.panel_fill        = BG;
    v.window_fill       = CARD;
    v.window_stroke     = Stroke::new(1.0, BORDER);
    v.popup_shadow      = egui::Shadow::NONE;
    v.window_shadow     = egui::Shadow::NONE;
    v.extreme_bg_color  = INPUT_BG;
    v.faint_bg_color    = Color32::from_rgb(16, 22, 30);
    v.warn_fg_color     = YELLOW;
    v.error_fg_color    = RED;
    v.hyperlink_color   = ACCENT;
    v.selection.bg_fill = Color32::from_rgb(0, 120, 90);

    {
        let w = &mut v.widgets.noninteractive;
        w.bg_fill      = CARD;
        w.weak_bg_fill = BG;
        w.fg_stroke    = Stroke::new(1.0, MUTED);
        w.bg_stroke    = Stroke::new(1.0, BORDER);
        w.corner_radius = cr6;
    }
    {
        let w = &mut v.widgets.inactive;
        w.bg_fill      = Color32::from_rgb(28, 35, 45);
        w.weak_bg_fill = CARD;
        w.fg_stroke    = Stroke::new(1.0, MUTED);
        w.bg_stroke    = Stroke::new(1.0, BORDER);
        w.corner_radius = cr6;
        w.expansion    = 0.0;
    }
    {
        let w = &mut v.widgets.hovered;
        w.bg_fill      = Color32::from_rgb(34, 43, 56);
        w.weak_bg_fill = Color32::from_rgb(27, 35, 45);
        w.fg_stroke    = Stroke::new(1.0, TEXT);
        w.bg_stroke    = Stroke::new(1.0, ACCENT);
        w.corner_radius = cr6;
    }
    {
        let w = &mut v.widgets.active;
        w.bg_fill      = ACCENT_DIM;
        w.weak_bg_fill = ACCENT_DIM;
        w.fg_stroke    = Stroke::new(1.0, Color32::WHITE);
        w.corner_radius = cr6;
    }
    {
        let w = &mut v.widgets.open;
        w.bg_fill   = Color32::from_rgb(28, 35, 45);
        w.bg_stroke = Stroke::new(1.0, ACCENT);
        w.corner_radius = cr6;
    }

    ctx.set_visuals(v);

    let mut s = (*ctx.style()).clone();
    s.text_styles = [
        (egui::TextStyle::Small,     egui::FontId::proportional(11.0)),
        (egui::TextStyle::Body,      egui::FontId::proportional(13.5)),
        (egui::TextStyle::Button,    egui::FontId::proportional(13.5)),
        (egui::TextStyle::Heading,   egui::FontId::proportional(20.0)),
        (egui::TextStyle::Monospace, egui::FontId::monospace(12.5)),
    ].into();
    s.spacing.item_spacing   = egui::vec2(8.0, 6.0);
    s.spacing.button_padding = egui::vec2(10.0, 6.0);
    s.spacing.scroll.bar_width = 5.0;
    ctx.set_style(s);
}

// ── Frame builders ────────────────────────────────────────────────────────────
pub fn card() -> egui::Frame {
    egui::Frame::new()
        .fill(CARD)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(8u8)
        .inner_margin(Margin::same(14))
}

pub fn card_sm() -> egui::Frame {
    egui::Frame::new()
        .fill(CARD)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(8u8)
        .inner_margin(Margin::same(10))
}

pub fn danger_card() -> egui::Frame {
    egui::Frame::new()
        .fill(Color32::from_rgb(38, 10, 10))
        .stroke(Stroke::new(1.5, RED))
        .corner_radius(8u8)
        .inner_margin(Margin::same(12))
}

pub fn ok_card() -> egui::Frame {
    egui::Frame::new()
        .fill(Color32::from_rgb(9, 30, 18))
        .stroke(Stroke::new(1.0, Color32::from_rgb(32, 85, 50)))
        .corner_radius(8u8)
        .inner_margin(Margin::same(12))
}

pub fn warn_card() -> egui::Frame {
    egui::Frame::new()
        .fill(Color32::from_rgb(32, 25, 5))
        .stroke(Stroke::new(1.0, Color32::from_rgb(100, 75, 10)))
        .corner_radius(8u8)
        .inner_margin(Margin::same(12))
}

// ── Widget helpers ────────────────────────────────────────────────────────────
/// Uppercase section header in muted color
pub fn section_label(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).size(10.5).color(MUTED).strong());
}

/// Large accent heading
pub fn heading(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).size(16.0).color(TEXT).strong());
}

/// Green if non-negative, red otherwise
pub fn pnl_color(non_negative: bool) -> Color32 {
    if non_negative { GREEN } else { RED }
}

/// Coloured pill/badge
pub fn pill(ui: &mut egui::Ui, text: &str, bg: Color32, fg: Color32) {
    egui::Frame::new()
        .fill(bg)
        .stroke(Stroke::new(1.0, fg))
        .corner_radius(10u8)
        .inner_margin(Margin { left: 9, right: 9, top: 3, bottom: 3 })
        .show(ui, |ui| {
            ui.label(egui::RichText::new(text).color(fg).size(11.5).strong());
        });
}

/// Risk progress bar: label | [====   ] used% / limit%
pub fn dd_bar(ui: &mut egui::Ui, label: &str, used_pct: f32, limit_pct: f32) {
    let ratio = (used_pct / limit_pct).clamp(0.0, 1.0);
    let color = if ratio < 0.55 { GREEN } else if ratio < 0.80 { YELLOW } else { RED };
    let warn = if ratio >= 1.0 { "  LIMIT HIT" } else if ratio >= 0.80 { "  WARNING" } else { "" };
    ui.horizontal(|ui| {
        ui.add_sized(
            [145.0, 18.0],
            egui::Label::new(egui::RichText::new(label).color(MUTED).size(12.0)),
        );
        ui.add(egui::ProgressBar::new(ratio).desired_width(150.0).fill(color));
        ui.label(
            egui::RichText::new(format!("{:.2}% / {:.0}%{}", used_pct, limit_pct, warn))
                .color(color)
                .monospace()
                .size(11.5),
        );
    });
}

/// Large value + small sub-label
pub fn big_stat(ui: &mut egui::Ui, value: &str, sublabel: &str, color: Color32) {
    ui.vertical(|ui| {
        ui.label(egui::RichText::new(value).size(22.0).color(color).strong().monospace());
        ui.label(egui::RichText::new(sublabel).size(11.0).color(MUTED));
    });
}
