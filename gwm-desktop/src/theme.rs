//! Colours and fonts, after Zed's "One Dark".

use egui::{Color32, FontData, FontDefinitions, FontFamily, FontId, Stroke, Visuals};

const fn hex(rgb: u32) -> Color32 {
    Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

pub const BG: Color32 = hex(0x3b414d); // title bar, status bar
pub const PANEL: Color32 = hex(0x2f343e); // sidebar, tab bar, inactive tabs
pub const EDITOR: Color32 = hex(0x282c33); // terminal + active tab
pub const ELEVATED: Color32 = hex(0x2f343e); // palette, dialogs
pub const BORDER: Color32 = hex(0x464b57);
pub const BORDER_SOFT: Color32 = hex(0x363c46);
pub const HOVER: Color32 = hex(0x363c46);
pub const SELECTED: Color32 = hex(0x454a56);
pub const TEXT: Color32 = hex(0xdce0e5);
pub const MUTED: Color32 = hex(0xa9afbc);
pub const FAINT: Color32 = hex(0x878a98);
pub const ACCENT: Color32 = hex(0x74ade8);
pub const GREEN: Color32 = hex(0xa1c181);
pub const YELLOW: Color32 = hex(0xdec184);
pub const RED: Color32 = hex(0xd07277);
pub const SELECTION: Color32 = Color32::from_rgba_premultiplied(0x3a, 0x55, 0x78, 0xff);

/// ANSI 0–15 for the terminal.
pub const ANSI: [Color32; 16] = [
    hex(0x282c33), hex(0xd07277), hex(0xa1c181), hex(0xdec184),
    hex(0x74ade8), hex(0xb477cf), hex(0x6eb4bf), hex(0xdce0e5),
    hex(0x636d83), hex(0xe88388), hex(0xb5cf96), hex(0xf0d197),
    hex(0x8cc0f0), hex(0xc891e0), hex(0x89c9d3), hex(0xf0f2f5),
];

pub fn ansi256(i: u8) -> Color32 {
    match i {
        0..=15 => ANSI[i as usize],
        16..=231 => {
            let i = i - 16;
            let step = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
            Color32::from_rgb(step(i / 36), step((i / 6) % 6), step(i % 6))
        }
        _ => {
            let g = 8 + (i - 232) * 10;
            Color32::from_rgb(g, g, g)
        }
    }
}

pub const MONO: &str = "gwm-mono";

pub fn mono(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(MONO.into()))
}

/// Menlo for the terminal (falls back to egui's Hack + system fonts for symbols), SF for the interface.
pub fn install(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let mut mono_stack: Vec<String> = Vec::new();
    for (name, path, index) in [
        ("menlo", "/System/Library/Fonts/Menlo.ttc", 0u32),
        ("sf", "/System/Library/Fonts/SFNS.ttf", 0),
    ] {
        if let Ok(bytes) = std::fs::read(path) {
            let mut data = FontData::from_owned(bytes);
            data.index = index;
            fonts.font_data.insert(name.to_owned(), data.into());
            if name == "menlo" {
                mono_stack.push(name.to_owned());
            } else if let Some(p) = fonts.families.get_mut(&FontFamily::Proportional) {
                p.insert(0, name.to_owned());
            }
        }
    }
    // egui's own monospace + emoji/symbol fallbacks after Menlo
    if let Some(m) = fonts.families.get(&FontFamily::Monospace) {
        mono_stack.extend(m.iter().cloned());
    }
    fonts.families.insert(FontFamily::Name(MONO.into()), mono_stack);
    ctx.set_fonts(fonts);

    let mut v = Visuals::dark();
    v.panel_fill = PANEL;
    v.window_fill = ELEVATED;
    v.extreme_bg_color = EDITOR;
    v.faint_bg_color = HOVER;
    v.window_stroke = Stroke::new(1.0, BORDER);
    v.override_text_color = Some(TEXT);
    v.selection.bg_fill = SELECTION;
    v.selection.stroke = Stroke::new(1.0, ACCENT);
    v.hyperlink_color = ACCENT;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER_SOFT);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, MUTED);
    v.widgets.inactive.weak_bg_fill = PANEL;
    v.widgets.inactive.bg_fill = HOVER;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER_SOFT);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);
    v.widgets.hovered.weak_bg_fill = HOVER;
    v.widgets.hovered.bg_fill = HOVER;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, BORDER);
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, TEXT);
    v.widgets.active.weak_bg_fill = SELECTED;
    v.widgets.active.bg_fill = SELECTED;
    v.widgets.active.fg_stroke = Stroke::new(1.0, TEXT);
    v.widgets.open.weak_bg_fill = SELECTED;
    ctx.set_visuals(v);
    ctx.global_style_mut(|s| {
        s.spacing.item_spacing = egui::vec2(6.0, 4.0);
        s.spacing.button_padding = egui::vec2(8.0, 3.0);
        s.text_styles.insert(egui::TextStyle::Body, FontId::proportional(13.0));
        s.text_styles.insert(egui::TextStyle::Button, FontId::proportional(13.0));
        s.text_styles.insert(egui::TextStyle::Small, FontId::proportional(11.0));
    });
}
