//! Background thread that owns the HID handles. The UI sends `Job`s and gets `Msg`s back,
//! so slow USB round-trips never block a frame.

use crate::device::{self as d, DpiStages, Effect, KeyAssign};
use crate::keymap;
use anyhow::Result;
use hidapi::{HidApi, HidDevice};
use std::collections::HashMap;
use std::sync::mpsc::{Receiver, Sender};

pub enum Job {
    Refresh,
    KbThresholds { keys: Vec<u8>, press: u8, release: u8 },
    KbDebounce([u8; 4]),
    KbPolling(u8),
    KbBrightness(u8),
    KbEffect(Effect, [u8; 3], u8),
    MouseStages(DpiStages),
    MousePolling(u8),
    MouseSleep(u16),
    MouseLowBatt(u8),
    MouseBattery,
}

#[derive(Clone, Default)]
pub struct KbState {
    pub fw: String,
    pub profile: u8,
    pub profiles: Vec<u8>,
    pub debounce: [u8; 4],
    pub polling: u8,
    pub brightness: u8,
    pub effect: u8,
    pub color: [u8; 3],
    /// key id -> (press, release) bytes for normal mode
    pub keys: HashMap<u8, (u8, u8)>,
}

#[derive(Clone, Default)]
pub struct MouseState {
    pub fw: String,
    pub stages: DpiStages,
    pub dpi: (u16, u16),
    pub polling: u8,
    pub battery: u8,
    pub charging: bool,
    pub sleep: u16,
    pub low_batt: u8,
}

pub enum Msg {
    Kb(Option<KbState>),
    Mouse(Option<MouseState>),
    KbKeys(HashMap<u8, (u8, u8)>),
    Battery(u8, bool),
    Progress(f32),
    Status(String),
    Error(String),
}

struct Worker {
    api: HidApi,
    kb: Option<HidDevice>,
    mouse: Option<HidDevice>,
    profile: u8,
    /// (key, mode) -> current assignment, for the active profile
    assigns: HashMap<(u8, u8), KeyAssign>,
    tx: Sender<Msg>,
    ctx: egui::Context,
}

pub fn spawn(ctx: egui::Context) -> (Sender<Job>, Receiver<Msg>) {
    let (jtx, jrx) = std::sync::mpsc::channel::<Job>();
    let (mtx, mrx) = std::sync::mpsc::channel::<Msg>();
    std::thread::Builder::new()
        .name("razer-io".into())
        .spawn(move || {
            let api = match HidApi::new() {
                Ok(a) => a,
                Err(e) => {
                    let _ = mtx.send(Msg::Error(format!("HID init failed: {e}")));
                    return;
                }
            };
            let mut w = Worker { api, kb: None, mouse: None, profile: 1, assigns: HashMap::new(), tx: mtx, ctx };
            for job in jrx {
                if let Err(e) = w.run(job) {
                    w.send(Msg::Error(format!("{e:#}")));
                }
                w.send(Msg::Progress(1.0));
            }
        })
        .expect("spawn io thread");
    (jtx, mrx)
}

impl Worker {
    fn send(&self, m: Msg) {
        let _ = self.tx.send(m);
        self.ctx.request_repaint();
    }

    fn kb(&self) -> Result<&HidDevice> {
        self.kb.as_ref().ok_or_else(|| anyhow::anyhow!("keyboard not connected"))
    }
    fn mouse(&self) -> Result<&HidDevice> {
        self.mouse.as_ref().ok_or_else(|| anyhow::anyhow!("mouse not connected"))
    }

    fn run(&mut self, job: Job) -> Result<()> {
        match job {
            Job::Refresh => self.refresh(),
            Job::KbThresholds { keys, press, release } => self.set_thresholds(&keys, press, release),
            Job::KbDebounce(v) => {
                d::kb_set_debounce(self.kb()?, v)?;
                self.send(Msg::Status("Debounce saved".into()));
                Ok(())
            }
            Job::KbPolling(c) => {
                d::kb_set_polling(self.kb()?, c)?;
                self.send(Msg::Status("Keyboard polling rate saved".into()));
                Ok(())
            }
            Job::KbBrightness(v) => d::kb_set_brightness(self.kb()?, v),
            Job::KbEffect(e, rgb, s) => {
                d::kb_set_effect(self.kb()?, e, rgb, s)?;
                self.send(Msg::Status(format!("Lighting: {}", e.name())));
                Ok(())
            }
            Job::MouseStages(s) => {
                d::mouse_set_stages(self.mouse()?, &s)?;
                self.send(Msg::Status("DPI stages saved".into()));
                self.refresh_mouse()
            }
            Job::MousePolling(c) => {
                d::mouse_set_polling(self.mouse()?, c)?;
                self.send(Msg::Status("Mouse polling rate saved".into()));
                self.refresh_mouse()
            }
            Job::MouseSleep(s) => {
                d::mouse_set_sleep(self.mouse()?, s)?;
                self.send(Msg::Status("Sleep timer saved".into()));
                Ok(())
            }
            Job::MouseLowBatt(p) => {
                d::mouse_set_low_batt(self.mouse()?, p)?;
                self.send(Msg::Status("Low-battery level saved".into()));
                Ok(())
            }
            Job::MouseBattery => {
                if let Some(m) = &self.mouse {
                    if let Ok((b, c)) = d::mouse_battery(m) {
                        self.send(Msg::Battery(b, c));
                    }
                }
                Ok(())
            }
        }
    }

    fn refresh(&mut self) -> Result<()> {
        let _ = self.api.refresh_devices();
        self.kb = d::open(&self.api, d::KB_PID, d::KB_IFACE).ok();
        self.mouse = d::MOUSE_PIDS.iter().find_map(|&p| d::open(&self.api, p, d::MOUSE_IFACE).ok());
        self.refresh_mouse()?;
        self.refresh_kb()
    }

    fn refresh_mouse(&mut self) -> Result<()> {
        let st = match &self.mouse {
            Some(m) => {
                let (battery, charging) = d::mouse_battery(m).unwrap_or((0, false));
                Some(MouseState {
                    fw: d::firmware(m).unwrap_or_default(),
                    stages: d::mouse_get_stages(m)?,
                    dpi: d::mouse_get_dpi(m)?,
                    polling: d::mouse_get_polling(m).unwrap_or(0),
                    battery,
                    charging,
                    sleep: d::mouse_get_sleep(m).unwrap_or(0),
                    low_batt: d::mouse_get_low_batt(m).unwrap_or(0),
                })
            }
            None => None,
        };
        self.send(Msg::Mouse(st));
        Ok(())
    }

    fn refresh_kb(&mut self) -> Result<()> {
        let Some(kb) = &self.kb else {
            self.send(Msg::Kb(None));
            return Ok(());
        };
        let profs = d::kb_profiles(kb)?;
        self.profile = profs.active;
        let (effect, color) = d::kb_get_effect(kb).unwrap_or((0, [0, 255, 0]));
        let mut st = KbState {
            fw: d::firmware(kb).unwrap_or_default(),
            profile: profs.active,
            profiles: profs.ids,
            debounce: d::kb_get_debounce(kb).unwrap_or_default(),
            polling: d::kb_get_polling(kb).unwrap_or(0),
            brightness: d::kb_get_brightness(kb).unwrap_or(255),
            effect,
            color,
            keys: HashMap::new(),
        };
        self.send(Msg::Kb(Some(st.clone())));

        // Per-key read is ~200 round trips; stream progress.
        self.assigns.clear();
        let ids = keymap::all_ids();
        let mut fails = 0;
        for (i, &k) in ids.iter().enumerate() {
            for mode in 0..=1u8 {
                match d::kb_get_key(kb, self.profile, k, mode) {
                    Ok(a) => {
                        fails = 0;
                        if mode == 0 {
                            st.keys.insert(k, (a.press(), a.release()));
                        }
                        self.assigns.insert((k, mode), a);
                    }
                    Err(e) => {
                        fails += 1;
                        if fails >= 4 {
                            anyhow::bail!("keyboard busy, stopped reading keys: {e:#}");
                        }
                    }
                }
            }
            if i % 8 == 0 {
                self.send(Msg::Progress(i as f32 / ids.len() as f32));
            }
        }
        self.send(Msg::KbKeys(st.keys));
        Ok(())
    }

    fn set_thresholds(&mut self, keys: &[u8], press: u8, release: u8) -> Result<()> {
        let kb = self.kb.as_ref().ok_or_else(|| anyhow::anyhow!("keyboard not connected"))?;
        let mut out = HashMap::new();
        let mut failed = 0;
        for (i, &k) in keys.iter().enumerate() {
            for mode in 0..=1u8 {
                let cur = match self.assigns.get(&(k, mode)) {
                    Some(a) => a.clone(),
                    None => match d::kb_get_key(kb, self.profile, k, mode) {
                        Ok(a) => a,
                        Err(_) => {
                            failed += 1;
                            continue;
                        }
                    },
                };
                match d::kb_set_thresholds(kb, &cur, press, release) {
                    Ok(a) => {
                        if mode == 0 {
                            out.insert(k, (a.press(), a.release()));
                        }
                        self.assigns.insert((k, mode), a);
                    }
                    Err(_) => failed += 1,
                }
            }
            let _ = self.tx.send(Msg::Progress(i as f32 / keys.len() as f32));
            self.ctx.request_repaint();
        }
        self.send(Msg::KbKeys(out));
        if failed > 0 {
            self.send(Msg::Error(format!("{failed} key writes failed")));
        } else {
            self.send(Msg::Status(format!("Saved actuation for {} keys to onboard profile {}", keys.len(), self.profile)));
        }
        Ok(())
    }
}
