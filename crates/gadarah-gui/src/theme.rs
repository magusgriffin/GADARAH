//! Visual design system — colors, frames, helpers

use eframe::egui::{self, Color32, CornerRadius, Margin, Stroke};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeVariant {
    Dark,
    Light,
}

// ── Palette ───────────────────────────────────────────────────────────────────
pub const BG: Color32 = Color32::from_rgb(8, 12, 18);
pub const CARD: Color32 = Color32::from_rgb(16, 21, 30);
pub const CARD2: Color32 = Color32::from_rgb(22, 28, 38);
pub const BORDER: Color32 = Color32::from_rgb(38, 46, 56);
pub const ACCENT: Color32 = Color32::from_rgb(0, 210, 160);
pub const ACCENT_DIM: Color32 = Color32::from_rgb(0, 80, 60);
pub const TEXT: Color32 = Color32::from_rgb(230, 237, 244);
pub const MUTED: Color32 = Color32::from_rgb(120, 132, 148);
pub const DIM: Color32 = Color32::from_rgb(55, 65, 78);
pub const GREEN: Color32 = Color32::from_rgb(46, 196, 82);
pub const RED: Color32 = Color32::from_rgb(248, 68, 58);
pub const YELLOW: Color32 = Color32::from_rgb(220, 170, 30);
pub const ORANGE: Color32 = Color32::from_rgb(230, 118, 35);
pub const BLUE: Color32 = Color32::from_rgb(70, 155, 255);
pub const INPUT_BG: Color32 = Color32::from_rgb(5, 8, 14);
pub const GOLD: Color32 = Color32::from_rgb(255, 195, 0);

// ── Light palette ─────────────────────────────────────────────────────────────
pub const LIGHT_BG: Color32 = Color32::from_rgb(246, 248, 251);
pub const LIGHT_CARD: Color32 = Color32::from_rgb(255, 255, 255);
pub const LIGHT_BORDER: Color32 = Color32::from_rgb(214, 222, 234);
pub const LIGHT_TEXT: Color32 = Color32::from_rgb(20, 28, 40);
pub const LIGHT_MUTED: Color32 = Color32::from_rgb(92, 104, 120);
pub const LIGHT_INPUT_BG: Color32 = Color32::from_rgb(238, 242, 247);

// ── Forge palette (dark-fantasy overlay used by the ornaments layer) ──────────
// These are NOT the main app chrome; they are consumed by widgets/ornaments and
// the demo-banner / mascot layer for the parchment-and-gold aesthetic.
pub const FORGE_BG: Color32 = Color32::from_rgb(18, 8, 6);
pub const FORGE_GOLD: Color32 = Color32::from_rgb(212, 168, 71);
pub const FORGE_GOLD_DIM: Color32 = Color32::from_rgb(139, 111, 46);
pub const FORGE_CRIMSON: Color32 = Color32::from_rgb(139, 26, 26);
pub const FORGE_CHROME: Color32 = Color32::from_rgb(138, 139, 147);
pub const FORGE_OBSIDIAN: Color32 = Color32::from_rgb(30, 31, 46);
pub const FORGE_PARCHMENT: Color32 = Color32::from_rgb(54, 38, 22);

// ── Palette structure ─────────────────────────────────────────────────────────
struct Palette {
    bg: Color32,
    card: Color32,
    border: Color32,
    text: Color32,
    muted: Color32,
    input_bg: Color32,
    faint_bg: Color32,
    selection_bg: Color32,
    warn_fg: Color32,
    hyperlink: Color32,
    // Widget-state fills
    inactive_bg: Color32,
    hovered_bg: Color32,
    hovered_weak_bg: Color32,
    active_bg: Color32,
    active_weak_bg: Color32,
    active_fg: Color32,
    open_bg: Color32,
    // Shared across variants
    base: BaseVariant,
}

/// Which egui base visuals to start from.
enum BaseVariant {
    Dark,
    Light,
}

const DARK: Palette = Palette {
    bg: BG,
    card: CARD,
    border: BORDER,
    text: TEXT,
    muted: MUTED,
    input_bg: INPUT_BG,
    faint_bg: Color32::from_rgb(12, 17, 24),
    selection_bg: Color32::from_rgb(0, 100, 75),
    warn_fg: YELLOW,
    hyperlink: ACCENT,
    inactive_bg: Color32::from_rgb(24, 30, 40),
    hovered_bg: Color32::from_rgb(30, 38, 50),
    hovered_weak_bg: Color32::from_rgb(24, 32, 42),
    active_bg: ACCENT_DIM,
    active_weak_bg: ACCENT_DIM,
    active_fg: Color32::WHITE,
    open_bg: Color32::from_rgb(24, 30, 40),
    base: BaseVariant::Dark,
};

const LIGHT: Palette = Palette {
    bg: LIGHT_BG,
    card: LIGHT_CARD,
    border: LIGHT_BORDER,
    text: LIGHT_TEXT,
    muted: LIGHT_MUTED,
    input_bg: LIGHT_INPUT_BG,
    faint_bg: Color32::from_rgb(232, 238, 246),
    selection_bg: Color32::from_rgb(190, 238, 224),
    warn_fg: Color32::from_rgb(183, 132, 8),
    hyperlink: Color32::from_rgb(20, 140, 110),
    inactive_bg: Color32::from_rgb(232, 238, 246),
    hovered_bg: Color32::from_rgb(220, 230, 244),
    hovered_weak_bg: Color32::from_rgb(236, 242, 250),
    active_bg: Color32::from_rgb(200, 232, 222),
    active_weak_bg: Color32::from_rgb(208, 238, 228),
    active_fg: LIGHT_TEXT,
    open_bg: LIGHT_INPUT_BG,
    base: BaseVariant::Light,
};

// ── Initialise ────────────────────────────────────────────────────────────────
pub fn setup(ctx: &egui::Context) {
    setup_dark(ctx);
}

pub fn setup_variant(ctx: &egui::Context, variant: ThemeVariant) {
    match variant {
        ThemeVariant::Dark => setup_dark(ctx),
        ThemeVariant::Light => setup_light(ctx),
    }
}

pub fn setup_light(ctx: &egui::Context) {
    apply_palette(ctx, &LIGHT);
}

pub fn setup_dark(ctx: &egui::Context) {
    apply_palette(ctx, &DARK);
}

fn apply_palette(ctx: &egui::Context, p: &Palette) {
    let cr8 = CornerRadius::same(8);
    let mut v = match p.base {
        BaseVariant::Dark => egui::Visuals::dark(),
        BaseVariant::Light => egui::Visuals::light(),
    };
    v.panel_fill = p.bg;
    v.window_fill = p.card;
    v.window_stroke = Stroke::new(1.0, p.border);
    v.popup_shadow = egui::Shadow::NONE;
    v.window_shadow = egui::Shadow::NONE;
    v.extreme_bg_color = p.input_bg;
    v.faint_bg_color = p.faint_bg;
    v.warn_fg_color = p.warn_fg;
    v.error_fg_color = RED;
    v.hyperlink_color = p.hyperlink;
    v.selection.bg_fill = p.selection_bg;

    {
        let w = &mut v.widgets.noninteractive;
        w.bg_fill = p.card;
        w.weak_bg_fill = p.bg;
        w.fg_stroke = Stroke::new(1.0, p.muted);
        w.bg_stroke = Stroke::new(1.0, p.border);
        w.corner_radius = cr8;
    }
    {
        let w = &mut v.widgets.inactive;
        w.bg_fill = p.inactive_bg;
        w.weak_bg_fill = p.card;
        // Dark theme uses muted inactive text; light uses full text.
        let inactive_fg = match p.base {
            BaseVariant::Dark => p.muted,
            BaseVariant::Light => p.text,
        };
        w.fg_stroke = Stroke::new(1.0, inactive_fg);
        w.bg_stroke = Stroke::new(1.0, p.border);
        w.corner_radius = cr8;
        w.expansion = 0.0;
    }
    {
        let w = &mut v.widgets.hovered;
        w.bg_fill = p.hovered_bg;
        w.weak_bg_fill = p.hovered_weak_bg;
        w.fg_stroke = Stroke::new(1.0, p.text);
        w.bg_stroke = Stroke::new(1.0, ACCENT);
        w.corner_radius = cr8;
    }
    {
        let w = &mut v.widgets.active;
        w.bg_fill = p.active_bg;
        w.weak_bg_fill = p.active_weak_bg;
        w.fg_stroke = Stroke::new(1.0, p.active_fg);
        w.corner_radius = cr8;
    }
    {
        let w = &mut v.widgets.open;
        w.bg_fill = p.open_bg;
        w.bg_stroke = Stroke::new(1.0, ACCENT);
        w.corner_radius = cr8;
    }

    ctx.set_visuals(v);
    apply_typography(ctx);
}

fn apply_typography(ctx: &egui::Context) {
    let mut s = (*ctx.style()).clone();
    s.text_styles = [
        (egui::TextStyle::Small, egui::FontId::proportional(11.0)),
        (egui::TextStyle::Body, egui::FontId::proportional(13.5)),
        (egui::TextStyle::Button, egui::FontId::proportional(13.5)),
        (egui::TextStyle::Heading, egui::FontId::proportional(20.0)),
        (egui::TextStyle::Monospace, egui::FontId::monospace(13.0)),
    ]
    .into();
    s.spacing.item_spacing = egui::vec2(8.0, 6.0);
    s.spacing.button_padding = egui::vec2(12.0, 7.0);
    s.spacing.scroll.bar_width = 4.0;
    ctx.set_style(s);
}

// ── Frame builders ────────────────────────────────────────────────────────────
pub fn card() -> egui::Frame {
    egui::Frame::new()
        .fill(CARD)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(10u8)
        .inner_margin(Margin::same(16))
}

pub fn card_sm() -> egui::Frame {
    egui::Frame::new()
        .fill(CARD)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(10u8)
        .inner_margin(Margin::same(12))
}

/// Card with a colored left accent border
pub fn accent_card(accent_color: Color32) -> egui::Frame {
    egui::Frame::new()
        .fill(CARD)
        .stroke(Stroke::new(1.0, accent_color.linear_multiply(0.3)))
        .corner_radius(10u8)
        .inner_margin(Margin::same(16))
}

pub fn danger_card() -> egui::Frame {
    egui::Frame::new()
        .fill(Color32::from_rgb(32, 8, 8))
        .stroke(Stroke::new(1.5, RED))
        .corner_radius(10u8)
        .inner_margin(Margin::same(14))
}

pub fn ok_card() -> egui::Frame {
    egui::Frame::new()
        .fill(Color32::from_rgb(6, 26, 16))
        .stroke(Stroke::new(1.0, Color32::from_rgb(25, 75, 45)))
        .corner_radius(10u8)
        .inner_margin(Margin::same(14))
}

pub fn warn_card() -> egui::Frame {
    egui::Frame::new()
        .fill(Color32::from_rgb(28, 22, 4))
        .stroke(Stroke::new(1.0, Color32::from_rgb(90, 68, 8)))
        .corner_radius(10u8)
        .inner_margin(Margin::same(14))
}

// ── Widget helpers ────────────────────────────────────────────────────────────
/// Uppercase section header in muted color
pub fn section_label(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).size(10.0).color(DIM).strong());
}

/// Large accent heading
pub fn heading(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).size(18.0).color(TEXT).strong());
}

/// Breathing-glow modulation for ornaments. Returns `base` multiplied by a
/// gentle 0.70..1.00 envelope driven by `time_secs * freq_hz`. Callers pass the
/// app clock (seconds since start) so every ornament on screen pulses in sync.
pub fn breathing_glow(time_secs: f32, base: Color32, freq_hz: f32) -> Color32 {
    let phase = (time_secs * freq_hz * std::f32::consts::TAU).sin();
    let factor = 0.85 + 0.15 * phase;
    base.linear_multiply(factor.clamp(0.70, 1.00))
}

/// Green if non-negative, red otherwise
pub fn pnl_color(non_negative: bool) -> Color32 {
    if non_negative {
        GREEN
    } else {
        RED
    }
}

/// Coloured pill/badge
pub fn pill(ui: &mut egui::Ui, text: &str, bg: Color32, fg: Color32) {
    egui::Frame::new()
        .fill(bg)
        .stroke(Stroke::new(1.0, fg.linear_multiply(0.6)))
        .corner_radius(12u8)
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

/// Risk progress bar: label | [====   ] used% / limit%
pub fn dd_bar(ui: &mut egui::Ui, label: &str, used_pct: f32, limit_pct: f32) {
    let ratio = (used_pct / limit_pct).clamp(0.0, 1.0);
    let color = if ratio < 0.55 {
        GREEN
    } else if ratio < 0.80 {
        YELLOW
    } else {
        RED
    };
    let warn = if ratio >= 1.0 {
        "  LIMIT HIT"
    } else if ratio >= 0.80 {
        "  WARNING"
    } else {
        ""
    };
    ui.horizontal(|ui| {
        ui.add_sized(
            [120.0, 18.0],
            egui::Label::new(egui::RichText::new(label).color(MUTED).size(11.5)),
        );
        ui.add(
            egui::ProgressBar::new(ratio)
                .desired_width(140.0)
                .fill(color),
        );
        ui.label(
            egui::RichText::new(format!("{:.1}% / {:.0}%{}", used_pct, limit_pct, warn))
                .color(color)
                .monospace()
                .size(11.0),
        );
    });
}

/// Large value + small sub-label
pub fn big_stat(ui: &mut egui::Ui, value: &str, sublabel: &str, color: Color32) {
    ui.vertical(|ui| {
        ui.label(
            egui::RichText::new(value)
                .size(24.0)
                .color(color)
                .strong()
                .monospace(),
        );
        ui.add_space(2.0);
        ui.label(egui::RichText::new(sublabel).size(11.0).color(MUTED));
    });
}

/// Centered empty-state message with icon-like header
pub fn empty_state(ui: &mut egui::Ui, icon: &str, title: &str, subtitle: &str) {
    ui.vertical_centered(|ui| {
        ui.add_space(24.0);
        ui.label(egui::RichText::new(icon).size(28.0).color(DIM));
        ui.add_space(8.0);
        ui.label(egui::RichText::new(title).size(14.0).color(MUTED));
        ui.add_space(4.0);
        ui.label(egui::RichText::new(subtitle).size(12.0).color(DIM));
        ui.add_space(24.0);
    });
}

/// Stat card used in summary rows — compact, clean
pub fn stat_card(ui: &mut egui::Ui, label: &str, value: &str, color: Color32, width: f32) {
    card_sm().show(ui, |ui| {
        ui.set_width(width);
        section_label(ui, label);
        ui.add_space(6.0);
        ui.label(
            egui::RichText::new(value)
                .size(20.0)
                .color(color)
                .strong()
                .monospace(),
        );
    });
}
