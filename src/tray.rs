//! Tray icon + the main loop. Owns the main thread.
//! With no window open the process is just this loop + the device thread (no GPU context).
//! 1 click opens the popover, 2 clicks open the full window, right-click shows the menu.

use crate::state::{Shared, Snapshot};
use crate::ui::Mode;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use windows_sys::Win32::System::Threading::{GetCurrentThreadId, INFINITE};
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TrayMsg {
    Click,
    Double,
    Open,
    ToggleAutostart,
    Quit,
    /// From a second instance or the dev MCP server
    Show(Mode),
}

static EVENTS: Mutex<VecDeque<TrayMsg>> = Mutex::new(VecDeque::new());
static MAIN_TID: Mutex<u32> = Mutex::new(0);
/// The open window's egui context, so tray events can wake it.
pub static UI_CTX: Mutex<Option<egui::Context>> = Mutex::new(None);

const WM_WAKE: u32 = WM_APP + 1;

/// Debug log to the temp dir (razerlite.log) when RAZERLITE_LOG is set.
pub fn dlog(msg: impl AsRef<str>) {
    if std::env::var_os("RAZERLITE_LOG").is_none() {
        return;
    }
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(std::env::temp_dir().join("razerlite.log")) {
        let _ = writeln!(f, "[{}] {}", std::process::id(), msg.as_ref());
    }
}

pub fn post(m: TrayMsg) {
    dlog(format!("post {m:?}"));
    EVENTS.lock().unwrap().push_back(m);
    wake_main();
    if let Some(c) = UI_CTX.lock().unwrap().as_ref() {
        c.request_repaint();
    }
}

fn wake_main() {
    let tid = *MAIN_TID.lock().unwrap();
    if tid != 0 {
        unsafe { PostThreadMessageW(tid, WM_WAKE, 0, 0) };
    }
}

pub fn take_events() -> Vec<TrayMsg> {
    EVENTS.lock().unwrap().drain(..).collect()
}

struct Tray {
    icon: TrayIcon,
    autostart: CheckMenuItem,
    last: (String, u8),
}

thread_local! {
    static TRAY: RefCell<Option<Tray>> = const { RefCell::new(None) };
}

/// Update tooltip + icon from device state. Cheap, safe to call every frame.
pub fn sync(s: &Snapshot) {
    TRAY.with(|t| {
        let mut t = t.borrow_mut();
        let Some(t) = t.as_mut() else { return };
        let mut tip = Vec::new();
        if let Some(m) = &s.mouse {
            tip.push(format!("Viper V3 Pro {}%{}", m.battery, if m.charging { " ⚡" } else { "" }));
        }
        if s.kb.is_some() {
            tip.push("Huntsman V2 Analog".into());
        }
        let tip = if tip.is_empty() { "RazerLite · no devices".to_string() } else { format!("RazerLite\n{}", tip.join("\n")) };
        let low = s.mouse.as_ref().is_some_and(|m| !m.charging && m.battery <= m.low_batt.max(5));
        let state = if s.kb.is_none() && s.mouse.is_none() { 2 } else if low { 1 } else { 0 };
        if t.last.0 != tip {
            let _ = t.icon.set_tooltip(Some(&tip));
        }
        if t.last.1 != state {
            let _ = t.icon.set_icon(Some(make_icon(state)));
        }
        t.last = (tip, state);
    });
}

/// 32×32 RGBA icon: green tile, amber dot when low (1), grey when no devices (2).
pub fn icon_rgba(state: u8, size: u32) -> Vec<u8> {
    let n = size as f32;
    let mut px = vec![0u8; (size * size * 4) as usize];
    for y in 0..size {
        for x in 0..size {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            let r = n * 0.22;
            let dx = (fx - n / 2.0).abs() - (n / 2.0 - r - n * 0.04);
            let dy = (fy - n / 2.0).abs() - (n / 2.0 - r - n * 0.04);
            let dist = (dx.max(0.0).powi(2) + dy.max(0.0).powi(2)).sqrt() - r;
            let a = (0.5 - dist).clamp(0.0, 1.0);
            if a <= 0.0 {
                continue;
            }
            let t = fy / n;
            let (mut cr, mut cg, mut cb) = (0x44 as f32 * (1.0 - t) + 0x17 as f32 * t, 0xd6 as f32 * (1.0 - t) + 0x8a as f32 * t, 0x2c as f32 * (1.0 - t) + 0x10 as f32 * t);
            // three-prong mark
            let (cx, cy) = (fx - n / 2.0, fy - n / 2.0);
            let ang = cy.atan2(cx);
            let rad = (cx * cx + cy * cy).sqrt();
            let prong = ((ang * 3.0).sin()).abs() > 0.82 && rad > n * 0.12 && rad < n * 0.34;
            let hub = rad < n * 0.1;
            if prong || hub {
                (cr, cg, cb) = (12.0, 24.0, 10.0);
            }
            if state == 2 {
                let g = (cr * 0.3 + cg * 0.59 + cb * 0.11) * 0.7;
                (cr, cg, cb) = (g, g, g);
            }
            let i = ((y * size + x) * 4) as usize;
            px[i..i + 4].copy_from_slice(&[cr as u8, cg as u8, cb as u8, (a * 255.0) as u8]);
        }
    }
    if state == 1 {
        let (cx, cy, r) = (n * 0.78, n * 0.78, n * 0.2);
        for y in 0..size {
            for x in 0..size {
                let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
                if d < r + 1.5 {
                    let i = ((y * size + x) * 4) as usize;
                    let c = if d < r { [0xea, 0xb3, 0x08, 255] } else { [0x1c, 0x1d, 0x1f, 255] };
                    px[i..i + 4].copy_from_slice(&c);
                }
            }
        }
    }
    px
}

fn make_icon(state: u8) -> Icon {
    Icon::from_rgba(icon_rgba(state, 32), 32, 32).expect("icon")
}

/// Run the tray until Quit. `open_first` opens a window right away (normal launch).
pub fn run(shared: Arc<Shared>, open_first: Option<Mode>) {
    *MAIN_TID.lock().unwrap() = unsafe { GetCurrentThreadId() };

    let menu = Menu::new();
    let open = MenuItem::new("Open RazerLite", true, None);
    let autostart = CheckMenuItem::new("Start with Windows", true, crate::config::autostart_enabled(), None);
    let quit = MenuItem::new("Quit", true, None);
    let _ = menu.append_items(&[&open, &autostart, &PredefinedMenuItem::separator(), &quit]);
    let ids = (open.id().clone(), autostart.id().clone(), quit.id().clone());

    let icon = TrayIconBuilder::new()
        .with_icon(make_icon(0))
        .with_tooltip("RazerLite")
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .build()
        .expect("tray icon");
    TRAY.with(|t| *t.borrow_mut() = Some(Tray { icon, autostart, last: (String::new(), 0) }));

    TrayIconEvent::set_event_handler(Some(|e: TrayIconEvent| match e {
        TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } => post(TrayMsg::Click),
        TrayIconEvent::DoubleClick { button: MouseButton::Left, .. } => post(TrayMsg::Double),
        _ => {}
    }));
    let (oid, aid, qid) = ids;
    MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
        if e.id == oid {
            post(TrayMsg::Open)
        } else if e.id == aid {
            post(TrayMsg::ToggleAutostart)
        } else if e.id == qid {
            post(TrayMsg::Quit)
        }
    }));
    // Device changes: refresh tooltip/icon from the main thread.
    let waker = shared.add_waker(Arc::new(wake_main));

    let dbl = Duration::from_millis(unsafe { windows_sys::Win32::UI::Input::KeyboardAndMouse::GetDoubleClickTime() } as u64);
    let mut next = open_first;
    let mut pending_click: Option<Instant> = None;
    let mut ignore_clicks_until = Instant::now();
    let mut child: Option<Child> = None;

    'main: loop {
        if let Some(mode) = next.take() {
            match &child {
                Some(c) if c.mode == mode => c.focus(),
                _ => {
                    if let Some(c) = child.take() {
                        c.kill();
                    }
                    child = Child::spawn(mode);
                }
            }
        }
        sync(&shared.get());

        // Wait for a message, the window process to exit, or a pending single click to become a popover.
        let timeout = match pending_click {
            Some(t) => dbl.saturating_sub(t.elapsed()).as_millis() as u32,
            None => INFINITE,
        };
        let handles: Vec<_> = child.iter().map(|c| c.handle).collect();
        let r = unsafe { MsgWaitForMultipleObjects(handles.len() as u32, handles.as_ptr(), 0, timeout, QS_ALLINPUT) };
        if !handles.is_empty() && r == 0 {
            let c = child.take().unwrap();
            let code = c.exit_code();
            dlog(format!("child exited {code}"));
            match code {
                EXIT_OPEN_FULL => next = Some(Mode::Full),
                EXIT_OPEN_POPOVER => next = Some(Mode::Popover),
                EXIT_OPEN_SETTINGS => {
                    *NEXT_TAB.lock().unwrap() = Some("settings");
                    next = Some(Mode::Full)
                }
                EXIT_QUIT => break 'main,
                _ => {}
            }
            ignore_clicks_until = Instant::now() + Duration::from_millis(150);
            // Pick up changes the window made (battery, DPI…).
            shared.send(crate::state::Job::Refresh);
        }
        unsafe {
            let mut msg: MSG = std::mem::zeroed();
            while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                if msg.message == WM_QUIT {
                    break 'main;
                }
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }

        for ev in take_events() {
            dlog(format!("event {ev:?}"));
            let open_mode = child.as_ref().map(|c| c.mode);
            match ev {
                TrayMsg::Click => {
                    if Instant::now() > ignore_clicks_until {
                        pending_click = Some(Instant::now());
                    }
                }
                TrayMsg::Double => {
                    pending_click = None;
                    ignore_clicks_until = Instant::now() + dbl;
                    next = Some(Mode::Full);
                }
                TrayMsg::Open => next = Some(Mode::Full),
                TrayMsg::Show(m) => next = Some(m),
                TrayMsg::ToggleAutostart => toggle_autostart(),
                TrayMsg::Quit => break 'main,
            }
            let _ = open_mode;
        }
        if pending_click.is_some_and(|t| t.elapsed() >= dbl) && next.is_none() {
            pending_click = None;
            match child.as_ref().map(|c| c.mode) {
                // A click while the popover is open closes it (it usually closed itself on focus loss).
                Some(Mode::Popover) => {
                    if let Some(c) = child.take() {
                        c.kill();
                    }
                }
                Some(Mode::Full) => child.as_ref().unwrap().focus(),
                None => next = Some(Mode::Popover),
            }
        }
    }
    if let Some(c) = child.take() {
        c.kill();
    }
    shared.remove_waker(waker);
    TRAY.with(|t| *t.borrow_mut() = None);
}

// ---------------------------------------------------------------- window process

pub const EXIT_CLOSED: u32 = 0;
pub const EXIT_OPEN_FULL: u32 = 10;
pub const EXIT_OPEN_POPOVER: u32 = 11;
pub const EXIT_OPEN_SETTINGS: u32 = 12;
pub const EXIT_QUIT: u32 = 20;
/// Tab for the next full window (set by the popover's gear).
static NEXT_TAB: Mutex<Option<&'static str>> = Mutex::new(None);

/// Extra args passed to window processes (demo, dev MCP port).
pub static CHILD_ARGS: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// Pid of the running window process, for the dev MCP proxy.
pub static CHILD_PID: Mutex<Option<u32>> = Mutex::new(None);

/// Each window runs in its own short-lived process (`razerlite --window full|popover`):
/// closing it frees the GPU driver's memory, so the tray process stays small.
struct Child {
    mode: Mode,
    handle: windows_sys::Win32::Foundation::HANDLE,
    pid: u32,
}

impl Child {
    fn spawn(mode: Mode) -> Option<Child> {
        use windows_sys::Win32::UI::Shell::{SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW};
        let exe = std::env::current_exe().ok()?;
        let mut args = vec!["--window".to_string(), if mode == Mode::Full { "full" } else { "popover" }.to_string()];
        args.extend(CHILD_ARGS.lock().unwrap().iter().cloned());
        if let Some(t) = NEXT_TAB.lock().unwrap().take() {
            args.extend(["--tab".to_string(), t.to_string()]);
        }
        let w = |s: &str| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
        let file = w(&exe.to_string_lossy());
        let params = w(&args.join(" "));
        let verb = w("open");
        // ShellExecute starts the window like Explorer would (no inherited handles or startup info).
        let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
        info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
        info.fMask = SEE_MASK_NOCLOSEPROCESS;
        info.lpVerb = verb.as_ptr();
        info.lpFile = file.as_ptr();
        info.lpParameters = params.as_ptr();
        info.nShow = SW_SHOWNORMAL;
        if unsafe { ShellExecuteExW(&mut info) } == 0 || info.hProcess.is_null() {
            dlog("ShellExecuteExW failed");
            return None;
        }
        let pid = unsafe { windows_sys::Win32::System::Threading::GetProcessId(info.hProcess) };
        dlog(format!("spawned {mode:?} pid {pid}"));
        *CHILD_PID.lock().unwrap() = Some(pid);
        Some(Child { mode, handle: info.hProcess, pid })
    }
    fn exit_code(&self) -> u32 {
        let mut code = 0u32;
        unsafe {
            windows_sys::Win32::System::Threading::GetExitCodeProcess(self.handle, &mut code);
            windows_sys::Win32::Foundation::CloseHandle(self.handle);
        }
        *CHILD_PID.lock().unwrap() = None;
        code
    }
    fn kill(self) {
        unsafe {
            windows_sys::Win32::System::Threading::TerminateProcess(self.handle, 0);
            windows_sys::Win32::Foundation::CloseHandle(self.handle);
        }
        *CHILD_PID.lock().unwrap() = None;
    }
    fn focus(&self) {
        unsafe extern "system" fn cb(hwnd: HWND, lp: isize) -> i32 {
            let mut pid = 0u32;
            unsafe {
                GetWindowThreadProcessId(hwnd, &mut pid);
                if pid == lp as u32 && IsWindowVisible(hwnd) != 0 {
                    ShowWindow(hwnd, SW_RESTORE);
                    SetForegroundWindow(hwnd);
                    return 0;
                }
            }
            1
        }
        unsafe { EnumWindows(Some(cb), self.pid as isize) };
    }
}


pub fn toggle_autostart() {
    let on = !crate::config::autostart_enabled();
    let _ = crate::config::set_autostart(on);
    set_autostart_check(on);
}

pub fn set_autostart_check(on: bool) {
    TRAY.with(|t| {
        if let Some(t) = t.borrow().as_ref() {
            t.autostart.set_checked(on);
        }
    });
}

// ---------------------------------------------------------------- single instance

/// Returns false if another RazerLite is running (and asks it to show its window).
pub fn claim_single_instance() -> bool {
    use windows_sys::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError};
    use windows_sys::Win32::System::Threading::*;
    let name: Vec<u16> = "Local\\RazerLite.Instance\0".encode_utf16().collect();
    let ev_name: Vec<u16> = "Local\\RazerLite.Show\0".encode_utf16().collect();
    unsafe {
        let m = CreateMutexW(std::ptr::null(), 0, name.as_ptr());
        if !m.is_null() && GetLastError() == ERROR_ALREADY_EXISTS {
            let ev = OpenEventW(EVENT_MODIFY_STATE, 0, ev_name.as_ptr());
            if !ev.is_null() {
                SetEvent(ev);
            }
            return false;
        }
        // Keep the mutex for the life of the process.
        let ev = CreateEventW(std::ptr::null(), 0, 0, ev_name.as_ptr()) as usize;
        std::thread::Builder::new()
            .name("show-signal".into())
            .spawn(move || loop {
                if WaitForSingleObject(ev as _, INFINITE) == 0 {
                    post(TrayMsg::Show(Mode::Full));
                }
            })
            .ok();
    }
    true
}
