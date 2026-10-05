//! The floating composer: the live status line and pinned approval card
//! above it, the `@`/`/` menus and help, attachment chips, the running-task
//! tray, and the input with its toolbar.

use std::time::Duration;

use flint_agent::AgentKind;
use flint_agent::ApprovalMode;
use flint_agent::ReasoningEffort;
use gpui_kit::assets::IconName;
use gpui_kit::base::Disableable as _;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
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
    let panel_height = app.popover_room.area_height().unwrap_or(height);
    let card_height = (panel_height * 0.55).max(140.);
    let view = &app.session().view;
    let running = view.running;
    let busy = app.prompt_busy(app.active);
    let queued = !app.session().prompt_queue.items.is_empty();
    let queueing = busy || queued || app.session().prompt_queue.paused;
    let has_prompt = crate::app_input::composer_has_text(app.composer.read(cx))
        || !app.image_attachments.is_empty();
    let compact = panel_height < 600.;
    let panel_width = app
        .popover_room
        .area
        .get()
        .map_or(f32::from(window.viewport_size().width), |area| {
            f32::from(area.size.width)
        });
    let narrow = panel_width < 480.;
    let now = app.now();
    let (task_count, tasks) = view.running_command_projection();
    crate::automation::begin_composer_capture(running, task_count);

    let tray = (task_count > 0).then(|| {
        let count = task_count;
        div()
            .id("composer-task-tray")
            .relative()
            .max_h(px(if compact { 88. } else { 144. }))
            .overflow_y_scroll()
            .mx(px(10.))
            .mt(px(10.))
            .px(px(12.))
            .py(px(10.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .rounded(px(10.))
            .bg(p.raised)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(ui::icon(IconName::Terminal, 13., p.text_muted))
                    .child(ui::label("Live tasks", size::XS, p.text_muted))
                    .child(ui::pill(count.to_string(), p.accent, p.accent_soft)),
            )
            .children(tasks.map(|call| {
                let last = call
                    .output
                    .rsplit('\n')
                    .find(|line| !line.trim().is_empty())
                    .unwrap_or("")
                    .trim();
                let last: String = last.chars().take(160).collect();
                let elapsed = now.saturating_sub(call.started);
                div()
                    .id(SharedString::from(format!(
                        "running-command-{}",
                        call.call_id
                    )))
                    .h(px(26.))
                    .min_w_0()
                    .overflow_hidden()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(ui::spinner(now, 14., p.accent))
                    .child(
                        ui::mono(ui::one_line(&call.summary), size::SM, p.text)
                            .id(SharedString::from(format!(
                                "running-command-summary-{}",
                                call.call_id
                            )))
                            .flex_shrink_0()
                            .min_w_0()
                            .max_w(relative(0.55))
                            .truncate()
                            .test_support(),
                    )
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
                    .child(
                        ui::label(
                            ui::duration(Duration::from_secs(elapsed.as_secs())),
                            size::XS,
                            p.text_subtle,
                        )
                        .flex_shrink_0(),
                    )
                    .test_support()
            }))
            .when(crate::automation::capture_enabled(), |tray| {
                tray.child(crate::automation::control_probe("task_tray"))
            })
            .test_support()
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
                    .max_w_full()
                    .min_w_0()
                    .h(px(28.))
                    .pl(px(10.))
                    .pr(px(4.))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(p.border)
                    .bg(p.raised)
                    .child(ui::icon(IconName::FileText, 13., p.text_muted).flex_shrink_0())
                    .child(
                        ui::mono(path.clone(), size::XS, p.text)
                            .min_w_0()
                            .truncate(),
                    )
                    .child(
                        div()
                            .id(("remove-attachment", n))
                            .role(gpui_kit::Role::Button)
                            .aria_label(format!("Remove attachment {path}"))
                            .tab_index(0)
                            .size(px(20.))
                            .flex_shrink_0()
                            .rounded(px(5.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .hover(|s| s.bg(p.border_strong))
                            .focus_visible(|s| {
                                s.bg(p.border_strong).border_1().border_color(p.accent)
                            })
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
                    .border_1()
                    .border_color(p.border)
                    .bg(p.raised)
                    .child(ui::icon(IconName::Paperclip, 13., p.accent))
                    .child(ui::mono(crate::image_attach::name(path), size::XS, p.text).truncate())
                    .child(
                        div()
                            .id(("remove-image", n))
                            .role(gpui_kit::Role::Button)
                            .aria_label(format!("Remove image {}", crate::image_attach::name(path)))
                            .tab_index(0)
                            .size(px(20.))
                            .flex_shrink_0()
                            .rounded(px(5.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .hover(|s| s.bg(p.border_strong))
                            .focus_visible(|s| {
                                s.bg(p.border_strong).border_1().border_color(p.accent)
                            })
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
        "native · approving similar calls"
    } else if session.agent != AgentKind::Flint {
        "Flint default for new native sessions"
    } else {
        mode_text
    };

    let send_label = if app.session().stopping {
        "Stopping"
    } else if app.pending_image_pastes > 0 {
        "Preparing"
    } else if queueing {
        "Queue"
    } else {
        "Send"
    };
    let send: AnyElement = Button::new("send")
        .primary()
        .h(px(36.))
        .rounded(px(10.))
        .label(send_label)
        .accessibility_label(if queueing {
            "Queue follow-up"
        } else {
            "Send message"
        })
        .tooltip(if queueing {
            "Enter to queue a follow-up"
        } else {
            "Enter to send"
        })
        .icon(IconName::ArrowUp)
        .disabled(
            !has_prompt
                || app.session().stopping
                || app.pending_image_pastes > 0
                || app.permission_choice_open,
        )
        .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx)))
        .into_any_element();

    let toolbar = div()
        .id("composer-toolbar")
        .min_h(px(52.))
        .flex_shrink_0()
        .pl(px(10.))
        .pr(px(10.))
        .py(px(8.))
        .flex()
        .items_center()
        .gap(px(8.))
        .border_t_1()
        .border_color(p.border)
        .child(
            round_button("attach", p.text_muted, hsla(0., 0., 0., 0.))
                .border_1()
                .border_color(p.border_strong)
                .tooltip(|window, cx| {
                    gpui_kit::component::tooltip::Tooltip::new(
                        "Attach files or images, or choose a workspace",
                    )
                    .build(window, cx)
                })
                .child(ui::icon(IconName::Plus, 17., p.text_muted))
                .on_click(cx.listener(|this, _, _, cx| this.toggle_project_menu(cx)))
                .test_support(),
        )
        .child(
            chip("model-chip")
                .max_w(px(if narrow { 132. } else { 240. }))
                .on_click(cx.listener(|this, _, _, cx| this.toggle_agent_menu(cx)))
                .child(ui::icon(IconName::Sparkles, 14., p.accent).flex_shrink_0())
                .child(
                    ui::label(
                        app.agent_label(app.session().agent),
                        size::BASE - 1.,
                        p.text,
                    )
                    .min_w_0()
                    .truncate(),
                )
                .child(ui::icon(IconName::ChevronDown, 12., p.text_subtle))
                .test_support(),
        )
        .child(div().flex_1())
        .when(
            queued || app.session().prompt_queue.error.is_some(),
            |bar| {
                bar.child(
                    Button::new("queue-toggle")
                        .small()
                        .icon(IconName::ListTree)
                        .label(app.session().prompt_queue.items.len().to_string())
                        .accessibility_label(format!(
                            "View {} pending prompts",
                            app.session().prompt_queue.items.len()
                        ))
                        .tooltip("View pending prompts")
                        .on_click(cx.listener(|app, _, _, cx| app.toggle_prompt_queue(cx))),
                )
            },
        )
        .when(app.pending_image_pastes > 0, |bar| {
            bar.child(ui::label("Preparing image...", size::XS, p.text_muted))
        })
        .when(running && !narrow, |bar| {
            bar.child(
                Button::new("steer-draft")
                    .label("Steer")
                    .tooltip(if app.session().agent == AgentKind::Flint {
                        "Apply this draft at the next model step. Your current tools keep running."
                    } else {
                        "Stop this turn and send your draft next."
                    })
                    .disabled(
                        !has_prompt
                            || app.session().stopping
                            || app.session().steering_pending.is_some()
                            || app.session().prompt_queue.steer_after_turn.is_some()
                            || app.pending_image_pastes > 0,
                    )
                    .on_click(cx.listener(|app, _, window, cx| app.steer_draft(window, cx))),
            )
        })
        .when(running, |bar| {
            bar.child(
                round_button("stop", p.danger, p.raised)
                    .relative()
                    .tooltip(|window, cx| {
                        gpui_kit::component::tooltip::Tooltip::new(
                            "Stop this turn and pause queued prompts (Esc)",
                        )
                        .build(window, cx)
                    })
                    .child(ui::icon(IconName::Square, 12., p.danger))
                    .on_click(cx.listener(|this, _, _, cx| this.interrupt(cx)))
                    .when(crate::automation::capture_enabled(), |stop| {
                        stop.child(crate::automation::control_probe("stop"))
                    })
                    .test_support(),
            )
        })
        .child(
            div()
                .id("send-button")
                .relative()
                .child(send)
                .when(crate::automation::capture_enabled(), |button| {
                    button.child(crate::automation::control_probe("send"))
                })
                .test_support(),
        )
        .test_support();

    let options = div()
        .id("composer-options")
        // Leave a readable input and the send/stop row in compact tiles.
        // Wrapped choices scroll instead of crushing the textarea.
        .max_h(px((card_height - 100.).clamp(34., 90.)))
        .flex_shrink_0()
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
                    .test_support()
                    .aria_label(format!("Native permission mode: {mode_text}"))
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

    // The chat panel can be one pane of a split, so cap the card by the
    // panel's height once it has been measured, not the window's.
    let card = div()
        .w_full()
        .max_h(px(card_height))
        .flex()
        .flex_col()
        .rounded(px(16.))
        .border_1()
        .border_color(if app.composer.focus_handle(cx).is_focused(window) {
            p.accent.opacity(0.55)
        } else {
            p.border_strong
        })
        .bg(p.surface)
        .shadow_sm()
        .overflow_hidden()
        .child(
            div()
                .id("composer-body")
                .test_support()
                .min_h(px(44.))
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
                    div()
                        .px(px(14.))
                        .pt(px(12.))
                        .min_h(px(44.))
                        .when(!compact, |body| {
                            body.child(
                                div()
                                    .id("composer-heading")
                                    .mb(px(6.))
                                    .flex()
                                    .items_center()
                                    .gap(px(6.))
                                    .child(ui::icon(IconName::MessageSquare, 13., p.text_muted))
                                    .child(ui::label(
                                        if queueing {
                                            "Queue a follow-up"
                                        } else {
                                            "New message"
                                        },
                                        size::XS,
                                        p.text_muted,
                                    ))
                                    .child(div().flex_1())
                                    .test_support(),
                            )
                        })
                        .child(
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
    // On the welcome screen they prefer to open below the composer (over the
    // suggestions), elsewhere above it. Either way they take the side and
    // height the chat panel has room for (see `popover::place`): they are
    // deferred, so nothing clips them if they are taller than that.
    let welcome = app.session().view.items.is_empty();
    let placement = app.popover_room.placement(welcome, height);
    let below = placement.below;
    let popover_height = placement.max_height;
    // List menus scroll their own rows under a fixed header; only the help
    // card scrolls as a whole.
    let list_menu = app.mention.is_some()
        || app.option_menu.is_some()
        || app.agent_menu
        || app.slash.is_some()
        || app.project_menu.is_some();
    let popover = crate::menus::render(app, popover_height, window, cx)
        .or_else(|| crate::project_menu::render(app, popover_height, cx))
        .or_else(|| {
            app.queue_popover
                .then(|| crate::queue_view::render(app, popover_height, cx))
                .flatten()
        })
        .map(|menu| {
            deferred(
                div()
                    .id("composer-popover")
                    .absolute()
                    .left_0()
                    .w_full()
                    .when(below, |d| d.top_full().mt(px(crate::popover::GAP)))
                    .when(!below, |d| d.bottom_full().mb(px(crate::popover::GAP)))
                    .child(
                        div()
                            .id("composer-popover-content")
                            .max_h(px(popover_height))
                            .overflow_y_scroll()
                            .track_scroll(&app.popover_scroll)
                            .child(menu)
                            .test_support(),
                    )
                    .when(!list_menu, |popover| {
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
        .pending_approval_ref()
        .map(|(call_id, kind, summary)| {
            crate::transcript::pinned_approval(app, call_id, kind, summary, cx)
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
        .when(!compact && queued && !app.queue_popover, |col| {
            col.children(crate::queue_view::render(
                app,
                (panel_height * 0.27).clamp(180., 240.),
                cx,
            ))
        })
        .child(
            div()
                .relative()
                .w_full()
                .child(card)
                .child(crate::popover::probe(&app.popover_room, |room| {
                    &room.anchor
                }))
                .children(popover),
        )
        .when(panel_height >= 380., |col| {
            col.child(
                div()
                    .id("composer-shortcuts")
                    .px(px(6.))
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(ui::label("@ files", size::XS, p.text_subtle))
                    .child(ui::label("·", size::XS, p.text_subtle))
                    .child(ui::label("/ actions", size::XS, p.text_subtle))
                    .child(div().flex_1())
                    .when(!narrow, |row| {
                        row.child(key_hint("↵"))
                            .child(ui::label(
                                if queueing { "queue" } else { "send" },
                                size::XS,
                                p.text_subtle,
                            ))
                            .child(key_hint("⇧ ↵"))
                            .child(ui::label("new line", size::XS, p.text_subtle))
                    })
                    .test_support(),
            )
        })
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
    let tokens = view.output_tokens();
    div()
        .id("status-line")
        .px(px(6.))
        .min_w_0()
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
        .child(
            ui::label(
                format!("({elapsed} · ↓ {} tokens)", ui::tokens(tokens)),
                size::XS,
                p.text_subtle,
            )
            .min_w_0()
            .truncate(),
        )
        .child(div().flex_1())
        .child(key_hint("esc"))
        .test_support()
}

fn key_hint(text: &'static str) -> Div {
    let p = palette();
    ui::mono(text, size::XS - 1., p.text_subtle)
        .flex_shrink_0()
        .px(px(5.))
        .h(px(20.))
        .flex()
        .items_center()
        .rounded(px(4.))
        .border_1()
        .border_color(p.border)
        .bg(p.surface)
}

fn round_button(id: &'static str, fg: Hsla, bg: Hsla) -> Stateful<Div> {
    let p = palette();
    div()
        .id(id)
        .role(gpui_kit::Role::Button)
        .aria_label(match id {
            "send" => "Send message",
            "stop" => "Stop generation",
            "attach" => "Choose workspace or attach a file",
            _ => id,
        })
        .tab_index(0)
        .size(px(36.))
        .flex_shrink_0()
        .rounded(px(10.))
        .bg(bg)
        .text_color(fg)
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .hover(|style| style.opacity(0.85))
        .focus_visible(|style| style.border_1().border_color(p.accent))
}

fn chip(id: &'static str) -> Stateful<Div> {
    let p = palette();
    div()
        .id(id)
        .role(gpui_kit::Role::Button)
        .aria_label(match id {
            "model-chip" => "Choose agent and model",
            "effort-chip" => "Reasoning effort",
            "approval-hint" => "Native permission mode",
            _ => id,
        })
        .tab_index(0)
        .h(px(34.))
        .max_w_full()
        .min_w_0()
        .px(px(11.))
        .rounded(px(8.))
        .border_1()
        .border_color(p.border)
        .bg(p.raised)
        .flex()
        .items_center()
        .overflow_hidden()
        .gap(px(7.))
        .cursor_pointer()
        .hover(|style| style.bg(hsla(0., 0., 1., 0.05)))
        .focus_visible(|style| style.border_color(p.accent))
}

#[cfg(test)]
mod tests {
    use super::{chip, round_button};
    use crate::theme::palette;
    use gpui_kit::Element as _;

    #[test]
    fn composer_controls_have_accessible_button_roles() {
        let p = palette();
        assert_eq!(
            round_button("send", p.text, p.bg).a11y_role(),
            Some(gpui_kit::Role::Button)
        );
        assert_eq!(chip("model-chip").a11y_role(), Some(gpui_kit::Role::Button));
    }
}
