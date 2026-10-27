//! Shared device state + the background I/O thread.
//! The thread outlives any window: the tray keeps it running so battery and low-battery
//! alerts work while no UI is open. Windows read `Snapshot` and send `Job`s.

use crate::device::{self as d, DpiStages, Effect, KeyAssign, KeyFn};
use crate::keymap;
use anyhow::{Result, anyhow};
use hidapi::{HidApi, HidDevice};
use std::collections::HashMap;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const BATTERY_POLL: Duration = Duration::from_secs(300);

pub enum Job {
    Refresh,
    Battery,
    KbThresholds { keys: Vec<u8>, press: u8, release: u8 },
    KbFunction { keys: Vec<u8>, mode: u8, f: Option<KeyFn> },
    KbDebounce([u8; 4]),
    KbPolling(u8),
    KbBrightness(u8),
    KbEffect(Effect, [u8; 3], u8),
    MouseStages(DpiStages),
    MousePolling(u8),
    MouseSleep(u16),
    MouseLowBatt(u8),
}

#[derive(Clone, Default)]
pub struct KbState {
    pub fw: String,
    pub profile: u8,
    pub debounce: [u8; 4],
    pub polling: u8,
    pub brightness: u8,
    pub effect: Effect,
    pub color: [u8; 3],
    /// key id -> (press, release) bytes, normal layer
    pub thresholds: HashMap<u8, (u8, u8)>,
    /// (key id, layer) -> function
    pub functions: HashMap<(u8, u8), KeyFn>,
    pub keys_loaded: bool,
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

#[derive(Clone, Default)]
pub struct Snapshot {
    pub kb: Option<KbState>,
    pub mouse: Option<MouseState>,
    pub busy: bool,
    pub progress: f32,
    pub status: String,
    pub error: Option<String>,
    pub synapse_running: bool,
    pub demo: bool,
}

type Waker = Arc<dyn Fn() + Send + Sync>;

pub struct Shared {
    pub snap: Mutex<Snapshot>,
    jobs: Mutex<Sender<Job>>,
    wakers: Mutex<Vec<(u64, Waker)>>,
}

impl Shared {
    pub fn send(&self, j: Job) {
        {
            let mut s = self.snap.lock().unwrap();
            s.busy = true;
            s.progress = 0.0;
            s.error = None;
        }
        let _ = self.jobs.lock().unwrap().send(j);
    }
    pub fn get(&self) -> Snapshot {
        self.snap.lock().unwrap().clone()
    }
    /// Register a callback run after every state change. Returns a handle for `remove_waker`.
    pub fn add_waker(&self, f: Waker) -> u64 {
        let mut w = self.wakers.lock().unwrap();
        let id = w.iter().map(|x| x.0).max().unwrap_or(0) + 1;
        w.push((id, f));
        id
    }
    pub fn remove_waker(&self, id: u64) {
        self.wakers.lock().unwrap().retain(|x| x.0 != id);
    }
    fn wake(&self) {
        let w: Vec<Waker> = self.wakers.lock().unwrap().iter().map(|x| x.1.clone()).collect();
        for f in w {
            f();
        }
    }
    fn update(&self, f: impl FnOnce(&mut Snapshot)) {
        f(&mut self.snap.lock().unwrap());
        self.wake();
    }
}

/// `light` = tray process: no per-key reads, owns low-battery toasts.
pub fn spawn(demo: bool, light: bool) -> Arc<Shared> {
    let (tx, rx) = std::sync::mpsc::channel();
    let shared = Arc::new(Shared {
        snap: Mutex::new(Snapshot { demo, busy: true, status: "Reading devices…".into(), ..Default::default() }),
        jobs: Mutex::new(tx.clone()),
        wakers: Mutex::new(Vec::new()),
    });
    let s2 = shared.clone();
    std::thread::Builder::new()
        .name("razer-io".into())
        .spawn(move || {
            if demo {
                demo_loop(s2, rx);
            } else {
                io_loop(s2, rx, light);
            }
        })
        .expect("spawn io thread");
    let _ = tx.send(Job::Refresh);
    shared
}

// ---------------------------------------------------------------- real devices

struct Io {
    api: HidApi,
    kb: Option<HidDevice>,
    mouse: Option<HidDevice>,
    profile: u8,
    assigns: HashMap<(u8, u8), KeyAssign>,
    shared: Arc<Shared>,
    low_alerted: bool,
    light: bool,
}

fn io_loop(shared: Arc<Shared>, rx: Receiver<Job>, light: bool) {
    let api = match HidApi::new() {
        Ok(a) => a,
        Err(e) => {
            shared.update(|s| s.error = Some(format!("HID init failed: {e}")));
            return;
        }
    };
    let mut io = Io { api, kb: None, mouse: None, profile: 1, assigns: HashMap::new(), shared, low_alerted: false, light };
    loop {
        let job = match rx.recv_timeout(BATTERY_POLL) {
            Ok(j) => j,
            Err(RecvTimeoutError::Timeout) => Job::Battery,
            Err(RecvTimeoutError::Disconnected) => return,
        };
        let res = io.run(job);
        io.shared.update(|s| {
            if let Err(e) = res {
                s.error = Some(format!("{e:#}"));
            }
            s.busy = false;
            s.progress = 1.0;
            s.synapse_running = synapse_running();
        });
    }
}

impl Io {
    fn kb(&self) -> Result<&HidDevice> {
        self.kb.as_ref().ok_or_else(|| anyhow!("keyboard not connected"))
    }
    fn mouse(&self) -> Result<&HidDevice> {
        self.mouse.as_ref().ok_or_else(|| anyhow!("mouse not connected"))
    }
    fn status(&self, s: impl Into<String>) {
        let s = s.into();
        self.shared.update(|x| x.status = s);
    }
    fn progress(&self, p: f32) {
        self.shared.update(|x| x.progress = p);
    }
    fn kb_mut(&self, f: impl FnOnce(&mut KbState)) {
        self.shared.update(|x| {
            if let Some(k) = &mut x.kb {
                f(k)
            }
        });
    }
    fn mouse_mut(&self, f: impl FnOnce(&mut MouseState)) {
        self.shared.update(|x| {
            if let Some(m) = &mut x.mouse {
                f(m)
            }
        });
    }

    fn run(&mut self, job: Job) -> Result<()> {
        match job {
            Job::Refresh => self.refresh(),
            Job::Battery => self.battery(),
            Job::KbThresholds { keys, press, release } => self.thresholds(&keys, press, release),
            Job::KbFunction { keys, mode, f } => self.functions(&keys, mode, f),
            Job::KbDebounce(v) => {
                d::kb_set_debounce(self.kb()?, v)?;
                self.kb_mut(|k| k.debounce = v);
                self.status("Debounce saved");
                Ok(())
            }
            Job::KbPolling(c) => {
                d::kb_set_polling(self.kb()?, c)?;
                self.kb_mut(|k| k.polling = c);
                self.status("Keyboard polling rate saved");
                Ok(())
            }
            Job::KbBrightness(v) => {
                d::kb_set_brightness(self.kb()?, v)?;
                self.kb_mut(|k| k.brightness = v);
                Ok(())
            }
            Job::KbEffect(e, rgb, s) => {
                d::kb_set_effect(self.kb()?, e, rgb, s)?;
                self.kb_mut(|k| {
                    k.effect = e;
                    k.color = rgb
                });
                self.status(format!("Lighting: {}", e.name()));
                Ok(())
            }
            Job::MouseStages(s) => {
                d::mouse_set_stages(self.mouse()?, &s)?;
                self.status("DPI saved");
                self.refresh_mouse()
            }
            Job::MousePolling(c) => {
                d::mouse_set_polling(self.mouse()?, c)?;
                self.mouse_mut(|m| m.polling = c);
                self.status("Mouse polling rate saved");
                Ok(())
            }
            Job::MouseSleep(s) => {
                d::mouse_set_sleep(self.mouse()?, s)?;
                self.mouse_mut(|m| m.sleep = s);
                self.status("Sleep timer saved");
                Ok(())
            }
            Job::MouseLowBatt(p) => {
                d::mouse_set_low_batt(self.mouse()?, p)?;
                self.mouse_mut(|m| m.low_batt = p);
                self.status("Low-battery level saved");
                Ok(())
            }
        }
    }

    fn refresh(&mut self) -> Result<()> {
        let _ = self.api.refresh_devices();
        self.kb = d::open(&self.api, d::KB_PID, d::KB_IFACE).ok();
        self.mouse = d::MOUSE_PIDS.iter().find_map(|&p| d::open(&self.api, p, d::MOUSE_IFACE).ok());
        self.status("Reading devices…");
        let mres = self.refresh_mouse();
        let kres = self.refresh_kb();
        self.status("Ready");
        mres.and(kres)
    }

    fn battery(&mut self) -> Result<()> {
        let Some(m) = &self.mouse else { return Ok(()) };
        let (b, c) = d::mouse_battery(m)?;
        self.mouse_mut(|x| {
            x.battery = b;
            x.charging = c
        });
        let warn = self.shared.get().mouse.map(|m| m.low_batt).unwrap_or(15).max(5);
        if b <= warn && !c && !self.low_alerted {
            self.low_alerted = true;
            if self.light && crate::config::Config::load().low_battery_toast {
                toast("Viper V3 Pro battery low", &format!("{b}% left. Plug it in soon."));
            }
        } else if b > warn + 5 || c {
            self.low_alerted = false;
        }
        Ok(())
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
        self.shared.update(|s| s.mouse = st);
        Ok(())
    }

    fn refresh_kb(&mut self) -> Result<()> {
        let Some(kb) = &self.kb else {
            self.shared.update(|s| s.kb = None);
            return Ok(());
        };
        let profs = d::kb_profiles(kb)?;
        self.profile = profs.active;
        let (eff, color) = d::kb_get_effect(kb).unwrap_or((1, [68, 214, 44]));
        let st = KbState {
            fw: d::firmware(kb).unwrap_or_default(),
            profile: profs.active,
            debounce: d::kb_get_debounce(kb).unwrap_or_default(),
            polling: d::kb_get_polling(kb).unwrap_or(1),
            brightness: d::kb_get_brightness(kb).unwrap_or(255),
            effect: Effect::from_code(eff),
            color,
            ..Default::default()
        };
        self.shared.update(|s| s.kb = Some(st));
        if self.light {
            return Ok(());
        }

        self.assigns.clear();
        let ids = keymap::all_ids();
        let (mut th, mut fns) = (HashMap::new(), HashMap::new());
        let mut fails = 0;
        for (i, &k) in ids.iter().enumerate() {
            for mode in 0..=1u8 {
                match d::kb_get_key(kb, self.profile, k, mode) {
                    Ok(a) => {
                        fails = 0;
                        if mode == 0 {
                            th.insert(k, (a.press(), a.release()));
                        }
                        fns.insert((k, mode), a.function());
                        self.assigns.insert((k, mode), a);
                    }
                    Err(e) => {
                        fails += 1;
                        if fails >= 4 {
                            return Err(anyhow!("keyboard busy, stopped reading keys: {e:#}"));
                        }
                    }
                }
            }
            if i % 10 == 0 {
                self.progress(i as f32 / ids.len() as f32);
            }
        }
        self.kb_mut(|k| {
            k.thresholds = th;
            k.functions = fns;
            k.keys_loaded = true;
        });
        Ok(())
    }

    fn assign(&mut self, k: u8, mode: u8) -> Result<KeyAssign> {
        if let Some(a) = self.assigns.get(&(k, mode)) {
            return Ok(a.clone());
        }
        let a = d::kb_get_key(self.kb()?, self.profile, k, mode)?;
        self.assigns.insert((k, mode), a.clone());
        Ok(a)
    }

    fn thresholds(&mut self, keys: &[u8], press: u8, release: u8) -> Result<()> {
        let mut failed = 0;
        for (i, &k) in keys.iter().enumerate() {
            for mode in 0..=1u8 {
                let r = self.assign(k, mode).and_then(|cur| d::kb_set_thresholds(self.kb()?, &cur, press, release));
                match r {
                    Ok(a) => {
                        self.assigns.insert((k, mode), a);
                    }
                    Err(_) => failed += 1,
                }
            }
            self.kb_mut(|s| {
                s.thresholds.insert(k, (press, release));
            });
            self.progress(i as f32 / keys.len() as f32);
        }
        if failed > 0 {
            return Err(anyhow!("{failed} key writes failed"));
        }
        self.status(format!("Saved actuation for {} keys", keys.len()));
        Ok(())
    }

    fn functions(&mut self, keys: &[u8], mode: u8, f: Option<KeyFn>) -> Result<()> {
        for &k in keys {
            if k == keymap::FN_KEY {
                continue;
            }
            let target = match f {
                Some(f) => f,
                None => KeyFn::Key { hid: keymap::default_hid(k).ok_or_else(|| anyhow!("no default for key {k}"))?, mods: 0 },
            };
            let cur = self.assign(k, mode)?;
            let a = d::kb_set_function(self.kb()?, &cur, target)?;
            self.assigns.insert((k, mode), a);
            self.kb_mut(|s| {
                s.functions.insert((k, mode), target);
            });
        }
        self.status(format!("Saved {} key{}", keys.len(), if keys.len() == 1 { "" } else { "s" }));
        Ok(())
    }
}

// ---------------------------------------------------------------- demo devices

/// Fake devices for UI work and screenshots (`--demo`). No HID access at all.
fn demo_loop(shared: Arc<Shared>, rx: Receiver<Job>) {
    let mut kb = KbState {
        fw: "1.6".into(),
        profile: 1,
        debounce: [0, 0, 4, 0],
        polling: 1,
        brightness: 220,
        effect: Effect::Static,
        color: [68, 214, 44],
        keys_loaded: true,
        ..Default::default()
    };
    for k in keymap::all_ids() {
        let v = if keymap::WASD.contains(&k) { (d::mm_to_byte(1.5), d::mm_to_byte(1.3)) } else { (d::mm_to_byte(2.0), d::mm_to_byte(1.6)) };
        kb.thresholds.insert(k, v);
        for m in 0..=1 {
            kb.functions.insert((k, m), KeyFn::Key { hid: keymap::default_hid(k).unwrap_or(0), mods: 0 });
        }
    }
    let mut mouse = MouseState {
        fw: "1.11".into(),
        stages: DpiStages { active: 3, stages: vec![(400, 400), (800, 800), (1200, 1200), (1600, 1600)] },
        dpi: (1200, 1200),
        polling: 0x08,
        battery: 98,
        charging: false,
        sleep: 120,
        low_batt: 30,
    };
    loop {
        let job = match rx.recv_timeout(BATTERY_POLL) {
            Ok(j) => j,
            Err(RecvTimeoutError::Timeout) => Job::Battery,
            Err(RecvTimeoutError::Disconnected) => return,
        };
        std::thread::sleep(Duration::from_millis(120));
        let mut status = "Saved (demo)".to_string();
        match job {
            Job::Refresh => status = "Ready (demo devices)".into(),
            Job::Battery => {}
            Job::KbThresholds { keys, press, release } => {
                for k in keys {
                    kb.thresholds.insert(k, (press, release));
                }
            }
            Job::KbFunction { keys, mode, f } => {
                for k in keys {
                    let f = f.unwrap_or(KeyFn::Key { hid: keymap::default_hid(k).unwrap_or(0), mods: 0 });
                    kb.functions.insert((k, mode), f);
                }
            }
            Job::KbDebounce(v) => kb.debounce = v,
            Job::KbPolling(c) => kb.polling = c,
            Job::KbBrightness(v) => kb.brightness = v,
            Job::KbEffect(e, c, _) => {
                kb.effect = e;
                kb.color = c
            }
            Job::MouseStages(s) => {
                mouse.dpi = s.stages[(s.active as usize).clamp(1, s.stages.len()) - 1];
                mouse.stages = s
            }
            Job::MousePolling(c) => mouse.polling = c,
            Job::MouseSleep(s) => mouse.sleep = s,
            Job::MouseLowBatt(p) => mouse.low_batt = p,
        }
        let (k, m) = (kb.clone(), mouse.clone());
        shared.update(|s| {
            s.kb = Some(k);
            s.mouse = Some(m);
            s.busy = false;
            s.progress = 1.0;
            s.status = status;
        });
    }
}

// ---------------------------------------------------------------- helpers

pub fn synapse_running() -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::*;
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            return false;
        }
        let mut e: PROCESSENTRY32W = std::mem::zeroed();
        e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut found = false;
        if Process32FirstW(snap, &mut e) != 0 {
            loop {
                let n = String::from_utf16_lossy(&e.szExeFile[..e.szExeFile.iter().position(|&c| c == 0).unwrap_or(0)]);
                if n.eq_ignore_ascii_case("RazerAppEngine.exe") {
                    found = true;
                    break;
                }
                if Process32NextW(snap, &mut e) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snap);
        found
    }
}

pub fn toast(title: &str, body: &str) {
    let _ = tauri_winrt_notification::Toast::new(tauri_winrt_notification::Toast::POWERSHELL_APP_ID)
        .title(title)
        .text1(body)
        .show();
}
