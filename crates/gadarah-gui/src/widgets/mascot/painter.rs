//! Procedural dragon painter. No bitmap assets; everything is drawn with
//! egui primitives so the mascot is resolution-independent and HiDPI-safe.
//!
//! Composition:
//!   - Central body (obsidian plate, chrome rim)
//!   - Five necks radiating upward/out in a fan
//!   - One head per neck, with an eye and a subsystem glyph
//!   - Optional glow ring per head driven by mood + app clock
//!
//! All positions are parameterised by the allocated rect; the painter does
//! not leak layout side-effects.

use eframe::egui::{self, Color32, CornerRadius, FontId, Pos2, Stroke};

use crate::theme;
use crate::widgets::mascot::{
    bubble::speech_bubble, MascotMood, MascotState, MascotSubsystem,
};

/// Paint the dragon into the available rect. Allocates its own space.
pub fn paint(ui: &mut egui::Ui, state: &MascotState, time_secs: f32) {
    let desired = egui::vec2(ui.available_width().min(520.0), 260.0);
    let (rect, _) = ui.allocate_exact_size(desired, egui::Sense::hover());
    let painter = ui.painter();

    // ── Backdrop (obsidian slate) ────────────────────────────────────────────
    painter.rect_filled(rect, CornerRadius::same(10), theme::FORGE_OBSIDIAN);
    painter.rect_stroke(
        rect,
        CornerRadius::same(10),
        Stroke::new(1.5, theme::FORGE_CHROME),
        egui::StrokeKind::Inside,
    );

    // Forge glow from the bottom — subtle amber gradient mimic with two rects.
    let glow_top = rect.bottom() - 60.0;
    painter.rect_filled(
        egui::Rect::from_min_max(egui::pos2(rect.left(), glow_top), rect.right_bottom()),
        CornerRadius::same(10),
        theme::FORGE_CRIMSON.linear_multiply(0.25),
    );

    // ── Body (the torso plate) ──────────────────────────────────────────────
    let body_center = egui::pos2(rect.center().x, rect.bottom() - 38.0);
    paint_body(&painter, body_center, time_secs);

    // ── Necks + heads in a fan ──────────────────────────────────────────────
    // Angles in degrees measured from "up", positive = clockwise.
    let head_angles: [(MascotSubsystem, f32); 5] = [
        (MascotSubsystem::Chronicler, -70.0),
        (MascotSubsystem::MarketFeed, -35.0),
        (MascotSubsystem::RiskGate, 0.0),
        (MascotSubsystem::ChallengeClock, 35.0),
        (MascotSubsystem::Oracle, 70.0),
    ];

    let neck_len = 130.0;
    let head_radius = 22.0;

    // Pass 1 — necks.
    for (_, angle_deg) in head_angles {
        let angle_rad = angle_deg.to_radians();
        let dx = angle_rad.sin();
        let dy = -angle_rad.cos(); // up is negative y
        let head_center = egui::pos2(
            body_center.x + dx * neck_len,
            body_center.y + dy * neck_len,
        );
        paint_neck(&painter, body_center, head_center);
    }

    // Pass 2 — heads (on top of necks).
    let mut head_positions: Vec<(MascotSubsystem, Pos2)> = Vec::new();
    for (sub, angle_deg) in head_angles {
        let angle_rad = angle_deg.to_radians();
        let dx = angle_rad.sin();
        let dy = -angle_rad.cos();
        let head_center = egui::pos2(
            body_center.x + dx * neck_len,
            body_center.y + dy * neck_len,
        );
        let mood = state.mood_of(sub);
        paint_head(&painter, head_center, head_radius, sub, mood, time_secs);
        head_positions.push((sub, head_center));
    }

    // Pass 3 — nameplate below body.
    painter.text(
        egui::pos2(body_center.x, rect.bottom() - 10.0),
        egui::Align2::CENTER_CENTER,
        "The Five-Crowned Watcher",
        FontId::proportional(11.0),
        theme::FORGE_GOLD_DIM,
    );

    // ── Bubble (if any) ──────────────────────────────────────────────────────
    if let Some((target, text, tone)) = &state.bubble {
        if let Some((_, center)) = head_positions.iter().find(|(s, _)| s == target) {
            speech_bubble(&painter, rect, *center, head_radius, text, *tone);
        }
    }
}

fn paint_body(painter: &egui::Painter, center: Pos2, time_secs: f32) {
    // Torso — dark plate with chrome rim and a crimson heart glow.
    let w = 88.0;
    let h = 44.0;
    let torso = egui::Rect::from_center_size(center, egui::vec2(w, h));
    painter.rect_filled(torso, CornerRadius::same(8), theme::FORGE_OBSIDIAN);
    painter.rect_stroke(
        torso,
        CornerRadius::same(8),
        Stroke::new(1.5, theme::FORGE_CHROME),
        egui::StrokeKind::Inside,
    );
    // Heart-glow circle
    let heart = theme::breathing_glow(time_secs, theme::FORGE_CRIMSON, 0.5);
    painter.circle_filled(center, 8.0, heart);
    painter.circle_stroke(center, 9.5, Stroke::new(1.0, theme::FORGE_GOLD_DIM));
    // Belly plates (three hash marks)
    for i in -1..=1 {
        let y = center.y + h * 0.25;
        let x = center.x + i as f32 * 14.0;
        painter.line_segment(
            [egui::pos2(x - 4.0, y), egui::pos2(x + 4.0, y)],
            Stroke::new(1.0, theme::FORGE_GOLD_DIM),
        );
    }
}

fn paint_neck(painter: &egui::Painter, from: Pos2, to: Pos2) {
    // Two-tone neck: dark stroke core + chrome highlight.
    painter.line_segment([from, to], Stroke::new(7.0, theme::FORGE_OBSIDIAN));
    painter.line_segment([from, to], Stroke::new(5.0, Color32::from_rgb(46, 48, 60)));
    painter.line_segment(
        [from, to],
        Stroke::new(1.0, theme::FORGE_CHROME.linear_multiply(0.5)),
    );
}

fn paint_head(
    painter: &egui::Painter,
    center: Pos2,
    radius: f32,
    sub: MascotSubsystem,
    mood: MascotMood,
    time_secs: f32,
) {
    let base = sub.color();
    let (amp, tint) = mood.amp_tint();
    let modulated = if amp > 0.0 {
        // Pulse between 1-amp and 1+amp.
        let phase = (time_secs * 0.7 * std::f32::consts::TAU).sin();
        base.linear_multiply((1.0 + amp * phase).clamp(0.3, 1.5) * tint)
    } else {
        base.linear_multiply(tint)
    };

    // Outer glow (only if alarmed/warning)
    if matches!(mood, MascotMood::Warning | MascotMood::Alarmed) {
        painter.circle_filled(center, radius + 5.0, modulated.linear_multiply(0.25));
    }

    // Head capsule
    painter.circle_filled(center, radius, theme::FORGE_OBSIDIAN);
    painter.circle_stroke(center, radius, Stroke::new(2.0, modulated));
    painter.circle_stroke(
        center,
        radius - 3.0,
        Stroke::new(1.0, theme::FORGE_CHROME.linear_multiply(0.5)),
    );

    // Eye (small amber/modulated dot, offset toward body)
    let eye_offset = egui::vec2(0.0, 4.0);
    painter.circle_filled(center + eye_offset, 2.2, modulated);

    // Glyph
    painter.text(
        center - egui::vec2(0.0, 4.0),
        egui::Align2::CENTER_CENTER,
        sub.glyph(),
        FontId::proportional(radius * 0.8),
        modulated,
    );
}
