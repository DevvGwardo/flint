//! The composer's "Up next" tray and compact-pane popover.

use gpui_kit::assets::IconName;
use gpui_kit::base::Disableable as _;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Textarea;
use gpui_kit::component::scroll::{Scrollbar, ScrollbarMode};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::app::FlintApp;
use crate::theme::{palette, size};
use crate::ui;

fn preview(text: &str) -> String {
    let text = text.trim_start();
    if text.is_empty() {
        "Image prompt".to_string()
    } else {
        ui::one_line(text)
    }
}

const MAX_EXACT_QUEUE_CHARS: usize = 2_000;

fn character_detail(text: &str) -> String {
    let count = if text.len() <= MAX_EXACT_QUEUE_CHARS {
        text.chars().count()
    } else {
        text.chars().take(MAX_EXACT_QUEUE_CHARS + 1).count()
    };
    if count <= MAX_EXACT_QUEUE_CHARS {
        format!("{count} characters")
    } else {
        "More than 2,000 characters".to_string()
    }
}

pub fn render(app: &FlintApp, max_height: f32, cx: &mut Context<FlintApp>) -> Option<AnyElement> {
    let session = app.session();
    let queue = &session.prompt_queue;
    if queue.items.is_empty() && queue.error.is_none() {
        return None;
    }
    let editing = app.queue_edit.as_ref().filter(|edit| {
        edit.uid == session.uid && queue.items.iter().any(|prompt| prompt.id == edit.id)
    });
    let footer_height = if editing.is_some() { 44. } else { 0. };
    let p = palette();
    let roomy = app
        .popover_room
        .area
        .get()
        .is_none_or(|area| area.size.width >= px(480.));
    let state = if queue.steer_after_turn.is_some() {
        "Stopping to steer"
    } else if session.steering_pending.is_some() {
        "Applying steering"
    } else if queue.paused {
        "Paused"
    } else {
        "Runs after this turn"
    };
    let header = div()
        .id("queue-header")
        .h(px(36.))
        .flex_shrink_0()
        .px(px(12.))
        .flex()
        .items_center()
        .gap(px(8.))
        .child(ui::icon(IconName::ListTree, 13., p.text_muted))
        .child(ui::label(
            format!("Up next · {}", queue.items.len()),
            size::XS,
            p.text,
        ))
        .when(roomy, |header| {
            header.child(ui::label(
                state,
                size::XS,
                if queue.paused {
                    p.warning
                } else {
                    p.text_subtle
                },
            ))
        })
        .child(div().flex_1())
        .child(
            Button::new("queue-pause")
                .small()
                .label(if queue.paused { "Resume" } else { "Pause" })
                .disabled(queue.steer_after_turn.is_some() || session.steering_pending.is_some())
                .on_click(cx.listener(|app, _, _, cx| app.pause_prompt_queue(cx))),
        )
        .test_support();
    let rows = queue.items.iter().enumerate().map(|(ix, prompt)| {
        let id = prompt.id;
        let sending = session.steering_pending == Some(id) || queue.steer_after_turn == Some(id);
        let editing = editing.filter(|edit| edit.id == id);
        let preview = preview(&prompt.text);
        let detail = if sending {
            "Applying…".to_string()
        } else if prompt.images.is_empty() {
            character_detail(&prompt.text)
        } else {
            format!(
                "{} image{}",
                prompt.images.len(),
                if prompt.images.len() == 1 { "" } else { "s" }
            )
        };
        let actions = div()
            .flex()
            .items_center()
            .flex_wrap()
            .gap(px(3.))
            .child(
                Button::new(SharedString::from(format!("queue-up-{id}")))
                    .small()
                    .icon(IconName::ChevronUp)
                    .accessibility_label("Move queued prompt earlier")
                    .tooltip("Move earlier")
                    .disabled(ix == 0 || sending)
                    .on_click(cx.listener(move |app, _, _, cx| app.move_queued_prompt(id, -1, cx))),
            )
            .child(
                Button::new(SharedString::from(format!("queue-down-{id}")))
                    .small()
                    .icon(IconName::ChevronDown)
                    .accessibility_label("Move queued prompt later")
                    .tooltip("Move later")
                    .disabled(ix + 1 == queue.items.len() || sending)
                    .on_click(cx.listener(move |app, _, _, cx| app.move_queued_prompt(id, 1, cx))),
            )
            .child(
                Button::new(SharedString::from(format!("queue-edit-{id}")))
                    .small()
                    .icon(IconName::Pencil)
                    .accessibility_label("Edit queued prompt")
                    .tooltip("Edit queued prompt")
                    .disabled(sending)
                    .on_click(cx.listener(move |app, _, window, cx| {
                        app.edit_queued_prompt(id, window, cx)
                    })),
            )
            .child(
                Button::new(SharedString::from(format!("queue-steer-{id}")))
                    .small()
                    .label(if app.prompt_busy(app.active) {
                        "Steer now"
                    } else {
                        "Send now"
                    })
                    .tooltip(if session.agent == flint_agent::AgentKind::Flint {
                        "Apply at the next model step without interrupting tools."
                    } else {
                        "Stop the current turn, then send this prompt next."
                    })
                    .disabled(
                        sending
                            || session.stopping
                            || session.steering_pending.is_some()
                            || queue.steer_after_turn.is_some(),
                    )
                    .on_click(cx.listener(move |app, _, _, cx| app.steer_queued_prompt(id, cx))),
            )
            .child(
                Button::new(SharedString::from(format!("queue-remove-{id}")))
                    .small()
                    .icon(IconName::X)
                    .accessibility_label("Remove queued prompt")
                    .tooltip("Remove queued prompt")
                    .disabled(sending)
                    .on_click(cx.listener(move |app, _, _, cx| app.remove_queued_prompt(id, cx))),
            );
        div()
            .id(SharedString::from(format!("queue-row-{id}")))
            .px(px(12.))
            .py(px(9.))
            .border_t_1()
            .border_color(p.border)
            .flex()
            .flex_col()
            .gap(px(5.))
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(9.))
                    .child(ui::mono(format!("{:02}", ix + 1), size::XS, p.text_subtle).pt(px(2.)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(3.))
                            .child(
                                div()
                                    .id(SharedString::from(format!("queue-preview-{id}")))
                                    .truncate()
                                    .child(ui::label(preview, size::SM, p.text))
                                    .test_support(),
                            )
                            .child(
                                ui::label(detail, size::XS, p.text_muted)
                                    .id(SharedString::from(format!("queue-detail-{id}")))
                                    .min_w_0()
                                    .truncate()
                                    .test_support(),
                            ),
                    ),
            )
            .when_some(editing, |row, edit| {
                row.child(
                    div()
                        .id(SharedString::from(format!("queue-edit-input-{id}")))
                        .child(Textarea::new(&edit.input).text_size(px(size::SM)))
                        .test_support(),
                )
            })
            .when(editing.is_none(), |row| row.child(actions))
            .test_support()
    });
    Some(
        div()
            .id("prompt-queue")
            .w_full()
            .min_h_0()
            .max_h(px(max_height))
            .rounded(px(12.))
            .border_1()
            .border_color(p.border_strong)
            .bg(p.bg)
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(header)
            .child(
                div()
                    .relative()
                    .min_h_0()
                    .overflow_hidden()
                    .child(
                        div()
                            .id("queue-rows")
                            .max_h(px((max_height - 36. - footer_height - 2.).max(0.)))
                            .overflow_y_scroll()
                            .track_scroll(&session.queue_scroll)
                            .children(rows)
                            .test_support(),
                    )
                    .child(Scrollbar::vertical(&session.queue_scroll).mode(ScrollbarMode::Always)),
            )
            .children(queue.error.as_ref().map(|error| {
                div()
                    .id("queue-error")
                    .px(px(12.))
                    .py(px(8.))
                    .child(ui::label(error.clone(), size::XS, p.warning))
                    .test_support()
            }))
            .when(editing.is_some(), |queue| {
                queue.child(
                    div()
                        .id("queue-edit-footer")
                        .h(px(44.))
                        .flex_shrink_0()
                        .px(px(12.))
                        .border_t_1()
                        .border_color(p.border)
                        .flex()
                        .items_center()
                        .justify_end()
                        .gap(px(6.))
                        .child(
                            Button::new("queue-edit-cancel")
                                .small()
                                .label("Cancel")
                                .on_click(cx.listener(|app, _, window, cx| {
                                    app.queue_edit = None;
                                    app.composer.update(cx, |state, cx| state.focus(window, cx));
                                    app.dispatch_next_prompt(app.session().uid, cx);
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new("queue-edit-save")
                                .small()
                                .primary()
                                .label("Save prompt")
                                .on_click(cx.listener(|app, _, window, cx| {
                                    app.save_queued_edit(window, cx)
                                })),
                        )
                        .test_support(),
                )
            })
            .test_support()
            .into_any_element(),
    )
}

#[cfg(test)]
#[path = "queue_view_tests.rs"]
mod tests;
