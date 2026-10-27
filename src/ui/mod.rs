//! Windows. Each call to `run` opens one window (popover or full app) and returns when it closes,
//! so the GPU context and its memory go away while RazerLite sits in the tray.

mod art;
mod full;
mod images;
mod popover;
pub mod theme;
mod widgets;

use crate::config::Config;
use crate::devmcp::{self, DevCmd};
use crate::state::{Job, Shared, Snapshot};
use crate::tray::{self, TrayMsg};
use egui::{Event, Pos2};
use serde_json::{Value, json};
use std::collections::{HashSet, VecDeque};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Mode {
    Popover,
    Full,
}

pub enum UiExit {
    Closed,
    Open(Mode),
    OpenSettings,
    Quit,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum Tab {
    Mouse,
    Keyboard,
    Macros,
    Settings,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum Sub {
    Customize,
    Actuation,
    Lighting,
    Performance,
    Power,
}

pub const POP_W: f32 = 372.0;
pub const POP_H: f32 = 478.0;

pub fn run(shared: Arc<Shared>, mode: Mode, start_tab: Option<Tab>) -> UiExit {
    let exit = Arc::new(std::sync::Mutex::new(UiExit::Closed));
    let e2 = exit.clone();
    let icon = Arc::new(egui::IconData { rgba: tray::icon_rgba(0, 64), width: 64, height: 64 });
    let viewport = match mode {
        Mode::Full => egui::ViewportBuilder::default()
            .with_title("RazerLite")
            .with_inner_size([1180.0, 820.0])
            .with_min_inner_size([900.0, 620.0])
            .with_decorations(false)
            .with_icon(icon),
        Mode::Popover => {
            let (x, y) = popover_pos();
            egui::ViewportBuilder::default()
                .with_title("RazerLite")
                .with_inner_size([POP_W, POP_H])
                .with_position([x, y])
                .with_decorations(false)
                .with_resizable(false)
                .with_transparent(true)
                .with_always_on_top()
                .with_taskbar(false)
                .with_icon(icon)
        }
    };
    // No file drops: also avoids a COM error when launched from the tray process.
    let viewport = viewport.with_drag_and_drop(false);
    let opts = eframe::NativeOptions { viewport, centered: mode == Mode::Full, ..Default::default() };
    let r = eframe::run_native(
        "RazerLite",
        opts,
        Box::new(move |cc| {
            theme::install(&cc.egui_ctx);
            *tray::UI_CTX.lock().unwrap() = Some(cc.egui_ctx.clone());
            let ctx = cc.egui_ctx.clone();
            let waker = shared.add_waker(Arc::new(move || ctx.request_repaint()));
            let mut app = App::new(shared, mode, e2, waker);
            app.images = std::rc::Rc::new(images::load(&cc.egui_ctx));
            if let Some(t) = start_tab {
                app.tab = t;
            }
            Ok(Box::new(app))
        }),
    );
    *tray::UI_CTX.lock().unwrap() = None;
    if let Err(e) = r {
        eprintln!("window error: {e}");
    }
    std::mem::replace(&mut *exit.lock().unwrap(), UiExit::Closed)
}

/// Bottom-right of the work area, in logical points.
fn popover_pos() -> (f32, f32) {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::UI::HiDpi::GetDpiForSystem;
    use windows_sys::Win32::UI::WindowsAndMessaging::{SPI_GETWORKAREA, SystemParametersInfoW};
    unsafe {
        let mut r: RECT = std::mem::zeroed();
        SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut r as *mut _ as _, 0);
        let scale = GetDpiForSystem() as f32 / 96.0;
        let (right, bottom) = (r.right as f32 / scale, r.bottom as f32 / scale);
        (right - POP_W - 12.0, bottom - POP_H - 12.0)
    }
}

pub struct App {
    pub shared: Arc<Shared>,
    pub mode: Mode,
    exit: Arc<std::sync::Mutex<UiExit>>,
    waker: u64,
    pub cfg: Config,
    pub snap: Snapshot,
    frame: u64,
    was_focused: bool,
    pending_input: VecDeque<Vec<Event>>,
    pub last_pointer: Pos2,

    // full window
    pub tab: Tab,
    pub kb_sub: Sub,
    pub mouse_sub: Sub,
    pub sel: HashSet<u8>,
    pub drag: Option<full::KeyDrag>,
    pub press_mm: f32,
    pub release_mm: f32,
    pub slider_grab: Option<bool>,
    pub layer: u8,
    pub remap_hid: u8,
    pub remap_mods: u8,
    pub remap_kind: full::RemapKind,
    pub remap_media: u16,
    pub remap_win: u8,
    pub remap_power: u8,
    pub remap_turbo: bool,
    pub remap_rate: u8,
    pub remap_mouse: u8,
    pub light_color: [u8; 3],
    pub light_speed: u8,
    pub bright: u8,
    pub stage_edit: usize,
    pub stages_dirty: Option<crate::device::DpiStages>,
    pub debounce: [u8; 4],
    pub sleep_min: u16,
    pub low_batt: u8,
    pub macro_sel: usize,
    pub recording: Option<std::time::Instant>,
    pub rec_last: Option<std::time::Instant>,
    pub autostart: bool,
    pub show_synapse_help: bool,
    pub kb_scale: f32,
    pub images: std::rc::Rc<images::DeviceImages>,
}

impl App {
    fn new(shared: Arc<Shared>, mode: Mode, exit: Arc<std::sync::Mutex<UiExit>>, waker: u64) -> Self {
        let snap = shared.get();
        let cfg = Config::load();
        let (light_color, bright) = snap.kb.as_ref().map(|k| (k.color, k.brightness)).unwrap_or(([68, 214, 44], 255));
        let (sleep_min, low_batt) = snap.mouse.as_ref().map(|m| ((m.sleep / 60).max(1), m.low_batt)).unwrap_or((2, 30));
        let debounce = snap.kb.as_ref().map(|k| k.debounce).unwrap_or_default();
        let mut tab = Tab::Keyboard;
        if snap.kb.is_none() && snap.mouse.is_some() {
            tab = Tab::Mouse;
        }
        Self {
            shared,
            mode,
            exit,
            waker,
            cfg,
            snap,
            frame: 0,
            was_focused: false,
            pending_input: VecDeque::new(),
            last_pointer: Pos2::ZERO,
            tab,
            kb_sub: Sub::Actuation,
            mouse_sub: Sub::Performance,
            sel: HashSet::new(),
            drag: None,
            press_mm: 2.0,
            release_mm: 1.6,
            slider_grab: None,
            layer: 0,
            remap_hid: 4,
            remap_mods: 0,
            remap_kind: full::RemapKind::Default,
            remap_media: 205,
            remap_win: 99,
            remap_power: 130,
            remap_turbo: false,
            remap_rate: 10,
            remap_mouse: 1,
            light_color,
            light_speed: 2,
            bright,
            stage_edit: 0,
            stages_dirty: None,
            debounce,
            sleep_min,
            low_batt,
            macro_sel: 0,
            recording: None,
            rec_last: None,
            autostart: crate::config::autostart_enabled(),
            show_synapse_help: false,
            kb_scale: 1.0,
            images: std::rc::Rc::new(images::DeviceImages { kb: None, kb_base: None, kb_glow: None, mouse: None, mouse_mask: None }),
        }
    }

    pub fn send(&self, j: Job) {
        self.shared.send(j);
    }

    pub fn close(&self, ctx: &egui::Context, how: UiExit) {
        *self.exit.lock().unwrap() = how;
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    fn handle_tray(&mut self, ctx: &egui::Context) {
        for ev in tray::take_events() {
            match ev {
                TrayMsg::Double | TrayMsg::Open | TrayMsg::Show(Mode::Full) => match self.mode {
                    Mode::Full => {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    }
                    Mode::Popover => self.close(ctx, UiExit::Open(Mode::Full)),
                },
                TrayMsg::Click => {
                    if self.mode == Mode::Popover {
                        self.close(ctx, UiExit::Closed);
                    } else {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    }
                }
                TrayMsg::Show(Mode::Popover) => {
                    if self.mode == Mode::Full {
                        self.close(ctx, UiExit::Open(Mode::Popover));
                    }
                }
                TrayMsg::ToggleAutostart => {
                    tray::toggle_autostart();
                    self.autostart = crate::config::autostart_enabled();
                }
                TrayMsg::Quit => self.close(ctx, UiExit::Quit),
            }
        }
    }

    fn handle_dev(&mut self, ctx: &egui::Context) {
        for (cmd, reply) in devmcp::take() {
            let out: Value = match cmd {
                DevCmd::State => json!({
                    "mode": self.mode, "tab": self.tab, "keyboard_sub": self.kb_sub, "mouse_sub": self.mouse_sub,
                    "selected_keys": self.sel.iter().collect::<Vec<_>>(), "press_mm": self.press_mm, "release_mm": self.release_mm,
                    "window": [ctx.content_rect().width(), ctx.content_rect().height()], "pixels_per_point": ctx.pixels_per_point(),
                    "status": self.snap.status, "error": self.snap.error, "demo": self.snap.demo,
                }),
                DevCmd::Navigate { tab, sub } => {
                    if let Some(t) = tab {
                        self.tab = t;
                    }
                    if let Some(s) = sub {
                        match s {
                            Sub::Customize | Sub::Actuation | Sub::Lighting if self.tab == Tab::Keyboard => self.kb_sub = s,
                            Sub::Customize | Sub::Performance | Sub::Power if self.tab == Tab::Mouse => self.mouse_sub = s,
                            _ => {}
                        }
                    }
                    json!("ok")
                }
                DevCmd::Screenshot => {
                    let r = reply.clone();
                    ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::ScreenshotCallback::new(move |img| {
                        let _ = r.send(devmcp::png_reply(&img));
                    })));
                    continue;
                }
                DevCmd::Input(batches) => {
                    self.pending_input.extend(batches);
                    json!("ok")
                }
                DevCmd::Close => {
                    self.close(ctx, UiExit::Closed);
                    json!("ok")
                }
            };
            let _ = reply.send(out);
        }
    }
}

impl App {
    /// Mouse photo with the battery level filled inside its outline. Falls back to line art.
    pub fn paint_mouse(&self, p: &egui::Painter, area: egui::Rect, battery: u8, col: egui::Color32) -> egui::Rect {
        if let (Some(img), Some(mask)) = (&self.images.mouse, &self.images.mouse_mask) {
            let (r, _) = img.fit(area);
            img.paint(p, r, egui::Color32::WHITE);
            let from = 1.0 - battery.min(100) as f32 / 100.0;
            mask.paint_bottom(p, r, from, theme::alpha(col, 34));
            r
        } else {
            art::mouse(p, area, egui::Color32::from_rgb(0xcf, 0xd2, 0xd6), Some((battery, col)), 1.5);
            area
        }
    }

    /// Keyboard photo lit in the current effect color. Falls back to the outline art.
    pub fn paint_kb_photo(&self, p: &egui::Painter, area: egui::Rect, kb: &crate::state::KbState, col: egui::Color32, outline_alpha: u8, glow: f32) {
        use crate::device::Effect;
        if let (Some(full), Some(base), Some(lights)) = (&self.images.kb, &self.images.kb_base, &self.images.kb_glow) {
            let (r, _) = full.fit(area);
            match kb.effect {
                Effect::Spectrum | Effect::WaveLeft | Effect::WaveRight | Effect::StarlightRandom | Effect::BreathingRandom => {
                    full.paint(p, r, egui::Color32::from_gray(((kb.brightness as f32 / 255.0) * 155.0 + 100.0) as u8))
                }
                Effect::Off => base.paint(p, r, egui::Color32::WHITE),
                _ => {
                    base.paint(p, r, egui::Color32::WHITE);
                    lights.paint(p, r, theme::alpha(col, (kb.brightness as f32 * 0.85 + 30.0).min(255.0) as u8));
                }
            }
        } else {
            art::kb_outline(p, area, theme::alpha(col, outline_alpha), glow);
        }
    }
}

impl eframe::App for App {
    fn clear_color(&self, _v: &egui::Visuals) -> [f32; 4] {
        match self.mode {
            Mode::Popover => [0.0; 4],
            Mode::Full => theme::BG.to_normalized_gamma_f32(),
        }
    }

    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        if let Some(batch) = self.pending_input.pop_front() {
            raw.events.extend(batch);
            ctx.request_repaint();
        }
        for e in &raw.events {
            if let Event::PointerMoved(p) = e {
                self.last_pointer = *p;
            }
        }
    }

    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.frame += 1;
        self.snap = self.shared.get();
        tray::sync(&self.snap);
        self.handle_tray(ctx);
        self.handle_dev(ctx);
        if !self.pending_input.is_empty() {
            ctx.request_repaint();
        }
        if self.mode == Mode::Popover {
            let focused = ctx.input(|i| i.viewport().focused);
            if focused == Some(true) {
                self.was_focused = true;
            } else if focused == Some(false) && self.was_focused && !devmcp::ACTIVE.load(std::sync::atomic::Ordering::Relaxed) {
                self.close(ctx, UiExit::Closed);
            }
            if self.frame < 3 {
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
            if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.close(ctx, UiExit::Closed);
            }
        } else if ctx.input(|i| i.viewport().close_requested()) && !self.cfg.close_to_tray {
            *self.exit.lock().unwrap() = UiExit::Quit;
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        match self.mode {
            Mode::Popover => self.popover(ui),
            Mode::Full => self.full(ui),
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.shared.remove_waker(self.waker);
        self.cfg.save();
    }
}
