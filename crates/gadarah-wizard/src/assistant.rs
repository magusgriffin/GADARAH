//! The Binder — an animated assistant in the spirit of Clippy, reworked for
//! GADARAH's gothic aesthetic. Drawn entirely from egui primitives: a
//! twisted rune-paperclip with a single embedded eye that blinks and
//! follows the wizard's current phase.
//!
//! Animations are frame-based on the app clock — no asset files. This keeps
//! the wizard binary self-contained (no decoder dependencies) and makes the
//! look consistent with the main app's dragon mascot.

use eframe::egui::{self, Color32, CornerRadius, FontId, Pos2, Rect, Stroke};

use crate::theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssistantMood {
    Greeting,
    Explaining,
    Working,
    Triumph,
    Worry,
}

impl AssistantMood {
    fn blink_hz(self) -> f32 {
        match self {
            Self::Greeting => 0.33,
            Self::Explaining => 0.25,
            Self::Working => 0.5,
            Self::Triumph => 0.2,
            Self::Worry => 0.9,
        }
    }

    fn sway_amp(self) -> f32 {
        match self {
            Self::Greeting => 3.0,
            Self::Explaining => 1.5,
            Self::Working => 4.0,
            Self::Triumph => 5.0,
            Self::Worry => 2.0,
        }
    }

    fn eye_color(self) -> Color32 {
        match self {
            Self::Greeting => theme::FORGE_GOLD,
            Self::Explaining => theme::ACCENT,
            Self::Working => theme::YELLOW,
            Self::Triumph => theme::GREEN,
            Self::Worry => theme::RED,
        }
    }
}

/// Render the assistant at the top of `ui.available_rect()` and its speech
/// bubble just below. The assistant does not consume interaction events; it
/// is decorative.
pub fn show(ui: &mut egui::Ui, mood: AssistantMood, time_secs: f32, speech: &str) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 220.0), egui::Sense::hover());
    let painter = ui.painter();

    // Backdrop: obsidian slate with chrome rim.
    painter.rect_filled(rect, CornerRadius::same(10), theme::FORGE_OBSIDIAN);
    painter.rect_stroke(
        rect,
        CornerRadius::same(10),
        Stroke::new(1.5, theme::FORGE_CHROME),
        egui::StrokeKind::Inside,
    );
    // Faint forge glow at the base.
    let glow_top = rect.bottom() - 30.0;
    painter.rect_filled(
        Rect::from_min_max(egui::pos2(rect.left(), glow_top), rect.right_bottom()),
        CornerRadius::same(10),
        theme::FORGE_CRIMSON.linear_multiply(0.2),
    );

    // The Binder sits in the top half, centered.
    let figure_center = egui::pos2(rect.center().x, rect.top() + 72.0);
    let sway = (time_secs * 1.2).sin() * mood.sway_amp();
    let figure_center = egui::pos2(figure_center.x + sway, figure_center.y);
    paint_binder(&painter, figure_center, mood, time_secs);

    // Nameplate
    painter.text(
        egui::pos2(rect.center().x, rect.top() + 148.0),
        egui::Align2::CENTER_CENTER,
        "The Binder",
        FontId::proportional(12.0),
        theme::FORGE_GOLD_DIM,
    );

    // Speech bubble below the figure
    let bubble_anchor = egui::pos2(rect.center().x, rect.top() + 165.0);
    paint_bubble(&painter, rect, bubble_anchor, speech, mood);
}

fn paint_binder(painter: &egui::Painter, center: Pos2, mood: AssistantMood, time_secs: f32) {
    // The body is a twisted paperclip rune — two concentric arcs forming an
    // elongated 'U' that folds back on itself. Drawn as two rounded rects
    // overlaid, with a chrome highlight.

    // Outer ring (the long fold)
    let outer = Rect::from_center_size(center, egui::vec2(56.0, 92.0));
    painter.rect_stroke(
        outer,
        CornerRadius::same(28),
        Stroke::new(3.5, theme::FORGE_CHROME),
        egui::StrokeKind::Inside,
    );

    // Inner fold — the twisted part that gives the paperclip its form
    let inner = Rect::from_center_size(
        egui::pos2(center.x, center.y + 4.0),
        egui::vec2(30.0, 68.0),
    );
    painter.rect_stroke(
        inner,
        CornerRadius::same(15),
        Stroke::new(3.0, theme::FORGE_CHROME.linear_multiply(0.7)),
        egui::StrokeKind::Inside,
    );

    // Gold rune etched on the front
    painter.text(
        egui::pos2(center.x, center.y + 28.0),
        egui::Align2::CENTER_CENTER,
        "§",
        FontId::proportional(14.0),
        theme::FORGE_GOLD,
    );

    // The eye — an oval that blinks periodically.
    let blink_phase = (time_secs * mood.blink_hz() * std::f32::consts::TAU).sin();
    let blinking = blink_phase < -0.92; // brief closed window each cycle
    let eye_center = egui::pos2(center.x, center.y - 22.0);
    let eye_color = theme::breathing_glow(time_secs, mood.eye_color(), 0.4);

    // Eye socket (dark oval)
    painter.circle_filled(eye_center, 12.0, theme::FORGE_BG);
    painter.circle_stroke(
        eye_center,
        12.0,
        Stroke::new(1.5, theme::FORGE_GOLD_DIM),
    );

    if !blinking {
        // Iris
        painter.circle_filled(eye_center, 7.0, eye_color);
        // Pupil — tracks slightly with the sway so it reads as alive.
        let pupil_offset = egui::vec2((time_secs * 0.7).sin() * 1.5, 0.0);
        painter.circle_filled(eye_center + pupil_offset, 3.0, Color32::BLACK);
        // Highlight
        painter.circle_filled(
            eye_center + egui::vec2(2.0, -2.0),
            1.5,
            Color32::from_rgb(240, 240, 240),
        );
    } else {
        // Closed eye — a horizontal gold bar.
        painter.line_segment(
            [
                egui::pos2(eye_center.x - 8.0, eye_center.y),
                egui::pos2(eye_center.x + 8.0, eye_center.y),
            ],
            Stroke::new(2.0, theme::FORGE_GOLD),
        );
    }
}

fn paint_bubble(
    painter: &egui::Painter,
    bounds: Rect,
    anchor: Pos2,
    text: &str,
    mood: AssistantMood,
) {
    if text.is_empty() {
        return;
    }
    let border = match mood {
        AssistantMood::Worry => theme::FORGE_CRIMSON,
        AssistantMood::Triumph => theme::GREEN,
        _ => theme::FORGE_GOLD,
    };

    let font = FontId::proportional(11.5);
    let wrap = (bounds.width() - 48.0).max(200.0);
    let galley = painter.layout(
        text.to_string(),
        font,
        Color32::from_rgb(232, 220, 190),
        wrap,
    );
    let text_size = galley.size();
    let pad = egui::vec2(12.0, 10.0);
    let size = text_size + pad * 2.0;
    let min = egui::pos2(anchor.x - size.x / 2.0, anchor.y + 10.0);
    let rect = Rect::from_min_size(min, size);
    let clamped = bounds.shrink(8.0);
    let rect = clamp_rect(rect, clamped);

    painter.rect_filled(rect, CornerRadius::same(6), theme::FORGE_PARCHMENT);
    painter.rect_stroke(
        rect,
        CornerRadius::same(6),
        Stroke::new(1.5, border),
        egui::StrokeKind::Inside,
    );

    // Tail pointing up to the anchor
    let tail_tip = egui::pos2(rect.center().x, rect.top());
    let tri = vec![
        tail_tip,
        egui::pos2(tail_tip.x - 6.0, tail_tip.y + 6.0),
        egui::pos2(tail_tip.x + 6.0, tail_tip.y + 6.0),
    ];
    painter.add(egui::Shape::convex_polygon(
        tri,
        theme::FORGE_PARCHMENT,
        Stroke::new(1.5, border),
    ));

    painter.galley(rect.min + pad, galley, Color32::from_rgb(232, 220, 190));
}

fn clamp_rect(mut r: Rect, into: Rect) -> Rect {
    if r.left() < into.left() {
        r = r.translate(egui::vec2(into.left() - r.left(), 0.0));
    }
    if r.right() > into.right() {
        r = r.translate(egui::vec2(into.right() - r.right(), 0.0));
    }
    if r.bottom() > into.bottom() {
        r = r.translate(egui::vec2(0.0, into.bottom() - r.bottom()));
    }
    r
}
