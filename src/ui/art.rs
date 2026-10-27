//! Painter drawings: mouse silhouette, keyboard outline and key layout.
//! Our own line art (no Razer product images).

use super::theme::*;
use crate::keymap::ROWS;
use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, StrokeKind, pos2, vec2};

fn cubic(p0: Pos2, p1: Pos2, p2: Pos2, p3: Pos2, out: &mut Vec<Pos2>) {
    for i in 1..=10 {
        let t = i as f32 / 10.0;
        let u = 1.0 - t;
        out.push(pos2(
            u * u * u * p0.x + 3.0 * u * u * t * p1.x + 3.0 * u * t * t * p2.x + t * t * t * p3.x,
            u * u * u * p0.y + 3.0 * u * u * t * p1.y + 3.0 * u * t * t * p2.y + t * t * t * p3.y,
        ));
    }
}

/// Mouse body (top-down) in a 100×170 box, mapped into `r` keeping aspect.
fn mouse_body(r: Rect) -> (Vec<Pos2>, impl Fn(f32, f32) -> Pos2) {
    let s = (r.width() / 100.0).min(r.height() / 170.0);
    let o = r.center() - vec2(50.0 * s, 85.0 * s);
    let m = move |x: f32, y: f32| pos2(o.x + x * s, o.y + y * s);
    let mut pts = vec![m(50.0, 4.0)];
    cubic(m(50.0, 4.0), m(24.0, 4.0), m(13.0, 28.0), m(13.0, 60.0), &mut pts);
    pts.push(m(15.0, 122.0));
    cubic(m(15.0, 122.0), m(17.0, 152.0), m(33.0, 166.0), m(50.0, 166.0), &mut pts);
    cubic(m(50.0, 166.0), m(67.0, 166.0), m(83.0, 152.0), m(85.0, 122.0), &mut pts);
    pts.push(m(87.0, 60.0));
    cubic(m(87.0, 60.0), m(87.0, 28.0), m(76.0, 4.0), m(50.0, 4.0), &mut pts);
    pts.pop();
    (pts, m)
}

/// Keep the part of a convex polygon below `y`.
fn clip_below(poly: &[Pos2], y: f32) -> Vec<Pos2> {
    let mut out = Vec::new();
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
        let (ain, bin) = (a.y >= y, b.y >= y);
        if ain {
            out.push(a);
        }
        if ain != bin {
            let t = (y - a.y) / (b.y - a.y);
            out.push(pos2(a.x + (b.x - a.x) * t, y));
        }
    }
    out
}

/// Mouse silhouette. `fill` = battery 0..=100 drawn as a liquid level.
pub fn mouse(p: &Painter, r: Rect, line: Color32, fill: Option<(u8, Color32)>, width: f32) {
    let (body, m) = mouse_body(r);
    let top = body.iter().map(|q| q.y).fold(f32::MAX, f32::min);
    let bot = body.iter().map(|q| q.y).fold(f32::MIN, f32::max);
    if let Some((pct, c)) = fill {
        let level = bot - (bot - top) * pct.min(100) as f32 / 100.0;
        let part = clip_below(&body, level);
        if part.len() >= 3 {
            p.add(Shape::convex_polygon(part, alpha(c, 46), Stroke::NONE));
        }
        if pct > 0 && pct < 92 {
            let xs: Vec<f32> = body.iter().filter(|q| (q.y - level).abs() < (bot - top) * 0.08).map(|q| q.x).collect();
            if let (Some(a), Some(b)) = (xs.iter().cloned().reduce(f32::min), xs.iter().cloned().reduce(f32::max)) {
                p.line_segment([pos2(a + 2.0, level), pos2(b - 2.0, level)], Stroke::new(width, alpha(c, 210)));
            }
        }
    }
    let mut closed = body.clone();
    closed.push(body[0]);
    p.add(Shape::line(closed, Stroke::new(width, line)));
    p.line_segment([m(50.0, 4.0), m(50.0, 64.0)], Stroke::new(width, line));
    let mut seam = Vec::new();
    for i in 0..=12 {
        let t = i as f32 / 12.0;
        let x = 14.0 + 72.0 * t;
        let y = 62.0 + 12.0 * (1.0 - (2.0 * t - 1.0).powi(2));
        seam.push(m(x, y));
    }
    p.add(Shape::line(seam, Stroke::new(width, line)));
    let wheel = Rect::from_min_max(m(45.0, 20.0), m(55.0, 42.0));
    p.rect_stroke(wheel, (5.0 * wheel.width() / 10.0) as u8, Stroke::new(width, line), StrokeKind::Middle);
    for (a, b) in [(70.0, 88.0), (94.0, 112.0)] {
        p.line_segment([m(13.0, a), m(13.0, b)], Stroke::new(width * 1.6, alpha(line, 170)));
    }
}

/// Key rectangles in key units (1.0 = one key), plus total size.
/// Extra space under the F-key row, in key units (measured on the real keyboard).
pub const F_ROW_GAP: f32 = 0.42;

pub fn kb_units() -> (Vec<(u8, &'static str, Rect)>, egui::Vec2) {
    const GAP: f32 = 0.1;
    let mut out = Vec::new();
    let mut maxw: f32 = 0.0;
    for (r, row) in ROWS.iter().enumerate() {
        let y = r as f32 * (1.0 + GAP) + if r > 0 { F_ROW_GAP } else { 0.0 };
        let mut x = 0.0;
        for k in row.iter() {
            let w = k.w + (k.w - 1.0).max(0.0) * GAP;
            if k.id != 0 {
                let h = k.h + (k.h - 1.0) * GAP;
                out.push((k.id, k.label, Rect::from_min_size(pos2(x, y), vec2(w, h))));
                x += w + GAP;
            } else {
                // Spacers are measured in key pitch, so every row's clusters line up.
                x += k.w * (1.0 + GAP);
            }
        }
        maxw = maxw.max(x - GAP);
    }
    let h = ROWS.len() as f32 * (1.0 + GAP) - GAP + F_ROW_GAP;
    (out, vec2(maxw, h))
}

/// Outline keyboard (popover art). `glow` 0..1.
pub fn kb_outline(p: &Painter, r: Rect, col: Color32, glow: f32) {
    let (keys, size) = kb_units();
    let pad = vec2(0.5, 0.9);
    let s = (r.width() / (size.x + pad.x * 2.0)).min(r.height() / (size.y + pad.y + 0.4));
    let body = Rect::from_center_size(r.center(), vec2((size.x + pad.x * 2.0) * s, (size.y + pad.y + 0.4) * s));
    let o = body.min + vec2(pad.x * s, pad.y * s);
    let g = alpha(col, (glow * 70.0) as u8);
    p.rect_stroke(body, 6, Stroke::new(3.0, alpha(col, (glow * 35.0) as u8)), StrokeKind::Middle);
    p.rect_stroke(body, 6, Stroke::new(1.2, col), StrokeKind::Middle);
    for (i, rr) in [0.62, 0.5, 0.5, 0.5].iter().enumerate() {
        let c = pos2(body.max.x - (0.55 + i as f32 * 0.55) * s, body.min.y + 0.42 * s);
        p.circle_stroke(c, rr * 0.36 * s, Stroke::new(0.9, col));
    }
    for (_, _, k) in keys {
        let kr = Rect::from_min_size(o + k.min.to_vec2() * s, k.size() * s);
        if glow > 0.05 {
            p.rect_stroke(kr.expand(1.0), 2, Stroke::new(2.0, g), StrokeKind::Middle);
        }
        p.rect_stroke(kr, 2, Stroke::new(0.8, col), StrokeKind::Middle);
    }
}

/// Small keyboard / mouse glyphs for tabs.
pub fn kb_icon(p: &Painter, r: Rect, c: Color32) {
    let b = Rect::from_center_size(r.center(), vec2(r.width(), r.width() * 0.62));
    p.rect_stroke(b, 2, Stroke::new(1.4, c), StrokeKind::Middle);
    p.line_segment([pos2(b.min.x + b.width() * 0.25, b.max.y - b.height() * 0.3), pos2(b.max.x - b.width() * 0.25, b.max.y - b.height() * 0.3)], Stroke::new(1.4, c));
    for i in 0..4 {
        let x = b.min.x + b.width() * (0.2 + i as f32 * 0.2);
        p.circle_filled(pos2(x, b.min.y + b.height() * 0.35), 0.9, c);
    }
}
pub fn mouse_icon(p: &Painter, r: Rect, c: Color32) {
    let b = Rect::from_center_size(r.center(), vec2(r.height() * 0.6, r.height()));
    p.rect_stroke(b, (b.width() / 2.0) as u8, Stroke::new(1.4, c), StrokeKind::Middle);
    p.line_segment([pos2(b.center().x, b.min.y + 3.0), pos2(b.center().x, b.min.y + b.height() * 0.38)], Stroke::new(1.4, c));
}

pub fn logo(p: &Painter, r: Rect) {
    let t = egui::TextureId::default();
    let _ = t;
    p.rect_filled(r, (r.width() * 0.25) as u8, ACC);
    let c = r.center();
    for i in 0..3 {
        let a = std::f32::consts::TAU * (i as f32 / 3.0) - std::f32::consts::FRAC_PI_2;
        let d = vec2(a.cos(), a.sin());
        p.line_segment([c + d * r.width() * 0.12, c + d * r.width() * 0.34], Stroke::new(r.width() * 0.12, ON_ACC));
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn keys_do_not_overlap() {
        let (keys, size) = super::kb_units();
        for (i, a) in keys.iter().enumerate() {
            assert!(a.2.max.x <= size.x + 0.01 && a.2.max.y <= size.y + 0.01, "{} outside board", a.1);
            for b in &keys[i + 1..] {
                assert!(!a.2.shrink(0.01).intersects(b.2.shrink(0.01)), "{} overlaps {}", a.1, b.1);
            }
        }
    }
}
