//! App settings and macros, stored in %APPDATA%\RazerLite\config.json.
//! "Start with Windows" lives in the registry instead (HKCU Run key).

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum MacroStep {
    Down(String),
    Up(String),
    Delay(u32),
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Macro {
    pub name: String,
    pub steps: Vec<MacroStep>,
    /// 0 = recorded delays, 1 = fixed 20 ms, 2 = none
    pub delay_mode: u8,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Config {
    pub close_to_tray: bool,
    pub low_battery_toast: bool,
    pub macros: Vec<Macro>,
}

impl Default for Config {
    fn default() -> Self {
        Self { close_to_tray: true, low_battery_toast: true, macros: Vec::new() }
    }
}

fn path() -> PathBuf {
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    base.join("RazerLite").join("config.json")
}

impl Config {
    pub fn load() -> Self {
        std::fs::read_to_string(path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }
    pub fn save(&self) {
        let p = path();
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(p, s);
        }
    }
}

// ---------------------------------------------------------------- autostart

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_NAME: &str = "RazerLite";

pub fn autostart_enabled() -> bool {
    let hk = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER);
    hk.open_subkey(RUN_KEY).and_then(|k| k.get_value::<String, _>(RUN_NAME)).is_ok()
}

pub fn set_autostart(on: bool) -> std::io::Result<()> {
    let hk = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER);
    let (key, _) = hk.create_subkey(RUN_KEY)?;
    if on {
        let exe = std::env::current_exe()?;
        key.set_value(RUN_NAME, &format!("\"{}\" --tray", exe.display()))
    } else {
        match key.delete_value(RUN_NAME) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            r => r,
        }
    }
}
