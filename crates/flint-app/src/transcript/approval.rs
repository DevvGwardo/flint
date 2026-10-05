//! Approvals. The pending request is pinned just above the composer (where
//! the user is looking); the transcript keeps a one-line record of it.

use flint_agent::ApprovalDecision;
use flint_agent::ToolKind;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::*;
use gpui_kit::prelude::FluentBuilder as _;
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
    app: &FlintApp,
    call_id: &str,
    kind: ToolKind,
    summary: &str,
    cx: &mut Context<FlintApp>,
) -> AnyElement {
    let p = palette();
    let native = app.session().agent == flint_agent::AgentKind::Flint;
    let preview = app
        .session()
        .view
        .approval_preview(call_id, &app.session().workspace, native);
    let no_details = preview.fields.is_empty();
    let expanded = app.approval_preview.as_deref() == Some(call_id);
    let confirming = app.approval_confirm.as_deref() == Some(call_id);
    let count = app.session().view.pending_approvals;
    let answer = |id: &'static str, label: &'static str, decision: ApprovalDecision| {
        let call_id = call_id.to_string();
        Button::new(id)
            .label(label)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.answer_approval(call_id.clone(), decision, cx);
            }))
    };
    let approve = answer("approve", "Approve once", ApprovalDecision::Approve).primary();
    let always = answer(
        "always",
        if confirming {
            "Confirm broad approval"
        } else {
            "Approve broadly…"
        },
        ApprovalDecision::ApproveAlways,
    )
    .outline();
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
                .id("approval-header")
                .h(px(26.))
                .overflow_hidden()
                .flex()
                .items_center()
                .gap(px(10.))
                .child(ui::icon(IconName::ShieldCheck, 17., p.warning))
                .child(ui::label(question(kind), size::BASE, p.text))
                .child(
                    ui::mono(ui::one_line(summary), size::SM, p.text_muted)
                        .id("approval-summary")
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .test_support(),
                ),
        )
        .child(ui::label(
            if native {
                "Native engine · asks before commands and edits in this session"
            } else {
                "ACP agent · permissions come from the agent or ACP adapter, not the native engine"
            },
            size::SM,
            p.text_muted,
        ))
        .when_some(preview.agent, |card, agent| {
            card.child(ui::label(format!("Requesting agent: {agent}"), size::SM, p.text_muted))
        })
        .when(count > 1, |card| {
            card.child(ui::label(
                format!("{count} pending requests in this session"),
                size::SM,
                p.warning,
            ))
        })
        .child(
            div()
                .id("approval-preview-toggle")
                .role(gpui_kit::Role::Button)
                .aria_label(if expanded { "Hide request details" } else { "Inspect request details" })
                .tab_index(0)
                .focus_visible(|style| style.border_1().border_color(p.accent))
                .cursor_pointer()
                .text_color(p.text)
                .on_click(cx.listener({
                    let call_id = call_id.to_string();
                    move |this, _, _, cx| {
                        this.approval_preview = if this.approval_preview.as_deref() == Some(&call_id) {
                            None
                        } else {
                            Some(call_id.clone())
                        };
                        cx.notify();
                    }
                }))
                .child(if expanded { "Hide request details" } else { "Inspect request details" })
                .test_support(),
        )
        .when(expanded, |card| {
            card.child(
                div()
                    .id("approval-preview")
                    .max_h(px(210.))
                    .overflow_y_scroll()
                    .overflow_x_scroll()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .children(preview.fields.into_iter().map(|(label, value)| {
                        div()
                            .flex()
                            .flex_col()
                            .child(ui::label(label, size::SM, p.text_muted))
                            .children(value.lines().map(|line| {
                                ui::mono(line.to_string(), size::SM, p.text)
                            }))
                    }))
                    .when(no_details, |details| {
                        details.child(ui::label(
                            "The agent did not provide a full command or edit preview.",
                            size::SM,
                            p.text_muted,
                        ))
                    })
                    .test_support(),
            )
        })
        .when(confirming, |card| {
            card.child(ui::label(
                if native {
                    "This approves ALL pending parent and subagent requests and skips future approvals in this native engine, including later turns. It does not change other sessions or your saved setting."
                } else {
                    "This asks the ACP agent to allow this request broadly and makes Flint auto-approve later requests in this ACP session. The agent may apply its own permission rules. It does not change other sessions."
                },
                size::SM,
                p.warning,
            ))
        })
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
            "Approved broadly in this session".to_string(),
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
            ui::mono(ui::one_line(summary), size::SM, p.text_muted)
                .min_w_0()
                .truncate(),
        )
}
