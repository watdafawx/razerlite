#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
mod devmcp;
mod device;
mod keymap;
mod mcp;
mod proto;
mod state;
mod tray;
mod ui;

const USAGE: &str = "razerlite [--tray] [--popover] [--demo] [--dev-mcp] | --mcp

  (no args)   open the window, keep running in the tray
  --tray      start hidden in the tray (used by Start with Windows)
  --popover   open the tray popover first
  --demo      fake devices, nothing is written (UI work, screenshots)
  --dev-mcp   dev MCP server on 127.0.0.1:47851/mcp (env RAZERLITE_MCP_PORT)
  --mcp       stdio MCP server for device control (no window, no tray)";

fn main() {
    std::panic::set_hook(Box::new(|info| {
        tray::dlog(format!("PANIC: {info}"));
        eprintln!("{info}");
    }));
    let args: Vec<String> = std::env::args().skip(1).collect();
    let has = |f: &str| args.iter().any(|a| a == f);
    let val = |f: &str| args.iter().position(|a| a == f).and_then(|i| args.get(i + 1)).cloned();
    if has("--help") || has("-h") {
        println!("{USAGE}");
        return;
    }
    if has("--mcp") {
        if let Err(e) = mcp::run() {
            eprintln!("razerlite mcp: {e:#}");
            std::process::exit(1);
        }
        return;
    }
    let demo = has("--demo");

    // Window process, started by the tray process.
    if let Some(w) = val("--window") {
        let mode = if w == "popover" { ui::Mode::Popover } else { ui::Mode::Full };
        let tab = match val("--tab").as_deref() {
            Some("settings") => Some(ui::Tab::Settings),
            Some("mouse") => Some(ui::Tab::Mouse),
            Some("macros") => Some(ui::Tab::Macros),
            _ => None,
        };
        if let Some(port) = val("--dev-port").and_then(|p| p.parse().ok()) {
            devmcp::start(port, devmcp::Role::Window);
        }
        let shared = state::spawn(demo, false);
        let code = match ui::run(shared, mode, tab) {
            ui::UiExit::Closed => tray::EXIT_CLOSED,
            ui::UiExit::Open(ui::Mode::Full) => tray::EXIT_OPEN_FULL,
            ui::UiExit::Open(ui::Mode::Popover) => tray::EXIT_OPEN_POPOVER,
            ui::UiExit::OpenSettings => tray::EXIT_OPEN_SETTINGS,
            ui::UiExit::Quit => tray::EXIT_QUIT,
        };
        std::process::exit(code as i32);
    }

    // Tray process. Demo/dev runs may sit next to the user's real RazerLite.
    let dev = has("--dev-mcp");
    if !demo && !dev && !tray::claim_single_instance() {
        return;
    }
    let mut child_args = Vec::new();
    if demo {
        child_args.push("--demo".to_string());
    }
    if dev {
        let port: u16 = std::env::var("RAZERLITE_MCP_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(47851);
        devmcp::start(port, devmcp::Role::Tray);
        child_args.extend(["--dev-port".to_string(), (port + 1).to_string()]);
    }
    *tray::CHILD_ARGS.lock().unwrap() = child_args;
    let shared = state::spawn(demo, true);
    let first = if has("--tray") {
        None
    } else if has("--popover") {
        Some(ui::Mode::Popover)
    } else {
        Some(ui::Mode::Full)
    };
    tray::run(shared, first);
}
