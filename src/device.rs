//! Device commands for the Huntsman V2 Analog keyboard and Viper V3 Pro mouse.
//! Command IDs and payloads were taken from Synapse 4's own device code.

use crate::proto::{Cmd, cmd, transact};
use anyhow::{Context, Result, bail};
use hidapi::{HidApi, HidDevice};

pub const RAZER_VID: u16 = 0x1532;
pub const KB_PID: u16 = 0x0266; // Huntsman V2 Analog
pub const KB_IFACE: i32 = 3;
pub const MOUSE_PIDS: [u16; 2] = [0x00c1, 0x00c0]; // Viper V3 Pro: dongle, wired
pub const MOUSE_IFACE: i32 = 0;

pub fn open(api: &HidApi, pid: u16, iface: i32) -> Result<HidDevice> {
    let info = api
        .device_list()
        .find(|d| d.vendor_id() == RAZER_VID && d.product_id() == pid && d.interface_number() == iface)
        .with_context(|| format!("{pid:04x} not connected"))?;
    Ok(info.open_device(api)?)
}

fn be16(v: u16) -> [u8; 2] {
    v.to_be_bytes()
}
fn rd16(b: &[u8]) -> u16 {
    u16::from_be_bytes([b[0], b[1]])
}

pub fn firmware(dev: &HidDevice) -> Result<String> {
    let r = transact(dev, cmd(2, 0x00, 0x81), &[], None)?;
    Ok(format!("{}.{}", r.args()[0], r.args()[1]))
}

// ---------------------------------------------------------------- keyboard

const KB_GET_KEY: Cmd = cmd(80, 0x02, 0x92);
const KB_SET_KEY: Cmd = cmd(80, 0x02, 0x12);

/// Travel covered by the one-byte actuation value (0..=255 -> 0..3.6 mm).
pub const KB_TRAVEL_MM: f32 = 3.6;
pub const KB_DEFAULT: u8 = 0xff;

pub fn mm_to_byte(mm: f32) -> u8 {
    (mm / KB_TRAVEL_MM * 255.0).round().clamp(1.0, 254.0) as u8
}
pub fn byte_to_mm(b: u8) -> f32 {
    b as f32 * KB_TRAVEL_MM / 255.0
}

/// One key's stored assignment: [profile, key, mode, press, release, fnType, fnLen, fnData...]
#[derive(Clone, Debug)]
pub struct KeyAssign {
    pub raw: Vec<u8>,
}

impl KeyAssign {
    pub fn press(&self) -> u8 {
        self.raw[3]
    }
    pub fn release(&self) -> u8 {
        self.raw[4]
    }
}

pub fn kb_get_key(dev: &HidDevice, profile: u8, key: u8, mode: u8) -> Result<KeyAssign> {
    let r = transact(dev, KB_GET_KEY, &[profile, key, mode], None)?;
    let a = r.args();
    if a[0] != profile || a[1] != key || a[2] != mode {
        bail!("key {key}: reply mismatch");
    }
    let len = 7 + a[6] as usize;
    Ok(KeyAssign { raw: a[..len.min(80)].to_vec() })
}

/// Rewrite press/release points and keep the key's existing function.
pub fn kb_set_thresholds(dev: &HidDevice, cur: &KeyAssign, press: u8, release: u8) -> Result<KeyAssign> {
    let mut a = cur.raw.clone();
    a[3] = press;
    a[4] = release;
    transact(dev, KB_SET_KEY, &a, None)?;
    Ok(KeyAssign { raw: a })
}

/// What a key does, stored on the keyboard. Function codes from Synapse's table
/// (Off=0, ButtonCode=1, KeyCode=2, PowerKeys=9, MediaKeys=10, DoubleClick=11,
/// ModeButtonkey=12 (Hypershift), TurboModeKey=13, Win8ShortcutsKey=21).
/// Controller, joystick, launch-app and lighting remaps are Synapse software, not firmware.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum KeyFn {
    Disabled,
    /// HID usage + modifier bits (1 ctrl, 2 shift, 4 alt, 8 win)
    Key { hid: u8, mods: u8 },
    /// Key repeated while held, `interval_ms` between presses
    Turbo { hid: u8, mods: u8, interval_ms: u16 },
    /// See `MOUSE_BUTTONS`
    Mouse(u8),
    DoubleClick,
    /// USB consumer usage, see `MEDIA_KEYS`
    Media(u16),
    /// See `WIN_SHORTCUTS`
    WinShortcut(u8),
    /// See `POWER_KEYS`
    Power(u8),
    /// Hold for the Hypershift layer (like Fn)
    Hypershift,
    Other(u8),
}

pub const MOUSE_BUTTONS: [(&str, u8); 7] =
    [("Left click", 1), ("Right click", 2), ("Middle click", 3), ("Back (button 4)", 4), ("Forward (button 5)", 5), ("Scroll up", 9), ("Scroll down", 10)];
pub const MEDIA_KEYS: [(&str, u16); 10] = [
    ("Play / pause", 205),
    ("Next track", 181),
    ("Previous track", 182),
    ("Stop", 183),
    ("Volume up", 233),
    ("Volume down", 234),
    ("Mute", 226),
    ("Display brightness up", 111),
    ("Display brightness down", 112),
    ("Eject", 184),
];
pub const WIN_SHORTCUTS: [(&str, u8); 3] = [("Screenshot", 99), ("Xbox Game Bar", 100), ("Cycle apps", 101)];
pub const POWER_KEYS: [(&str, u8); 3] = [("Sleep", 130), ("Wake", 131), ("Power", 129)];

pub fn name_of<T: PartialEq + Copy>(table: &[(&'static str, T)], v: T) -> &'static str {
    table.iter().find(|x| x.1 == v).map(|x| x.0).unwrap_or("?")
}

impl KeyAssign {
    pub fn function(&self) -> KeyFn {
        let d = &self.raw[7..];
        let b = |i: usize| d.get(i).copied().unwrap_or(0);
        match self.raw[5] {
            0 => KeyFn::Disabled,
            1 => KeyFn::Mouse(b(0)),
            2 => KeyFn::Key { mods: b(0), hid: b(1) },
            9 => KeyFn::Power(b(0)),
            10 => KeyFn::Media(u16::from_be_bytes([b(0), b(1)])),
            11 => KeyFn::DoubleClick,
            12 => KeyFn::Hypershift,
            13 => KeyFn::Turbo { mods: b(0), hid: b(1), interval_ms: u16::from_be_bytes([b(2), b(3)]) },
            21 => KeyFn::WinShortcut(b(0)),
            x => KeyFn::Other(x),
        }
    }
}

/// Function bytes: [type, data length, data...]
pub fn encode_fn(f: KeyFn) -> Result<Vec<u8>> {
    Ok(match f {
        KeyFn::Disabled => vec![0, 0],
        KeyFn::Mouse(c) => vec![1, 1, c],
        KeyFn::Key { hid, mods } => vec![2, 2, mods, hid],
        KeyFn::Power(c) => vec![9, 1, c],
        KeyFn::Media(u) => {
            let [h, l] = u.to_be_bytes();
            vec![10, 2, h, l]
        }
        KeyFn::DoubleClick => vec![11, 1, 1],
        KeyFn::Hypershift => vec![12, 1, 1],
        KeyFn::Turbo { hid, mods, interval_ms } => {
            let [h, l] = interval_ms.to_be_bytes();
            vec![13, 4, mods, hid, h, l]
        }
        KeyFn::WinShortcut(c) => vec![21, 1, c],
        KeyFn::Other(_) => bail!("can't write this function type"),
    })
}

/// Rewrite what a key does and keep its press/release points.
pub fn kb_set_function(dev: &HidDevice, cur: &KeyAssign, f: KeyFn) -> Result<KeyAssign> {
    let mut a = cur.raw[..5].to_vec();
    a.extend(encode_fn(f)?);
    transact(dev, KB_SET_KEY, &a, None)?;
    Ok(KeyAssign { raw: a })
}

pub struct KbProfiles {
    pub ids: Vec<u8>,
    pub active: u8,
}

pub fn kb_profiles(dev: &HidDevice) -> Result<KbProfiles> {
    let list = transact(dev, cmd(80, 0x05, 0x81), &[], None)?;
    let n = list.args()[0] as usize;
    let ids = list.args()[1..1 + n.min(16)].to_vec();
    let active = transact(dev, cmd(1, 0x05, 0x84), &[], None)?.args()[0];
    Ok(KbProfiles { ids, active })
}

/// [press debounce, press mask, release debounce, release mask] in ms.
pub fn kb_get_debounce(dev: &HidDevice) -> Result<[u8; 4]> {
    let r = transact(dev, cmd(4, 0x02, 0x82), &[], None)?;
    Ok(r.args()[..4].try_into().unwrap())
}
pub fn kb_set_debounce(dev: &HidDevice, v: [u8; 4]) -> Result<()> {
    transact(dev, cmd(4, 0x02, 0x02), &v, None).map(|_| ())
}

/// Classic polling codes: 1 = 1000 Hz, 2 = 500 Hz, 8 = 125 Hz.
pub fn kb_get_polling(dev: &HidDevice) -> Result<u8> {
    Ok(transact(dev, cmd(1, 0x00, 0x85), &[], None)?.args()[0])
}
pub fn kb_set_polling(dev: &HidDevice, code: u8) -> Result<()> {
    transact(dev, cmd(1, 0x00, 0x05), &[code], None).map(|_| ())
}

pub fn kb_get_brightness(dev: &HidDevice) -> Result<u8> {
    Ok(transact(dev, cmd(3, 0x0f, 0x84), &[1, 0], None)?.args()[2])
}
pub fn kb_set_brightness(dev: &HidDevice, v: u8) -> Result<()> {
    transact(dev, cmd(3, 0x0f, 0x04), &[1, 0, v], None).map(|_| ())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    Off,
    Static,
    Breathing,
    BreathingRandom,
    Spectrum,
    WaveRight,
    WaveLeft,
    Reactive,
    Starlight,
    StarlightRandom,
}

impl Default for Effect {
    fn default() -> Self {
        Effect::Static
    }
}

impl Effect {
    pub fn from_code(c: u8) -> Self {
        match c {
            0 => Effect::Off,
            2 => Effect::Breathing,
            3 => Effect::Spectrum,
            4 => Effect::WaveRight,
            5 => Effect::Reactive,
            7 => Effect::Starlight,
            _ => Effect::Static,
        }
    }
    pub const ALL: [Effect; 10] = [
        Effect::Static,
        Effect::Spectrum,
        Effect::WaveRight,
        Effect::WaveLeft,
        Effect::Breathing,
        Effect::BreathingRandom,
        Effect::Reactive,
        Effect::Starlight,
        Effect::StarlightRandom,
        Effect::Off,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Effect::Off => "Off",
            Effect::Static => "Static",
            Effect::Breathing => "Breathing",
            Effect::BreathingRandom => "Breathing (random)",
            Effect::Spectrum => "Spectrum cycle",
            Effect::WaveRight => "Wave right",
            Effect::WaveLeft => "Wave left",
            Effect::Reactive => "Reactive",
            Effect::Starlight => "Starlight",
            Effect::StarlightRandom => "Starlight (random)",
        }
    }
    pub fn uses_color(self) -> bool {
        matches!(self, Effect::Static | Effect::Breathing | Effect::Reactive | Effect::Starlight)
    }
    pub fn uses_speed(self) -> bool {
        matches!(
            self,
            Effect::WaveLeft | Effect::WaveRight | Effect::Reactive | Effect::Starlight | Effect::StarlightRandom
        )
    }
}

/// Extended-matrix effect (class 0x0F id 0x02). `speed` 1 (fast) ..= 3 (slow).
pub fn kb_set_effect(dev: &HidDevice, e: Effect, rgb: [u8; 3], speed: u8) -> Result<()> {
    let [r, g, b] = rgb;
    let s = speed.clamp(1, 3);
    let args: Vec<u8> = match e {
        Effect::Off => vec![1, 0, 0x00],
        Effect::Static => vec![1, 0, 0x01, 0, 0, 1, r, g, b],
        Effect::Breathing => vec![1, 0, 0x02, 1, 0, 1, r, g, b],
        Effect::BreathingRandom => vec![1, 0, 0x02, 0, 0, 0],
        Effect::Spectrum => vec![1, 0, 0x03],
        Effect::WaveRight => vec![1, 0, 0x04, 1, [0x10, 0x28, 0x50][s as usize - 1]],
        Effect::WaveLeft => vec![1, 0, 0x04, 2, [0x10, 0x28, 0x50][s as usize - 1]],
        Effect::Reactive => vec![1, 0, 0x05, 0, s, 1, r, g, b],
        Effect::Starlight => vec![1, 0, 0x07, 0, s, 1, r, g, b],
        Effect::StarlightRandom => vec![1, 0, 0x07, 0, s, 0],
    };
    transact(dev, cmd(0x0c, 0x0f, 0x02), &args, None).map(|_| ())
}

pub fn kb_get_effect(dev: &HidDevice) -> Result<(u8, [u8; 3])> {
    let r = transact(dev, cmd(80, 0x0f, 0x82), &[1, 0], None)?;
    let a = r.args();
    Ok((a[2], [a[6], a[7], a[8]]))
}

// ---------------------------------------------------------------- mouse

#[derive(Clone, Debug, Default)]
pub struct DpiStages {
    pub active: u8,
    /// (x, y) per stage, 1..=5 stages
    pub stages: Vec<(u16, u16)>,
}

pub fn mouse_get_stages(dev: &HidDevice) -> Result<DpiStages> {
    let r = transact(dev, cmd(80, 0x04, 0x86), &[1], None)?;
    let a = r.args();
    let count = (a[2] as usize).min(5);
    let mut stages = Vec::new();
    for i in 0..count {
        let o = 3 + i * 7;
        stages.push((rd16(&a[o + 1..]), rd16(&a[o + 3..])));
    }
    Ok(DpiStages { active: a[1], stages })
}

pub fn mouse_set_stages(dev: &HidDevice, s: &DpiStages) -> Result<()> {
    let n = s.stages.len().clamp(1, 5);
    let mut a = vec![1u8, s.active.clamp(1, n as u8), n as u8];
    for (i, &(x, y)) in s.stages.iter().take(n).enumerate() {
        a.push(i as u8 + 1);
        a.extend(be16(x));
        a.extend(be16(y));
        a.extend([0, 0]);
    }
    transact(dev, cmd(80, 0x04, 0x06), &a, None)?;
    // Also make the active stage's DPI current right away.
    let (x, y) = s.stages[(s.active as usize).clamp(1, n) - 1];
    let mut d = vec![1u8];
    d.extend(be16(x));
    d.extend(be16(y));
    d.extend([0, 0]);
    transact(dev, cmd(7, 0x04, 0x05), &d, None).map(|_| ())
}

pub fn mouse_get_dpi(dev: &HidDevice) -> Result<(u16, u16)> {
    let r = transact(dev, cmd(7, 0x04, 0x85), &[1], None)?;
    Ok((rd16(&r.args()[1..]), rd16(&r.args()[3..])))
}

/// HyperPolling codes.
pub const MOUSE_RATES: [(u16, u8); 7] =
    [(8000, 0x01), (4000, 0x02), (2000, 0x04), (1000, 0x08), (500, 0x10), (250, 0x20), (125, 0x40)];

pub fn mouse_get_polling(dev: &HidDevice) -> Result<u8> {
    Ok(transact(dev, cmd(2, 0x00, 0xc0), &[], None)?.args()[1])
}
pub fn mouse_set_polling(dev: &HidDevice, code: u8) -> Result<()> {
    transact(dev, cmd(2, 0x00, 0x40), &[0, code], None).map(|_| ())
}

/// Battery 0..=100 %, charging flag.
pub fn mouse_battery(dev: &HidDevice) -> Result<(u8, bool)> {
    let lvl = transact(dev, cmd(2, 0x07, 0x80), &[], None)?.args()[1];
    let chg = transact(dev, cmd(2, 0x07, 0x84), &[], None)?.args()[1];
    Ok(((lvl as u32 * 100 / 255) as u8, chg != 0))
}

pub fn mouse_get_sleep(dev: &HidDevice) -> Result<u16> {
    Ok(rd16(transact(dev, cmd(2, 0x07, 0x83), &[], None)?.args()))
}
pub fn mouse_set_sleep(dev: &HidDevice, secs: u16) -> Result<()> {
    transact(dev, cmd(2, 0x07, 0x03), &be16(secs.max(60)), None).map(|_| ())
}

/// Low battery warning, 0..=100 %.
pub fn mouse_get_low_batt(dev: &HidDevice) -> Result<u8> {
    let v = transact(dev, cmd(1, 0x07, 0x81), &[], None)?.args()[0];
    Ok(((v as u32 * 100 + 127) / 255) as u8)
}
pub fn mouse_set_low_batt(dev: &HidDevice, pct: u8) -> Result<()> {
    let v = (pct.min(100) as u32 * 255 / 100) as u8;
    transact(dev, cmd(1, 0x07, 0x01), &[v], None).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actuation_bytes_match_synapse() {
        // Synapse stored 2.0 mm as 0x8f-ish and 3.6 mm is full travel.
        assert_eq!(mm_to_byte(3.6), 254);
        assert!((byte_to_mm(mm_to_byte(2.0)) - 2.0).abs() < 0.02);
        assert!((byte_to_mm(0x8f) - 2.02).abs() < 0.02);
        assert_ne!(mm_to_byte(0.0), KB_DEFAULT);
        assert_ne!(mm_to_byte(9.0), KB_DEFAULT);
    }

    #[test]
    fn every_function_round_trips() {
        let fns = [
            KeyFn::Disabled,
            KeyFn::Key { hid: 0x1a, mods: 3 },
            KeyFn::Turbo { hid: 4, mods: 0, interval_ms: 142 },
            KeyFn::Mouse(5),
            KeyFn::DoubleClick,
            KeyFn::Media(233),
            KeyFn::WinShortcut(99),
            KeyFn::Power(130),
            KeyFn::Hypershift,
        ];
        for f in fns {
            let mut raw = vec![1, 18, 0, 0xff, 0xff];
            raw.extend(encode_fn(f).unwrap());
            assert_eq!(raw[6] as usize, raw.len() - 7, "length byte for {f:?}");
            assert_eq!(KeyAssign { raw }.function(), f);
        }
    }

    #[test]
    fn key_function_round_trip() {
        let a = KeyAssign { raw: vec![1, 18, 0, 0xff, 0xff, 2, 2, 0, 0x1a] };
        assert_eq!(a.function(), KeyFn::Key { hid: 0x1a, mods: 0 });
        assert_eq!(a.press(), 0xff);
        let off = KeyAssign { raw: vec![1, 18, 0, 0xff, 0xff, 0, 0] };
        assert_eq!(off.function(), KeyFn::Disabled);
    }
}
