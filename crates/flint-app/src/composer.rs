//! The floating composer: the live status line and pinned approval card
//! above it, the `@`/`/` menus and help, attachment chips, the running-task
//! tray, and the input with its toolbar.

use std::time::Duration;

use flint_agent::AgentKind;
use flint_agent::ApprovalMode;
use flint_agent::ReasoningEffort;
use gpui_kit::assets::IconName;
use gpui_kit::component::input::Textarea;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::app::FlintApp;
use crate::theme::MONO_FONT;
use crate::theme::palette;
use crate::theme::size;
use crate::transcript::COLUMN_WIDTH;
use crate::turns::Activity;
use crate::ui;

const THINKING_VERBS: &[&str] = &["Thinking", "Reasoning", "Pondering", "Mulling"];
const WORKING_VERBS: &[&str] = &["Working", "Forging", "Tempering", "Kindling"];

pub fn render(app: &FlintApp, cx: &mut Context<FlintApp>) -> AnyElement {
    let p = palette();
    let view = &app.session().view;
    let running = view.running;
    let now = app.now();
    let tasks = view.running_commands();

    let tray = (!tasks.is_empty()).then(|| {
        let count = tasks.len();
        div()
            .px(px(18.))
            .pt(px(14.))
            .pb(px(12.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .border_b_1()
            .border_color(p.border)
            .child(ui::label(
                format!("{count} task{} running", if count == 1 { "" } else { "s" }),
                size::XS,
                p.text_subtle,
            ))
            .children(tasks.iter().map(|call| {
                let last = call
                    .output
                    .rsplit('\n')
                    .find(|line| !line.trim().is_empty())
                    .unwrap_or("")
                    .trim()
                    .to_string();
                let elapsed = now.saturating_sub(call.started);
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(ui::spinner(now, 14., p.accent))
                    .child(ui::mono(call.summary.clone(), size::SM, p.text).flex_shrink_0())
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .font_family(MONO_FONT)
                            .font_features(FontFeatures::disable_ligatures())
                            .text_size(px(size::XS))
                            .text_color(p.text_subtle)
                            .child(last),
                    )
                    .child(ui::label(
                        ui::duration(Duration::from_secs(elapsed.as_secs())),
                        size::XS,
                        p.text_subtle,
                    ))
            }))
    });

    let chips = (!app.attachments.is_empty()).then(|| {
        div()
            .px(px(14.))
            .pt(px(12.))
            .flex()
            .flex_wrap()
            .gap(px(6.))
            .children(app.attachments.iter().enumerate().map(|(n, path)| {
                let path_owned = path.clone();
                div()
                    .h(px(28.))
                    .pl(px(10.))
                    .pr(px(4.))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .rounded(px(8.))
                    .bg(p.raised)
                    .child(ui::icon(IconName::FileText, 13., p.text_muted))
                    .child(ui::mono(path.clone(), size::XS, p.text))
                    .child(
                        div()
                            .id(("remove-attachment", n))
                            .size(px(20.))
                            .rounded(px(5.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .hover(|s| s.bg(p.border_strong))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.remove_attachment(&path_owned, cx)
                            }))
                            .child(ui::icon(IconName::X, 11., p.text_subtle)),
                    )
            }))
    });

    let effort_label = match app.effort {
        Some(ReasoningEffort::Low) => "Low",
        Some(ReasoningEffort::Medium) => "Medium",
        Some(ReasoningEffort::High) => "High",
        None => "Default",
    };
    let (mode_icon, mode_text, mode_color) = match app.approval {
        ApprovalMode::Auto => (IconName::ChevronsRight, "auto-run on", p.text_muted),
        ApprovalMode::AskForChanges => (IconName::Hand, "ask before changes", p.warning),
    };

    let send: AnyElement = if running {
        round_button("stop", p.text, p.raised)
            .child(div().size(px(11.)).rounded(px(2.5)).bg(p.text))
            .on_click(cx.listener(|this, _, _, cx| this.interrupt(cx)))
            .test_support()
            .into_any_element()
    } else {
        round_button("send", p.on_accent, p.accent)
            .child(ui::icon(IconName::ArrowUp, 18., p.on_accent))
            .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx)))
            .test_support()
            .into_any_element()
    };

    let toolbar = div()
        .h(px(52.))
        .pl(px(10.))
        .pr(px(10.))
        .flex()
        .items_center()
        .gap(px(6.))
        .child(
            round_button("attach", p.text_muted, hsla(0., 0., 0., 0.))
                .border_1()
                .border_color(p.border_strong)
                .child(ui::icon(IconName::Plus, 17., p.text_muted))
                .on_click(cx.listener(|this, _, window, cx| this.open_mention_picker(window, cx)))
                .test_support(),
        )
        .child(
            chip("model-chip")
                .on_click(cx.listener(|this, _, _, cx| this.toggle_agent_menu(cx)))
                .child(ui::label(
                    app.agent_label(app.session().agent),
                    size::BASE - 1.,
                    p.text,
                ))
                .child(ui::icon(IconName::ChevronDown, 12., p.text_subtle))
                .test_support(),
        )
        // ACP agents choose their own reasoning depth; the chip is flint's.
        .when(
            app.effort_supported && app.session().agent == AgentKind::Flint,
            |bar| {
                bar.child(
                    chip("effort-chip")
                        .on_click(cx.listener(|this, _, _, cx| this.cycle_effort(cx)))
                        .child(ui::icon(IconName::Brain, 14., p.text_subtle))
                        .child(ui::label(effort_label, size::BASE - 1., p.text_subtle))
                        .test_support(),
                )
            },
        )
        .child(div().flex_1())
        .child(
            chip("approval-hint")
                .on_click(cx.listener(|this, _, _, cx| this.toggle_approval(cx)))
                .child(ui::icon(mode_icon, 14., mode_color))
                .child(ui::label(mode_text, size::SM, mode_color))
                .child(ui::label("shift+tab", size::SM, p.text_subtle)),
        )
        .child(div().w(px(4.)))
        .child(send);

    let card = div()
        .w_full()
        .rounded(px(20.))
        .border_1()
        .border_color(p.border_strong)
        .bg(p.surface)
        .shadow_lg()
        .overflow_hidden()
        .children(tray)
        .children(chips)
        .child(
            div().px(px(12.)).pt(px(12.)).min_h(px(44.)).child(
                Textarea::new(&app.composer)
                    .appearance(false)
                    .bordered(false)
                    .text_size(px(size::MD)),
            ),
        )
        .child(toolbar);

    let pinned = app
        .session()
        .view
        .pending_approval()
        .map(|(call_id, kind, summary)| {
            crate::transcript::pinned_approval(&call_id, kind, &summary, cx)
        });

    div()
        .w_full()
        .max_w(px(COLUMN_WIDTH + 40.))
        .flex()
        .flex_col()
        .gap(px(10.))
        .when(running && pinned.is_none(), |col| {
            col.child(status_line(app))
        })
        .children(pinned)
        .children(crate::menus::render(app, cx))
        .child(card)
        .into_any_element()
}

/// `✻ Forging… (29s · ↓ 383 tokens) · esc to interrupt`
fn status_line(app: &FlintApp) -> impl IntoElement {
    let p = palette();
    let view = &app.session().view;
    let now = app.now();
    let rotate = (now.as_millis() / 2400) as usize;
    let verb = match view.activity() {
        Activity::Thinking => THINKING_VERBS[rotate % THINKING_VERBS.len()].to_string(),
        Activity::Writing => "Writing".to_string(),
        Activity::Running(_) => "Running".to_string(),
        Activity::Editing(_) => "Editing".to_string(),
        Activity::Reading(_) => "Reading".to_string(),
        Activity::Searching => "Searching".to_string(),
        Activity::Working => WORKING_VERBS[rotate % WORKING_VERBS.len()].to_string(),
    };
    let elapsed = view
        .elapsed(now)
        .map(|d| ui::duration(Duration::from_secs(d.as_secs())))
        .unwrap_or_default();
    let tokens = view.usage.output_tokens + view.usage.reasoning_tokens;
    div()
        .id("status-line")
        .px(px(18.))
        .flex()
        .items_center()
        .gap(px(8.))
        .text_size(px(size::BASE))
        .child(ui::work_glyph(now, 15., p.accent))
        .child(
            div()
                .text_color(p.accent)
                .font_weight(FontWeight::MEDIUM)
                .child(format!("{verb}…")),
        )
        .child(ui::label(
            format!("({elapsed} · ↓ {} tokens)", ui::tokens(tokens)),
            size::BASE,
            p.text_subtle,
        ))
        .child(ui::label("· esc to interrupt", size::BASE, p.text_subtle))
        .test_support()
}

fn round_button(id: &'static str, fg: Hsla, bg: Hsla) -> Stateful<Div> {
    div()
        .id(id)
        .size(px(36.))
        .flex_shrink_0()
        .rounded_full()
        .bg(bg)
        .text_color(fg)
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .hover(|style| style.opacity(0.85))
}

fn chip(id: &'static str) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(34.))
        .px(px(11.))
        .rounded(px(17.))
        .flex()
        .items_center()
        .gap(px(7.))
        .cursor_pointer()
        .hover(|style| style.bg(hsla(0., 0., 1., 0.05)))
}
