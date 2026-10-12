//! Hooter's visual system: layered charcoal, warm ivory, and a restrained
//! apricot accent. Avenir Next for UI and JetBrains Mono for code.
//! Installed over gpui-component's dark theme so stock
//! components (buttons, inputs, palette) match the custom surfaces.

use std::sync::LazyLock;

use gpui_kit::component::Theme;
use gpui_kit::component::ThemeMode;
use gpui_kit::*;

/// App-specific colors the component theme has no slot for.
pub struct Palette {
    /// Tint over the blurred window backdrop (around the floating sidebar).
    pub window_tint: Hsla,
    /// The floating sidebar's translucent fill.
    pub sidebar_fill: Hsla,
    /// The user's message bubble.
    pub bubble: Hsla,
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
    /// Text on the accent fill.
    pub on_accent: Hsla,
    pub success: Hsla,
    pub danger: Hsla,
    pub warning: Hsla,
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
    window_tint: hexa(0x101113, 0.96),
    sidebar_fill: hex(0x191a1d),
    bubble: hex(0x222327),
    bg: hex(0x131416),
    chrome: hex(0x17181b),
    surface: hex(0x1b1c20),
    raised: hex(0x25262b),
    border: hex(0x2c2d32),
    border_strong: hex(0x3a3b42),
    text: hex(0xf1eee8),
    text_muted: hex(0xb0b0b8),
    // >= 4.5:1 even against the raised and message-bubble surfaces.
    text_subtle: hex(0x999aa5),
    accent: hex(0xf3b58b),
    accent_soft: hexa(0xf3b58b, 0.12),
    on_accent: hex(0x241912),
    success: hex(0x8cba9b),
    danger: hex(0xf2555a),
    warning: hex(0xe5b454),
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
pub const DISPLAY_FONT: &str = "Avenir Next";

/// Text sizes used across the app (px).
pub mod size {
    pub const XS: f32 = 13.;
    /// Mono text: code, paths, commands.
    pub const SM: f32 = 14.;
    pub const BASE: f32 = 16.;
    pub const MD: f32 = 16.;
    pub const PROSE: f32 = 17.;
}

/// Installs the dark theme and overrides its colors with the palette.
pub fn install(cx: &mut App) {
    Theme::change(ThemeMode::Dark, None, cx);
    let p = palette();
    Theme::update(cx, |theme| {
        theme.font_size = px(size::BASE);
        #[cfg(target_os = "macos")]
        {
            theme.font_family = DISPLAY_FONT.into();
        }
        theme.mono_font_family = MONO_FONT.into();
        theme.mono_font_size = px(13.5);
        theme.radius = px(8.);
        theme.radius_lg = px(14.);
        theme.shadow = true;

        let c = &mut theme.colors;
        c.background = p.bg;
        c.foreground = p.text;
        c.border = p.border;
        c.input = p.border_strong;
        c.ring = p.accent.opacity(0.45);
        c.caret = p.accent;
        c.selection = p.accent.opacity(0.22);
        c.muted = p.surface;
        c.muted_foreground = p.text_muted;
        c.accent = p.raised;
        c.accent_foreground = p.text;
        c.popover = p.surface;
        c.popover_foreground = p.text;
        c.overlay = hexa(0x000000, 0.55);
        c.primary = p.accent;
        c.primary_hover = hex(0xffc7a1);
        c.primary_active = hex(0xe5a57a);
        c.primary_foreground = p.on_accent;
        c.button_primary = p.accent;
        c.button_primary_hover = c.primary_hover;
        c.button_primary_active = c.primary_active;
        c.button_primary_foreground = p.on_accent;
        c.secondary = p.raised;
        c.secondary_hover = hex(0x2e2f35);
        c.secondary_active = hex(0x35363d);
        c.secondary_foreground = p.text;
        c.button = p.raised;
        c.button_hover = c.secondary_hover;
        c.button_active = c.secondary_active;
        c.button_foreground = p.text;
        c.danger = p.danger;
        c.danger_foreground = p.text;
        c.success = p.success;
        c.warning = p.warning;
        c.info = p.info;
        c.link = p.info;
        c.list = p.chrome;
        c.list_hover = p.raised;
        c.list_active = p.accent_soft;
        c.list_active_border = p.accent.opacity(0.35);
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
