//! Decorative primitives: wax seal, etched frame, rune border, parchment
//! backing, stone tablet. Pure painting — no layout side-effects beyond the
//! rect the caller allocates.

use eframe::egui::{self, Color32, CornerRadius, Margin, Stroke};

use crate::theme;

/// Parchment-coloured card frame with a thin gold border. Used behind
/// inscriptions (ACV readout, tome headers).
pub fn parchment_card() -> egui::Frame {
    egui::Frame::new()
        .fill(theme::FORGE_PARCHMENT)
        .stroke(Stroke::new(1.0, theme::FORGE_GOLD_DIM))
        .corner_radius(6u8)
        .inner_margin(Margin::same(14))
}

/// Obsidian slate frame with a chrome border — the mascot's home and any
/// "stone-tablet" inscriptions (EOD debrief, warnings).
pub fn stone_tablet() -> egui::Frame {
    egui::Frame::new()
        .fill(theme::FORGE_OBSIDIAN)
        .stroke(Stroke::new(1.5, theme::FORGE_CHROME))
        .corner_radius(4u8)
        .inner_margin(Margin::same(16))
}

/// Gold-etched card frame — use for high-importance readouts (current ACV,
/// live position total). The stroke is dim by default; pair with
/// [`theme::breathing_glow`] if the caller wants it to pulse.
pub fn etched_frame(border: Color32) -> egui::Frame {
    egui::Frame::new()
        .fill(theme::CARD)
        .stroke(Stroke::new(1.5, border))
        .corner_radius(8u8)
        .inner_margin(Margin::same(14))
}

/// Paints a circular wax seal at `center` with the given radius. `glyph` is a
/// single short string (1–3 chars) stamped in the middle. The seal has a
/// slightly darker inner ring to read as pressed wax rather than a flat disc.
pub fn wax_seal(ui: &mut egui::Ui, center: egui::Pos2, radius: f32, color: Color32, glyph: &str) {
    let painter = ui.painter();
    let outer = color;
    let inner = color.linear_multiply(0.65);
    painter.circle_filled(center, radius, outer);
    painter.circle_stroke(center, radius * 0.82, Stroke::new(1.0, inner));
    painter.text(
        center,
        egui::Align2::CENTER_CENTER,
        glyph,
        egui::FontId::proportional(radius * 0.85),
        Color32::from_rgb(24, 10, 8),
    );
}

/// Paints an inset rune-border around `rect` — four corner marks plus a hair
/// stroke along the edges. Does not draw a fill.
pub fn rune_border(ui: &mut egui::Ui, rect: egui::Rect, color: Color32) {
    let painter = ui.painter();
    let stroke = Stroke::new(1.0, color);
    painter.rect_stroke(rect, CornerRadius::same(2), stroke, egui::StrokeKind::Inside);
    let tick = 6.0_f32.min(rect.width().min(rect.height()) * 0.12);
    let corners = [
        (rect.left_top(), egui::vec2(tick, 0.0), egui::vec2(0.0, tick)),
        (
            rect.right_top(),
            egui::vec2(-tick, 0.0),
            egui::vec2(0.0, tick),
        ),
        (
            rect.left_bottom(),
            egui::vec2(tick, 0.0),
            egui::vec2(0.0, -tick),
        ),
        (
            rect.right_bottom(),
            egui::vec2(-tick, 0.0),
            egui::vec2(0.0, -tick),
        ),
    ];
    let thick = Stroke::new(1.5, color);
    for (p, h, v) in corners {
        painter.line_segment([p, p + h], thick);
        painter.line_segment([p, p + v], thick);
    }
}

/// Fills `rect` with a flat parchment colour. Kept as a function (rather than
/// a constant) so a future pass can swap in a proper texture without touching
/// call sites.
pub fn parchment_bg(ui: &mut egui::Ui, rect: egui::Rect) {
    ui.painter()
        .rect_filled(rect, CornerRadius::same(4), theme::FORGE_PARCHMENT);
}
