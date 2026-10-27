//! Dev MCP server (like Hopper's). Off unless started with `--dev-mcp`.
//! HTTP JSON-RPC on 127.0.0.1:<port>/mcp (default 47851, env RAZERLITE_MCP_PORT).
//! The tray process serves `open` + device tools and forwards window tools to the window
//! process, which listens on <port>+1. Window tools drive the open window (screenshot, click,
//! drag, key…); device tools are the same as `razerlite --mcp`.

use crate::tray::{self, TrayMsg};
use crate::ui::{Mode, Sub, Tab};
use egui::{Event, Key, Modifiers, PointerButton, Pos2, pos2, vec2};
use serde_json::{Value, json};
use std::sync::Mutex;
use std::sync::mpsc::{Sender, channel};
use std::time::Duration;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Tray,
    Window,
}

pub enum DevCmd {
    State,
    Navigate { tab: Option<Tab>, sub: Option<Sub> },
    Screenshot,
    Input(Vec<Vec<Event>>),
    Close,
}

static QUEUE: Mutex<Vec<(DevCmd, Sender<Value>)>> = Mutex::new(Vec::new());
/// Set when the dev server runs: the popover then stays open on focus loss so tools can drive it.
pub static ACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn take() -> Vec<(DevCmd, Sender<Value>)> {
    std::mem::take(&mut *QUEUE.lock().unwrap())
}

fn ui_open() -> bool {
    tray::UI_CTX.lock().unwrap().is_some()
}

/// Queue a command for the open window and wait for its reply.
fn ask(cmd: DevCmd) -> Result<Value, String> {
    if !ui_open() {
        return Err("no window open: call `open` first".into());
    }
    let (tx, rx) = channel();
    QUEUE.lock().unwrap().push((cmd, tx));
    if let Some(c) = tray::UI_CTX.lock().unwrap().as_ref() {
        c.request_repaint();
    }
    rx.recv_timeout(Duration::from_secs(6)).map_err(|_| "window did not answer".to_string())
}

pub fn png_reply(img: &egui::ColorImage) -> Value {
    let (w, h) = (img.size[0] as u32, img.size[1] as u32);
    let mut buf = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut buf, w, h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut wr = enc.write_header().expect("png header");
        let raw: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
        wr.write_image_data(&raw).expect("png data");
    }
    let path = std::env::temp_dir().join("razerlite-shot.png");
    let _ = std::fs::write(&path, &buf);
    json!({"png_base64": b64(&buf), "path": path.display().to_string(), "width": w, "height": h})
}

fn b64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= c.len() {
                s.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                s.push('=');
            }
        }
    }
    s
}

pub fn start(port: u16, role: Role) {
    ACTIVE.store(true, std::sync::atomic::Ordering::Relaxed);
    let server = match tiny_http::Server::http(("127.0.0.1", port)) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("dev mcp: can't bind {port}: {e}");
            return;
        }
    };
    std::thread::Builder::new()
        .name("dev-mcp".into())
        .spawn(move || {
            for mut req in server.incoming_requests() {
                // Browsers must not drive this.
                if req.headers().iter().any(|h| h.field.equiv("Origin") && !h.value.as_str().contains("127.0.0.1") && !h.value.as_str().contains("localhost")) {
                    let _ = req.respond(tiny_http::Response::empty(403));
                    continue;
                }
                let mut body = String::new();
                let _ = req.as_reader().read_to_string(&mut body);
                let reply = handle(&body, role, port);
                let hdr = tiny_http::Header::from_bytes("Content-Type", "application/json").unwrap();
                let resp = match reply {
                    Some(v) => tiny_http::Response::from_string(v.to_string()).with_header(hdr),
                    None => tiny_http::Response::from_string("").with_status_code(202),
                };
                let _ = req.respond(resp);
            }
        })
        .ok();
}

fn handle(body: &str, role: Role, port: u16) -> Option<Value> {
    let msg: Value = serde_json::from_str(body).ok()?;
    let id = msg.get("id").cloned()?;
    let params = msg.get("params").cloned().unwrap_or(Value::Null);
    let result = match msg["method"].as_str().unwrap_or("") {
        "initialize" => json!({"protocolVersion": params["protocolVersion"].as_str().unwrap_or("2025-06-18"),
            "capabilities":{"tools":{}}, "serverInfo":{"name":"razerlite-dev","version":env!("CARGO_PKG_VERSION")}}),
        "ping" => json!({}),
        "tools/list" => {
            let mut t = window_tools();
            t.extend(crate::mcp::tools());
            json!({"tools": t})
        }
        "tools/call" => {
            let name = params["name"].as_str().unwrap_or("");
            if role == Role::Tray && is_window_tool(name) && name != "open" {
                return Some(forward(body, port + 1).unwrap_or_else(|e| json!({"jsonrpc":"2.0","id":id,"result":err(e)})));
            }
            let mut args = params.get("arguments").cloned().unwrap_or(json!({}));
            if let Some(s) = args.as_str() {
                args = serde_json::from_str(s).unwrap_or(json!({}));
            }
            call(name, &args)
        }
        _ => return Some(json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"method not found"}})),
    };
    Some(json!({"jsonrpc":"2.0","id":id,"result":result}))
}

fn text(v: Value) -> Value {
    json!({"content":[{"type":"text","text": if let Value::String(s) = &v { s.clone() } else { serde_json::to_string_pretty(&v).unwrap_or_default() }}]})
}
fn err(e: impl std::fmt::Display) -> Value {
    json!({"content":[{"type":"text","text": e.to_string()}],"isError":true})
}

fn is_window_tool(n: &str) -> bool {
    matches!(n, "open" | "close" | "ui_state" | "navigate" | "screenshot" | "click" | "hover" | "drag" | "key" | "type" | "scroll")
}

/// Pass a JSON-RPC request to the window process's server.
fn forward(body: &str, port: u16) -> Result<Value, String> {
    use std::io::{Read, Write};
    if tray::CHILD_PID.lock().unwrap().is_none() {
        return Err("no window open: call `open` first".into());
    }
    let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).map_err(|e| format!("window not reachable: {e}"))?;
    s.set_read_timeout(Some(Duration::from_secs(10))).ok();
    let req = format!(
        "POST /mcp HTTP/1.0\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    s.write_all(req.as_bytes()).map_err(|e| e.to_string())?;
    let mut out = String::new();
    s.read_to_string(&mut out).map_err(|e| e.to_string())?;
    let json = out.split("\r\n\r\n").nth(1).ok_or("bad reply")?;
    serde_json::from_str(json).map_err(|e| e.to_string())
}

fn wait_for_port(port: u16) -> bool {
    for _ in 0..60 {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

fn window_tools() -> Vec<Value> {
    let t = |n: &str, d: &str, p: Value| json!({"name":n,"description":d,"inputSchema":{"type":"object","properties":p}});
    let xy = json!({"x":{"type":"number"},"y":{"type":"number"}});
    vec![
        t("open", "Open a window: mode popover or full.", json!({"mode":{"type":"string","enum":["popover","full"]}})),
        t("close", "Close the open window (back to tray).", json!({})),
        t("ui_state", "Window state: mode, tab, sub-tab, selected keys, size.", json!({})),
        t("navigate", "Switch tab (mouse/keyboard/macros/settings) and sub-tab (customize/actuation/lighting/performance/power).",
          json!({"tab":{"type":"string"},"sub":{"type":"string"}})),
        t("screenshot", "PNG of the open window (also saved to %TEMP%\\razerlite-shot.png). Coordinates = logical points.", json!({})),
        t("click", "Left click at x,y (logical points).", xy.clone()),
        t("hover", "Move the pointer to x,y.", xy),
        t("drag", "Drag from x1,y1 to x2,y2 (alt=true removes keys on the keyboard).",
          json!({"x1":{"type":"number"},"y1":{"type":"number"},"x2":{"type":"number"},"y2":{"type":"number"},"alt":{"type":"boolean"}})),
        t("key", "Press and release a key by egui name (e.g. Escape, Enter, A).", json!({"key":{"type":"string"}})),
        t("type", "Type text into the focused field.", json!({"text":{"type":"string"}})),
        t("scroll", "Scroll at x,y; dy positive = down.", json!({"x":{"type":"number"},"y":{"type":"number"},"dy":{"type":"number"}})),
    ]
}

fn pt(a: &Value, x: &str, y: &str) -> Pos2 {
    pos2(a[x].as_f64().unwrap_or(0.0) as f32, a[y].as_f64().unwrap_or(0.0) as f32)
}
fn btn(p: Pos2, pressed: bool, m: Modifiers) -> Event {
    Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers: m }
}

fn call(name: &str, a: &Value) -> Value {
    let input = |b: Vec<Vec<Event>>| ask(DevCmd::Input(b)).map(text).unwrap_or_else(err);
    match name {
        "open" => {
            let mode = if a["mode"].as_str() == Some("popover") { Mode::Popover } else { Mode::Full };
            tray::post(TrayMsg::Show(mode));
            std::thread::sleep(Duration::from_millis(300));
            let port: u16 = std::env::var("RAZERLITE_MCP_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(47851) + 1;
            if wait_for_port(port) {
                std::thread::sleep(Duration::from_millis(500));
                text(json!("opened"))
            } else {
                err("window did not open")
            }
        }
        "close" => ask(DevCmd::Close).map(text).unwrap_or_else(err),
        "ui_state" => ask(DevCmd::State).map(text).unwrap_or_else(err),
        "navigate" => {
            let tab = a["tab"].as_str().and_then(|s| serde_json::from_value::<Tab>(json!(cap(s))).ok());
            let sub = a["sub"].as_str().and_then(|s| serde_json::from_value::<Sub>(json!(cap(s))).ok());
            ask(DevCmd::Navigate { tab, sub }).map(text).unwrap_or_else(err)
        }
        "screenshot" => match ask(DevCmd::Screenshot) {
            Ok(v) => json!({"content":[
                {"type":"image","data": v["png_base64"], "mimeType":"image/png"},
                {"type":"text","text": format!("{}x{} saved to {}", v["width"], v["height"], v["path"].as_str().unwrap_or(""))}]}),
            Err(e) => err(e),
        },
        "click" => {
            let p = pt(a, "x", "y");
            input(vec![vec![Event::PointerMoved(p)], vec![btn(p, true, Modifiers::NONE)], vec![btn(p, false, Modifiers::NONE)]])
        }
        "hover" => input(vec![vec![Event::PointerMoved(pt(a, "x", "y"))]]),
        "drag" => {
            let (p1, p2) = (pt(a, "x1", "y1"), pt(a, "x2", "y2"));
            let m = if a["alt"].as_bool() == Some(true) { Modifiers::ALT } else { Modifiers::NONE };
            let mut b = vec![vec![Event::PointerMoved(p1)], vec![btn(p1, true, m)]];
            for i in 1..=8 {
                b.push(vec![Event::PointerMoved(p1 + (p2 - p1) * (i as f32 / 8.0))]);
            }
            b.push(vec![btn(p2, false, m)]);
            input(b)
        }
        "key" => match a["key"].as_str().and_then(Key::from_name) {
            Some(k) => {
                let ev = |down| Event::Key { key: k, physical_key: None, pressed: down, repeat: false, modifiers: Modifiers::NONE };
                input(vec![vec![ev(true)], vec![ev(false)]])
            }
            None => err("unknown key name"),
        },
        "type" => input(vec![vec![Event::Text(a["text"].as_str().unwrap_or("").to_string())]]),
        "scroll" => {
            let p = pt(a, "x", "y");
            let dy = a["dy"].as_f64().unwrap_or(100.0) as f32;
            input(vec![
                vec![Event::PointerMoved(p)],
                vec![Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: vec2(0.0, -dy),
                    phase: egui::TouchPhase::Move,
                    source: egui::MouseWheelSource::Wheel,
                    modifiers: Modifiers::NONE,
                }],
            ])
        }
        _ => {
            // device tools
            let api = match hidapi::HidApi::new() {
                Ok(a) => a,
                Err(e) => return err(e),
            };
            match crate::mcp::call(&api, name, a) {
                Ok(v) => text(v),
                Err(e) => err(format!("{e:#}")),
            }
        }
    }
}

fn cap(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + &c.as_str().to_lowercase()).unwrap_or_default()
}
