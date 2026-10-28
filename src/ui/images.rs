//! Device images. Razer's product renders (copied from the Synapse cache by the user, kept out
//! of git) are looked up in `<exe>\assets\devices`, the repo's `assets\devices`, then
//! `%APPDATA%\RazerLite\devices`. Without them the app uses its own generic renders
//! (`assets\generic`, made by `tools\render_devices.py`), built into the exe.

use egui::{Color32, ColorImage, Pos2, Rect, TextureHandle, TextureOptions, pos2, vec2};
use std::path::PathBuf;

pub const KB_FILE: &str = "huntsman_v2_analog.png";
pub const MOUSE_FILE: &str = "viper_v3_pro.png";
const GENERIC_KB: &[u8] = include_bytes!("../../assets/generic/keyboard.png");
const GENERIC_MOUSE: &[u8] = include_bytes!("../../assets/generic/mouse.png");

/// Where key (0,0) of `art::kb_units` sits in the 750×420 keyboard image, and the size of
/// one key unit in image pixels. Measured on Synapse's top-down render.
pub const KB_ORIGIN: Pos2 = pos2(46.5, 115.0);
pub const KB_UNIT: f32 = 26.36;

pub struct Img {
    pub tex: TextureHandle,
    /// Full image size in pixels
    pub size: egui::Vec2,
    /// Non-transparent area, in pixels
    pub bbox: Rect,
}

impl Img {
    /// Draw the visible part fitted into `area`; returns the drawn rect and the px→screen scale.
    pub fn fit(&self, area: Rect) -> (Rect, f32) {
        let s = (area.width() / self.bbox.width()).min(area.height() / self.bbox.height());
        let r = Rect::from_center_size(area.center(), self.bbox.size() * s);
        (r, s)
    }
    pub fn uv(&self) -> Rect {
        Rect::from_min_max(
            pos2(self.bbox.min.x / self.size.x, self.bbox.min.y / self.size.y),
            pos2(self.bbox.max.x / self.size.x, self.bbox.max.y / self.size.y),
        )
    }
    pub fn paint(&self, p: &egui::Painter, r: Rect, tint: Color32) {
        p.image(self.tex.id(), r, self.uv(), tint);
    }
    /// Paint only rows from `from` (0..1 of the visible height) down to the bottom.
    pub fn paint_bottom(&self, p: &egui::Painter, r: Rect, from: f32, tint: Color32) {
        let uv = self.uv();
        let y_uv = egui::lerp(uv.min.y..=uv.max.y, from);
        let y_px = egui::lerp(r.min.y..=r.max.y, from);
        p.image(self.tex.id(), Rect::from_min_max(pos2(r.min.x, y_px), r.max), Rect::from_min_max(pos2(uv.min.x, y_uv), uv.max), tint);
    }
}

pub struct DeviceImages {
    /// Original keyboard render (rainbow lighting baked in)
    pub kb: Option<Img>,
    /// Keyboard with the lighting removed (grey)
    pub kb_base: Option<Img>,
    /// Only the lit parts, white: tint with the current lighting color
    pub kb_glow: Option<Img>,
    pub mouse: Option<Img>,
    /// Mouse silhouette in white, for the battery fill
    pub mouse_mask: Option<Img>,
}

fn find(name: &str) -> Option<PathBuf> {
    // Dev: preview the built-in renders even when Razer's images are installed.
    if std::env::var_os("RAZERLITE_GENERIC").is_some() {
        return None;
    }
    let mut dirs = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(d) = exe.parent() {
            dirs.push(d.join("assets").join("devices"));
            dirs.push(d.join("..").join("..").join("assets").join("devices"));
        }
    }
    if let Some(a) = std::env::var_os("APPDATA") {
        dirs.push(PathBuf::from(a).join("RazerLite").join("devices"));
    }
    dirs.into_iter().map(|d| d.join(name)).find(|p| p.is_file())
}

/// The user's file if present, else the built-in generic render.
fn load_or(name: &str, generic: &[u8]) -> Option<ColorImage> {
    find(name).and_then(|p| std::fs::read(p).ok()).and_then(|b| decode(&b)).or_else(|| decode(generic))
}

fn decode(bytes: &[u8]) -> Option<ColorImage> {
    let mut dec = png::Decoder::new(std::io::Cursor::new(bytes));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = dec.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    let (w, h) = (info.width as usize, info.height as usize);
    let rgba: Vec<u8> = match info.color_type {
        png::ColorType::Rgba => buf[..w * h * 4].to_vec(),
        png::ColorType::Rgb => buf[..w * h * 3].chunks(3).flat_map(|c| [c[0], c[1], c[2], 255]).collect(),
        png::ColorType::GrayscaleAlpha => buf[..w * h * 2].chunks(2).flat_map(|c| [c[0], c[0], c[0], c[1]]).collect(),
        png::ColorType::Grayscale => buf[..w * h].iter().flat_map(|&g| [g, g, g, 255]).collect(),
        _ => return None,
    };
    Some(ColorImage::from_rgba_unmultiplied([w, h], &rgba))
}

fn bbox(img: &ColorImage) -> Rect {
    let [w, h] = img.size;
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
    for y in 0..h {
        for x in 0..w {
            if img.pixels[y * w + x].a() > 8 {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x + 1);
                y1 = y1.max(y + 1);
            }
        }
    }
    if x1 <= x0 {
        return Rect::from_min_size(Pos2::ZERO, vec2(w as f32, h as f32));
    }
    Rect::from_min_max(pos2(x0 as f32, y0 as f32), pos2(x1 as f32, y1 as f32))
}

fn upload(ctx: &egui::Context, name: &str, img: ColorImage, bb: Rect) -> Img {
    let size = vec2(img.size[0] as f32, img.size[1] as f32);
    Img { tex: ctx.load_texture(name, img, TextureOptions::LINEAR), size, bbox: bb }
}

/// Split the keyboard render into a grey base and a white "light" layer from the saturated pixels.
fn split_lighting(img: &ColorImage) -> (ColorImage, ColorImage) {
    let mut base = img.clone();
    let mut glow = img.clone();
    for (b, g) in base.pixels.iter_mut().zip(glow.pixels.iter_mut()) {
        let [r, gg, bb, a] = b.to_srgba_unmultiplied();
        let (mx, mn) = (r.max(gg).max(bb) as f32, r.min(gg).min(bb) as f32);
        let sat = if mx > 0.0 { (mx - mn) / mx } else { 0.0 };
        let lum = 0.3 * r as f32 + 0.59 * gg as f32 + 0.11 * bb as f32;
        // Lit parts become dark keycaps in the base; neutral parts (frame, legends in shadow) stay.
        let lit = ((sat - 0.25) / 0.45).clamp(0.0, 1.0);
        let grey = (lum * (1.0 - lit) + lum * 0.25 * lit).min(255.0) as u8;
        *b = Color32::from_rgba_unmultiplied(grey, grey, grey, a);
        let ga = (lit * (mx / 255.0).powf(0.8) * a as f32).min(255.0) as u8;
        *g = Color32::from_rgba_unmultiplied(255, 255, 255, ga);
    }
    (base, glow)
}

pub fn load(ctx: &egui::Context) -> DeviceImages {
    let mut out = DeviceImages { kb: None, kb_base: None, kb_glow: None, mouse: None, mouse_mask: None };
    if let Some(img) = load_or(KB_FILE, GENERIC_KB) {
        let bb = bbox(&img);
        let (base, glow) = split_lighting(&img);
        out.kb_base = Some(upload(ctx, "kb_base", base, bb));
        out.kb_glow = Some(upload(ctx, "kb_glow", glow, bb));
        out.kb = Some(upload(ctx, "kb", img, bb));
    }
    if let Some(img) = load_or(MOUSE_FILE, GENERIC_MOUSE) {
        let bb = bbox(&img);
        let mut mask = img.clone();
        for p in mask.pixels.iter_mut() {
            let a = p.a();
            *p = Color32::from_rgba_unmultiplied(255, 255, 255, a);
        }
        out.mouse_mask = Some(upload(ctx, "mouse_mask", mask, bb));
        out.mouse = Some(upload(ctx, "mouse", img, bb));
    }
    out
}
