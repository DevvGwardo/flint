//! The approval card: the one bordered, attention-colored row in the stream.

use flint_agent::ApprovalDecision;
use flint_agent::ToolKind;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::*;
use gpui_kit::*;

use super::rows::GUTTER;
use crate::app::FlintApp;
use crate::theme::palette;
use crate::theme::size;
use crate::ui;

pub(super) fn render(
    call_id: &str,
    kind: ToolKind,
    summary: &str,
    decision: Option<ApprovalDecision>,
    cx: &mut Context<FlintApp>,
) -> impl IntoElement {
    let p = palette();
    let question = match kind {
        ToolKind::Command => "Run this command?",
        ToolKind::Edit => "Apply this edit?",
        _ => "Allow this tool call?",
    };
    let answer =
        |id: &'static str, label: &'static str, hint: &'static str, decision: ApprovalDecision| {
            let call_id = call_id.to_string();
            let button =
                Button::new(id)
                    .label(label)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.answer_approval(call_id.clone(), decision, cx);
                    }));
            (button, ui::label(hint, size::SM, p.text_subtle))
        };
    let footer: AnyElement = match decision {
        None => {
            let (approve, approve_hint) =
                answer("approve", "Approve", "⌘⏎", ApprovalDecision::Approve);
            let (always, always_hint) = answer(
                "always",
                "Always allow",
                "⌘⇧⏎",
                ApprovalDecision::ApproveAlways,
            );
            let (deny, deny_hint) = answer("deny", "Deny", "⌘⌫", ApprovalDecision::Deny);
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(approve.primary())
                .child(approve_hint)
                .child(div().w(px(8.)))
                .child(always)
                .child(always_hint)
                .child(div().w(px(8.)))
                .child(deny.ghost())
                .child(deny_hint)
                .into_any_element()
        }
        Some(decision) => {
            let (text, color) = match decision {
                ApprovalDecision::Approve => ("Approved", p.success),
                ApprovalDecision::ApproveAlways => ("Approved · always allow", p.success),
                ApprovalDecision::Deny => ("Denied", p.danger),
            };
            ui::label(text, size::BASE, color).into_any_element()
        }
    };
    div()
        .ml(px(GUTTER))
        .rounded(px(14.))
        .border_1()
        .border_color(if decision.is_none() {
            p.warning
        } else {
            p.border
        })
        .bg(p.surface)
        .p(px(16.))
        .flex()
        .flex_col()
        .gap(px(14.))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .child(ui::icon(IconName::ShieldCheck, 16., p.warning))
                .child(ui::label(question, size::BASE, p.text))
                .child(ui::mono(summary.to_string(), size::SM, p.text_muted)),
        )
        .child(footer)
}
