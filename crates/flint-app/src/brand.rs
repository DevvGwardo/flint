//! User-facing identity. Storage keys and crate names stay compatible with Flint.

use gpui_kit::*;

use crate::theme::{DISPLAY_FONT, palette};

pub const NAME: &str = flint_agent::NATIVE_AGENT_NAME;
pub const APP_ID: &str = "com.hooter.desktop";

pub fn mark(edge: f32) -> Div {
    let p = palette();
    div()
        .size(px(edge))
        .flex_shrink_0()
        .rounded(px(edge * 0.28))
        .bg(p.accent)
        .flex()
        .items_center()
        .justify_center()
        .font_family(DISPLAY_FONT)
        .font_weight(FontWeight::BOLD)
        .text_size(px(edge * 0.64))
        .line_height(px(edge))
        .text_color(p.on_accent)
        .child("h")
}
