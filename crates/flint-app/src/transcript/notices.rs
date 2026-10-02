//! Inline notices: harness nudges, tool repairs, approvals, errors, and the
//! end-of-turn summary.

use std::time::Duration;

use flint_agent::ApprovalDecision;
use flint_agent::NudgeReason;
use flint_agent::ToolKind;
use flint_agent::TurnEndReason;
use flint_agent::Usage;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::*;
use gpui_kit::component::*;
use gpui_kit::*;

use crate::app::FlintApp;
use crate::theme::MONO_FONT;
use crate::theme::palette;
use crate::theme::size;
use crate::ui;

pub fn nudge(reason: NudgeReason, message: &str) -> impl IntoElement {
    let p = palette();
    let title = match reason {
        NudgeReason::Stuck => "loop detected",
        NudgeReason::Verify => "verify before done",
        NudgeReason::Watchdog => "no changes made yet",
        NudgeReason::LeakedCall => "tool call written as text",
    };
    div()
        .w_full()
        .flex()
        .items_center()
        .gap(px(8.))
        .child(
            div()
                .flex_shrink_0()
                .flex()
                .items_center()
                .gap(px(5.))
                .px(px(7.))
                .h(px(20.))
                .rounded(px(5.))
                .bg(p.warning_soft)
                .child(ui::icon(IconName::ShieldAlert, 11., p.warning))
                .child(
                    div()
                        .text_size(px(size::XS))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(p.warning)
                        .child(format!("Guard · {title}")),
                ),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(px(size::SM))
                .text_color(p.text_subtle)
                .child(message.to_string()),
        )
}

pub fn repair(tool: &str, detail: &str) -> impl IntoElement {
    let p = palette();
    div()
        .flex()
        .items_center()
        .gap(px(6.))
        .child(ui::icon(IconName::Wrench, 11., p.text_subtle))
        .child(ui::label("Repaired tool call", size::SM, p.text_subtle))
        .child(ui::mono(tool.to_string(), size::XS, p.text_muted))
        .child(ui::label(format!("· {detail}"), size::SM, p.text_subtle))
}

pub fn approval(
    call_id: &str,
    kind: ToolKind,
    summary: &str,
    decision: Option<ApprovalDecision>,
    cx: &mut Context<FlintApp>,
) -> impl IntoElement {
    let p = palette();
    let what = match kind {
        ToolKind::Command => "Run this command?",
        ToolKind::Edit => "Apply this edit?",
        _ => "Allow this tool call?",
    };
    let answer = |id: &'static str, label: &'static str, decision: ApprovalDecision| {
        let call_id = call_id.to_string();
        Button::new(id)
            .small()
            .label(label)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.answer_approval(call_id.clone(), decision, cx);
            }))
    };
    let footer: AnyElement = match decision {
        None => div()
            .flex()
            .gap(px(6.))
            .child(answer("approve", "Approve", ApprovalDecision::Approve).primary())
            .child(answer(
                "always",
                "Always allow",
                ApprovalDecision::ApproveAlways,
            ))
            .child(answer("deny", "Deny", ApprovalDecision::Deny).ghost())
            .into_any_element(),
        Some(decision) => {
            let (text, color) = match decision {
                ApprovalDecision::Approve => ("Approved", p.success),
                ApprovalDecision::ApproveAlways => ("Approved · always allow", p.success),
                ApprovalDecision::Deny => ("Denied", p.danger),
            };
            ui::label(text, size::SM, color).into_any_element()
        }
    };
    div()
        .w_full()
        .rounded(px(8.))
        .border_1()
        .border_color(if decision.is_none() {
            p.warning
        } else {
            p.border
        })
        .bg(p.surface)
        .px(px(12.))
        .py(px(10.))
        .flex()
        .flex_col()
        .gap(px(8.))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(ui::icon(IconName::ShieldCheck, 13., p.warning))
                .child(
                    div()
                        .text_size(px(size::BASE))
                        .font_weight(FontWeight::MEDIUM)
                        .child(what),
                ),
        )
        .child(
            div()
                .px(px(10.))
                .py(px(7.))
                .rounded(px(6.))
                .bg(p.bg)
                .border_1()
                .border_color(p.border)
                .font_family(MONO_FONT)
                .font_features(FontFeatures::disable_ligatures())
                .text_size(px(size::SM))
                .child(summary.to_string()),
        )
        .child(footer)
}

pub fn error(message: &str) -> impl IntoElement {
    let p = palette();
    div()
        .w_full()
        .rounded(px(8.))
        .border_1()
        .border_color(p.danger_soft)
        .bg(p.danger_soft)
        .px(px(12.))
        .py(px(9.))
        .flex()
        .gap(px(8.))
        .child(
            div()
                .mt(px(2.))
                .child(ui::icon(IconName::CircleAlert, 13., p.danger)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_size(px(size::BASE))
                .text_color(p.text)
                .child(message.to_string()),
        )
}

pub fn turn_summary(
    reason: &TurnEndReason,
    duration: Duration,
    steps: u32,
    usage: &Usage,
) -> impl IntoElement {
    let p = palette();
    let (icon, text, color) = match reason {
        TurnEndReason::Completed => (IconName::CircleCheck, "Done".to_string(), p.success),
        TurnEndReason::Interrupted => (IconName::CircleStop, "Stopped".to_string(), p.warning),
        TurnEndReason::StepLimit => (
            IconName::CircleAlert,
            "Hit the step limit".to_string(),
            p.warning,
        ),
        TurnEndReason::Failed(why) => (IconName::CircleX, format!("Failed: {why}"), p.danger),
    };
    let mut details = vec![ui::duration(Duration::from_secs(duration.as_secs().max(1)))];
    if steps > 0 {
        details.push(format!("{steps} steps"));
    }
    if usage.input_tokens > 0 {
        details.push(format!(
            "{} in · {} out",
            ui::tokens(usage.input_tokens),
            ui::tokens(usage.output_tokens)
        ));
    }
    let rule = || div().flex_1().h(px(1.)).bg(p.border);
    div()
        .w_full()
        .flex()
        .items_center()
        .gap(px(10.))
        .child(rule())
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .child(ui::icon(icon, 12., color))
                .child(ui::label(text, size::SM, color))
                .child(ui::label(
                    format!("· {}", details.join(" · ")),
                    size::SM,
                    p.text_subtle,
                )),
        )
        .child(rule())
}
