//! Small shared building blocks: icons, labels, key hints, number formatting.

use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::*;

use crate::theme::MONO_FONT;
use crate::theme::size;

pub fn icon(name: IconName, size_px: f32, color: Hsla) -> Icon {
    Icon::new(name).size(px(size_px)).text_color(color)
}

/// Muted label text at the given size.
pub fn label(text: impl Into<SharedString>, size_px: f32, color: Hsla) -> Div {
    div()
        .text_size(px(size_px))
        .text_color(color)
        .child(text.into())
}

pub fn mono(text: impl Into<SharedString>, size_px: f32, color: Hsla) -> Div {
    div()
        .font_family(MONO_FONT)
        .font_features(FontFeatures::disable_ligatures())
        .text_size(px(size_px))
        .text_color(color)
        .child(text.into())
}

/// A small pill: colored text on a soft tint.
pub fn pill(text: impl Into<SharedString>, fg: Hsla, bg: Hsla) -> Div {
    div()
        .px(px(6.))
        .h(px(18.))
        .flex()
        .items_center()
        .rounded(px(4.))
        .bg(bg)
        .text_size(px(size::XS))
        .font_weight(FontWeight::MEDIUM)
        .text_color(fg)
        .child(text.into())
}

/// `24.9k`, `1.2M`, `512`.
pub fn tokens(n: u64) -> String {
    match n {
        0..=999 => n.to_string(),
        1_000..=999_999 => trim_decimal(n as f64 / 1_000., "k"),
        _ => trim_decimal(n as f64 / 1_000_000., "M"),
    }
}

fn trim_decimal(value: f64, suffix: &str) -> String {
    if value >= 100. {
        format!("{value:.0}{suffix}")
    } else {
        format!("{value:.1}{suffix}")
    }
}

/// `850ms`, `4.2s`, `1m 12s`.
pub fn duration(d: Duration) -> String {
    let ms = d.as_millis();
    if ms < 1_000 {
        format!("{ms}ms")
    } else if ms < 60_000 {
        format!("{:.1}s", d.as_secs_f64())
    } else {
        let secs = d.as_secs();
        format!("{}m {}s", secs / 60, secs % 60)
    }
}

/// `now`, `5m`, `2h`, `3d` since `elapsed`.
pub fn ago(elapsed: Duration) -> String {
    let secs = elapsed.as_secs();
    match secs {
        0..=59 => "now".to_string(),
        60..=3_599 => format!("{}m", secs / 60),
        3_600..=86_399 => format!("{}h", secs / 3_600),
        _ => format!("{}d", secs / 86_400),
    }
}
