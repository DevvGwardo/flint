//! User messages, assistant markdown, and collapsible thinking blocks.

use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::component::shimmer::ShimmerText;
use gpui_kit::component::text::TextView;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::app::FlintApp;
use crate::theme::palette;
use crate::theme::size;
use crate::ui;

pub fn user(text: &str) -> impl IntoElement {
    let p = palette();
    div()
        .w_full()
        .flex()
        .gap(px(10.))
        .px(px(14.))
        .py(px(11.))
        .rounded(px(10.))
        .bg(p.surface)
        .border_1()
        .border_color(p.border)
        .child(
            div()
                .mt(px(1.))
                .size(px(20.))
                .flex_shrink_0()
                .rounded_full()
                .bg(p.raised)
                .border_1()
                .border_color(p.border_strong)
                .flex()
                .items_center()
                .justify_center()
                .child(ui::icon(IconName::User, 11., p.text_muted)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_size(px(size::MD))
                .line_height(px(22.))
                .text_color(p.text)
                .child(text.to_string()),
        )
}

pub fn assistant(ix: usize, text: &str, streaming: bool) -> impl IntoElement {
    let p = palette();
    div()
        .w_full()
        .text_size(px(size::MD))
        .line_height(px(22.))
        .text_color(p.text)
        .child(
            TextView::markdown(("md", ix), text.to_string())
                .selectable(true)
                .stream_fade(streaming),
        )
}

pub fn thinking(
    ix: usize,
    text: &str,
    duration: Option<Duration>,
    expanded: bool,
    cx: &mut Context<FlintApp>,
) -> impl IntoElement {
    let p = palette();
    let tokens_estimate = text.split_whitespace().count();
    let header: AnyElement = match duration {
        None => ShimmerText::new("Thinking…")
            .id(("thinking-shimmer", ix))
            .into_any_element(),
        Some(d) => div()
            .text_color(p.text_muted)
            .child(format!("Thought for {}", ui::duration(d)))
            .into_any_element(),
    };
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(6.))
        .child(
            div()
                .id(("thinking", ix))
                .flex()
                .items_center()
                .gap(px(6.))
                .text_size(px(size::BASE))
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| this.toggle_item(ix, cx)))
                .child(ui::icon(IconName::Brain, 13., p.text_subtle))
                .child(header)
                .child(ui::label(
                    format!("· {tokens_estimate} words"),
                    size::SM,
                    p.text_subtle,
                ))
                .child(ui::icon(
                    if expanded {
                        IconName::ChevronDown
                    } else {
                        IconName::ChevronRight
                    },
                    12.,
                    p.text_subtle,
                )),
        )
        .when(expanded, |block| {
            block.child(
                div()
                    .ml(px(6.))
                    .pl(px(13.))
                    .border_l_1()
                    .border_color(p.border_strong)
                    .text_size(px(size::BASE))
                    .line_height(px(20.))
                    .text_color(p.text_muted)
                    .italic()
                    .child(text.to_string()),
            )
        })
}
