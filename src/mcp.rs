//! `razerlite --mcp`: a stdio MCP server for development.
//! Lets an agent read device state, change settings, and send raw protocol commands
//! (for working out new features) without the GUI.
//!
//! Register with Claude Code:
//!   claude mcp add razerlite -- X:\Dev\razerlite\target\release\razerlite.exe --mcp

use crate::device::{self as d, DpiStages, Effect};
use crate::proto::{cmd, transact};
use anyhow::{Context, Result, anyhow, bail};
use hidapi::{HidApi, HidDevice};
use serde_json::{Value, json};
use std::io::{BufRead, Write};

const PROTOCOL_VERSION: &str = "2025-06-18";

pub fn run() -> Result<()> {
    let mut api = HidApi::new()?;
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                write_msg(&mut out, &json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":e.to_string()}}))?;
                continue;
            }
        };
        let Some(id) = msg.get("id").cloned() else {
            continue; // notification
        };
        let method = msg["method"].as_str().unwrap_or("");
        let params = msg.get("params").cloned().unwrap_or(Value::Null);
        let reply = match method {
            "initialize" => json!({
                "protocolVersion": params["protocolVersion"].as_str().unwrap_or(PROTOCOL_VERSION),
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "razerlite", "version": env!("CARGO_PKG_VERSION")},
                "instructions": "Controls a Razer Huntsman V2 Analog keyboard and Viper V3 Pro mouse over HID. \
                    Settings are saved to the devices' onboard memory. Never kill Synapse while its drivers are \
                    installed: its filter driver swallows all keystrokes without it."
            }),
            "ping" => json!({}),
            "tools/list" => json!({"tools": tools()}),
            "tools/call" => {
                let name = params["name"].as_str().unwrap_or("");
                let args = params.get("arguments").cloned().unwrap_or(json!({}));
                let _ = api.refresh_devices();
                match call(&api, name, &args) {
                    Ok(v) => json!({"content":[{"type":"text","text": serde_json::to_string_pretty(&v)?}]}),
                    Err(e) => json!({"content":[{"type":"text","text": format!("{e:#}")}],"isError":true}),
                }
            }
            _ => {
                write_msg(&mut out, &json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"method not found"}}))?;
                continue;
            }
        };
        write_msg(&mut out, &json!({"jsonrpc":"2.0","id":id,"result":reply}))?;
        if method == "tools/call" {
            api = HidApi::new()?; // drop handles between calls so the GUI/Synapse can use the device
        }
    }
    Ok(())
}

fn write_msg(out: &mut impl Write, v: &Value) -> Result<()> {
    writeln!(out, "{}", serde_json::to_string(v)?)?;
    out.flush()?;
    Ok(())
}

fn tool(name: &str, desc: &str, props: Value, required: &[&str]) -> Value {
    json!({"name":name,"description":desc,"inputSchema":{"type":"object","properties":props,"required":required}})
}

fn tools() -> Vec<Value> {
    let device = json!({"type":"string","enum":["keyboard","mouse"]});
    vec![
        tool("list_interfaces", "List every connected Razer HID interface (pid, interface, usage page/usage).", json!({}), &[]),
        tool("state", "Read current settings from the keyboard and mouse (no per-key data).", json!({}), &[]),
        tool(
            "kb_keys",
            "Read press/release points for keys in the active onboard profile. Values in mm; null = firmware default.",
            json!({"keys":{"type":"array","items":{"type":"integer"},"description":"Firmware key IDs (W=18 A=31 S=32 D=33). Omit for all keys."},
                   "mode":{"type":"integer","enum":[0,1],"description":"0 normal, 1 Hypershift"}}),
            &[],
        ),
        tool(
            "kb_set_actuation",
            "Write press/release points (mm) for keys in the active onboard profile, normal and Hypershift. \
             press 1.5-3.6, release must be below press. default=true restores firmware default.",
            json!({"keys":{"type":"array","items":{"type":"integer"}},"press_mm":{"type":"number"},
                   "release_mm":{"type":"number"},"default":{"type":"boolean"}}),
            &["keys"],
        ),
        tool("kb_set_debounce", "Keyboard debounce in ms.", json!({"press_ms":{"type":"integer"},"release_ms":{"type":"integer"}}), &["press_ms","release_ms"]),
        tool("kb_set_polling", "Keyboard polling rate.", json!({"hz":{"type":"integer","enum":[1000,500,125]}}), &["hz"]),
        tool("kb_set_brightness", "Keyboard brightness 0-255.", json!({"value":{"type":"integer"}}), &["value"]),
        tool(
            "kb_set_effect",
            "Keyboard lighting effect.",
            json!({"effect":{"type":"string","enum":["off","static","breathing","breathing_random","spectrum","wave_right","wave_left","reactive","starlight","starlight_random"]},
                   "rgb":{"type":"array","items":{"type":"integer"},"minItems":3,"maxItems":3},
                   "speed":{"type":"integer","minimum":1,"maximum":3}}),
            &["effect"],
        ),
        tool(
            "mouse_set_dpi",
            "Set mouse DPI stages (1-5) and the active stage (1-based).",
            json!({"stages":{"type":"array","items":{"type":"array","items":{"type":"integer"},"minItems":2,"maxItems":2}},
                   "active":{"type":"integer"}}),
            &["stages","active"],
        ),
        tool("mouse_set_polling", "Mouse polling rate.", json!({"hz":{"type":"integer","enum":[8000,4000,2000,1000,500,250,125]}}), &["hz"]),
        tool("mouse_set_sleep", "Mouse sleep timeout in seconds (min 60).", json!({"seconds":{"type":"integer"}}), &["seconds"]),
        tool("mouse_set_low_battery", "Mouse low-battery warning level in percent.", json!({"percent":{"type":"integer"}}), &["percent"]),
        tool(
            "raw",
            "Send a raw 90-byte Razer report and return the reply. Read commands (id >= 0x80) always allowed. \
             Writes need allow_write=true. Class 0xFE (analog calibration) and class 0x00 ids 0x0a-0x0c (bootloader/reset) are blocked.",
            json!({"device":device,"class":{"type":"integer"},"id":{"type":"integer"},"size":{"type":"integer","description":"data size byte (default 80)"},
                   "args":{"type":"array","items":{"type":"integer"}},"allow_write":{"type":"boolean"}}),
            &["device","class","id"],
        ),
    ]
}

fn kb(api: &HidApi) -> Result<HidDevice> {
    d::open(api, d::KB_PID, d::KB_IFACE)
}
fn mouse(api: &HidApi) -> Result<HidDevice> {
    d::MOUSE_PIDS
        .iter()
        .find_map(|&p| d::open(api, p, d::MOUSE_IFACE).ok())
        .ok_or_else(|| anyhow!("mouse not connected"))
}

fn arg_u64(a: &Value, k: &str) -> Result<u64> {
    a[k].as_u64().with_context(|| format!("missing integer `{k}`"))
}

fn mm_or_null(b: u8) -> Value {
    if b == d::KB_DEFAULT { Value::Null } else { json!((d::byte_to_mm(b) * 100.0).round() / 100.0) }
}

fn call(api: &HidApi, name: &str, a: &Value) -> Result<Value> {
    Ok(match name {
        "list_interfaces" => {
            let v: Vec<Value> = api
                .device_list()
                .filter(|i| i.vendor_id() == d::RAZER_VID)
                .map(|i| json!({"pid":format!("{:04x}",i.product_id()),"interface":i.interface_number(),
                    "usage_page":format!("{:04x}",i.usage_page()),"usage":format!("{:04x}",i.usage()),
                    "product":i.product_string()}))
                .collect();
            json!(v)
        }
        "state" => {
            let kbv = kb(api).and_then(|k| {
                let p = d::kb_profiles(&k)?;
                let (eff, rgb) = d::kb_get_effect(&k)?;
                Ok(json!({"firmware":d::firmware(&k)?,"profiles":p.ids,"active_profile":p.active,
                    "debounce_ms":d::kb_get_debounce(&k)?,"polling_code":d::kb_get_polling(&k)?,
                    "brightness":d::kb_get_brightness(&k)?,"effect_code":eff,"effect_rgb":rgb}))
            });
            let mv = mouse(api).and_then(|m| {
                let s = d::mouse_get_stages(&m)?;
                let (b, c) = d::mouse_battery(&m)?;
                let code = d::mouse_get_polling(&m)?;
                let hz = d::MOUSE_RATES.iter().find(|r| r.1 == code).map(|r| r.0);
                Ok(json!({"firmware":d::firmware(&m)?,"dpi":d::mouse_get_dpi(&m)?,"stages":s.stages,"active_stage":s.active,
                    "polling_hz":hz,"battery_pct":b,"charging":c,"sleep_s":d::mouse_get_sleep(&m)?,
                    "low_battery_pct":d::mouse_get_low_batt(&m)?}))
            });
            json!({"keyboard": kbv.unwrap_or_else(|e| json!({"error":format!("{e:#}")})),
                   "mouse": mv.unwrap_or_else(|e| json!({"error":format!("{e:#}")}))})
        }
        "kb_keys" => {
            let k = kb(api)?;
            let prof = d::kb_profiles(&k)?.active;
            let mode = a["mode"].as_u64().unwrap_or(0) as u8;
            let keys: Vec<u8> = match a["keys"].as_array() {
                Some(v) => v.iter().filter_map(|x| x.as_u64()).map(|x| x as u8).collect(),
                None => crate::keymap::all_ids(),
            };
            let mut out = serde_json::Map::new();
            for key in keys {
                let v = match d::kb_get_key(&k, prof, key, mode) {
                    Ok(x) => json!({"press_mm":mm_or_null(x.press()),"release_mm":mm_or_null(x.release()),
                        "raw":x.raw.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ")}),
                    Err(e) => json!({"error":format!("{e:#}")}),
                };
                out.insert(key.to_string(), v);
            }
            json!({"profile":prof,"mode":mode,"keys":out})
        }
        "kb_set_actuation" => {
            let k = kb(api)?;
            let prof = d::kb_profiles(&k)?.active;
            let keys: Vec<u8> = a["keys"].as_array().context("keys")?.iter().filter_map(|x| x.as_u64()).map(|x| x as u8).collect();
            let (press, release) = if a["default"].as_bool() == Some(true) {
                (d::KB_DEFAULT, d::KB_DEFAULT)
            } else {
                let p = a["press_mm"].as_f64().context("press_mm")? as f32;
                let r = a["release_mm"].as_f64().context("release_mm")? as f32;
                if !(1.5..=3.6).contains(&p) || r >= p || r < 0.1 {
                    bail!("need 1.5 <= press_mm <= 3.6 and 0.1 <= release_mm < press_mm");
                }
                (d::mm_to_byte(p), d::mm_to_byte(r))
            };
            let mut done = 0;
            for &key in &keys {
                for mode in 0..=1u8 {
                    let cur = d::kb_get_key(&k, prof, key, mode)?;
                    d::kb_set_thresholds(&k, &cur, press, release)?;
                }
                done += 1;
            }
            json!({"profile":prof,"keys_written":done})
        }
        "kb_set_debounce" => {
            let (p, r) = (arg_u64(a, "press_ms")? as u8, arg_u64(a, "release_ms")? as u8);
            d::kb_set_debounce(&kb(api)?, [p, 0, r, 0])?;
            json!("ok")
        }
        "kb_set_polling" => {
            let code = match arg_u64(a, "hz")? {
                1000 => 1,
                500 => 2,
                125 => 8,
                _ => bail!("hz must be 1000, 500 or 125"),
            };
            d::kb_set_polling(&kb(api)?, code)?;
            json!("ok")
        }
        "kb_set_brightness" => {
            d::kb_set_brightness(&kb(api)?, arg_u64(a, "value")?.min(255) as u8)?;
            json!("ok")
        }
        "kb_set_effect" => {
            let e = match a["effect"].as_str().unwrap_or("") {
                "off" => Effect::Off,
                "static" => Effect::Static,
                "breathing" => Effect::Breathing,
                "breathing_random" => Effect::BreathingRandom,
                "spectrum" => Effect::Spectrum,
                "wave_right" => Effect::WaveRight,
                "wave_left" => Effect::WaveLeft,
                "reactive" => Effect::Reactive,
                "starlight" => Effect::Starlight,
                "starlight_random" => Effect::StarlightRandom,
                x => bail!("unknown effect {x}"),
            };
            let rgb: Vec<u8> = a["rgb"].as_array().map(|v| v.iter().filter_map(|x| x.as_u64()).map(|x| x as u8).collect()).unwrap_or(vec![0, 255, 0]);
            let rgb: [u8; 3] = rgb.try_into().map_err(|_| anyhow!("rgb needs 3 values"))?;
            d::kb_set_effect(&kb(api)?, e, rgb, a["speed"].as_u64().unwrap_or(2) as u8)?;
            json!("ok")
        }
        "mouse_set_dpi" => {
            let stages: Vec<(u16, u16)> = a["stages"]
                .as_array()
                .context("stages")?
                .iter()
                .map(|s| (s[0].as_u64().unwrap_or(800) as u16, s[1].as_u64().unwrap_or(800) as u16))
                .collect();
            if stages.is_empty() || stages.len() > 5 || stages.iter().any(|s| s.0 < 100 || s.1 < 100 || s.0 > 35000 || s.1 > 35000) {
                bail!("1-5 stages, each 100..=35000");
            }
            let m = mouse(api)?;
            d::mouse_set_stages(&m, &DpiStages { active: arg_u64(a, "active")? as u8, stages })?;
            json!({"dpi": d::mouse_get_dpi(&m)?})
        }
        "mouse_set_polling" => {
            let hz = arg_u64(a, "hz")? as u16;
            let code = d::MOUSE_RATES.iter().find(|r| r.0 == hz).map(|r| r.1).context("unsupported rate")?;
            let m = mouse(api)?;
            d::mouse_set_polling(&m, code)?;
            json!({"polling_code": d::mouse_get_polling(&m)?})
        }
        "mouse_set_sleep" => {
            d::mouse_set_sleep(&mouse(api)?, arg_u64(a, "seconds")? as u16)?;
            json!("ok")
        }
        "mouse_set_low_battery" => {
            d::mouse_set_low_batt(&mouse(api)?, arg_u64(a, "percent")? as u8)?;
            json!("ok")
        }
        "raw" => {
            let class = arg_u64(a, "class")? as u8;
            let id = arg_u64(a, "id")? as u8;
            let size = a["size"].as_u64().unwrap_or(80) as u8;
            let args: Vec<u8> = a["args"].as_array().map(|v| v.iter().filter_map(|x| x.as_u64()).map(|x| x as u8).collect()).unwrap_or_default();
            let is_read = id & 0x80 != 0;
            if class == 0xfe && !is_read {
                bail!("class 0xFE writes (analog calibration) are blocked");
            }
            if class == 0x00 && (0x0a..=0x0c).contains(&id) {
                bail!("class 0x00 ids 0x0a-0x0c are blocked (bootloader/reset)");
            }
            if !is_read && a["allow_write"].as_bool() != Some(true) {
                bail!("write command: pass allow_write=true");
            }
            let dev = match a["device"].as_str() {
                Some("keyboard") => kb(api)?,
                Some("mouse") => mouse(api)?,
                _ => bail!("device must be keyboard or mouse"),
            };
            let r = transact(&dev, cmd(size, class, id), &args, None)?;
            let hex: Vec<String> = r.args().iter().map(|b| format!("{b:02x}")).collect();
            let last = r.args().iter().rposition(|&b| b != 0).map(|i| i + 1).unwrap_or(0).max(8);
            json!({"status":format!("{:?}",r.status()),"args":hex[..last].join(" ")})
        }
        _ => bail!("unknown tool {name}"),
    })
}
