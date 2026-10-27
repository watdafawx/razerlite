use crate::device::{self as d, DpiStages, Effect};
use crate::keymap;
use crate::worker::{self, Job, KbState, MouseState, Msg};
use egui::{Color32, RichText, Vec2};
use std::collections::HashSet;
use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant};

#[derive(PartialEq, Eq, Clone, Copy)]
enum Tab {
    Keyboard,
    Mouse,
}

pub struct App {
    jobs: Sender<Job>,
    msgs: Receiver<Msg>,
    tab: Tab,
    busy: bool,
    progress: f32,
    status: String,
    error: Option<String>,
    last_battery: Instant,

    kb: Option<KbState>,
    kb_loaded: bool,
    selected: HashSet<u8>,
    press_mm: f32,
    reset_mm: f32,
    debounce: [u8; 4],
    effect: Effect,
    color: [u8; 3],
    speed: u8,
    brightness: u8,

    mouse: Option<MouseState>,
    stages: DpiStages,
    sleep_min: u16,
    low_batt: u8,
}

impl App {
    pub fn new(cc: &eframe::CreationContext) -> Self {
        let (jobs, msgs) = worker::spawn(cc.egui_ctx.clone());
        let _ = jobs.send(Job::Refresh);
        cc.egui_ctx.set_visuals(egui::Visuals::dark());
        Self {
            jobs,
            msgs,
            tab: Tab::Keyboard,
            busy: true,
            progress: 0.0,
            status: "Reading devices…".into(),
            error: None,
            last_battery: Instant::now(),
            kb: None,
            kb_loaded: false,
            selected: HashSet::new(),
            press_mm: 2.0,
            reset_mm: 0.4,
            debounce: [0; 4],
            effect: Effect::Static,
            color: [0, 255, 0],
            speed: 2,
            brightness: 255,
            mouse: None,
            stages: DpiStages::default(),
            sleep_min: 2,
            low_batt: 30,
        }
    }

    fn send(&mut self, j: Job) {
        self.busy = true;
        self.progress = 0.0;
        self.error = None;
        let _ = self.jobs.send(j);
    }

    fn drain(&mut self) {
        while let Ok(m) = self.msgs.try_recv() {
            match m {
                Msg::Kb(s) => {
                    if let Some(s) = &s {
                        self.debounce = s.debounce;
                        self.brightness = s.brightness;
                        self.color = s.color;
                        if let Some(e) = effect_from_code(s.effect) {
                            self.effect = e;
                        }
                    }
                    self.kb = s;
                }
                Msg::KbKeys(keys) => {
                    if let Some(kb) = &mut self.kb {
                        kb.keys.extend(keys);
                        self.kb_loaded = true;
                    }
                }
                Msg::Mouse(s) => {
                    if let Some(s) = &s {
                        self.stages = s.stages.clone();
                        self.sleep_min = (s.sleep / 60).max(1);
                        self.low_batt = s.low_batt;
                    }
                    self.mouse = s;
                }
                Msg::Battery(b, c) => {
                    if let Some(m) = &mut self.mouse {
                        m.battery = b;
                        m.charging = c;
                    }
                }
                Msg::Progress(p) => {
                    self.progress = p;
                    if p >= 1.0 {
                        self.busy = false;
                        if self.status.starts_with("Reading") {
                            self.status = "Ready".into();
                        }
                    }
                }
                Msg::Status(s) => self.status = s,
                Msg::Error(e) => self.error = Some(e),
            }
        }
    }
}

fn effect_from_code(c: u8) -> Option<Effect> {
    Some(match c {
        0 => Effect::Off,
        1 => Effect::Static,
        2 => Effect::Breathing,
        3 => Effect::Spectrum,
        4 => Effect::WaveRight,
        5 => Effect::Reactive,
        7 => Effect::Starlight,
        _ => return None,
    })
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.drain();

        // Poll battery once a minute while the window is open; otherwise stay idle.
        if self.last_battery.elapsed() > Duration::from_secs(60) && !self.busy {
            self.last_battery = Instant::now();
            let _ = self.jobs.send(Job::MouseBattery);
        }
        ctx.request_repaint_after(Duration::from_secs(60));

        egui::Panel::top("tabs").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.tab, Tab::Keyboard, RichText::new("Keyboard").size(16.0));
                ui.selectable_value(&mut self.tab, Tab::Mouse, RichText::new("Mouse").size(16.0));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add_enabled(!self.busy, egui::Button::new("⟳ Reload")).clicked() {
                        self.kb_loaded = false;
                        self.status = "Reading devices…".into();
                        self.send(Job::Refresh);
                    }
                });
            });
        });

        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                if self.busy {
                    ui.add(egui::ProgressBar::new(self.progress).desired_width(140.0).show_percentage());
                }
                if let Some(e) = &self.error {
                    ui.colored_label(Color32::from_rgb(255, 110, 110), e);
                } else {
                    ui.label(&self.status);
                }
            });
        });

        egui::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| match self.tab {
                Tab::Keyboard => self.keyboard_ui(ui),
                Tab::Mouse => self.mouse_ui(ui),
            });
        });
    }
}

const ACCENT: Color32 = Color32::from_rgb(68, 214, 44);

impl App {
    fn keyboard_ui(&mut self, ui: &mut egui::Ui) {
        let Some(kb) = self.kb.clone() else {
            ui.label("Huntsman V2 Analog not found. Close Synapse if it is running, then Reload.");
            return;
        };
        ui.label(format!(
            "Huntsman V2 Analog · firmware {} · editing onboard profile {} (stored: {:?})",
            kb.fw, kb.profile, kb.profiles
        ));
        ui.add_space(6.0);

        // ---- key map
        ui.heading("Actuation");
        ui.label("Click keys to select. Labels show the press point in mm (· = factory default).");
        ui.add_space(4.0);
        let unit = 38.0;
        let gap = 3.0;
        for row in keymap::ROWS {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for key in row.iter() {
                    let w = key.w * unit + (key.w - 1.0).max(0.0) * gap;
                    if key.id == 0 {
                        ui.add_space(w - gap);
                        continue;
                    }
                    let sel = self.selected.contains(&key.id);
                    let value = kb.keys.get(&key.id).map(|&(p, _)| {
                        if p == d::KB_DEFAULT { "·".to_string() } else { format!("{:.1}", d::byte_to_mm(p)) }
                    });
                    let text = format!("{}\n{}", key.label, value.unwrap_or_default());
                    let mut b = egui::Button::new(RichText::new(text).size(10.5)).min_size(Vec2::new(w, unit));
                    if sel {
                        b = b.fill(ACCENT.gamma_multiply(0.55));
                    }
                    if ui.add(b).clicked() {
                        if !self.selected.remove(&key.id) {
                            self.selected.insert(key.id);
                        }
                    }
                }
            });
        }
        ui.horizontal(|ui| {
            if ui.button("Select all").clicked() {
                self.selected = keymap::all_ids().into_iter().collect();
            }
            if ui.button("WASD").clicked() {
                self.selected = keymap::WASD.into_iter().collect();
            }
            if ui.button("Clear").clicked() {
                self.selected.clear();
            }
            ui.label(format!("{} selected", self.selected.len()));
        });
        ui.add_space(6.0);

        egui::Grid::new("act").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            ui.label("Press point");
            ui.add(egui::Slider::new(&mut self.press_mm, 1.5..=3.6).step_by(0.1).suffix(" mm"));
            ui.end_row();
            ui.label("Release distance");
            ui.add(egui::Slider::new(&mut self.reset_mm, 0.1..=1.0).step_by(0.1).suffix(" mm"))
                .on_hover_text("How far the key must rise above the press point before it releases.\nBigger = no double typing from finger wobble.");
            ui.end_row();
        });
        let release_mm = (self.press_mm - self.reset_mm).max(0.1);
        ui.label(format!("Key presses at {:.1} mm, releases at {:.1} mm.", self.press_mm, release_mm));
        ui.horizontal(|ui| {
            let can = !self.busy && self.kb_loaded && !self.selected.is_empty();
            if ui.add_enabled(can, egui::Button::new("Apply to selected")).clicked() {
                let mut keys: Vec<u8> = self.selected.iter().copied().collect();
                keys.sort();
                self.send(Job::KbThresholds {
                    keys,
                    press: d::mm_to_byte(self.press_mm),
                    release: d::mm_to_byte(release_mm),
                });
            }
            if ui.add_enabled(can, egui::Button::new("Factory default")).clicked() {
                let keys: Vec<u8> = self.selected.iter().copied().collect();
                self.send(Job::KbThresholds { keys, press: d::KB_DEFAULT, release: d::KB_DEFAULT });
            }
        });
        ui.add_space(4.0);
        ui.small("Rapid Trigger and Snap Tap on this keyboard run inside Synapse software, not the firmware, so they are not available here.");

        ui.separator();
        ui.heading("Debounce");
        egui::Grid::new("deb").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            ui.label("Press debounce");
            ui.add(egui::Slider::new(&mut self.debounce[0], 0..=20).suffix(" ms"));
            ui.end_row();
            ui.label("Release debounce");
            ui.add(egui::Slider::new(&mut self.debounce[2], 0..=20).suffix(" ms"))
                .on_hover_text("Ignores a new press for this long after a release. 3–5 ms stops chatter.");
            ui.end_row();
        });
        if ui.add_enabled(!self.busy, egui::Button::new("Save debounce")).clicked() {
            self.send(Job::KbDebounce(self.debounce));
        }

        ui.separator();
        ui.heading("Polling rate");
        ui.horizontal(|ui| {
            for (hz, code) in [(1000, 1u8), (500, 2), (125, 8)] {
                let on = kb.polling == code;
                if ui.add_enabled(!self.busy, egui::Button::new(format!("{hz} Hz")).selected(on)).clicked() {
                    if let Some(k) = &mut self.kb {
                        k.polling = code;
                    }
                    self.send(Job::KbPolling(code));
                }
            }
        });

        ui.separator();
        ui.heading("Lighting");
        egui::Grid::new("light").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            ui.label("Effect");
            egui::ComboBox::from_id_salt("effect").selected_text(self.effect.name()).show_ui(ui, |ui| {
                for e in Effect::ALL {
                    ui.selectable_value(&mut self.effect, e, e.name());
                }
            });
            ui.end_row();
            if self.effect.uses_color() {
                ui.label("Color");
                ui.color_edit_button_srgb(&mut self.color);
                ui.end_row();
            }
            if self.effect.uses_speed() {
                ui.label("Speed");
                ui.add(egui::Slider::new(&mut self.speed, 1..=3).custom_formatter(|v, _| {
                    ["fast", "medium", "slow"][(v as usize).clamp(1, 3) - 1].into()
                }));
                ui.end_row();
            }
            ui.label("Brightness");
            let r = ui.add(egui::Slider::new(&mut self.brightness, 0..=255));
            if r.drag_stopped() || (r.changed() && !r.dragged()) {
                let _ = self.jobs.send(Job::KbBrightness(self.brightness));
            }
            ui.end_row();
        });
        if ui.add_enabled(!self.busy, egui::Button::new("Apply lighting")).clicked() {
            self.send(Job::KbEffect(self.effect, self.color, self.speed));
        }
    }

    fn mouse_ui(&mut self, ui: &mut egui::Ui) {
        let Some(m) = self.mouse.clone() else {
            ui.label("Viper V3 Pro not found. Plug in the dongle or cable, then Reload.");
            return;
        };
        ui.label(format!("Viper V3 Pro · firmware {}", m.fw));
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label("Battery");
            ui.add(
                egui::ProgressBar::new(m.battery as f32 / 100.0)
                    .desired_width(180.0)
                    .text(format!("{}%{}", m.battery, if m.charging { " · charging" } else { "" })),
            );
        });
        ui.label(format!("Current DPI: {} × {}", m.dpi.0, m.dpi.1));

        ui.separator();
        ui.heading("DPI stages");
        let n = self.stages.stages.len();
        let mut remove = None;
        egui::Grid::new("stages").num_columns(4).spacing([10.0, 6.0]).show(ui, |ui| {
            for i in 0..n {
                let active = self.stages.active as usize == i + 1;
                if ui.radio(active, format!("Stage {}", i + 1)).clicked() {
                    self.stages.active = i as u8 + 1;
                }
                let (x, y) = &mut self.stages.stages[i];
                let before = *x;
                ui.add(egui::DragValue::new(x).range(100..=35000).speed(10).prefix("X "));
                if *y == before {
                    *y = *x; // keep X/Y linked unless the user split them
                }
                ui.add(egui::DragValue::new(y).range(100..=35000).speed(10).prefix("Y "));
                if n > 1 && ui.small_button("✕").clicked() {
                    remove = Some(i);
                }
                ui.end_row();
            }
        });
        if let Some(i) = remove {
            self.stages.stages.remove(i);
            let len = self.stages.stages.len() as u8;
            self.stages.active = self.stages.active.min(len).max(1);
        }
        ui.horizontal(|ui| {
            if n < 5 && ui.button("+ Add stage").clicked() {
                let last = self.stages.stages.last().copied().unwrap_or((800, 800));
                let v = (last.0 * 2).min(35000);
                self.stages.stages.push((v, v));
            }
            if ui.add_enabled(!self.busy, egui::Button::new("Save DPI")).clicked() {
                let s = self.stages.clone();
                self.send(Job::MouseStages(s));
            }
        });

        ui.separator();
        ui.heading("Polling rate");
        ui.horizontal_wrapped(|ui| {
            for (hz, code) in d::MOUSE_RATES {
                let on = m.polling == code;
                if ui.add_enabled(!self.busy, egui::Button::new(format!("{hz} Hz")).selected(on)).clicked() {
                    self.send(Job::MousePolling(code));
                }
            }
        });
        ui.small("4000/8000 Hz need the HyperPolling dongle and cost battery.");

        ui.separator();
        ui.heading("Power");
        egui::Grid::new("power").num_columns(3).spacing([12.0, 6.0]).show(ui, |ui| {
            ui.label("Sleep after");
            ui.add(egui::Slider::new(&mut self.sleep_min, 1..=15).suffix(" min"));
            if ui.add_enabled(!self.busy, egui::Button::new("Save")).clicked() {
                self.send(Job::MouseSleep(self.sleep_min * 60));
            }
            ui.end_row();
            ui.label("Low-battery warning");
            ui.add(egui::Slider::new(&mut self.low_batt, 5..=100).step_by(5.0).suffix(" %"));
            if ui.add_enabled(!self.busy, egui::Button::new("Save ")).clicked() {
                self.send(Job::MouseLowBatt(self.low_batt));
            }
            ui.end_row();
        });
    }
}
