//! The floating composer: the live status line and pinned approval card
//! above it, the `@`/`/` menus and help, attachment chips, the running-task
//! tray, and the input with its toolbar.

use std::time::Duration;

use flint_agent::AgentKind;
use flint_agent::ApprovalMode;
use flint_agent::ReasoningEffort;
use gpui_kit::assets::IconName;
use gpui_kit::component::input::Paste;
use gpui_kit::component::input::Textarea;
use gpui_kit::component::scroll::{Scrollbar, ScrollbarMode};
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

pub fn render(app: &FlintApp, window: &Window, cx: &mut Context<FlintApp>) -> AnyElement {
    let p = palette();
    let height = f32::from(window.viewport_size().height);
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

    let chips = (!app.attachments.is_empty() || !app.image_attachments.is_empty()).then(|| {
        div()
            .id("composer-attachments")
            .test_support()
            .max_h(px(96.))
            .overflow_y_scroll()
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
            .children(app.image_attachments.iter().enumerate().map(|(n, path)| {
                let path_owned = path.clone();
                div()
                    .h(px(28.))
                    .max_w_full()
                    .pl(px(10.))
                    .pr(px(4.))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .rounded(px(8.))
                    .bg(p.raised)
                    .child(ui::icon(IconName::Paperclip, 13., p.accent))
                    .child(ui::mono(crate::image_attach::name(path), size::XS, p.text).truncate())
                    .child(
                        div()
                            .id(("remove-image", n))
                            .size(px(20.))
                            .flex_shrink_0()
                            .rounded(px(5.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .hover(|s| s.bg(p.border_strong))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.remove_image_attachment(&path_owned, cx);
                                this.composer
                                    .update(cx, |state, cx| state.focus(window, cx));
                            }))
                            .child(ui::icon(IconName::X, 11., p.text_subtle))
                            .test_support(),
                    )
            }))
    });

    let effort_label = match app.effort {
        Some(ReasoningEffort::Low) => "Low",
        Some(ReasoningEffort::Medium) => "Medium",
        Some(ReasoningEffort::High) => "High",
        None => "Default",
    };
    let session = app.session();
    let mode = session.native_approval.unwrap_or(app.approval);
    let (mode_icon, mode_text, mode_color) = match mode {
        ApprovalMode::Auto => (IconName::ChevronsRight, "native · auto-run", p.text_muted),
        ApprovalMode::AskForChanges => (IconName::Hand, "native · ask for changes", p.warning),
    };
    let mode_text = if session.native_allow_all {
        "native · all allowed in this engine"
    } else if session.agent != AgentKind::Flint {
        "Flint default for new native sessions"
    } else {
        mode_text
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
        .id("composer-toolbar")
        .min_h(px(52.))
        .pl(px(10.))
        .pr(px(10.))
        .py(px(8.))
        .flex()
        .items_center()
        .gap(px(6.))
        .child(
            round_button("attach", p.text_muted, hsla(0., 0., 0., 0.))
                .border_1()
                .border_color(p.border_strong)
                .child(ui::icon(IconName::Plus, 17., p.text_muted))
                .on_click(cx.listener(|this, _, _, cx| this.toggle_project_menu(cx)))
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
        .child(div().flex_1())
        .when(app.pending_image_pastes > 0, |bar| {
            bar.child(ui::label("Preparing image...", size::XS, p.text_muted))
        })
        .child(send)
        .test_support();

    let options = div()
        .id("composer-options")
        .max_h(px(90.))
        .overflow_y_scroll()
        .px(px(10.))
        .pb(px(8.))
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(6.))
        .children(crate::option_chips::chips(app, cx))
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
        // When the agent has its own modes, its mode chip replaces auto-run.
        .when(app.session().agent == AgentKind::Flint, |bar| {
            bar.child(
                chip("approval-hint")
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_approval(cx)))
                    .tooltip(|window, cx| {
                        gpui_kit::component::tooltip::Tooltip::new(
                        "Changes apply to new native engines only; running engines keep their mode."
                    ).build(window, cx)
                    })
                    .child(ui::icon(mode_icon, 14., mode_color))
                    .child(ui::label(mode_text, size::SM, mode_color))
                    .child(ui::label("shift+tab", size::SM, p.text_subtle)),
            )
        })
        .test_support();

    let card = div()
        .w_full()
        .max_h(px((height * 0.55).max(140.)))
        .flex()
        .flex_col()
        .rounded(px(20.))
        .border_1()
        .border_color(p.border_strong)
        .bg(p.surface)
        .shadow_lg()
        .overflow_hidden()
        .child(
            div()
                .id("composer-body")
                .min_h_0()
                .overflow_y_scroll()
                .children(tray)
                .children(chips)
                .key_context("Composer")
                .capture_action(cx.listener(|this, _: &Paste, _, cx| {
                    if this.paste_composer_image(cx) {
                        cx.stop_propagation();
                    } else {
                        cx.propagate();
                    }
                }))
                .child(
                    div().px(px(12.)).pt(px(12.)).min_h(px(44.)).child(
                        Textarea::new(&app.composer)
                            .appearance(false)
                            .bordered(false)
                            .text_size(px(size::MD)),
                    ),
                ),
        )
        .child(toolbar);
    let card = card.child(options);

    // Menus float over the page instead of taking room in the column, so
    // opening one never pushes the welcome screen's hero into the header.
    // On the welcome screen they open below the composer (over the
    // suggestions), unless the window is short; then they open above it.
    let welcome = app.session().view.items.is_empty();
    let below = welcome && height >= 700.;
    let popover_height = (height * if below { 0.42 } else { 0.32 }).max(100.);
    let popover = crate::menus::render(app, popover_height, window, cx)
        .or_else(|| crate::project_menu::render(app, cx))
        .map(|menu| {
            deferred(
                div()
                    .id("composer-popover")
                    .absolute()
                    .left_0()
                    .w_full()
                    .when(below, |d| d.top_full().mt(px(8.)))
                    .when(!below, |d| d.bottom_full().mb(px(8.)))
                    .child(
                        div()
                            .id("composer-popover-content")
                            .max_h(px(popover_height))
                            .overflow_y_scroll()
                            .track_scroll(&app.popover_scroll)
                            .child(menu)
                            .test_support(),
                    )
                    .when(app.option_menu.is_none(), |popover| {
                        popover.child(
                            Scrollbar::vertical(&app.popover_scroll).mode(ScrollbarMode::Always),
                        )
                    })
                    .test_support(),
            )
            .with_priority(1)
        });

    let pinned = app
        .session()
        .view
        .pending_approval()
        .map(|(call_id, kind, summary)| {
            crate::transcript::pinned_approval(app, &call_id, kind, &summary, cx)
        });

    div()
        .w_full()
        .max_w(px(COLUMN_WIDTH + 40.))
        .flex()
        .flex_col()
        .gap(px(10.))
        .when(app.session().agent_starting(), |col| {
            col.child(starting_line(app))
        })
        .when(
            running && pinned.is_none() && !app.session().agent_starting(),
            |col| col.child(status_line(app)),
        )
        .children(pinned)
        .child(div().relative().w_full().child(card).children(popover))
        .into_any_element()
}

/// "Starting Claude Code…" while an ACP adapter opens its session.
fn starting_line(app: &FlintApp) -> impl IntoElement {
    let p = palette();
    div()
        .id("agent-starting")
        .px(px(6.))
        .flex()
        .items_center()
        .gap(px(9.))
        .child(ui::spinner(app.now(), 13., p.accent))
        .child(ui::label(
            format!("Starting {}…", app.session().agent.label()),
            size::BASE - 1.,
            p.text,
        ))
        .child(ui::label(
            "the first start can take up to a minute",
            size::SM,
            p.text_subtle,
        ))
        .test_support()
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
        .max_w_full()
        .min_w_0()
        .px(px(11.))
        .rounded(px(17.))
        .flex()
        .items_center()
        .overflow_hidden()
        .gap(px(7.))
        .cursor_pointer()
        .hover(|style| style.bg(hsla(0., 0., 1., 0.05)))
}
