use super::theme::*;
use egui::{Color32, Response, Sense, Stroke, StrokeKind, Ui, vec2};

fn text_size(ui: &Ui, t: &str, f: egui::FontId) -> egui::Vec2 {
    ui.fonts_mut(|fo| fo.layout_no_wrap(t.to_string(), f, FG).size())
}

/// Rounded chip (popover quick controls).
pub fn chip(ui: &mut Ui, text: &str, on: bool) -> Response {
    let f = if on { font_sb(12.5) } else { font(12.5) };
    let ts = text_size(ui, text, f.clone());
    let (r, resp) = ui.allocate_exact_size(vec2(ts.x + 20.0, 26.0), Sense::click());
    let hov = resp.hovered();
    let (fill, stroke, col) = if on {
        (ACC, ACC, ON_ACC)
    } else {
        (if hov { alpha(FG, 26) } else { alpha(FG, 14) }, alpha(FG, 24), TXT)
    };
    ui.painter().rect(r, 13, fill, Stroke::new(1.0, stroke), StrokeKind::Inside);
    ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, text, f, col);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Segmented control. Returns the clicked index.
pub fn seg(ui: &mut Ui, items: &[&str], selected: Option<usize>, accent: bool) -> Option<usize> {
    let mut clicked = None;
    let f = font(13.0);
    let widths: Vec<f32> = items.iter().map(|t| text_size(ui, t, f.clone()).x + 24.0).collect();
    let total: f32 = widths.iter().sum::<f32>() + 4.0;
    let (r, _) = ui.allocate_exact_size(vec2(total, 32.0), Sense::hover());
    ui.painter().rect(r, 7, SURF, Stroke::new(1.0, BD), StrokeKind::Inside);
    let mut x = r.min.x + 2.0;
    for (i, t) in items.iter().enumerate() {
        let cr = egui::Rect::from_min_size(egui::pos2(x, r.min.y + 2.0), vec2(widths[i], 28.0));
        let resp = ui.interact(cr, ui.scope_id().with(("seg", t, i)), Sense::click());
        let on = selected == Some(i);
        if on {
            ui.painter().rect_filled(cr, 6, if accent { ACC } else { RAISED });
        } else if resp.hovered() {
            ui.painter().rect_filled(cr, 6, alpha(FG, 10));
        }
        let col = if on && accent { ON_ACC } else if on { FG } else { MUT };
        ui.painter().text(cr.center(), egui::Align2::CENTER_CENTER, *t, if on { font_sb(13.0) } else { f.clone() }, col);
        if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            clicked = Some(i);
        }
        x += widths[i];
    }
    clicked
}

pub fn pill(ui: &mut Ui, text: &str, accent: bool) -> Response {
    let ts = text_size(ui, text, font(12.0));
    let (r, resp) = ui.allocate_exact_size(vec2(ts.x + 18.0, 22.0), Sense::hover());
    let (fill, st, col) = if accent { (alpha(ACC, 28), alpha(ACC, 70), ACC2) } else { (SURF, BD, MUT) };
    ui.painter().rect(r, 11, fill, Stroke::new(1.0, st), StrokeKind::Inside);
    ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, text, font(12.0), col);
    resp
}

pub fn switch(ui: &mut Ui, on: &mut bool) -> Response {
    let (r, mut resp) = ui.allocate_exact_size(vec2(38.0, 22.0), Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let t = ui.ctx().animate_bool_with_time(resp.id, *on, 0.12);
    let fill = Color32::from_rgb(0x30, 0x34, 0x3b).lerp_to_gamma(ACC, t);
    ui.painter().rect_filled(r, 11, fill);
    let x = egui::lerp(r.min.x + 11.0..=r.max.x - 11.0, t);
    ui.painter().circle_filled(egui::pos2(x, r.center().y), 8.0, if *on { ON_ACC } else { TXT });
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub fn primary(ui: &mut Ui, text: &str, enabled: bool) -> Response {
    let b = egui::Button::new(sb(text, 13.5).color(if enabled { ON_ACC } else { DIM }))
        .fill(if enabled { ACC } else { SURF })
        .stroke(Stroke::NONE)
        .corner_radius(6)
        .min_size(vec2(0.0, 32.0));
    ui.add_enabled(enabled, b).on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub fn secondary(ui: &mut Ui, text: &str, enabled: bool) -> Response {
    let b = egui::Button::new(egui::RichText::new(text).size(13.5).color(if enabled { FG } else { DIM }))
        .fill(RAISED)
        .stroke(Stroke::new(1.0, Color32::from_rgb(0x3a, 0x3e, 0x46)))
        .corner_radius(6)
        .min_size(vec2(0.0, 32.0));
    ui.add_enabled(enabled, b)
}

/// Thin uppercase outline button (Synapse's "SELECT ALL KEYS").
pub fn outline(ui: &mut Ui, text: &str) -> Response {
    let t = text.to_uppercase();
    let ts = text_size(ui, &t, font(11.5));
    let (r, resp) = ui.allocate_exact_size(vec2(ts.x + 26.0, 28.0), Sense::click());
    let st = if resp.hovered() { Color32::from_rgb(0x8a, 0x90, 0x99) } else { Color32::from_rgb(0x4a, 0x4e, 0x55) };
    ui.painter().rect_stroke(r, 3, Stroke::new(1.0, st), StrokeKind::Inside);
    ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, t, font(11.5), TXT);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub fn link(ui: &mut Ui, text: &str) -> Response {
    let r = ui.add(egui::Label::new(egui::RichText::new(text).color(TXT).underline().size(13.0)).sense(Sense::click()));
    r.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Synapse-style panel: green uppercase title, dark card.
pub fn panel<R>(ui: &mut Ui, title: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::new()
        .fill(PANEL)
        .corner_radius(6)
        .inner_margin(egui::Margin::symmetric(20, 18))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(sb(title.to_uppercase(), 13.0).color(ACC));
            ui.add_space(2.0);
            add(ui)
        })
        .inner
}

pub fn hint(ui: &mut Ui, text: &str) {
    ui.label(egui::RichText::new(text).color(MUT).size(12.5));
}

/// Slider row: label, slider, value.
pub fn slider_row<N: egui::emath::Numeric>(ui: &mut Ui, label: &str, v: &mut N, range: std::ops::RangeInclusive<N>, suffix: &str) -> Response {
    ui.horizontal(|ui| {
        ui.add_sized(vec2(130.0, 22.0), egui::Label::new(egui::RichText::new(label).color(TXT)));
        ui.spacing_mut().slider_width = (ui.available_width() - 90.0).max(80.0);
        ui.add(egui::Slider::new(v, range).suffix(suffix).trailing_fill(true))
    })
    .inner
}
