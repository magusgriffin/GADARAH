//! Speech bubble — parchment rectangle with a pointed tail leading back to
//! the originating head. Tone drives the border color and the tiny wax-seal
//! glyph at the corner.

use eframe::egui::{self, Color32, CornerRadius, FontId, Pos2, Rect, Stroke};

use crate::theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BubbleTone {
    /// Neutral observation — parchment + gold trim.
    Chronicle,
    /// Warning — parchment + crimson trim.
    Warning,
    /// Victory / success — parchment + emerald trim.
    Rite,
    /// Oracle speech — obsidian + violet trim.
    Divination,
}

impl BubbleTone {
    fn bg(self) -> Color32 {
        match self {
            Self::Chronicle | Self::Warning | Self::Rite => theme::FORGE_PARCHMENT,
            Self::Divination => theme::FORGE_OBSIDIAN,
        }
    }

    fn fg(self) -> Color32 {
        match self {
            Self::Chronicle => Color32::from_rgb(232, 220, 190),
            Self::Warning => Color32::from_rgb(250, 210, 190),
            Self::Rite => Color32::from_rgb(220, 240, 210),
            Self::Divination => Color32::from_rgb(220, 210, 240),
        }
    }

    fn border(self) -> Color32 {
        match self {
            Self::Chronicle => theme::FORGE_GOLD,
            Self::Warning => theme::FORGE_CRIMSON,
            Self::Rite => Color32::from_rgb(80, 180, 130),
            Self::Divination => Color32::from_rgb(160, 120, 230),
        }
    }

    fn seal_glyph(self) -> &'static str {
        match self {
            Self::Chronicle => "§",
            Self::Warning => "!",
            Self::Rite => "✦",
            Self::Divination => "✴",
        }
    }
}

/// Paint a speech bubble anchored to `head_center`. The bubble is placed
/// above-and-right of the head by default, clipped to `bounds` so it never
/// escapes the mascot panel.
pub fn speech_bubble(
    painter: &egui::Painter,
    bounds: Rect,
    head_center: Pos2,
    head_radius: f32,
    text: &str,
    tone: BubbleTone,
) {
    let body_font = FontId::proportional(11.5);

    // Measure text — egui's layout returns a Galley we can size from.
    let wrap_width = 220.0_f32;
    let galley = painter.layout(
        text.to_string(),
        body_font.clone(),
        tone.fg(),
        wrap_width,
    );
    let text_size = galley.size();
    let pad = egui::vec2(12.0, 10.0);
    let bubble_size = text_size + pad * 2.0 + egui::vec2(0.0, 14.0); // extra for seal row

    // Prefer above-right; fall back below or above-left to stay in bounds.
    let anchor_right = egui::pos2(
        head_center.x + head_radius + 14.0,
        head_center.y - bubble_size.y - 8.0,
    );
    let mut rect = Rect::from_min_size(anchor_right, bubble_size);
    if !bounds.contains_rect(rect) {
        // Try above-left
        let anchor_left = egui::pos2(
            head_center.x - head_radius - 14.0 - bubble_size.x,
            head_center.y - bubble_size.y - 8.0,
        );
        rect = Rect::from_min_size(anchor_left, bubble_size);
    }
    if !bounds.contains_rect(rect) {
        // Clamp into bounds
        let clamped = bounds.shrink(6.0);
        let min = egui::pos2(
            (rect.min.x).clamp(clamped.min.x, clamped.max.x - bubble_size.x),
            (rect.min.y).clamp(clamped.min.y, clamped.max.y - bubble_size.y),
        );
        rect = Rect::from_min_size(min, bubble_size);
    }

    // ── Body ─────────────────────────────────────────────────────────────────
    painter.rect_filled(rect, CornerRadius::same(6), tone.bg());
    painter.rect_stroke(
        rect,
        CornerRadius::same(6),
        Stroke::new(1.5, tone.border()),
        egui::StrokeKind::Inside,
    );

    // Tail: triangle from bubble edge toward head_center.
    let tail_tip = head_center
        + ((rect.center() - head_center).normalized() * head_radius);
    let edge_point = nearest_edge_point(rect, tail_tip);
    let perp = (rect.center() - head_center).normalized();
    let perp_rot = egui::vec2(-perp.y, perp.x);
    let tail_left = edge_point + perp_rot * 6.0;
    let tail_right = edge_point - perp_rot * 6.0;
    let tri = vec![tail_tip, tail_left, tail_right];
    painter.add(egui::Shape::convex_polygon(
        tri,
        tone.bg(),
        Stroke::new(1.5, tone.border()),
    ));

    // ── Text ─────────────────────────────────────────────────────────────────
    painter.galley(rect.min + pad, galley, tone.fg());

    // ── Wax seal (corner glyph) ──────────────────────────────────────────────
    let seal_center = egui::pos2(rect.right() - 12.0, rect.bottom() - 10.0);
    painter.circle_filled(seal_center, 7.0, tone.border());
    painter.circle_stroke(
        seal_center,
        7.0,
        Stroke::new(1.0, tone.border().linear_multiply(0.5)),
    );
    painter.text(
        seal_center,
        egui::Align2::CENTER_CENTER,
        tone.seal_glyph(),
        FontId::proportional(9.0),
        Color32::from_rgb(18, 10, 8),
    );
}

fn nearest_edge_point(rect: Rect, toward: Pos2) -> Pos2 {
    let cx = toward.x.clamp(rect.min.x, rect.max.x);
    let cy = toward.y.clamp(rect.min.y, rect.max.y);
    // If inside, snap to the closest edge.
    if rect.contains(toward) {
        let dx_min = (toward.x - rect.min.x).abs();
        let dx_max = (rect.max.x - toward.x).abs();
        let dy_min = (toward.y - rect.min.y).abs();
        let dy_max = (rect.max.y - toward.y).abs();
        let m = dx_min.min(dx_max).min(dy_min).min(dy_max);
        if m == dx_min {
            return egui::pos2(rect.min.x, toward.y);
        }
        if m == dx_max {
            return egui::pos2(rect.max.x, toward.y);
        }
        if m == dy_min {
            return egui::pos2(toward.x, rect.min.y);
        }
        return egui::pos2(toward.x, rect.max.y);
    }
    egui::pos2(cx, cy)
}
