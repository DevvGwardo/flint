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

/// Characters retained from the first nonempty line, excluding the ellipsis.
pub const MAX_HEADER_PREVIEW_CHARS: usize = 240;

/// A bounded, single-line display preview. Keep the full text in the model
/// for command actions and expanded request details. Bound before allocating
/// or shaping, not just with visual ellipsis after layout.
pub fn one_line(text: &str) -> String {
    let mut chars = text.trim_start().chars();
    let mut preview = String::new();
    for _ in 0..MAX_HEADER_PREVIEW_CHARS {
        match chars.next() {
            Some('\n') | None => break,
            Some(ch) => preview.push(ch),
        }
    }
    preview.truncate(preview.trim_end().len());
    if !chars.as_str().trim().is_empty() {
        preview.push_str(" …");
    }
    preview
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

/// `base` given a hue from `seed`, keeping its lightness and alpha so that
/// emphasis (bright for active, dim for idle) survives the tint.
pub fn tinted(base: Hsla, seed: u64) -> Hsla {
    // FNV-1a over the seed's bytes spreads sequential ids across the wheel.
    let hash = seed
        .to_le_bytes()
        .iter()
        .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
        });
    hsla((hash % 360) as f32 / 360., 0.5, base.l, base.a)
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

/// The animation clock's period while something runs (see `FlintApp`'s
/// ticker). Glyphs below advance per tick, so nothing animates per frame and
/// an idle window never repaints.
pub const TICK_MS: u64 = 100;

fn phase(now: Duration, steps: u32) -> f32 {
    let tick = (now.as_millis() / TICK_MS as u128) as u32;
    (tick % steps) as f32 / steps as f32
}

/// A loading ring rotated by the animation clock.
pub fn spinner(now: Duration, size_px: f32, color: Hsla) -> Icon {
    let turn = phase(now, 12) * std::f32::consts::TAU;
    icon(IconName::LoaderCircle, size_px, color).rotate(radians(turn))
}

/// The "working" mark for the status line: a slowly turning asterisk drawn
/// from the bundled SVG icons (no font glyph fallback).
pub fn work_glyph(now: Duration, size_px: f32, color: Hsla) -> Icon {
    let turn = phase(now, 16) * std::f32::consts::TAU / 2.;
    icon(IconName::Asterisk, size_px, color).rotate(radians(turn))
}

#[cfg(test)]
mod tests {
    use super::{MAX_HEADER_PREVIEW_CHARS, one_line};

    #[test]
    #[ignore = "manual release-mode performance probe"]
    fn command_preview_perf_probe() {
        let summary = format!("echo {}", "x".repeat(8 * 1024 * 1024));
        let start = std::time::Instant::now();
        let mut max_capacity = 0;
        for _ in 0..100 {
            let preview = one_line(std::hint::black_box(&summary));
            max_capacity = max_capacity.max(preview.capacity());
            std::hint::black_box(preview);
        }
        eprintln!(
            "command_preview_100_runs_ms={:.3}",
            start.elapsed().as_secs_f64() * 1000.
        );
        eprintln!("command_preview_capacity_bytes={max_capacity}");
    }

    #[test]
    fn command_headers_are_one_line_without_losing_the_continuation_hint() {
        for (text, expected) in [
            ("echo ok", "echo ok"),
            ("  echo ok  ", "echo ok"),
            ("cat <<'END'\na\nEND", "cat <<'END' …"),
            ("\n \n  echo ok\nnext", "echo ok …"),
            ("\r\n  echo ok\r\nnext\r\n", "echo ok …"),
            ("echo ok\n\n", "echo ok"),
            ("\n \r\n", ""),
        ] {
            assert_eq!(one_line(text), expected);
        }
    }

    #[test]
    fn long_command_previews_are_unicode_safe_and_bounded() {
        for text in [
            "x".repeat(8 * 1024 * 1024),
            "界🙂".repeat(10_000),
            format!("\n \r\n{}\nsecond line", "a".repeat(10_000)),
        ] {
            let preview = one_line(&text);
            assert_eq!(preview.chars().count(), MAX_HEADER_PREVIEW_CHARS + 2);
            assert!(preview.ends_with(" …"));
            assert!(!preview.contains(['\n', '\r', '\u{fffd}']));
            assert!(preview.capacity() <= MAX_HEADER_PREVIEW_CHARS * 8);
        }
    }

    #[test]
    fn preview_boundary_and_trailing_blank_lines_do_not_add_false_ellipsis() {
        let exact = "界".repeat(MAX_HEADER_PREVIEW_CHARS);
        assert_eq!(one_line(&exact), exact);
        assert_eq!(one_line(&format!("{exact} \r\n \n")), exact);
        assert_eq!(one_line(&format!("{exact}x")), format!("{exact} …"));
        assert_eq!(one_line(&format!("{exact}\nnext")), format!("{exact} …"));
        assert_eq!(one_line("echo ok   \n \n"), "echo ok");
        assert_eq!(one_line(&" \r\n".repeat(1_000)), "");
    }
}
