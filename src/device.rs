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

impl Effect {
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
