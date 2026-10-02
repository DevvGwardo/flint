//! flint's visual system: a near-black, neutral, dark-first palette with one
//! restrained ember accent, hairline borders, system UI type and JetBrains
//! Mono for code. Installed over gpui-component's dark theme so stock
//! components (buttons, inputs, palette) match the custom surfaces.

use std::sync::LazyLock;

use gpui_kit::component::Theme;
use gpui_kit::component::ThemeMode;
use gpui_kit::*;

/// App-specific colors the component theme has no slot for.
pub struct Palette {
    /// Window background (transcript canvas).
    pub bg: Hsla,
    /// Chrome: title bar, sidebar, status bar.
    pub chrome: Hsla,
    /// Cards and code surfaces.
    pub surface: Hsla,
    /// Hovered / raised surfaces.
    pub raised: Hsla,
    pub border: Hsla,
    pub border_strong: Hsla,
    pub text: Hsla,
    pub text_muted: Hsla,
    pub text_subtle: Hsla,
    pub accent: Hsla,
    pub accent_soft: Hsla,
    pub success: Hsla,
    pub danger: Hsla,
    pub danger_soft: Hsla,
    pub warning: Hsla,
    pub warning_soft: Hsla,
    pub info: Hsla,
    pub diff_add_bg: Hsla,
    pub diff_del_bg: Hsla,
    pub diff_add_fg: Hsla,
    pub diff_del_fg: Hsla,
    pub diff_hunk_fg: Hsla,
}

fn hex(value: u32) -> Hsla {
    rgb(value).into()
}

fn hexa(value: u32, alpha: f32) -> Hsla {
    let mut color: Hsla = rgb(value).into();
    color.a = alpha;
    color
}

pub static PALETTE: LazyLock<Palette> = LazyLock::new(|| Palette {
    bg: hex(0x0c0c0e),
    chrome: hex(0x0f0f12),
    surface: hex(0x141418),
    raised: hex(0x1b1b20),
    border: hex(0x222228),
    border_strong: hex(0x2f2f37),
    text: hex(0xe7e7ea),
    text_muted: hex(0x9a9aa4),
    text_subtle: hex(0x63636d),
    accent: hex(0xff8a3d),
    accent_soft: hexa(0xff8a3d, 0.14),
    success: hex(0x4cc38a),
    danger: hex(0xf2555a),
    danger_soft: hexa(0xf2555a, 0.13),
    warning: hex(0xe5b454),
    warning_soft: hexa(0xe5b454, 0.12),
    info: hex(0x6aa6ff),
    diff_add_bg: hexa(0x4cc38a, 0.10),
    diff_del_bg: hexa(0xf2555a, 0.10),
    diff_add_fg: hex(0x8fd9b2),
    diff_del_fg: hex(0xf59a9d),
    diff_hunk_fg: hex(0x7d8cff),
});

pub fn palette() -> &'static Palette {
    &PALETTE
}

pub const MONO_FONT: &str = "JetBrains Mono";

/// Text sizes used across the app (px).
pub mod size {
    pub const XS: f32 = 11.;
    pub const SM: f32 = 12.;
    pub const BASE: f32 = 13.;
    pub const MD: f32 = 14.;
}

/// Installs the dark theme and overrides its colors with the palette.
pub fn install(cx: &mut App) {
    Theme::change(ThemeMode::Dark, None, cx);
    let p = palette();
    Theme::update(cx, |theme| {
        theme.font_size = px(size::BASE);
        theme.mono_font_family = MONO_FONT.into();
        theme.mono_font_size = px(12.);
        theme.radius = px(6.);
        theme.radius_lg = px(10.);
        theme.shadow = true;

        let c = &mut theme.colors;
        c.background = p.bg;
        c.foreground = p.text;
        c.border = p.border;
        c.input = p.border_strong;
        c.ring = hexa(0xff8a3d, 0.45);
        c.caret = p.accent;
        c.selection = hexa(0xff8a3d, 0.22);
        c.muted = p.surface;
        c.muted_foreground = p.text_muted;
        c.accent = p.raised;
        c.accent_foreground = p.text;
        c.popover = hex(0x151519);
        c.popover_foreground = p.text;
        c.overlay = hexa(0x000000, 0.55);
        c.primary = p.accent;
        c.primary_hover = hex(0xff9a55);
        c.primary_active = hex(0xf07a2c);
        c.primary_foreground = hex(0x1a0d04);
        c.button_primary = p.accent;
        c.button_primary_hover = hex(0xff9a55);
        c.button_primary_active = hex(0xf07a2c);
        c.button_primary_foreground = hex(0x1a0d04);
        c.secondary = p.raised;
        c.secondary_hover = hex(0x232329);
        c.secondary_active = hex(0x2a2a31);
        c.secondary_foreground = p.text;
        c.button = p.raised;
        c.button_hover = hex(0x232329);
        c.button_active = hex(0x2a2a31);
        c.button_foreground = p.text;
        c.danger = p.danger;
        c.danger_foreground = p.text;
        c.success = p.success;
        c.warning = p.warning;
        c.info = p.info;
        c.link = p.info;
        c.list = p.chrome;
        c.list_hover = p.raised;
        c.list_active = hexa(0xff8a3d, 0.12);
        c.list_active_border = hexa(0xff8a3d, 0.35);
        c.sidebar = p.chrome;
        c.sidebar_foreground = p.text;
        c.sidebar_border = p.border;
        c.sidebar_accent = p.raised;
        c.sidebar_accent_foreground = p.text;
        c.title_bar = p.chrome;
        c.title_bar_border = p.border;
        c.status_bar = p.chrome;
        c.status_bar_border = p.border;
        c.scrollbar = hexa(0x000000, 0.0);
        c.scrollbar_thumb = hexa(0xffffff, 0.12);
        c.scrollbar_thumb_hover = hexa(0xffffff, 0.2);
        c.tab_bar = p.chrome;
        c.window_border = p.border;
        c.drag_border = p.accent;
    });
}
