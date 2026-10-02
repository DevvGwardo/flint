//! Tool call cards: commands with live output, compact read/search rows, and
//! edit cards with inline diffs.

use std::time::Duration;

use flint_agent::ToolKind;
use gpui_kit::assets::IconName;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::app::FlintApp;
use crate::diff;
use crate::theme::MONO_FONT;
use crate::theme::palette;
use crate::theme::size;
use crate::ui;
use crate::view_model::ToolCall;

/// Output lines shown under a running command before it is expanded.
const LIVE_TAIL_LINES: usize = 8;
/// Finished output this short is shown without expanding the card.
const SHORT_OUTPUT_LINES: usize = 3;
/// Hard cap on rendered output lines, even expanded.
const MAX_OUTPUT_LINES: usize = 400;
/// Diff lines shown before "show more".
const DIFF_PREVIEW_LINES: usize = 16;

pub fn render(ix: usize, call: &ToolCall, now: Duration, cx: &mut Context<FlintApp>) -> AnyElement {
    match call.kind {
        ToolKind::Read | ToolKind::Search | ToolKind::Other => {
            compact(ix, call, cx).into_any_element()
        }
        ToolKind::Command => command(ix, call, now, cx).into_any_element(),
        ToolKind::Edit => edit(ix, call, cx).into_any_element(),
    }
}

fn status(call: &ToolCall, now: Duration) -> Div {
    let p = palette();
    let row = div().flex().items_center().gap(px(8.));
    match &call.result {
        None => row
            .child(ui::label(
                ui::duration(Duration::from_secs(
                    now.saturating_sub(call.started).as_secs(),
                )),
                size::XS,
                p.text_subtle,
            ))
            .child(Spinner::new().xsmall().color(p.accent)),
        Some(result) => {
            let duration = ui::duration(Duration::from_millis(result.duration_ms));
            let badge = match (result.success, result.exit_code) {
                (true, _) => ui::icon(IconName::Check, 12., p.success).into_any_element(),
                (false, Some(code)) => {
                    ui::pill(format!("exit {code}"), p.danger, p.danger_soft).into_any_element()
                }
                (false, None) => ui::pill("failed", p.danger, p.danger_soft).into_any_element(),
            };
            row.child(ui::label(duration, size::XS, p.text_subtle))
                .child(badge)
        }
    }
}

fn chevron(expanded: bool) -> impl IntoElement {
    ui::icon(
        if expanded {
            IconName::ChevronDown
        } else {
            IconName::ChevronRight
        },
        12.,
        palette().text_subtle,
    )
}

fn command(
    ix: usize,
    call: &ToolCall,
    now: Duration,
    cx: &mut Context<FlintApp>,
) -> impl IntoElement {
    let p = palette();
    let running = call.result.is_none();
    let failed = call.result.as_ref().is_some_and(|r| !r.success);
    let output = call.output.trim_end_matches('\n');
    // The exit code is already a badge in the header.
    let lines: Vec<&str> = output
        .lines()
        .filter(|line| !is_exit_code_line(line))
        .collect();
    let shown: Vec<&str> = if call.expanded {
        lines.iter().take(MAX_OUTPUT_LINES).copied().collect()
    } else if running {
        lines[lines.len().saturating_sub(LIVE_TAIL_LINES)..].to_vec()
    } else if lines.len() <= SHORT_OUTPUT_LINES {
        // Short results (e.g. a script's one-line output) stay visible.
        lines.clone()
    } else {
        Vec::new()
    };

    div()
        .w_full()
        .rounded(px(8.))
        .border_1()
        .border_color(if failed { p.danger_soft } else { p.border })
        .bg(p.surface)
        .overflow_hidden()
        .child(
            div()
                .id(("tool", ix))
                .h(px(34.))
                .px(px(10.))
                .flex()
                .items_center()
                .gap(px(8.))
                .cursor_pointer()
                .hover(|style| style.bg(p.raised))
                .on_click(cx.listener(move |this, _, _, cx| this.toggle_item(ix, cx)))
                .child(ui::icon(IconName::SquareTerminal, 13., p.text_muted))
                .child(ui::mono("$", size::SM, p.text_subtle))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .font_family(MONO_FONT)
                        .font_features(FontFeatures::disable_ligatures())
                        .text_size(px(size::SM))
                        .text_color(p.text)
                        .child(call.summary.clone()),
                )
                .child(status(call, now))
                .child(chevron(call.expanded)),
        )
        .when(!shown.is_empty(), |card| {
            card.child(
                div()
                    .border_t_1()
                    .border_color(p.border)
                    .bg(p.bg)
                    .px(px(12.))
                    .py(px(8.))
                    .font_family(MONO_FONT)
                    .font_features(FontFeatures::disable_ligatures())
                    .text_size(px(size::SM))
                    .line_height(px(18.))
                    .text_color(p.text_muted)
                    .children(shown.into_iter().map(|line| {
                        div()
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .text_color(output_line_color(line))
                            .child(if line.is_empty() {
                                " ".to_string()
                            } else {
                                line.to_string()
                            })
                    })),
            )
        })
}

fn is_exit_code_line(line: &str) -> bool {
    let line = line.trim().to_ascii_lowercase();
    line.starts_with("[exit code:") && line.ends_with(']')
}

/// Colors test-runner style lines so passes and failures stand out.
fn output_line_color(line: &str) -> Hsla {
    let p = palette();
    let trimmed = line.trim_start();
    if trimmed.starts_with('✓') || trimmed.contains(" passed") && !trimmed.contains("failed") {
        p.diff_add_fg
    } else if trimmed.starts_with('×')
        || trimmed.starts_with('→')
        || trimmed.contains("failed")
        || trimmed.starts_with("error")
        || trimmed.starts_with("Error")
    {
        p.diff_del_fg
    } else {
        p.text_muted
    }
}

fn compact(ix: usize, call: &ToolCall, cx: &mut Context<FlintApp>) -> impl IntoElement {
    let p = palette();
    let (icon, verb) = match call.kind {
        ToolKind::Read => (IconName::FileText, "Read"),
        ToolKind::Search => (IconName::Search, "Searched"),
        _ => (IconName::Wrench, "Called"),
    };
    let verb = if call.kind == ToolKind::Other {
        call.name.clone()
    } else {
        verb.to_string()
    };
    let failed = call.result.as_ref().is_some_and(|r| !r.success);
    let preview: Vec<&str> = if call.expanded {
        call.output.lines().take(40).collect()
    } else {
        Vec::new()
    };
    div()
        .w_full()
        .flex()
        .flex_col()
        .child(
            div()
                .id(("tool", ix))
                .h(px(24.))
                .px(px(2.))
                .flex()
                .items_center()
                .gap(px(8.))
                .rounded(px(5.))
                .cursor_pointer()
                .hover(|style| style.bg(p.surface))
                .on_click(cx.listener(move |this, _, _, cx| this.toggle_item(ix, cx)))
                .child(ui::icon(icon, 13., p.text_subtle))
                .child(ui::label(verb, size::BASE, p.text_muted))
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .font_family(MONO_FONT)
                        .font_features(FontFeatures::disable_ligatures())
                        .text_size(px(size::SM))
                        .text_color(if failed { p.danger } else { p.text })
                        .child(call.summary.clone()),
                )
                .when(call.result.is_none(), |row| {
                    row.child(Spinner::new().xsmall().color(p.text_subtle))
                })
                .when_some(call.result.as_ref(), |row, result| {
                    let lines = call.output.lines().count();
                    let unit = if lines == 1 { "line" } else { "lines" };
                    row.child(ui::label(
                        format!(
                            "{lines} {unit} · {}",
                            ui::duration(Duration::from_millis(result.duration_ms))
                        ),
                        size::XS,
                        p.text_subtle,
                    ))
                }),
        )
        .when(!preview.is_empty(), |col| {
            col.child(
                div()
                    .mt(px(4.))
                    .ml(px(22.))
                    .rounded(px(6.))
                    .border_1()
                    .border_color(p.border)
                    .bg(p.surface)
                    .px(px(10.))
                    .py(px(6.))
                    .font_family(MONO_FONT)
                    .font_features(FontFeatures::disable_ligatures())
                    .text_size(px(size::SM))
                    .line_height(px(18.))
                    .text_color(p.text_muted)
                    .children(preview.into_iter().map(|line| {
                        div()
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .child(if line.is_empty() {
                                " ".to_string()
                            } else {
                                line.to_string()
                            })
                    })),
            )
        })
}

fn edit(ix: usize, call: &ToolCall, cx: &mut Context<FlintApp>) -> impl IntoElement {
    let p = palette();
    let diff_data = call.result.as_ref().and_then(|r| r.diff.as_ref());
    let path = diff_data
        .map(|d| d.path.clone())
        .unwrap_or_else(|| call.summary.clone());
    let failed = call.result.as_ref().is_some_and(|r| !r.success);
    let (verb, icon) = match diff_data {
        Some(d) if d.created => ("Created", IconName::FilePlus),
        _ => ("Edited", IconName::FilePen),
    };
    let limit = if call.expanded {
        usize::MAX
    } else {
        DIFF_PREVIEW_LINES
    };
    let rendered = diff_data.map(|d| diff::render(&d.unified, limit));

    div()
        .w_full()
        .rounded(px(8.))
        .border_1()
        .border_color(if failed { p.danger_soft } else { p.border })
        .bg(p.surface)
        .overflow_hidden()
        .child(
            div()
                .id(("tool", ix))
                .h(px(34.))
                .px(px(10.))
                .flex()
                .items_center()
                .gap(px(8.))
                .cursor_pointer()
                .hover(|style| style.bg(p.raised))
                .on_click(cx.listener(move |this, _, _, cx| this.toggle_item(ix, cx)))
                .child(ui::icon(icon, 13., p.text_muted))
                .child(ui::label(verb, size::BASE, p.text_muted))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .font_family(MONO_FONT)
                        .font_features(FontFeatures::disable_ligatures())
                        .text_size(px(size::SM))
                        .text_color(p.text)
                        .child(path),
                )
                .when_some(diff_data, |row, d| {
                    row.child(ui::mono(format!("+{}", d.added), size::SM, p.success))
                        .child(ui::mono(format!("−{}", d.removed), size::SM, p.danger))
                })
                .when(call.result.is_none(), |row| {
                    row.child(Spinner::new().xsmall().color(p.accent))
                })
                .when(failed, |row| {
                    row.child(ui::pill("failed", p.danger, p.danger_soft))
                }),
        )
        .when_some(rendered, |card, (body, hidden)| {
            card.child(
                div()
                    .border_t_1()
                    .border_color(p.border)
                    .bg(p.bg)
                    .child(body),
            )
            .when(hidden > 0, |card| {
                card.child(
                    div()
                        .id(("tool-more", ix))
                        .h(px(26.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .border_t_1()
                        .border_color(p.border)
                        .cursor_pointer()
                        .hover(|style| style.bg(p.raised))
                        .on_click(cx.listener(move |this, _, _, cx| this.toggle_item(ix, cx)))
                        .child(ui::label(
                            format!("Show {hidden} more lines"),
                            size::XS,
                            p.text_muted,
                        )),
                )
            })
        })
        .when(failed && diff_data.is_none(), |card| {
            card.child(
                div()
                    .border_t_1()
                    .border_color(p.border)
                    .px(px(12.))
                    .py(px(8.))
                    .font_family(MONO_FONT)
                    .font_features(FontFeatures::disable_ligatures())
                    .text_size(px(size::SM))
                    .text_color(p.diff_del_fg)
                    .child(call.output.lines().take(6).collect::<Vec<_>>().join("\n")),
            )
        })
}
