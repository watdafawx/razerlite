//! Tray popover (design A4 "live art").

use super::theme::*;
use super::widgets::*;
use super::{App, Mode, UiExit, art};
use crate::device::{self as d, Effect};
use crate::state::Job;
use egui::{Align, Color32, Layout, RichText, Sense, Stroke, StrokeKind, vec2};

const SWATCHES: [[u8; 3]; 6] = [[68, 214, 44], [239, 68, 68], [59, 130, 246], [168, 85, 247], [249, 115, 22], [255, 255, 255]];

impl App {
    pub fn popover(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let full = ui.max_rect();
        let p = ui.painter();
        p.rect(full.shrink(0.5), 12, Color32::from_rgba_unmultiplied(0x1d, 0x1e, 0x21, 250), Stroke::new(1.0, alpha(FG, 28)), StrokeKind::Inside);

        let mut content = ui.new_child(egui::UiBuilder::new().max_rect(full.shrink2(vec2(14.0, 12.0))));
        let ui = &mut content;
        ui.spacing_mut().item_spacing = vec2(6.0, 6.0);

        // header
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
            art::logo(ui.painter(), r);
            ui.label(sb("RazerLite", 14.0).color(FG));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.add(egui::Button::new(RichText::new("⚙").size(15.0).color(MUT)).frame(false)).on_hover_text("Settings").clicked() {
                    self.close(&ctx, UiExit::OpenSettings);
                }
            });
        });
        sep(ui);

        let snap = self.snap.clone();
        if snap.mouse.is_none() && snap.kb.is_none() {
            ui.add_space(30.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new(if snap.busy { "Looking for devices…" } else { "No Razer devices found" }).color(MUT));
            });
        }

        // mouse card
        if let Some(m) = &snap.mouse {
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(96.0, 150.0), Sense::hover());
                let col = battery_color(m.battery, m.low_batt, m.charging);
                self.paint_mouse(ui.painter(), r.shrink(4.0), m.battery, col);
                ui.vertical(|ui| {
                    ui.add_space(8.0);
                    ui.label(sb("Viper V3 Pro", 14.0).color(FG));
                    ui.horizontal(|ui| {
                        ui.label(sb(format!("{}%", m.battery), 30.0).color(battery_color(m.battery, m.low_batt, m.charging)));
                        ui.label(RichText::new(if m.charging { "charging" } else { "" }).color(MUT).size(12.0));
                    });
                    let hz = d::MOUSE_RATES.iter().find(|r| r.1 == m.polling).map(|r| r.0).unwrap_or(0);
                    ui.label(RichText::new(format!("{} DPI · {} Hz", m.dpi.0, hz)).color(MUT).size(12.0));
                    ui.add_space(2.0);
                    ui.horizontal_wrapped(|ui| {
                        for (i, (x, _)) in m.stages.stages.iter().enumerate() {
                            if chip(ui, &x.to_string(), m.stages.active as usize == i + 1).clicked() {
                                let mut s = m.stages.clone();
                                s.active = i as u8 + 1;
                                self.send(Job::MouseStages(s));
                            }
                        }
                    });
                    ui.horizontal(|ui| {
                        for (hz, code) in [(1000, 0x08), (2000, 0x04), (4000, 0x02), (8000, 0x01)] {
                            if chip(ui, &format!("{}K", hz / 1000), m.polling == code).clicked() {
                                self.send(Job::MousePolling(code));
                            }
                        }
                    });
                });
            });
            sep(ui);
        }

        // keyboard card
        if let Some(k) = &snap.kb {
            let t = ctx.input(|i| i.time) as f32;
            let animated = matches!(k.effect, Effect::Spectrum | Effect::WaveLeft | Effect::WaveRight | Effect::StarlightRandom | Effect::BreathingRandom);
            let mut col = Color32::from_rgb(k.color[0], k.color[1], k.color[2]);
            if animated {
                col = egui::ecolor::Hsva::new((t * 0.12).fract(), 0.8, 1.0, 1.0).into();
                ctx.request_repaint_after(std::time::Duration::from_millis(50));
            }
            if k.effect == Effect::Off {
                col = DIM;
            }
            let glow = if k.effect == Effect::Off { 0.0 } else { 0.35 + self.bright as f32 / 400.0 };
            let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 104.0), Sense::hover());
            self.paint_kb_photo(ui.painter(), r.shrink2(vec2(2.0, 2.0)), k, col, (110.0 + self.bright as f32 * 0.55) as u8, glow);
            ui.horizontal(|ui| {
                ui.label(sb("Huntsman V2 Analog", 14.0).color(FG));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    pill(ui, "Wired", true);
                });
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new("Light").color(TXT));
                ui.spacing_mut().slider_width = ui.available_width() - 60.0;
                let resp = ui.add(egui::Slider::new(&mut self.bright, 0..=255).show_value(false).trailing_fill(true));
                ui.label(RichText::new(format!("{}%", self.bright as u32 * 100 / 255)).color(FG));
                if resp.drag_stopped() || (resp.changed() && !resp.dragged()) {
                    self.send(Job::KbBrightness(self.bright));
                }
            });
            ui.horizontal(|ui| {
                for (name, e) in [("Static", Effect::Static), ("Spectrum", Effect::Spectrum), ("Off", Effect::Off)] {
                    if chip(ui, name, k.effect == e).clicked() {
                        self.send(Job::KbEffect(e, k.color, 2));
                    }
                }
                ui.add_space(4.0);
                for c in SWATCHES {
                    let (r, resp) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::click());
                    ui.painter().rect_filled(r, 5, Color32::from_rgb(c[0], c[1], c[2]));
                    if k.color == c && k.effect != Effect::Spectrum {
                        ui.painter().rect_stroke(r.expand(1.5), 6, Stroke::new(2.0, FG), StrokeKind::Outside);
                    }
                    if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                        let e = if matches!(k.effect, Effect::Static | Effect::Breathing | Effect::Reactive | Effect::Starlight) { k.effect } else { Effect::Static };
                        self.send(Job::KbEffect(e, c, 2));
                    }
                }
            });
            sep(ui);
        }

        if snap.synapse_running {
            ui.label(RichText::new("⚠ Synapse is running: keyboard changes are blocked.").color(WARN).size(12.0));
        } else if let Some(e) = &snap.error {
            ui.label(RichText::new(e).color(BAD).size(12.0));
        }

        // footer
        ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
            ui.horizontal(|ui| {
                if primary(ui, "Open RazerLite", true).clicked() {
                    self.close(&ctx, UiExit::Open(Mode::Full));
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(RichText::new(if snap.busy { "Saving…" } else { "Saved on device" }).color(MUT).size(12.0));
                });
            });
        });
    }
}

fn sep(ui: &mut egui::Ui) {
    let r = ui.available_rect_before_wrap();
    ui.painter().hline(r.x_range(), r.min.y + 2.0, Stroke::new(1.0, alpha(FG, 14)));
    ui.add_space(6.0);
}
