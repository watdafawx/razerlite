//! Full window (design "app v2"): device tabs in the title bar, centered sub-tabs,
//! the device drawn in the middle, Synapse-style panels underneath.

use super::theme::*;
use super::widgets::*;
use super::{App, Sub, Tab, UiExit, art, images};
use crate::config::{Macro, MacroStep};
use crate::device::{self as d, Effect, KeyFn};
use crate::keymap;
use crate::state::Job;
use egui::{Align, Align2, Color32, Layout, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use std::collections::HashSet;

pub struct KeyDrag {
    origin: Pos2,
    base: HashSet<u8>,
    remove: bool,
}

const TITLE_H: f32 = 38.0;
const SUB_H: f32 = 48.0;
const PRESETS: [(&str, &str, f32, f32, Option<(f32, f32)>); 4] = [
    ("Typing", "2.0 / 1.5 mm, all keys", 2.0, 1.5, None),
    ("Gaming", "WASD 1.5 / 1.3, rest 2.0 / 1.5", 2.0, 1.5, Some((1.5, 1.3))),
    ("Light touch", "1.5 / 1.2 mm, all keys", 1.5, 1.2, None),
    ("Deep", "3.0 / 2.4 mm, all keys", 3.0, 2.4, None),
];
const SWATCHES: [[u8; 3]; 7] = [[68, 214, 44], [239, 68, 68], [59, 130, 246], [168, 85, 247], [249, 115, 22], [234, 179, 8], [255, 255, 255]];

impl App {
    pub fn full(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let r = ui.max_rect();
        // No OS frame, so draw our own resize zones on the edges and corners.
        let resizing = !ctx.input(|i| i.viewport().maximized.unwrap_or(false)) && resize_edges(&ctx, r);
        let title = Rect::from_min_size(r.min, vec2(r.width(), TITLE_H));
        self.title_bar(ui, title, &ctx, resizing);

        let mut body = r;
        body.min.y += TITLE_H;
        if matches!(self.tab, Tab::Keyboard | Tab::Mouse) {
            let sub = Rect::from_min_size(body.min, vec2(r.width(), SUB_H));
            self.sub_bar(ui, sub);
            body.min.y += SUB_H;
        }
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(body));
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(&mut child, |ui| {
            if self.snap.synapse_running {
                centered(ui, 1000.0, |ui| self.synapse_banner(ui));
            }
            match self.tab {
                Tab::Keyboard => self.keyboard_tab(ui),
                Tab::Mouse => self.mouse_tab(ui),
                Tab::Macros => self.macros_tab(ui),
                Tab::Settings => self.settings_tab(ui, &ctx),
            }
            ui.add_space(24.0);
        });
    }

    // ------------------------------------------------------------ chrome

    fn title_bar(&mut self, ui: &mut Ui, r: Rect, ctx: &egui::Context, resizing: bool) {
        let p = ui.painter().clone();
        p.rect_filled(r, 0, TOP);
        let drag = ui.interact(r, ui.scope_id().with("titlebar"), Sense::click_and_drag());
        if drag.drag_started() && !resizing {
            ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        if drag.double_clicked() {
            let m = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!m));
        }
        let mut x = r.min.x + 14.0;
        art::logo(&p, Rect::from_center_size(pos2(x + 7.0, r.center().y), vec2(14.0, 14.0)));
        x += 22.0;
        let g = p.text(pos2(x, r.center().y), Align2::LEFT_CENTER, "RAZERLITE", font(11.5), MUT);
        x = g.max.x + 14.0;

        let batt = self.snap.mouse.as_ref().map(|m| (m.battery, m.low_batt, m.charging));
        let tabs: [(Tab, &str); 4] = [(Tab::Mouse, "VIPER V3 PRO"), (Tab::Keyboard, "HUNTSMAN V2 ANALOG"), (Tab::Macros, "MACROS"), (Tab::Settings, "SETTINGS")];
        for (t, name) in tabs {
            let galley_w = ui.fonts_mut(|f| f.layout_no_wrap(name.into(), font(11.5), FG).size().x);
            let extra = if t == Tab::Mouse && batt.is_some() { 36.0 } else { 0.0 };
            let w = galley_w + 46.0 + extra;
            let tr = Rect::from_min_size(pos2(x, r.min.y), vec2(w, r.height()));
            let resp = ui.interact(tr, ui.scope_id().with(("tab", name)), Sense::click());
            let on = self.tab == t;
            if on {
                p.rect_filled(tr, 0, SUB);
                p.hline(tr.x_range(), tr.min.y + 1.0, Stroke::new(2.0, ACC));
            } else if resp.hovered() {
                p.rect_filled(tr, 0, alpha(FG, 8));
            }
            let col = if on || resp.hovered() { FG } else { Color32::from_rgb(0x9a, 0xa0, 0xa8) };
            let ir = Rect::from_center_size(pos2(tr.min.x + 18.0, tr.center().y), vec2(14.0, 14.0));
            match t {
                Tab::Mouse => art::mouse_icon(&p, ir, col),
                Tab::Keyboard => art::kb_icon(&p, ir, col),
                Tab::Macros => {
                    p.text(ir.center(), Align2::CENTER_CENTER, "⌁", font(14.0), col);
                }
                Tab::Settings => {
                    p.text(ir.center(), Align2::CENTER_CENTER, "⚙", font(13.0), col);
                }
            }
            p.text(pos2(tr.min.x + 32.0, tr.center().y), Align2::LEFT_CENTER, name, font(11.5), col);
            if let (Tab::Mouse, Some((b, warn, chg))) = (t, batt) {
                p.text(pos2(tr.max.x - 14.0, tr.center().y), Align2::RIGHT_CENTER, format!("{b}%"), font_sb(11.0), battery_color(b, warn, chg));
            }
            p.vline(tr.max.x, tr.y_range(), Stroke::new(1.0, alpha(FG, 8)));
            if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                self.tab = t;
            }
            x += w;
        }

        // window buttons
        let mut bx = r.max.x;
        for (i, glyph) in ["✕", "▢", "—"].iter().enumerate() {
            let br = Rect::from_min_size(pos2(bx - 46.0, r.min.y), vec2(46.0, r.height()));
            let resp = ui.interact(br, ui.scope_id().with(("wb", i)), Sense::click());
            if resp.hovered() {
                p.rect_filled(br, 0, if i == 0 { Color32::from_rgb(0xc4, 0x2b, 0x1c) } else { alpha(FG, 16) });
            }
            p.text(br.center(), Align2::CENTER_CENTER, *glyph, font(if i == 1 { 13.0 } else { 12.0 }), if resp.hovered() { FG } else { MUT });
            if resp.clicked() {
                match i {
                    0 => {
                        let how = if self.cfg.close_to_tray { UiExit::Closed } else { UiExit::Quit };
                        self.close(ctx, how)
                    }
                    1 => {
                        let m = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!m));
                    }
                    _ => ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true)),
                }
            }
            bx -= 46.0;
        }
    }

    fn sub_bar(&mut self, ui: &mut Ui, r: Rect) {
        let p = ui.painter().clone();
        p.rect_filled(r, 0, SUB);
        p.hline(r.x_range(), r.max.y - 0.5, Stroke::new(1.0, Color32::BLACK));
        let mut left = ui.new_child(egui::UiBuilder::new().max_rect(r.shrink2(vec2(14.0, 0.0))).layout(Layout::left_to_right(Align::Center)));
        let profile = match self.tab {
            Tab::Keyboard => self.snap.kb.as_ref().map(|k| k.profile),
            _ => Some(1),
        };
        if let Some(pid) = profile {
            let t = format!("Onboard profile {pid}");
            let tw = left.fonts_mut(|f| f.layout_no_wrap(t.clone(), font(12.5), FG).size().x);
            let (br, _) = left.allocate_exact_size(vec2(tw + 24.0, 28.0), Sense::hover());
            p.rect(br, 4, Color32::from_rgb(0x11, 0x12, 0x13), Stroke::new(1.0, BD), StrokeKind::Inside);
            p.text(pos2(br.min.x + 12.0, br.center().y), Align2::LEFT_CENTER, t, font(12.5), FG);
            pill(&mut left, "Saved on device", true);
        }
        // status, right side
        let mut right = ui.new_child(egui::UiBuilder::new().max_rect(r.shrink2(vec2(14.0, 0.0))).layout(Layout::right_to_left(Align::Center)));
        if self.snap.busy {
            right.add(egui::Spinner::new().size(14.0).color(ACC));
            right.label(RichText::new(&self.snap.status).size(12.0).color(MUT));
        } else if let Some(e) = &self.snap.error {
            right.label(RichText::new(e).size(12.0).color(BAD)).on_hover_text(e);
        } else {
            right.label(RichText::new(&self.snap.status).size(12.0).color(DIM));
        }

        // centered pills
        let items: &[(Sub, &str)] = match self.tab {
            Tab::Keyboard => &[(Sub::Customize, "CUSTOMIZE"), (Sub::Actuation, "ACTUATION"), (Sub::Lighting, "LIGHTING")],
            _ => &[(Sub::Customize, "CUSTOMIZE"), (Sub::Performance, "PERFORMANCE"), (Sub::Power, "POWER")],
        };
        let cur = if self.tab == Tab::Keyboard { self.kb_sub } else { self.mouse_sub };
        let widths: Vec<f32> = items.iter().map(|(_, t)| ui.fonts_mut(|f| f.layout_no_wrap((*t).into(), font_sb(11.5), FG).size().x) + 30.0).collect();
        let total: f32 = widths.iter().sum::<f32>() + 6.0 * (items.len() as f32 - 1.0);
        let mut x = r.center().x - total / 2.0;
        for (i, (s, t)) in items.iter().enumerate() {
            let pr = Rect::from_min_size(pos2(x, r.center().y - 14.0), vec2(widths[i], 28.0));
            let resp = ui.interact(pr, ui.scope_id().with(("pill", t)), Sense::click());
            let on = *s == cur;
            if on {
                p.rect_filled(pr, 14, ACC);
            }
            let col = if on { ON_ACC } else if resp.hovered() { FG } else { Color32::from_rgb(0xb8, 0xbc, 0xc2) };
            p.text(pr.center(), Align2::CENTER_CENTER, *t, if on { font_sb(11.5) } else { font(11.5) }, col);
            if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                if self.tab == Tab::Keyboard {
                    self.kb_sub = *s;
                } else {
                    self.mouse_sub = *s;
                }
            }
            x += widths[i] + 6.0;
        }
    }

    fn synapse_banner(&mut self, ui: &mut Ui) {
        ui.add_space(12.0);
        egui::Frame::new().fill(alpha(WARN, 18)).stroke(Stroke::new(1.0, alpha(WARN, 64))).corner_radius(8).inner_margin(egui::Margin::symmetric(14, 10)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.label(sb("Synapse is running.", 13.0).color(WARN));
                ui.label(
                    RichText::new("It handles your keys and keeps the keyboard busy. Quit it from its tray icon, not Task Manager.")
                        .color(Color32::from_rgb(0xf5, 0xd3, 0x6b))
                        .size(13.0),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if secondary(ui, if self.show_synapse_help { "Hide" } else { "How to" }, true).clicked() {
                        self.show_synapse_help = !self.show_synapse_help;
                    }
                });
            });
            if self.show_synapse_help {
                ui.add_space(6.0);
                for s in [
                    "1. Right-click the Razer icon in the tray → Quit. Your keyboard keeps working.",
                    "2. Don't end Synapse in Task Manager: its driver then swallows every keystroke until Synapse starts again.",
                    "3. To stop it starting with Windows: Task Manager → Startup apps → disable Razer Synapse / Razer App Engine.",
                    "4. Optional, later: uninstall Synapse, Chroma and App Engine, plus leftover \"Razer Control Device\" drivers.",
                ] {
                    ui.label(RichText::new(s).color(TXT).size(12.5));
                }
            }
        });
    }

    // ------------------------------------------------------------ keyboard

    fn keyboard_tab(&mut self, ui: &mut Ui) {
        let Some(kb) = self.snap.kb.clone() else {
            empty(ui, "Huntsman V2 Analog not found", "Plug it in, then open this tab again.");
            return;
        };
        // stage
        let stage_h = 380.0;
        let (stage, _) = ui.allocate_exact_size(vec2(ui.available_width(), stage_h), Sense::hover());
        let p = ui.painter().clone();
        p.rect_filled(stage, 0, Color32::from_rgb(0x1b, 0x1c, 0x1e));
        let lit = self.kb_sub == Sub::Lighting;
        let glow_col = match kb.effect {
            Effect::Off => None,
            Effect::Spectrum | Effect::WaveLeft | Effect::WaveRight | Effect::StarlightRandom | Effect::BreathingRandom => {
                let t = ui.input(|i| i.time) as f32;
                if lit {
                    ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
                }
                Some(egui::ecolor::Hsva::new((t * 0.1).fract(), 0.8, 1.0, 1.0).into())
            }
            _ => Some(Color32::from_rgb(kb.color[0], kb.color[1], kb.color[2])),
        };
        self.draw_keyboard(ui, stage.shrink2(vec2(40.0, 30.0)), &kb, glow_col);

        ui.add_space(8.0);
        centered_row(ui, |ui| {
            if outline(ui, "Select all keys").clicked() {
                self.sel = keymap::all_ids().into_iter().collect();
            }
            if outline(ui, "Deselect all keys").clicked() {
                self.sel.clear();
            }
            if outline(ui, "WASD").clicked() {
                self.sel = keymap::WASD.into_iter().collect();
            }
            pill(ui, &format!("{} selected", self.sel.len()), !self.sel.is_empty());
        });
        ui.add_space(14.0);

        match self.kb_sub {
            Sub::Customize => self.kb_customize(ui, &kb),
            Sub::Lighting => self.kb_lighting(ui, &kb),
            _ => self.kb_actuation(ui, &kb),
        }
    }

    fn draw_keyboard(&mut self, ui: &mut Ui, area: Rect, kb: &crate::state::KbState, glow: Option<Color32>) {
        let (keys, size) = art::kb_units();
        let p = ui.painter().clone();
        let imgs = self.images.clone();

        // Screen rects for every key, plus the clickable body.
        let (body, rects, u, photo): (Rect, Vec<(u8, &str, Rect)>, f32, bool) = if let (Some(base), Some(full), Some(lights)) = (&imgs.kb_base, &imgs.kb, &imgs.kb_glow) {
            let (r, s) = full.fit(area);
            // Lighting: original render for rainbow effects, grey base + tinted light layer otherwise.
            match kb.effect {
                Effect::Spectrum | Effect::WaveLeft | Effect::WaveRight | Effect::StarlightRandom | Effect::BreathingRandom => {
                    full.paint(&p, r, Color32::from_gray(((kb.brightness as f32 / 255.0) * 155.0 + 100.0) as u8))
                }
                Effect::Off => base.paint(&p, r, Color32::WHITE),
                _ => {
                    base.paint(&p, r, Color32::WHITE);
                    if let Some(g) = glow {
                        lights.paint(&p, r, alpha(g, (kb.brightness as f32 * 0.85 + 30.0).min(255.0) as u8));
                    }
                }
            }
            let to_screen = |q: Pos2| r.min + (q - full.bbox.min) * s;
            let rects = keys
                .iter()
                .map(|(id, l, k)| {
                    let px_min = images::KB_ORIGIN + k.min.to_vec2() * images::KB_UNIT;
                    let px_max = images::KB_ORIGIN + k.max.to_vec2() * images::KB_UNIT;
                    (*id, *l, Rect::from_min_max(to_screen(px_min), to_screen(px_max)))
                })
                .collect();
            (r, rects, images::KB_UNIT * s, true)
        } else {
            let pad_t = 0.9;
            let pad = 0.42;
            let u = (area.width() / (size.x + pad * 2.0)).min(area.height() / (size.y + pad_t + pad)).min(44.0);
            let body = Rect::from_center_size(area.center(), vec2((size.x + pad * 2.0) * u, (size.y + pad_t + pad) * u));
            let o = body.min + vec2(pad * u, pad_t * u);
            p.rect_filled(body.translate(vec2(0.0, 10.0)).expand(6.0), 18, Color32::from_black_alpha(90));
            p.rect(body, 14, Color32::from_rgb(0x23, 0x25, 0x29), Stroke::new(1.0, Color32::from_rgb(0x3a, 0x3d, 0x43)), StrokeKind::Inside);
            art::logo(&p, Rect::from_min_size(body.min + vec2(0.45 * u, 0.28 * u), vec2(0.34 * u, 0.34 * u)));
            for i in 0..4 {
                let c = pos2(body.max.x - (0.55 + i as f32 * 0.5) * u, body.min.y + 0.45 * u);
                let rr = if i == 0 { 0.26 * u } else { 0.16 * u };
                p.circle(c, rr, Color32::from_rgb(0x12, 0x13, 0x15), Stroke::new(1.0, Color32::from_rgb(0x48, 0x4b, 0x52)));
            }
            let rects = keys.iter().map(|(id, l, k)| (*id, *l, Rect::from_min_size(o + k.min.to_vec2() * u, k.size() * u))).collect();
            (body, rects, u, false)
        };
        self.kb_scale = u;

        // interaction
        let resp = ui.interact(body, ui.scope_id().with("kbd"), Sense::click_and_drag());
        let pointer = ui.input(|i| i.pointer.interact_pos().or(i.pointer.hover_pos()));
        let key_at = |pt: Pos2| rects.iter().find(|(_, _, r)| r.contains(pt)).map(|x| x.0);
        if resp.drag_started() {
            let origin = ui.input(|i| i.pointer.press_origin()).unwrap_or(pointer.unwrap_or_default());
            let remove = ui.input(|i| i.modifiers.alt);
            let base = if ui.input(|i| i.modifiers.ctrl || i.modifiers.shift || i.modifiers.alt) || self.kb_sub != Sub::Customize { self.sel.clone() } else { HashSet::new() };
            self.drag = Some(KeyDrag { origin, base, remove });
        }
        let mut marquee = None;
        if let (Some(dg), Some(pt)) = (&self.drag, pointer) {
            if resp.dragged() {
                let m = Rect::from_two_pos(dg.origin, pt);
                marquee = Some(m);
                let mut s = dg.base.clone();
                for (id, _, kr) in &rects {
                    if kr.intersects(m) {
                        if dg.remove {
                            s.remove(id);
                        } else {
                            s.insert(*id);
                        }
                    }
                }
                self.sel = s;
            }
        }
        if resp.drag_stopped() {
            self.drag = None;
        }
        if resp.clicked() {
            if let Some(id) = pointer.and_then(key_at) {
                if self.kb_sub == Sub::Customize && !ui.input(|i| i.modifiers.ctrl || i.modifiers.shift) {
                    self.sel.clear();
                    self.sel.insert(id);
                } else if !self.sel.remove(&id) {
                    self.sel.insert(id);
                }
                self.on_key_selected(kb);
            }
        }

        // keys
        let fsz = (u * 0.27).clamp(8.5, 12.0);
        // Only label keys that differ from the usual press point (or are selected) so the photo stays readable.
        let common = {
            let mut c: std::collections::HashMap<u8, usize> = std::collections::HashMap::new();
            for (p, _) in kb.thresholds.values() {
                *c.entry(*p).or_default() += 1;
            }
            c.into_iter().max_by_key(|x| x.1).map(|x| x.0)
        };
        let hovered = if resp.hovered() { pointer.and_then(key_at) } else { None };
        for (id, label, kr) in &rects {
            let kr = if photo { kr.shrink2(vec2(u * 0.05, u * 0.05)) } else { kr.shrink(0.5) };
            let sel = self.sel.contains(id);
            if photo {
                if sel {
                    p.rect(kr, 4, alpha(ACC, 70), Stroke::new(1.6, ACC), StrokeKind::Inside);
                } else if hovered == Some(*id) {
                    p.rect_stroke(kr, 4, Stroke::new(1.2, alpha(FG, 200)), StrokeKind::Inside);
                }
            } else {
                if let Some(g) = glow {
                    let a = (kb.brightness as f32 / 255.0 * 120.0) as u8;
                    p.rect_filled(kr.expand(2.5), 7, alpha(g, a / 3));
                    p.rect_filled(kr.expand(1.0), 6, alpha(g, a / 2));
                }
                let fill = if sel { Color32::from_rgb(0x24, 0x3d, 0x1d) } else { Color32::from_rgb(0x15, 0x16, 0x18) };
                let st = if sel {
                    Stroke::new(1.5, ACC)
                } else if hovered == Some(*id) {
                    Stroke::new(1.0, Color32::from_rgb(0x6a, 0x70, 0x78))
                } else {
                    Stroke::new(1.0, Color32::from_rgb(0x2e, 0x31, 0x36))
                };
                p.rect(kr, 5, fill, st, StrokeKind::Inside);
                p.text(kr.min + vec2(5.0, 3.0), Align2::LEFT_TOP, *label, font(fsz), if sel { FG } else { TXT });
            }
            // value
            let (txt, col) = match self.kb_sub {
                Sub::Customize if *id == keymap::FN_KEY => (String::new(), DIM),
                Sub::Customize => match kb.functions.get(&(*id, self.layer)) {
                    Some(f) => fn_badge(*id, *f).unwrap_or((String::new(), DIM)),
                    None => (String::new(), DIM),
                },
                Sub::Lighting => (String::new(), DIM),
                _ => match kb.thresholds.get(id) {
                    Some(&(pr, _)) if pr != d::KB_DEFAULT && (!photo || sel || Some(pr) != common) => {
                        (format!("{:.1}", d::byte_to_mm(pr)), if sel { Color32::from_rgb(0xbf, 0xf3, 0xb3) } else { ACC2 })
                    }
                    Some(_) if !photo => ("·".into(), DIM),
                    _ => (String::new(), DIM),
                },
            };
            if !txt.is_empty() && u > 22.0 {
                let f = font(fsz * 0.85);
                if photo {
                    let g = p.layout_no_wrap(txt, f, col);
                    let tr = Rect::from_min_size(kr.max - g.size() - vec2(5.0, 3.0), g.size() + vec2(4.0, 1.0));
                    p.rect_filled(tr, 3, Color32::from_black_alpha(185));
                    p.galley(tr.min + vec2(2.0, 0.5), g, col);
                } else {
                    p.text(kr.max - vec2(4.0, 2.0), Align2::RIGHT_BOTTOM, txt, f, col);
                }
            }
        }
        if let Some(m) = marquee {
            p.rect(m, 0, alpha(ACC, 26), Stroke::new(1.0, ACC), StrokeKind::Inside);
        }
        if let (Some(id), None) = (hovered, &self.drag) {
            let tip = match kb.thresholds.get(&id) {
                Some(&(pr, rl)) if pr != d::KB_DEFAULT => format!("{}  ·  ↓ {:.1} mm  ↑ {:.1} mm", keymap::label(id), d::byte_to_mm(pr), d::byte_to_mm(rl)),
                _ => format!("{}  ·  factory default", keymap::label(id)),
            };
            resp.on_hover_text_at_pointer(tip);
        }
    }

    fn on_key_selected(&mut self, kb: &crate::state::KbState) {
        if self.sel.len() == 1 {
            let id = *self.sel.iter().next().unwrap();
            if let Some(&(pr, rl)) = kb.thresholds.get(&id) {
                if pr != d::KB_DEFAULT {
                    self.press_mm = (d::byte_to_mm(pr) * 10.0).round() / 10.0;
                    self.release_mm = (d::byte_to_mm(rl) * 10.0).round() / 10.0;
                }
            }
            if let Some(f) = kb.functions.get(&(id, self.layer)) {
                self.load_remap(id, *f);
            }
        }
    }

    /// Point the Customize controls at what a key currently does.
    fn load_remap(&mut self, id: u8, f: KeyFn) {
        self.remap_turbo = false;
        self.remap_kind = match f {
            KeyFn::Key { hid, mods } if Some(hid) == keymap::default_hid(id) && mods == 0 => RemapKind::Default,
            KeyFn::Key { hid, mods } => {
                self.remap_hid = hid;
                self.remap_mods = mods;
                RemapKind::Keyboard
            }
            KeyFn::Turbo { hid, mods, interval_ms } => {
                self.remap_hid = hid;
                self.remap_mods = mods;
                self.remap_turbo = true;
                self.remap_rate = (1000 / interval_ms.max(1)).clamp(1, 30) as u8;
                RemapKind::Keyboard
            }
            KeyFn::Mouse(b) => {
                self.remap_mouse = b;
                RemapKind::Mouse
            }
            KeyFn::DoubleClick => {
                self.remap_mouse = 0;
                RemapKind::Mouse
            }
            KeyFn::Media(m) => {
                self.remap_media = m;
                RemapKind::Media
            }
            KeyFn::WinShortcut(w) => {
                self.remap_win = w;
                RemapKind::Windows
            }
            KeyFn::Power(x) => {
                self.remap_power = x;
                RemapKind::Power
            }
            KeyFn::Hypershift => RemapKind::Hypershift,
            KeyFn::Disabled => RemapKind::Disable,
            KeyFn::Other(_) => RemapKind::Default,
        };
    }

    /// The function the Customize controls describe (None = what's printed on the key).
    fn remap_target(&self) -> Option<KeyFn> {
        Some(match self.remap_kind {
            RemapKind::Default => return None,
            RemapKind::Keyboard if self.remap_turbo => {
                KeyFn::Turbo { hid: self.remap_hid, mods: self.remap_mods, interval_ms: 1000 / self.remap_rate.max(1) as u16 }
            }
            RemapKind::Keyboard => KeyFn::Key { hid: self.remap_hid, mods: self.remap_mods },
            RemapKind::Mouse if self.remap_mouse == 0 => KeyFn::DoubleClick,
            RemapKind::Mouse => KeyFn::Mouse(self.remap_mouse),
            RemapKind::Media => KeyFn::Media(self.remap_media),
            RemapKind::Windows => KeyFn::WinShortcut(self.remap_win),
            RemapKind::Power => KeyFn::Power(self.remap_power),
            RemapKind::Hypershift => KeyFn::Hypershift,
            RemapKind::Disable => KeyFn::Disabled,
        })
    }

    fn selected_or_all(&self) -> Vec<u8> {
        let mut v: Vec<u8> = if self.sel.is_empty() { keymap::all_ids() } else { self.sel.iter().copied().collect() };
        v.sort();
        v
    }

    fn kb_actuation(&mut self, ui: &mut Ui, kb: &crate::state::KbState) {
        let loaded = kb.keys_loaded;
        two_panels(ui, |ui, ui2| {
{
                panel(ui, "Actuation", |ui| {
                    hint(ui, "Where a key presses and releases. A bigger gap stops double typing.");
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        self.dual_slider(ui);
                        ui.add_space(12.0);
                        ui.vertical(|ui| {
                            ui.add_space(30.0);
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("●").color(ACC));
                                ui.label(RichText::new("Press at").color(TXT));
                                ui.label(sb(format!("{:.1} mm", self.press_mm), 14.0).color(FG));
                            });
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("●").color(INFO));
                                ui.label(RichText::new("Release at").color(TXT));
                                ui.label(sb(format!("{:.1} mm", self.release_mm), 14.0).color(FG));
                            });
                            ui.add_space(8.0);
                            hint(ui, &format!("Release distance {:.1} mm", self.press_mm - self.release_mm));
                            hint(ui, "Drag the green dot for press,\nthe blue dot for release.");
                        });
                    });
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        let n = self.sel.len();
                        let (pb, rb) = (d::mm_to_byte(self.press_mm), d::mm_to_byte(self.release_mm));
                        if ui.add_enabled_ui(loaded && n > 0, |ui| link(ui, &format!("Apply to {n} selected"))).inner.clicked() && n > 0 {
                            self.send(Job::KbThresholds { keys: self.selected_or_all(), press: pb, release: rb });
                        }
                        if ui.add_enabled_ui(loaded, |ui| link(ui, "Apply to all keys")).inner.clicked() {
                            self.send(Job::KbThresholds { keys: keymap::all_ids(), press: pb, release: rb });
                        }
                        if ui.add_enabled_ui(loaded, |ui| link(ui, "Reset to default")).inner.clicked() {
                            self.send(Job::KbThresholds { keys: self.selected_or_all(), press: d::KB_DEFAULT, release: d::KB_DEFAULT });
                        }
                    });
                });
}
{ let ui = ui2;
                panel(ui, "Presets", |ui| {
                    hint(ui, "One click. Writes to the keyboard.");
                    ui.add_space(4.0);
                    for (name, desc, p, r, wasd) in PRESETS {
                        let resp = list_row(ui, name, desc, false);
                        if resp.clicked() && loaded {
                            self.press_mm = p;
                            self.release_mm = r;
                            let all = keymap::all_ids();
                            match wasd {
                                Some((wp, wr)) => {
                                    let rest: Vec<u8> = all.into_iter().filter(|k| !keymap::WASD.contains(k)).collect();
                                    self.send(Job::KbThresholds { keys: rest, press: d::mm_to_byte(p), release: d::mm_to_byte(r) });
                                    self.send(Job::KbThresholds { keys: keymap::WASD.to_vec(), press: d::mm_to_byte(wp), release: d::mm_to_byte(wr) });
                                }
                                None => self.send(Job::KbThresholds { keys: all, press: d::mm_to_byte(p), release: d::mm_to_byte(r) }),
                            }
                        }
                    }
                    ui.add_space(6.0);
                    ui.label(RichText::new("Rapid Trigger and Snap Tap run inside Synapse's software on this keyboard, so they aren't available here.").color(DIM).size(12.0));
                });
}
});
        ui.add_space(14.0);
        two_panels(ui, |ui, ui2| {
{
                panel(ui, "Polling rate", |ui| {
                    hint(ui, "How often the keyboard reports to the PC.");
                    let cur = [1u8, 2, 8].iter().position(|&c| c == kb.polling);
                    if let Some(i) = seg(ui, &["1000 Hz", "500 Hz", "125 Hz"], cur, true) {
                        self.send(Job::KbPolling([1, 2, 8][i]));
                    }
                });
}
{ let ui = ui2;
                panel(ui, "Debounce", |ui| {
                    hint(ui, "3–5 ms release debounce stops chatter.");
                    let a = slider_row(ui, "Press", &mut self.debounce[0], 0..=20, " ms");
                    let b = slider_row(ui, "Release", &mut self.debounce[2], 0..=20, " ms");
                    if [a, b].iter().any(|r| r.drag_stopped() || (r.changed() && !r.dragged())) {
                        self.send(Job::KbDebounce(self.debounce));
                    }
                });
}
});
    }

    /// Vertical 1.5 (top) .. 3.6 mm (bottom) track with press (green) and release (blue) thumbs.
    fn dual_slider(&mut self, ui: &mut Ui) {
        let (r, resp) = ui.allocate_exact_size(vec2(120.0, 220.0), Sense::click_and_drag());
        let p = ui.painter();
        let track_x = r.min.x + 84.0;
        let (top, bot) = (r.min.y + 14.0, r.max.y - 14.0);
        let (lo, hi) = (1.0f32, 3.6f32);
        let y_of = |mm: f32| top + (mm - lo) / (hi - lo) * (bot - top);
        let mm_of = |y: f32| ((lo + ((y - top) / (bot - top)).clamp(0.0, 1.0) * (hi - lo)) * 10.0).round() / 10.0;
        p.line_segment([pos2(track_x, top), pos2(track_x, bot)], Stroke::new(4.0, Color32::from_rgb(0x33, 0x36, 0x3c)));
        p.line_segment([pos2(track_x, y_of(self.release_mm)), pos2(track_x, y_of(self.press_mm))], Stroke::new(4.0, ACC));
        p.text(pos2(track_x + 14.0, top), Align2::LEFT_CENTER, "1.0", font(10.5), DIM);
        p.text(pos2(track_x + 14.0, bot), Align2::LEFT_CENTER, "3.6", font(10.5), DIM);
        p.text(pos2(track_x + 14.0, y_of(1.5)), Align2::LEFT_CENTER, "1.5", font(10.5), DIM);
        if let Some(pt) = resp.interact_pointer_pos() {
            if resp.drag_started() || resp.clicked() || self.slider_grab.is_none() {
                let dp = (pt.y - y_of(self.press_mm)).abs();
                let dr = (pt.y - y_of(self.release_mm)).abs();
                self.slider_grab = Some(dp <= dr);
            }
            let v = mm_of(pt.y);
            if self.slider_grab == Some(true) {
                self.press_mm = v.clamp(1.5, 3.6).max(self.release_mm + 0.1);
            } else {
                self.release_mm = v.clamp(1.0, self.press_mm - 0.1);
            }
        }
        if resp.drag_stopped() || (!resp.dragged() && !resp.is_pointer_button_down_on()) {
            self.slider_grab = None;
        }
        for (mm, col, label) in [(self.release_mm, INFO, "release"), (self.press_mm, ACC, "press")] {
            let c = pos2(track_x, y_of(mm));
            p.circle(c, 9.0, col, Stroke::new(3.0, PANEL));
            let tr = Rect::from_min_size(pos2(r.min.x, c.y - 10.0), vec2(66.0, 20.0));
            p.rect_filled(tr, 3, if label == "press" { RAISED } else { Color32::from_rgb(0x1e, 0x3a, 0x5f) });
            p.text(tr.center(), Align2::CENTER_CENTER, format!("{mm:.1} mm"), font(11.5), if label == "press" { FG } else { Color32::from_rgb(0xbc, 0xd9, 0xff) });
        }
        resp.on_hover_cursor(egui::CursorIcon::ResizeVertical);
    }

    fn kb_customize(&mut self, ui: &mut Ui, kb: &crate::state::KbState) {
        let n = self.sel.len();
        let title = match n {
            0 => "Key".to_string(),
            1 => format!("Key  {}", keymap::label(*self.sel.iter().next().unwrap())),
            _ => format!("{n} keys"),
        };
        two_panels(ui, |ui, ui2| {
            {
                panel(ui, &title, |ui| {
                    hint(ui, if n == 0 { "Click a key above. Ctrl+click or drag for several." } else { "Pick what it does. Saved on the keyboard, works without the app." });
                    ui.add_space(4.0);
                    for k in RemapKind::ALL {
                        if list_row(ui, k.name(), k.desc(), self.remap_kind == k).clicked() {
                            self.remap_kind = k;
                        }
                        if self.remap_kind == k && matches!(k, RemapKind::Keyboard | RemapKind::Mouse | RemapKind::Media | RemapKind::Windows | RemapKind::Power) {
                            ui.add_space(2.0);
                            self.remap_options(ui);
                            ui.add_space(6.0);
                        }
                    }
                    ui.add_space(8.0);
                    let can = n > 0 && kb.keys_loaded;
                    if primary(ui, &format!("Save to {n} key{}", if n == 1 { "" } else { "s" }), can).clicked() {
                        let keys: Vec<u8> = self.selected_or_all();
                        self.send(Job::KbFunction { keys, mode: self.layer, f: self.remap_target() });
                    }
                });
            }
            {
                let ui = ui2;
                panel(ui, "Layer", |ui| {
                    hint(ui, "Hypershift: hold Fn (or a key set to Hypershift) for a second layer.");
                    if let Some(i) = seg(ui, &["Standard", "Hypershift"], Some(self.layer as usize), true) {
                        self.layer = i as u8;
                        self.on_key_selected(kb);
                    }
                    ui.add_space(10.0);
                    let changed: Vec<String> = keymap::all_ids()
                        .into_iter()
                        .filter(|&id| id != keymap::FN_KEY)
                        .filter_map(|id| {
                            let f = kb.functions.get(&(id, self.layer))?;
                            fn_badge(id, *f).map(|_| format!("{} → {}", keymap::label(id), fn_long(*f)))
                        })
                        .collect();
                    ui.label(sb(format!("Changed keys ({})", changed.len()), 13.0).color(FG));
                    if changed.is_empty() {
                        hint(ui, "None. Every key does what's printed on it.");
                    }
                    for s in changed.iter().take(14) {
                        ui.label(RichText::new(s).color(TXT).size(12.5));
                    }
                    if changed.len() > 14 {
                        hint(ui, &format!("…and {} more", changed.len() - 14));
                    }
                });
                ui.add_space(14.0);
                panel(ui, "Gaming mode", |ui| {
                    hint(ui, "Disables the Windows and Menu keys on the keyboard itself.");
                    let on = [127u8, 129].iter().all(|k| matches!(kb.functions.get(&(*k, 0)), Some(KeyFn::Disabled)));
                    let mut v = on;
                    ui.horizontal(|ui| {
                        if switch(ui, &mut v).changed() && kb.keys_loaded {
                            self.send(Job::KbFunction { keys: vec![127, 129], mode: 0, f: if v { Some(KeyFn::Disabled) } else { None } });
                        }
                        ui.label(RichText::new(if on { "On: Win and Menu do nothing" } else { "Off" }).color(TXT));
                    });
                });
                ui.add_space(14.0);
                panel(ui, "Not on the keyboard", |ui| {
                    hint(ui, "Synapse does these in software through its driver, so they need RazerLite running in the background. Planned:");
                    ui.label(RichText::new("Controller / joystick (gamepad emulation) · Launch program · Macros · Switch lighting effect · Inter-device").color(DIM).size(12.5));
                });
            }
        });
    }

    fn remap_options(&mut self, ui: &mut Ui) {
        ui.indent("remap_opts", |ui| match self.remap_kind {
            RemapKind::Keyboard => {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("Sends").color(TXT));
                    egui::ComboBox::from_id_salt("remap_key").selected_text(keymap::hid_name(self.remap_hid)).height(320.0).show_ui(ui, |ui| {
                        for id in keymap::all_ids() {
                            if let Some(h) = keymap::default_hid(id) {
                                ui.selectable_value(&mut self.remap_hid, h, keymap::label(id));
                            }
                        }
                    });
                    for (bit, name) in [(1u8, "Ctrl"), (2, "Shift"), (4, "Alt"), (8, "Win")] {
                        let mut on = self.remap_mods & bit != 0;
                        if ui.checkbox(&mut on, name).changed() {
                            self.remap_mods ^= bit;
                        }
                    }
                });
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.remap_turbo, "Turbo (repeat while held)");
                    if self.remap_turbo {
                        ui.add(egui::Slider::new(&mut self.remap_rate, 1..=30).suffix(" /s"));
                    }
                });
            }
            RemapKind::Mouse => {
                ui.horizontal_wrapped(|ui| {
                    for (name, code) in d::MOUSE_BUTTONS.iter().copied().chain(std::iter::once(("Double click", 0u8))) {
                        if chip(ui, name, self.remap_mouse == code).clicked() {
                            self.remap_mouse = code;
                        }
                    }
                });
            }
            RemapKind::Media => {
                ui.horizontal_wrapped(|ui| {
                    for (name, code) in d::MEDIA_KEYS {
                        if chip(ui, name, self.remap_media == code).clicked() {
                            self.remap_media = code;
                        }
                    }
                });
            }
            RemapKind::Windows => {
                ui.horizontal_wrapped(|ui| {
                    for (name, code) in d::WIN_SHORTCUTS {
                        if chip(ui, name, self.remap_win == code).clicked() {
                            self.remap_win = code;
                        }
                    }
                });
            }
            RemapKind::Power => {
                ui.horizontal_wrapped(|ui| {
                    for (name, code) in d::POWER_KEYS {
                        if chip(ui, name, self.remap_power == code).clicked() {
                            self.remap_power = code;
                        }
                    }
                });
            }
            _ => {}
        });
    }

    fn kb_lighting(&mut self, ui: &mut Ui, kb: &crate::state::KbState) {
        two_panels(ui, |ui, ui2| {
{
                panel(ui, "Effect", |ui| {
                    hint(ui, "Runs on the keyboard itself.");
                    ui.add_space(4.0);
                    for e in Effect::ALL {
                        if list_row(ui, e.name(), "", kb.effect == e).clicked() {
                            self.send(Job::KbEffect(e, self.light_color, self.light_speed));
                        }
                    }
                });
}
{ let ui = ui2;
                panel(ui, "Color & brightness", |ui| {
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        for c in SWATCHES {
                            let (r, resp) = ui.allocate_exact_size(vec2(28.0, 28.0), Sense::click());
                            ui.painter().rect_filled(r, 6, Color32::from_rgb(c[0], c[1], c[2]));
                            if self.light_color == c {
                                ui.painter().rect_stroke(r.expand(1.5), 7, Stroke::new(2.0, FG), StrokeKind::Outside);
                            }
                            if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                                self.light_color = c;
                                if kb.effect.uses_color() {
                                    self.send(Job::KbEffect(kb.effect, c, self.light_speed));
                                }
                            }
                        }
                        if ui.color_edit_button_srgb(&mut self.light_color).changed() && kb.effect.uses_color() {
                            self.send(Job::KbEffect(kb.effect, self.light_color, self.light_speed));
                        }
                    });
                    ui.add_space(8.0);
                    let r = slider_row(ui, "Brightness", &mut self.bright, 0..=255, "");
                    if r.drag_stopped() || (r.changed() && !r.dragged()) {
                        self.send(Job::KbBrightness(self.bright));
                    }
                    if kb.effect.uses_speed() {
                        ui.horizontal(|ui| {
                            ui.add_sized(vec2(130.0, 22.0), egui::Label::new(RichText::new("Speed").color(TXT)));
                            if let Some(i) = seg(ui, &["Fast", "Medium", "Slow"], Some(self.light_speed as usize - 1), false) {
                                self.light_speed = i as u8 + 1;
                                self.send(Job::KbEffect(kb.effect, self.light_color, self.light_speed));
                            }
                        });
                    }
                });
}
});
    }

    // ------------------------------------------------------------ mouse

    fn mouse_tab(&mut self, ui: &mut Ui) {
        let Some(m) = self.snap.mouse.clone() else {
            empty(ui, "Viper V3 Pro not found", "Plug in the dongle or cable, then open this tab again.");
            return;
        };
        let (stage, _) = ui.allocate_exact_size(vec2(ui.available_width(), 300.0), Sense::hover());
        let p = ui.painter().clone();
        p.rect_filled(stage, 0, Color32::from_rgb(0x1b, 0x1c, 0x1e));
        let mr = self.paint_mouse(&p, Rect::from_center_size(stage.center() - vec2(0.0, 8.0), vec2(170.0, 250.0)), m.battery, battery_color(m.battery, m.low_batt, m.charging));
        let labels_l = [("Left click", 0.22), ("Side forward", 0.47), ("Side back", 0.6)];
        let labels_r = [("Right click", 0.22), ("Wheel", 0.17), ("DPI (bottom)", 0.85)];
        for (t, f) in labels_l {
            let y = mr.min.y + mr.height() * f;
            p.line_segment([pos2(mr.min.x - 70.0, y), pos2(mr.min.x + 18.0, y)], Stroke::new(1.0, Color32::from_rgb(0x55, 0x58, 0x5e)));
            p.text(pos2(mr.min.x - 78.0, y), Align2::RIGHT_CENTER, t, font(12.0), TXT);
        }
        for (i, (t, f)) in labels_r.iter().enumerate() {
            let y = mr.min.y + mr.height() * f + if i == 1 { -14.0 } else { 0.0 };
            p.line_segment([pos2(mr.max.x - 18.0 - if i == 1 { 52.0 } else { 0.0 }, y), pos2(mr.max.x + 70.0, y)], Stroke::new(1.0, Color32::from_rgb(0x55, 0x58, 0x5e)));
            p.text(pos2(mr.max.x + 78.0, y), Align2::LEFT_CENTER, *t, font(12.0), TXT);
        }
        p.text(pos2(stage.center().x, stage.max.y - 16.0), Align2::CENTER_CENTER, format!("{}%{}  ·  firmware {}", m.battery, if m.charging { " charging" } else { "" }, m.fw), font(12.0), MUT);
        ui.add_space(16.0);

        match self.mouse_sub {
            Sub::Power => self.mouse_power(ui, &m),
            Sub::Customize => {
                centered(ui, 1000.0, |ui| {
                    panel(ui, "Buttons", |ui| {
                        hint(ui, "Button remapping is next: the mouse uses a different command than the keyboard, still to be decoded from Synapse.");
                    });
                });
            }
            _ => self.mouse_perf(ui, &m),
        }
    }

    fn mouse_perf(&mut self, ui: &mut Ui, m: &crate::state::MouseState) {
        let mut st = self.stages_dirty.clone().unwrap_or_else(|| m.stages.clone());
        let mut changed = false;
        two_panels(ui, |ui, ui2| {
{
                panel(ui, "Sensitivity", |ui| {
                    hint(ui, "The DPI button cycles these. Click a stage to edit, double-click to make it active.");
                    ui.add_space(4.0);
                    ui.horizontal_wrapped(|ui| {
                        let colors = [BAD, WARN, ACC, INFO, Color32::from_rgb(0xa8, 0x55, 0xf7)];
                        for i in 0..st.stages.len() {
                            let (r, resp) = ui.allocate_exact_size(vec2(84.0, 64.0), Sense::click());
                            let active = st.active as usize == i + 1;
                            let editing = self.stage_edit == i;
                            ui.painter().rect(r, 8, if active { alpha(ACC, 28) } else { SURF }, Stroke::new(if editing { 2.0 } else { 1.0 }, if active || editing { ACC } else { BD }), StrokeKind::Inside);
                            ui.painter().text(r.min + vec2(10.0, 8.0), Align2::LEFT_TOP, format!("Stage {}", i + 1), font(11.0), DIM);
                            ui.painter().text(r.min + vec2(10.0, 26.0), Align2::LEFT_TOP, st.stages[i].0.to_string(), font_sb(19.0), FG);
                            ui.painter().circle_filled(pos2(r.max.x - 12.0, r.min.y + 12.0), 4.5, colors[i % 5]);
                            if resp.clicked() {
                                self.stage_edit = i;
                            }
                            if resp.double_clicked() {
                                st.active = i as u8 + 1;
                                changed = true;
                            }
                        }
                        if st.stages.len() < 5 && ui.add_sized(vec2(44.0, 64.0), egui::Button::new(RichText::new("+").size(20.0).color(DIM)).fill(Color32::TRANSPARENT).stroke(Stroke::new(1.0, BD))).clicked() {
                            let v = (st.stages.last().map(|s| s.0).unwrap_or(800) * 2).min(35000);
                            st.stages.push((v, v));
                            self.stage_edit = st.stages.len() - 1;
                            changed = true;
                        }
                    });
                    ui.add_space(6.0);
                    let i = self.stage_edit.min(st.stages.len().saturating_sub(1));
                    if i < st.stages.len() {
                        let mut v = st.stages[i].0;
                        let r = slider_row(ui, &format!("Stage {}", i + 1), &mut v, 100..=6400, " DPI");
                        if r.changed() {
                            st.stages[i] = (v / 50 * 50, v / 50 * 50);
                            changed = true;
                        }
                        let mut remove_stage = false;
                        ui.horizontal(|ui| {
                            ui.add_space(138.0);
                            let mut exact = st.stages[i].0;
                            if ui.add(egui::DragValue::new(&mut exact).range(100..=35000).speed(10).suffix(" DPI")).changed() {
                                st.stages[i] = (exact, exact);
                                changed = true;
                            }
                            if st.stages.len() > 1 && ui.button("Remove stage").clicked() {
                                remove_stage = true;
                            }
                        });
                        if remove_stage {
                            st.stages.remove(i);
                            st.active = st.active.min(st.stages.len() as u8).max(1);
                            self.stage_edit = 0;
                            changed = true;
                        }
                    }
                    ui.add_space(6.0);
                    let dirty = self.stages_dirty.is_some() || changed;
                    ui.horizontal(|ui| {
                        if primary(ui, "Save DPI", dirty).clicked() {
                            self.send(Job::MouseStages(st.clone()));
                            self.stages_dirty = None;
                            changed = false;
                        }
                        if dirty && secondary(ui, "Undo", true).clicked() {
                            self.stages_dirty = None;
                            changed = false;
                            st = m.stages.clone();
                        }
                    });
                });
}
{ let ui = ui2;
                panel(ui, "Polling rate", |ui| {
                    hint(ui, "4000 / 8000 Hz need the HyperPolling dongle and use more battery.");
                    let names = ["125", "250", "500", "1000", "2000", "4000", "8000"];
                    let codes = [0x40u8, 0x20, 0x10, 0x08, 0x04, 0x02, 0x01];
                    if let Some(i) = seg(ui, &names, codes.iter().position(|&c| c == m.polling), true) {
                        self.send(Job::MousePolling(codes[i]));
                    }
                    ui.label(RichText::new("Hz").color(DIM).size(12.0));
                });
}
});
        if changed {
            self.stages_dirty = Some(st);
        }
    }

    fn mouse_power(&mut self, ui: &mut Ui, m: &crate::state::MouseState) {
        two_panels(ui, |ui, ui2| {
{
                panel(ui, "Battery", |ui| {
                    ui.label(sb(format!("{}%", m.battery), 34.0).color(battery_color(m.battery, m.low_batt, m.charging)));
                    hint(ui, if m.charging { "Charging" } else { "On battery" });
                });
}
{ let ui = ui2;
                panel(ui, "Power saving", |ui| {
                    let a = slider_row(ui, "Sleep after", &mut self.sleep_min, 1..=15, " min");
                    if a.drag_stopped() || (a.changed() && !a.dragged()) {
                        self.send(Job::MouseSleep(self.sleep_min * 60));
                    }
                    let b = slider_row(ui, "Low battery at", &mut self.low_batt, 5..=50, "%");
                    if b.drag_stopped() || (b.changed() && !b.dragged()) {
                        self.send(Job::MouseLowBatt(self.low_batt));
                    }
                    hint(ui, "RazerLite shows a Windows notification at this level.");
                });
}
});
    }

    // ------------------------------------------------------------ macros

    fn macros_tab(&mut self, ui: &mut Ui) {
        // recording: capture key events
        if let Some(_start) = self.recording {
            let evs: Vec<(egui::Key, bool)> = ui.input(|i| i.events.iter().filter_map(|e| if let egui::Event::Key { key, pressed, repeat: false, .. } = e { Some((*key, *pressed)) } else { None }).collect());
            if !evs.is_empty() {
                if let Some(m) = self.cfg.macros.get_mut(self.macro_sel) {
                    for (k, down) in evs {
                        let now = std::time::Instant::now();
                        if let Some(last) = self.rec_last {
                            let ms = now.duration_since(last).as_millis() as u32;
                            if ms > 0 {
                                m.steps.push(MacroStep::Delay(ms));
                            }
                        }
                        self.rec_last = Some(now);
                        let name = k.name().to_string();
                        m.steps.push(if down { MacroStep::Down(name) } else { MacroStep::Up(name) });
                    }
                }
            }
            ui.ctx().request_repaint();
        }
        ui.add_space(16.0);
        let mut save = false;
        let mut delete = false;
        centered(ui, 1000.0, |ui| {
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(270.0);
                    panel(ui, "Macros", |ui| {
                    for (i, m) in self.cfg.macros.iter().enumerate() {
                        let n = m.steps.iter().filter(|s| matches!(s, MacroStep::Down(_))).count();
                        if list_row(ui, &m.name, &format!("{n} keys"), self.macro_sel == i).clicked() {
                            self.macro_sel = i;
                            self.recording = None;
                        }
                    }
                    if self.cfg.macros.is_empty() {
                        hint(ui, "No macros yet.");
                    }
                    ui.add_space(6.0);
                    if secondary(ui, "+ New macro", true).clicked() {
                        let n = self.cfg.macros.len() + 1;
                        self.cfg.macros.push(Macro { name: format!("Macro {n}"), steps: Vec::new(), delay_mode: 0 });
                        self.macro_sel = self.cfg.macros.len() - 1;
                        self.cfg.save();
                    }
                    });
                });
                ui.vertical(|ui| {
                    ui.set_width(ui.available_width());
                    self.macro_editor(ui, &mut save, &mut delete);
                });
            });
        });
        let sel = self.macro_sel;
        if delete && sel < self.cfg.macros.len() {
            self.cfg.macros.remove(sel);
            self.macro_sel = 0;
            self.recording = None;
            save = true;
        }
        if save {
            self.cfg.save();
        }
    }

    fn macro_editor(&mut self, ui: &mut Ui, save: &mut bool, delete: &mut bool) {
        let sel = self.macro_sel;
        if self.cfg.macros.get(sel).is_none() {
            panel(ui, "Editor", |ui| hint(ui, "Create a macro on the left, then record it."));
            return;
        }
        panel(ui, "Editor", |ui| {
                let recording = self.recording.is_some();
                ui.horizontal(|ui| {
                    let m = &mut self.cfg.macros[sel];
                    ui.add(egui::TextEdit::singleline(&mut m.name).desired_width(220.0));
                    let (r, resp) = ui.allocate_exact_size(vec2(34.0, 34.0), Sense::click());
                    ui.painter().circle(r.center(), 16.0, RAISED, Stroke::new(2.0, BAD));
                    if recording {
                        ui.painter().rect_filled(Rect::from_center_size(r.center(), vec2(11.0, 11.0)), 2, BAD);
                    } else {
                        ui.painter().circle_filled(r.center(), 6.0, BAD);
                    }
                    if resp.on_hover_text("Record keys").clicked() {
                        if recording {
                            self.recording = None;
                            *save = true;
                        } else {
                            m.steps.clear();
                            self.recording = Some(std::time::Instant::now());
                            self.rec_last = None;
                        }
                    }
                    ui.label(RichText::new(if recording { "Recording… press keys, click ■ to stop" } else { "Record" }).color(if recording { BAD } else { TXT }));
                });
                ui.horizontal(|ui| {
                    let m = &mut self.cfg.macros[sel];
                    ui.label(RichText::new("Delays").color(MUT));
                    if let Some(i) = seg(ui, &["Recorded", "Fixed 20 ms", "None"], Some(m.delay_mode as usize), false) {
                        m.delay_mode = i as u8;
                        *save = true;
                    }
                });
                ui.add_space(8.0);
                let m = &mut self.cfg.macros[sel];
                let mut remove = None;
                egui::Frame::new().fill(Color32::from_rgb(0x12, 0x13, 0x15)).stroke(Stroke::new(1.0, BD)).corner_radius(6).inner_margin(8).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    egui::ScrollArea::vertical().max_height(320.0).min_scrolled_height(220.0).auto_shrink([false, true]).show(ui, |ui| {
                        if m.steps.is_empty() {
                            hint(ui, "Empty. Click record and press some keys.");
                        }
                        for (i, s) in m.steps.iter().enumerate() {
                            ui.horizontal(|ui| {
                                ui.add_sized(vec2(30.0, 18.0), egui::Label::new(RichText::new(format!("{}", i + 1)).color(DIM).size(12.0)));
                                match s {
                                    MacroStep::Down(k) => {
                                        ui.label(RichText::new("↓").color(ACC2));
                                        ui.label(RichText::new(k).color(FG));
                                    }
                                    MacroStep::Up(k) => {
                                        ui.label(RichText::new("↑").color(INFO));
                                        ui.label(RichText::new(k).color(FG));
                                    }
                                    MacroStep::Delay(ms) => {
                                        ui.label(RichText::new("⏱").color(DIM));
                                        ui.label(RichText::new(format!("{ms} ms")).color(MUT).italics());
                                    }
                                }
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if ui.small_button("✕").clicked() {
                                        remove = Some(i);
                                    }
                                });
                            });
                        }
                    });
                });
                if let Some(i) = remove {
                    m.steps.remove(i);
                    *save = true;
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if secondary(ui, "Insert 50 ms delay", true).clicked() {
                        m.steps.push(MacroStep::Delay(50));
                        *save = true;
                    }
                    if secondary(ui, "Delete macro", true).clicked() {
                        *delete = true;
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        primary(ui, "Save to keyboard", false).on_disabled_hover_text("Needs Synapse's macro-memory commands decoded and a test with Synapse removed. Coming next.");
                    });
                });
                ui.add_space(4.0);
                ui.label(RichText::new("Macros are saved in RazerLite for now. Writing them to the keyboard's memory (so they work without the app) is the next protocol step.").color(DIM).size(12.0));
        });
    }

    // ------------------------------------------------------------ settings

    fn settings_tab(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        ui.add_space(20.0);
        centered(ui, 760.0, |ui| {
            panel(ui, "App", |ui| {
                if setting(ui, "Start with Windows", "Starts hidden in the tray when you sign in.", &mut self.autostart) {
                    let _ = crate::config::set_autostart(self.autostart);
                    crate::tray::set_autostart_check(self.autostart);
                }
                if setting(ui, "Close button hides to tray", "Quit from the tray menu. Off = closing quits.", &mut self.cfg.close_to_tray) {
                    self.cfg.save();
                }
                if setting(ui, "Low-battery notification", "Windows notification at the mouse's warning level.", &mut self.cfg.low_battery_toast) {
                    self.cfg.save();
                }
                ui.add_space(4.0);
                hint(ui, "Tray: one click opens quick controls, double-click opens this window.");
            });
            ui.add_space(12.0);
            panel(ui, "Devices", |ui| {
                let s = &self.snap;
                match &s.kb {
                    Some(k) => ui.label(RichText::new(format!("Huntsman V2 Analog · firmware {} · onboard profile {}", k.fw, k.profile)).color(TXT)),
                    None => ui.label(RichText::new("Huntsman V2 Analog · not connected").color(DIM)),
                };
                match &s.mouse {
                    Some(m) => ui.label(RichText::new(format!("Viper V3 Pro · firmware {} · {}%", m.fw, m.battery)).color(TXT)),
                    None => ui.label(RichText::new("Viper V3 Pro · not connected").color(DIM)),
                };
                if s.demo {
                    ui.label(RichText::new("Demo mode: fake devices, nothing is written.").color(WARN));
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if secondary(ui, "Reload devices", !s.busy).clicked() {
                        self.send(Job::Refresh);
                    }
                    if secondary(ui, "Quit RazerLite", true).clicked() {
                        self.close(ctx, UiExit::Quit);
                    }
                });
            });
            ui.add_space(12.0);
            ui.label(RichText::new(format!("RazerLite {} · settings are stored on the devices, not in the cloud", env!("CARGO_PKG_VERSION"))).color(DIM).size(12.0));
        });
    }
}

// ------------------------------------------------------------ layout helpers

/// Resize handles for the undecorated window: 6 px edges, 14 px corners.
/// Returns true while the pointer is over one (so the title bar doesn't start a move).
fn resize_edges(ctx: &egui::Context, r: Rect) -> bool {
    use egui::{CursorIcon as C, ResizeDirection as D};
    let Some(p) = ctx.input(|i| i.pointer.hover_pos()) else { return false };
    let (e, c) = (6.0, 14.0);
    let (l, rt, t, b) = (p.x - r.min.x, r.max.x - p.x, p.y - r.min.y, r.max.y - p.y);
    let dir = match () {
        _ if l < c && t < c => Some((D::NorthWest, C::ResizeNorthWest)),
        _ if rt < c && t < e => Some((D::NorthEast, C::ResizeNorthEast)),
        _ if l < c && b < c => Some((D::SouthWest, C::ResizeSouthWest)),
        _ if rt < c && b < c => Some((D::SouthEast, C::ResizeSouthEast)),
        _ if l < e => Some((D::West, C::ResizeWest)),
        _ if rt < e => Some((D::East, C::ResizeEast)),
        _ if t < 4.0 => Some((D::North, C::ResizeNorth)),
        _ if b < e => Some((D::South, C::ResizeSouth)),
        _ => None,
    };
    let Some((dir, cursor)) = dir else { return false };
    ctx.set_cursor_icon(cursor);
    if ctx.input(|i| i.pointer.primary_pressed()) {
        ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(dir));
    }
    true
}

fn centered(ui: &mut Ui, max_w: f32, add: impl FnOnce(&mut Ui)) {
    let w = ui.available_width().min(max_w);
    let pad = (ui.available_width() - w) / 2.0;
    ui.horizontal(|ui| {
        ui.add_space(pad);
        ui.vertical(|ui| {
            ui.set_width(w);
            add(ui);
        });
    });
}

/// Centers a row by measuring it on the previous frame.
fn centered_row(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    let id = ui.scope_id().with("centered_row_w");
    let w: f32 = ui.ctx().data(|d| d.get_temp(id)).unwrap_or(0.0);
    ui.horizontal(|ui| {
        ui.add_space(((ui.available_width() - w) / 2.0).max(0.0));
        let start = ui.cursor().min.x;
        add(ui);
        let used = ui.cursor().min.x - start - ui.spacing().item_spacing.x;
        if (used - w).abs() > 0.5 {
            ui.ctx().data_mut(|d| d.insert_temp(id, used));
            ui.ctx().request_repaint();
        }
    });
}

fn two_panels(ui: &mut Ui, add: impl FnOnce(&mut Ui, &mut Ui)) {
    centered(ui, 1000.0, |ui| {
        ui.columns(2, |c| {
            let (a, b) = c.split_at_mut(1);
            add(&mut a[0], &mut b[0]);
        });
    });
}

fn list_row(ui: &mut Ui, title: &str, desc: &str, on: bool) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::click());
    let fill = if on { alpha(ACC, 30) } else if resp.hovered() { Color32::from_rgb(0x2c, 0x2e, 0x33) } else { SURF };
    ui.painter().rect(r, 5, fill, if on { Stroke::new(1.0, alpha(ACC, 110)) } else { Stroke::NONE }, StrokeKind::Inside);
    ui.painter().text(pos2(r.min.x + 12.0, r.center().y), Align2::LEFT_CENTER, title, if on { font_sb(13.5) } else { font(13.5) }, FG);
    ui.painter().text(pos2(r.max.x - 12.0, r.center().y), Align2::RIGHT_CENTER, desc, font(12.0), MUT);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn setting(ui: &mut Ui, title: &str, desc: &str, v: &mut bool) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(RichText::new(title).color(FG).size(14.0));
            ui.label(RichText::new(desc).color(MUT).size(12.0));
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            changed = switch(ui, v).changed();
        });
    });
    ui.add_space(6.0);
    changed
}

fn empty(ui: &mut Ui, title: &str, desc: &str) {
    ui.add_space(120.0);
    ui.vertical_centered(|ui| {
        ui.label(sb(title, 18.0).color(FG));
        ui.label(RichText::new(desc).color(MUT));
    });
}


#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RemapKind {
    Default,
    Keyboard,
    Mouse,
    Media,
    Windows,
    Power,
    Hypershift,
    Disable,
}

impl RemapKind {
    const ALL: [RemapKind; 8] =
        [RemapKind::Default, RemapKind::Keyboard, RemapKind::Mouse, RemapKind::Media, RemapKind::Windows, RemapKind::Power, RemapKind::Hypershift, RemapKind::Disable];
    fn name(self) -> &'static str {
        match self {
            RemapKind::Default => "Default",
            RemapKind::Keyboard => "Keyboard function",
            RemapKind::Mouse => "Mouse function",
            RemapKind::Media => "Multimedia",
            RemapKind::Windows => "Windows shortcut",
            RemapKind::Power => "Power",
            RemapKind::Hypershift => "Razer Hypershift",
            RemapKind::Disable => "Disable",
        }
    }
    fn desc(self) -> &'static str {
        match self {
            RemapKind::Default => "what's printed on the key",
            RemapKind::Keyboard => "any key + modifiers, turbo",
            RemapKind::Mouse => "clicks, side buttons, scroll",
            RemapKind::Media => "play, volume, tracks",
            RemapKind::Windows => "screenshot, Game Bar",
            RemapKind::Power => "sleep, wake",
            RemapKind::Hypershift => "hold for the 2nd layer",
            RemapKind::Disable => "key does nothing",
        }
    }
}

/// Short badge for a remapped key (None = does what's printed on it).
fn fn_badge(id: u8, f: KeyFn) -> Option<(String, Color32)> {
    Some(match f {
        KeyFn::Key { hid, mods } if Some(hid) == keymap::default_hid(id) && mods == 0 => return None,
        KeyFn::Key { hid, mods } => (format!("{}{}", if mods != 0 { "+" } else { "" }, keymap::hid_name(hid)), ACC2),
        KeyFn::Turbo { hid, .. } => (format!("T·{}", keymap::hid_name(hid)), ACC2),
        KeyFn::Disabled => ("off".into(), BAD),
        KeyFn::Mouse(b) => (["", "L", "R", "Mid", "M4", "M5", "", "", "", "▲", "▼"].get(b as usize).copied().unwrap_or("M").to_string(), INFO),
        KeyFn::DoubleClick => ("2×".into(), INFO),
        KeyFn::Media(m) => (
            match m {
                205 => "⏯",
                181 => "⏭",
                182 => "⏮",
                183 => "⏹",
                233 => "Vol+",
                234 => "Vol−",
                226 => "Mute",
                111 => "☀+",
                112 => "☀−",
                _ => "♪",
            }
            .into(),
            WARN,
        ),
        KeyFn::WinShortcut(_) => ("Win".into(), WARN),
        KeyFn::Power(_) => ("Pwr".into(), WARN),
        KeyFn::Hypershift => ("HS".into(), Color32::from_rgb(0xf9, 0x73, 0x16)),
        KeyFn::Other(t) => (format!("#{t}"), DIM),
    })
}

fn fn_long(f: KeyFn) -> String {
    let mods = |m: u8| {
        let mut s = String::new();
        for (bit, n) in [(1u8, "Ctrl+"), (2, "Shift+"), (4, "Alt+"), (8, "Win+")] {
            if m & bit != 0 {
                s.push_str(n);
            }
        }
        s
    };
    match f {
        KeyFn::Key { hid, mods: m } => format!("{}{}", mods(m), keymap::hid_name(hid)),
        KeyFn::Turbo { hid, mods: m, interval_ms } => format!("turbo {}{} ({}/s)", mods(m), keymap::hid_name(hid), 1000 / interval_ms.max(1)),
        KeyFn::Disabled => "off".into(),
        KeyFn::Mouse(b) => d::name_of(&d::MOUSE_BUTTONS, b).to_lowercase(),
        KeyFn::DoubleClick => "double click".into(),
        KeyFn::Media(m) => d::name_of(&d::MEDIA_KEYS, m).to_lowercase(),
        KeyFn::WinShortcut(w) => d::name_of(&d::WIN_SHORTCUTS, w).to_string(),
        KeyFn::Power(x) => d::name_of(&d::POWER_KEYS, x).to_lowercase(),
        KeyFn::Hypershift => "Hypershift".into(),
        KeyFn::Other(t) => format!("type {t}"),
    }
}
