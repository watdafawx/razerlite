use egui::{Color32, FontData, FontDefinitions, FontFamily, FontId, RichText};
use std::sync::Arc;

pub const BG: Color32 = Color32::from_rgb(0x16, 0x17, 0x18);
pub const TOP: Color32 = Color32::from_rgb(0x07, 0x07, 0x08);
pub const SUB: Color32 = Color32::from_rgb(0x1c, 0x1d, 0x1f);
pub const PANEL: Color32 = Color32::from_rgb(0x1e, 0x1f, 0x22);
pub const SURF: Color32 = Color32::from_rgb(0x27, 0x29, 0x2d);
pub const RAISED: Color32 = Color32::from_rgb(0x30, 0x33, 0x38);
pub const BD: Color32 = Color32::from_rgb(0x2e, 0x31, 0x37);
pub const FG: Color32 = Color32::from_rgb(0xec, 0xee, 0xf1);
pub const TXT: Color32 = Color32::from_rgb(0xc9, 0xcc, 0xd1);
pub const MUT: Color32 = Color32::from_rgb(0xa0, 0xa5, 0xae);
pub const DIM: Color32 = Color32::from_rgb(0x6c, 0x71, 0x7a);
pub const ACC: Color32 = Color32::from_rgb(0x44, 0xd6, 0x2c);
pub const ACC2: Color32 = Color32::from_rgb(0x6e, 0xe0, 0x5a);
pub const ON_ACC: Color32 = Color32::from_rgb(0x07, 0x14, 0x0a);
pub const INFO: Color32 = Color32::from_rgb(0x60, 0xa5, 0xfa);
pub const WARN: Color32 = Color32::from_rgb(0xea, 0xb3, 0x08);
pub const BAD: Color32 = Color32::from_rgb(0xef, 0x44, 0x44);

pub fn semibold() -> FontFamily {
    FontFamily::Name("semibold".into())
}

pub fn sb(text: impl Into<String>, size: f32) -> RichText {
    RichText::new(text).family(semibold()).size(size)
}

pub fn font(size: f32) -> FontId {
    FontId::proportional(size)
}
pub fn font_sb(size: f32) -> FontId {
    FontId::new(size, semibold())
}

pub fn alpha(c: Color32, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a)
}

pub fn battery_color(pct: u8, warn: u8, charging: bool) -> Color32 {
    if charging || pct > warn {
        ACC
    } else if pct > 10 {
        WARN
    } else {
        BAD
    }
}

/// Segoe UI from Windows (falls back to egui's font if missing), dark visuals.
pub fn install(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let dir = std::path::Path::new(r"C:\Windows\Fonts");
    let load = |name: &str| std::fs::read(dir.join(name)).ok();
    if let Some(b) = load("segoeui.ttf") {
        fonts.font_data.insert("segoe".into(), Arc::new(FontData::from_owned(b)));
        fonts.families.entry(FontFamily::Proportional).or_default().insert(0, "segoe".into());
    }
    let semi = load("seguisb.ttf").or_else(|| load("segoeuib.ttf"));
    let fam = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    if let Some(b) = semi {
        fonts.font_data.insert("segoe-sb".into(), Arc::new(FontData::from_owned(b)));
        let mut f = vec!["segoe-sb".to_string()];
        f.extend(fam);
        fonts.families.insert(semibold(), f);
    } else {
        fonts.families.insert(semibold(), fam);
    }
    if let Some(b) = load("seguisym.ttf") {
        fonts.font_data.insert("symbols".into(), Arc::new(FontData::from_owned(b)));
        for f in [FontFamily::Proportional, semibold()] {
            fonts.families.entry(f).or_default().push("symbols".into());
        }
    }
    ctx.set_fonts(fonts);

    let mut v = egui::Visuals::dark();
    v.panel_fill = BG;
    v.window_fill = PANEL;
    v.extreme_bg_color = Color32::from_rgb(0x11, 0x12, 0x14);
    v.selection.bg_fill = ACC;
    v.selection.stroke = egui::Stroke::new(1.0, ON_ACC);
    v.hyperlink_color = ACC2;
    let w = &mut v.widgets;
    w.noninteractive.bg_stroke = egui::Stroke::new(1.0, BD);
    w.noninteractive.fg_stroke = egui::Stroke::new(1.0, TXT);
    for (s, fill) in [(&mut w.inactive, SURF), (&mut w.hovered, RAISED), (&mut w.active, RAISED)] {
        s.bg_fill = fill;
        s.weak_bg_fill = fill;
        s.corner_radius = 6.into();
    }
    w.inactive.fg_stroke = egui::Stroke::new(1.0, FG);
    w.hovered.bg_stroke = egui::Stroke::new(1.0, Color32::from_rgb(0x4a, 0x4f, 0x58));
    v.slider_trailing_fill = true;
    ctx.set_visuals(v);
    ctx.all_styles_mut(|s| {
        s.spacing.item_spacing = egui::vec2(8.0, 8.0);
        s.spacing.button_padding = egui::vec2(12.0, 6.0);
        s.spacing.slider_width = 220.0;
        s.text_styles.insert(egui::TextStyle::Body, font(14.0));
        s.text_styles.insert(egui::TextStyle::Button, font(13.5));
        s.text_styles.insert(egui::TextStyle::Small, font(12.0));
        s.interaction.selectable_labels = false;
    });
}
