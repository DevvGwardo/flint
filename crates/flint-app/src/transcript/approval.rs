//! Approvals. The pending request is pinned just above the composer (where
//! the user is looking); the transcript keeps a one-line record of it.

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

fn question(kind: ToolKind) -> &'static str {
    match kind {
        ToolKind::Command => "Run this command?",
        ToolKind::Edit => "Apply this edit?",
        _ => "Allow this tool call?",
    }
}

/// A key cap like `Y`.
fn key(text: &str) -> Div {
    let p = palette();
    div()
        .h(px(22.))
        .min_w(px(22.))
        .px(px(6.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(5.))
        .border_1()
        .border_color(p.border_strong)
        .text_size(px(size::XS))
        .text_color(p.text_muted)
        .child(text.to_string())
}

/// The pinned card above the composer.
pub fn pinned(
    call_id: &str,
    kind: ToolKind,
    summary: &str,
    cx: &mut Context<FlintApp>,
) -> AnyElement {
    let p = palette();
    let answer = |id: &'static str, label: &'static str, decision: ApprovalDecision| {
        let call_id = call_id.to_string();
        Button::new(id)
            .label(label)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.answer_approval(call_id.clone(), decision, cx);
            }))
    };
    let approve = answer("approve", "Approve", ApprovalDecision::Approve).primary();
    let always = answer("always", "Always allow", ApprovalDecision::ApproveAlways).outline();
    let deny = answer("deny", "Deny", ApprovalDecision::Deny);
    div()
        .id("approval-card")
        .w_full()
        .rounded(px(14.))
        .border_1()
        .border_color(p.warning)
        .bg(p.surface)
        .shadow_lg()
        .p(px(16.))
        .flex()
        .flex_col()
        .gap(px(14.))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .child(ui::icon(IconName::ShieldCheck, 17., p.warning))
                .child(ui::label(question(kind), size::BASE, p.text))
                .child(
                    ui::mono(summary.to_string(), size::SM, p.text_muted)
                        .flex_1()
                        .min_w_0()
                        .truncate(),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(div().id("approve-button").child(approve).test_support())
                .child(key("Y"))
                .child(div().w(px(10.)))
                .child(div().id("always-button").child(always).test_support())
                .child(key("A"))
                .child(div().w(px(10.)))
                .child(div().id("deny-button").child(deny).test_support())
                .child(key("N"))
                .child(div().flex_1())
                .child(ui::label("or ⌘⏎ to approve", size::XS, p.text_subtle)),
        )
        .test_support()
        .into_any_element()
}

/// The transcript's record of an approval.
pub fn record(
    kind: ToolKind,
    summary: &str,
    decision: Option<ApprovalDecision>,
) -> impl IntoElement {
    let p = palette();
    let (icon, text, color) = match decision {
        None => (
            IconName::ShieldAlert,
            format!("{} — waiting for you below", question(kind)),
            p.warning,
        ),
        Some(ApprovalDecision::Approve) => {
            (IconName::ShieldCheck, "Approved".to_string(), p.success)
        }
        Some(ApprovalDecision::ApproveAlways) => (
            IconName::ShieldCheck,
            "Approved · always allow".to_string(),
            p.success,
        ),
        Some(ApprovalDecision::Deny) => (IconName::ShieldX, "Denied".to_string(), p.danger),
    };
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .child(
            div()
                .w(px(GUTTER - 8.))
                .flex_shrink_0()
                .child(ui::icon(icon, 13., color)),
        )
        .child(ui::label(text, size::BASE, color))
        .child(
            ui::mono(summary.to_string(), size::SM, p.text_muted)
                .min_w_0()
                .truncate(),
        )
}
