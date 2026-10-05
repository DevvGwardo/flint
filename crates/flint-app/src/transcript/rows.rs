//! Activity rows, Claude Code style: `●` bullets for prose, dim one-liners
//! for tool calls with `└` detail lines, and output or diffs that open on
//! click. Only expanded output, diffs and approvals get a border.

use std::time::Duration;

use flint_agent::NudgeReason;
use flint_agent::ToolKind;
use gpui_kit::assets::IconName;
use gpui_kit::component::shimmer::ShimmerText;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::app::FlintApp;
use crate::diff;
use crate::subagent_ui::Conversation;
use crate::theme::MONO_FONT;
use crate::theme::palette;
use crate::theme::size;
use crate::ui;
use crate::view_model::Item;
use crate::view_model::ToolCall;

/// Width of the glyph gutter on the left of every activity row.
pub(super) const GUTTER: f32 = 24.;
/// Live output lines shown under a running command.
const LIVE_TAIL_LINES: usize = 4;
/// Cap on rendered output lines, even expanded.
const MAX_OUTPUT_LINES: usize = 400;

pub fn render(
    target: &Conversation,
    ix: usize,
    item: &Item,
    now: Duration,
    base_url: &str,
    workspace: &std::path::Path,
    cx: &mut Context<FlintApp>,
) -> AnyElement {
    match item {
        Item::User(text) => user(text).into_any_element(),
        Item::Assistant { text, streaming } => bullet(
            dot(palette().text_muted),
            div()
                .id(("assistant-text", ix))
                .child(
                    crate::file_preview::markdown(
                        ("md", ix),
                        text.clone(),
                        workspace.to_path_buf(),
                        cx,
                    )
                    .stream_fade(*streaming),
                )
                .test_support(),
        )
        .text_size(px(size::PROSE))
        .line_height(px(26.))
        .into_any_element(),
        Item::Thinking {
            text,
            duration,
            expanded,
            ..
        } => thinking(target, ix, text, *duration, *expanded, cx).into_any_element(),
        Item::Tool(call) => tool(target, ix, call, now, cx).into_any_element(),
        Item::Nudge {
            reason,
            message,
            expanded,
        } => nudge(target, ix, *reason, message, *expanded, cx).into_any_element(),
        Item::Compacted {
            before_tokens,
            after_tokens,
        } => bullet(
            ui::icon(IconName::Scissors, 12., palette().text_subtle),
            ui::label(
                format!(
                    "Context trimmed {} → {} tokens to stay within the budget",
                    ui::tokens(*before_tokens),
                    ui::tokens(*after_tokens)
                ),
                size::SM,
                palette().text_subtle,
            ),
        )
        .into_any_element(),
        Item::Reverted { restored, skipped } => {
            let mut text = if restored.is_empty() {
                "Undo restored no files".to_string()
            } else {
                format!("Undo restored {}", restored.join(", "))
            };
            if !skipped.is_empty() {
                text.push_str(&format!(" · left alone: {}", skipped.join("; ")));
            }
            bullet(
                ui::icon(IconName::ArchiveRestore, 12., palette().text_subtle),
                ui::label(text, size::SM, palette().text_subtle),
            )
            .into_any_element()
        }
        Item::Repair { tool, detail } => bullet(
            ui::icon(IconName::Wrench, 11., palette().text_subtle),
            ui::label(
                format!("Repaired {tool} · {detail}"),
                size::SM,
                palette().text_subtle,
            ),
        )
        .into_any_element(),
        Item::Approval {
            call_id,
            kind,
            summary,
            decision,
        } => {
            let _ = call_id;
            super::approval::record(*kind, summary, *decision).into_any_element()
        }
        Item::Error(message) if target.child.is_some() => {
            ui::label(message.clone(), size::SM, palette().danger).into_any_element()
        }
        Item::Error(message) => super::errors::render(ix, message, base_url, cx),
        Item::TurnSummary { .. } => div().into_any_element(),
    }
}

/// A row with a fixed glyph gutter and content.
fn bullet(glyph: impl IntoElement, content: impl IntoElement) -> Div {
    div()
        .w_full()
        .flex()
        .child(
            div()
                .w(px(GUTTER))
                .h(px(26.))
                .flex_shrink_0()
                .flex()
                .items_center()
                .child(glyph),
        )
        .child(div().flex_1().min_w_0().child(content))
}

fn dot(color: Hsla) -> Div {
    div().size(px(7.)).rounded_full().bg(color)
}

/// `└ detail`, indented under a row's content.
fn tree_child(content: impl IntoElement) -> Div {
    let p = palette();
    div()
        .pl(px(GUTTER))
        .flex()
        .gap(px(6.))
        .child(ui::mono("└", size::SM, p.text_subtle).flex_shrink_0())
        .child(div().flex_1().min_w_0().child(content))
}

fn user(text: &str) -> impl IntoElement {
    let p = palette();
    div().w_full().flex().justify_end().child(
        div()
            .max_w(relative(0.85))
            .px(px(18.))
            .py(px(12.))
            .rounded(px(18.))
            .bg(p.bubble)
            .text_size(px(size::PROSE))
            .line_height(px(26.))
            .child(text.to_string()),
    )
}

fn thinking(
    target: &Conversation,
    ix: usize,
    text: &str,
    duration: Option<Duration>,
    expanded: bool,
    cx: &mut Context<FlintApp>,
) -> impl IntoElement {
    let p = palette();
    let target = target.clone();
    let label: AnyElement = match duration {
        None => ShimmerText::new("Thinking…")
            .id(("thinking-shimmer", ix))
            .into_any_element(),
        Some(d) => ui::label(
            format!("Thought for {}", ui::duration(d)),
            size::BASE,
            p.text_muted,
        )
        .into_any_element(),
    };
    div()
        .flex()
        .flex_col()
        .gap(px(6.))
        .child(
            div()
                .id(("thinking", ix))
                .cursor_pointer()
                .on_click(
                    cx.listener(move |this, _, _, cx| {
                        this.toggle_conversation_item(&target, ix, cx)
                    }),
                )
                .child(bullet(
                    ui::label("✻", size::BASE, p.text_subtle),
                    div()
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .text_size(px(size::BASE))
                        .child(label)
                        .child(chevron(expanded)),
                )),
        )
        .when(expanded, |col| {
            col.child(
                div()
                    .ml(px(GUTTER))
                    .pl(px(12.))
                    .border_l_1()
                    .border_color(p.border_strong)
                    .text_size(px(size::BASE))
                    .line_height(px(24.))
                    .text_color(p.text_muted)
                    .italic()
                    .child(text.to_string()),
            )
        })
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

fn tool(
    target: &Conversation,
    ix: usize,
    call: &ToolCall,
    now: Duration,
    cx: &mut Context<FlintApp>,
) -> impl IntoElement {
    let p = palette();
    let toggle_target = target.clone();
    let running = call.result.is_none();
    let failed = call.result.as_ref().is_some_and(|r| !r.success);
    let glyph: AnyElement = if running {
        ui::spinner(now, 13., p.accent).into_any_element()
    } else if failed {
        dot(p.danger).into_any_element()
    } else {
        dot(p.success).into_any_element()
    };
    let (verb, summary) = match call.kind {
        ToolKind::Command => ("Ran", call.summary.as_str()),
        ToolKind::Read => ("Read", call.summary.as_str()),
        ToolKind::Search => ("Searched", call.summary.as_str()),
        ToolKind::Edit => match call.result.as_ref().and_then(|r| r.diff.as_ref()) {
            Some(d) if d.created => ("Created", d.path.as_str()),
            Some(d) => ("Edited", d.path.as_str()),
            None => ("Editing", call.summary.as_str()),
        },
        ToolKind::Other if call.name == flint_agent::tools::SPAWN_AGENT => (
            if running { "Delegating" } else { "Delegated" },
            call.summary.as_str(),
        ),
        ToolKind::Other => ("Called", call.name.as_str()),
    };
    let preview = ui::one_line(summary);
    let verb = if running && call.kind == ToolKind::Command {
        "Running"
    } else {
        verb
    };

    let meta: AnyElement = match &call.result {
        None => ui::label(
            ui::duration(Duration::from_secs(
                now.saturating_sub(call.started).as_secs(),
            )),
            size::XS,
            p.text_subtle,
        )
        .into_any_element(),
        Some(result) => {
            let time = ui::duration(Duration::from_millis(result.duration_ms));
            match (&result.diff, call.kind) {
                (Some(d), _) => div()
                    .flex()
                    .gap(px(5.))
                    .child(ui::mono(format!("+{}", d.added), size::XS, p.success))
                    .child(ui::mono(format!("−{}", d.removed), size::XS, p.danger))
                    .into_any_element(),
                (None, ToolKind::Command) => {
                    let status = match (result.success, result.exit_code) {
                        (true, _) => ui::label("✓", size::XS, p.success),
                        (false, Some(code)) => {
                            ui::label(format!("exit {code}"), size::XS, p.danger)
                        }
                        (false, None) => ui::label("failed", size::XS, p.danger),
                    };
                    div()
                        .flex()
                        .gap(px(5.))
                        .child(status)
                        .child(ui::label(time, size::XS, p.text_subtle))
                        .into_any_element()
                }
                _ => ui::label(time, size::XS, p.text_subtle).into_any_element(),
            }
        }
    };

    // Only what this frame shows is read from the (possibly large) output:
    // the tail from the end, the head from the start.
    let tail_lines = |n: usize| -> Vec<&str> {
        let mut lines: Vec<&str> = call
            .output
            .rsplit('\n')
            .filter(|line| !line.trim().is_empty() && !is_exit_code_line(line))
            .take(n)
            .collect();
        lines.reverse();
        lines
    };
    // The one-line summary under the row (the expanded view shows everything).
    let detail: Option<String> = if call.expanded || running {
        None
    } else {
        collapsed_detail(call.kind, &call.output, failed)
    };
    let live_tail: Vec<&str> = if running && call.kind == ToolKind::Command && !call.expanded {
        tail_lines(LIVE_TAIL_LINES)
    } else {
        Vec::new()
    };
    let output: Vec<&str> = if call.expanded {
        call.output
            .lines()
            .filter(|line| !is_exit_code_line(line))
            .take(MAX_OUTPUT_LINES)
            .collect()
    } else {
        Vec::new()
    };
    let diff = call.result.as_ref().and_then(|r| r.diff.as_ref());

    div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(2.))
        .child(
            div()
                .id(("tool", ix))
                .role(gpui_kit::Role::Button)
                .aria_label(format!(
                    "{} {}: {}",
                    if call.expanded { "Collapse" } else { "Expand" },
                    verb,
                    preview
                ))
                .cursor_pointer()
                .rounded(px(5.))
                .hover(|style| style.bg(hsla(0., 0., 1., 0.025)))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.toggle_conversation_item(&toggle_target, ix, cx)
                }))
                .child(bullet(
                    glyph,
                    div()
                        .id(("tool-header", ix))
                        .h(px(26.))
                        .overflow_hidden()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(ui::label(verb, size::BASE, p.text_muted).flex_shrink_0())
                        .child(
                            div()
                                .id(("tool-summary", ix))
                                .min_w_0()
                                .truncate()
                                .font_family(MONO_FONT)
                                .font_features(FontFeatures::disable_ligatures())
                                .text_size(px(size::SM))
                                .text_color(if failed { p.danger } else { p.text })
                                .child(preview)
                                .test_support(),
                        )
                        .child(meta)
                        .when(
                            call.kind == ToolKind::Command && target.child.is_none(),
                            |row| row.child(command_actions(target.parent, ix, call, cx)),
                        )
                        .child(chevron(call.expanded))
                        .test_support(),
                ))
                .test_support(),
        )
        .when_some(detail, |col, detail| {
            col.child(tree_child(
                div()
                    .id(("tool-detail", ix))
                    .truncate()
                    .font_family(MONO_FONT)
                    .font_features(FontFeatures::disable_ligatures())
                    .text_size(px(size::XS))
                    .text_color(if failed { p.diff_del_fg } else { p.text_subtle })
                    .child(detail)
                    .test_support(),
            ))
        })
        .when(!live_tail.is_empty(), |col| {
            col.child(tree_child(output_lines(&live_tail, p.text_subtle)))
        })
        .when_some(call.subagent.as_ref(), |col, child| {
            col.child(tree_child(
                ui::label(
                    format!("{} · {}", child.model, child.session_id),
                    size::XS,
                    p.text_subtle,
                )
                .truncate(),
            ))
        })
        .when(call.expanded, |col| {
            let block = if let Some(child) = &call.subagent {
                let owner = target.parent;
                let id = child.session_id.clone();
                div()
                    .p(px(12.))
                    .child(
                        gpui_kit::component::button::Button::new(("open-subagent", ix))
                            .label("Open full subagent conversation")
                            .on_click(cx.listener(move |app, _, window, cx| {
                                app.select_subagent(owner, &id, window, cx);
                            })),
                    )
                    .into_any_element()
            } else {
                match (diff, call.kind) {
                    (Some(d), _) => diff::render(&d.unified, usize::MAX).0.into_any_element(),
                    _ => div()
                        .px(px(16.))
                        .py(px(12.))
                        .when(call.kind == ToolKind::Command, |block| {
                            let command = crate::app_actions::command_text(call);
                            block.child(
                                ui::mono(command, size::SM, p.text)
                                    .id(("tool-command", ix))
                                    .w_full()
                                    .min_w_0()
                                    .line_height(px(21.))
                                    .mb(px(10.))
                                    .test_support(),
                            )
                        })
                        .child(output_lines(
                            &output
                                .iter()
                                .take(MAX_OUTPUT_LINES)
                                .copied()
                                .collect::<Vec<_>>(),
                            p.text_muted,
                        ))
                        .into_any_element(),
                }
            };
            col.child(
                div()
                    .ml(px(GUTTER))
                    .mt(px(6.))
                    .mb(px(6.))
                    .rounded(px(12.))
                    .border_1()
                    .border_color(p.border)
                    .bg(p.surface)
                    .overflow_hidden()
                    .child(block),
            )
        })
}

/// A command card's two small actions: bring the command's terminal up, and
/// send its output back to the agent.
fn command_actions(owner: u64, ix: usize, call: &ToolCall, cx: &mut Context<FlintApp>) -> Div {
    let p = palette();
    let button = |id: (&'static str, usize), icon: IconName, tip: &'static str| {
        div()
            .id(id)
            .size(px(22.))
            .flex_shrink_0()
            .rounded(px(6.))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .hover(|style| style.bg(p.raised))
            .tooltip(move |window, cx| Tooltip::new(tip).build(window, cx))
            .child(ui::icon(icon, 13., p.text_subtle))
    };
    div()
        .flex()
        .items_center()
        .gap(px(2.))
        .child(
            button(
                ("tool-terminal", ix),
                if call.terminal_id.is_some() {
                    IconName::Bot
                } else {
                    IconName::SquareTerminal
                },
                "Open in terminal",
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                let Some(session_ix) = this.session_index(owner) else {
                    return;
                };
                this.select_session_with_focus(session_ix, false, window, cx);
                this.open_in_terminal(ix, window, cx);
            }))
            .test_support(),
        )
        .when(!call.output.trim().is_empty(), |row| {
            row.child(
                button(
                    ("tool-send", ix),
                    IconName::Send,
                    "Send the output to the agent",
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    let Some(session_ix) = this.session_index(owner) else {
                        return;
                    };
                    this.select_session_with_focus(session_ix, false, window, cx);
                    this.send_command_to_agent(ix, cx);
                }))
                .test_support(),
            )
        })
}

fn output_lines(lines: &[&str], color: Hsla) -> impl IntoElement {
    div()
        .font_family(MONO_FONT)
        .font_features(FontFeatures::disable_ligatures())
        .text_size(px(size::SM))
        .line_height(px(21.))
        .text_color(color)
        .children(lines.iter().map(|line| {
            div()
                .whitespace_nowrap()
                .overflow_hidden()
                .child(if line.is_empty() {
                    " ".to_string()
                } else {
                    line.to_string()
                })
        }))
}

fn plural(n: usize, unit: &str) -> String {
    format!("{n} {unit}{}", if n == 1 { "" } else { "s" })
}

fn collapsed_detail(kind: ToolKind, output: &str, failed: bool) -> Option<String> {
    match kind {
        ToolKind::Command => output
            .rsplit('\n')
            .find(|line| !line.trim().is_empty() && !is_exit_code_line(line))
            .map(|line| ui::one_line(line.trim())),
        ToolKind::Read => Some(plural(output.lines().count(), "line")),
        ToolKind::Search => output.lines().next().map(|line| ui::one_line(line.trim())),
        ToolKind::Edit if failed => output.lines().next().map(|line| ui::one_line(line.trim())),
        _ => None,
    }
}

fn is_exit_code_line(line: &str) -> bool {
    let line = line.trim();
    let prefix = b"[exit code:";
    line.as_bytes()
        .get(..prefix.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
        && line.ends_with(']')
}

fn nudge(
    target: &Conversation,
    ix: usize,
    reason: NudgeReason,
    message: &str,
    expanded: bool,
    cx: &mut Context<FlintApp>,
) -> impl IntoElement {
    let p = palette();
    let target = target.clone();
    let title = match reason {
        NudgeReason::Stuck => "loop detected",
        NudgeReason::Verify => "verify before done",
        NudgeReason::Watchdog => "no changes made yet",
        NudgeReason::LeakedCall => "tool call written as text",
    };
    let tip = message.to_string();
    div()
        .id(("nudge", ix))
        .cursor_pointer()
        .rounded(px(5.))
        .hover(|style| style.bg(hsla(0., 0., 1., 0.025)))
        .on_click(cx.listener(move |this, _, _, cx| this.toggle_conversation_item(&target, ix, cx)))
        .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
        .child(bullet(
            ui::icon(IconName::ShieldAlert, 12., p.warning),
            div()
                .min_h(px(26.))
                .flex()
                .items_start()
                .gap(px(10.))
                .child(
                    div()
                        .h(px(26.))
                        .flex()
                        .items_center()
                        .child(ui::label(format!("Guard · {title}"), size::BASE, p.warning))
                        .flex_shrink_0(),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .pt(px(4.))
                        .text_size(px(size::SM))
                        .line_height(px(20.))
                        .text_color(p.text_subtle)
                        .when(!expanded, |text| text.truncate())
                        .child(message.to_string()),
                )
                .child(div().pt(px(6.)).child(chevron(expanded))),
        ))
        .test_support()
}

#[cfg(test)]
#[path = "rows_tests.rs"]
mod tests;
