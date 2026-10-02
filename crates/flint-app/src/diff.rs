//! Unified diff rendering: gutter markers, tinted add/remove rows, hunk headers.

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::theme::MONO_FONT;
use crate::theme::palette;
use crate::theme::size;

/// Renders up to `max_lines` lines of a unified diff; returns the element and
/// how many lines were left out.
pub fn render(unified: &str, max_lines: usize) -> (Div, usize) {
    let p = palette();
    let lines: Vec<&str> = unified
        .lines()
        .filter(|line| !line.starts_with("---") && !line.starts_with("+++"))
        .collect();
    let hidden = lines.len().saturating_sub(max_lines);
    let rows = lines.into_iter().take(max_lines).map(|line| {
        let (marker, body, fg, bg) = match line.chars().next() {
            Some('+') => ("+", &line[1..], p.diff_add_fg, Some(p.diff_add_bg)),
            Some('-') => ("−", &line[1..], p.diff_del_fg, Some(p.diff_del_bg)),
            Some('@') => ("", line, p.diff_hunk_fg, None),
            Some(' ') => (" ", &line[1..], p.text_muted, None),
            _ => (" ", line, p.text_muted, None),
        };
        div()
            .flex()
            .w_full()
            .min_h(px(18.))
            .when_some(bg, |row, bg| row.bg(bg))
            .child(
                div()
                    .w(px(22.))
                    .flex_shrink_0()
                    .text_color(fg)
                    .opacity(0.8)
                    .flex()
                    .justify_center()
                    .child(marker.to_string()),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .pr(px(10.))
                    .text_color(fg)
                    .whitespace_nowrap()
                    .overflow_hidden()
                    .child(if body.is_empty() {
                        " ".to_string()
                    } else {
                        body.to_string()
                    }),
            )
    });
    let element = div()
        .w_full()
        .py(px(4.))
        .font_family(MONO_FONT)
        .font_features(FontFeatures::disable_ligatures())
        .text_size(px(size::SM))
        .line_height(px(18.))
        .flex()
        .flex_col()
        .children(rows);
    (element, hidden)
}
