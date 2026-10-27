#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod device;
mod keymap;
mod mcp;
mod proto;
mod worker;

fn main() -> eframe::Result<()> {
    if std::env::args().any(|a| a == "--mcp") {
        if let Err(e) = mcp::run() {
            eprintln!("razerlite mcp: {e:#}");
            std::process::exit(1);
        }
        return Ok(());
    }
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("RazerLite")
            .with_inner_size([960.0, 760.0])
            .with_min_inner_size([700.0, 500.0]),
        ..Default::default()
    };
    eframe::run_native("RazerLite", opts, Box::new(|cc| Ok(Box::new(app::App::new(cc)))))
}
