//! The center column: a virtualized, tail-following transcript above the
//! composer, or the empty state for a fresh session.

mod message;
mod notices;
mod tool_card;

use gpui_kit::assets::IconName;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::app::FlintApp;
use crate::theme::palette;
use crate::theme::size;
use crate::ui;
use crate::view_model::Item;

/// Readable line length for the transcript and composer.
pub const COLUMN_WIDTH: f32 = 820.;

pub fn render_main(
    app: &FlintApp,
    window: &mut Window,
    cx: &mut Context<FlintApp>,
) -> impl IntoElement {
    let session = app.session();
    let content = if session.view.items.is_empty() {
        empty_state(cx).into_any_element()
    } else {
        list(
            session.list.clone(),
            cx.processor(|this, ix, window, cx| this.render_item(ix, window, cx)),
        )
        .size_full()
        .into_any_element()
    };
    div()
        .size_full()
        .flex()
        .flex_col()
        .bg(palette().bg)
        .child(div().flex_1().min_h_0().child(content))
        .child(crate::composer::render(app, window, cx))
}

impl FlintApp {
    fn render_item(
        &mut self,
        ix: usize,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let session = self.session();
        let Some(item) = session.view.items.get(ix) else {
            return div().into_any_element();
        };
        let last = ix + 1 == session.view.items.len();
        let (top, body) = match item {
            Item::User(text) => (20., message::user(text).into_any_element()),
            Item::Assistant { text, streaming } => (
                12.,
                message::assistant(ix, text, *streaming).into_any_element(),
            ),
            Item::Thinking {
                text,
                duration,
                expanded,
                ..
            } => (
                12.,
                message::thinking(ix, text, *duration, *expanded, cx).into_any_element(),
            ),
            Item::Tool(call) => (
                6.,
                tool_card::render(ix, call, self.now(), cx).into_any_element(),
            ),
            Item::Nudge { reason, message } => {
                (8., notices::nudge(*reason, message).into_any_element())
            }
            Item::Repair { tool, detail } => (6., notices::repair(tool, detail).into_any_element()),
            Item::Approval {
                call_id,
                kind,
                summary,
                decision,
            } => (
                10.,
                notices::approval(call_id, *kind, summary, *decision, cx).into_any_element(),
            ),
            Item::Error(message) => (10., notices::error(message).into_any_element()),
            Item::TurnSummary {
                reason,
                duration,
                steps,
                usage,
            } => (
                18.,
                notices::turn_summary(reason, *duration, *steps, usage).into_any_element(),
            ),
        };
        div()
            .w_full()
            .flex()
            .justify_center()
            .px(px(28.))
            .pt(px(if ix == 0 { 28. } else { top }))
            .when(last, |row| row.pb(px(28.)))
            .child(div().w_full().max_w(px(COLUMN_WIDTH)).child(body))
            .into_any_element()
    }
}

const SUGGESTIONS: &[(&str, IconName)] = &[
    ("Find and fix the failing tests", IconName::FlaskConical),
    (
        "Explain how this codebase is structured",
        IconName::ListTree,
    ),
    ("Add input validation to the CLI flags", IconName::SquarePen),
];

fn empty_state(cx: &mut Context<FlintApp>) -> impl IntoElement {
    let p = palette();
    let chips = SUGGESTIONS.iter().enumerate().map(|(ix, (text, icon))| {
        let text = *text;
        div()
            .id(("suggestion", ix))
            .px(px(12.))
            .h(px(34.))
            .flex()
            .items_center()
            .gap(px(10.))
            .rounded(px(8.))
            .border_1()
            .border_color(p.border)
            .bg(p.surface)
            .cursor_pointer()
            .hover(|style| style.bg(p.raised).border_color(p.border_strong))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.composer.update(cx, |state, cx| {
                    state.set_value(text, window, cx);
                    state.focus(window, cx);
                });
            }))
            .child(ui::icon(*icon, 14., p.text_subtle))
            .child(ui::label(text, size::BASE, p.text_muted))
    });
    div()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(22.))
        .pb(px(40.))
        .child(
            div()
                .size(px(44.))
                .rounded(px(12.))
                .bg(p.accent_soft)
                .border_1()
                .border_color(hsla(22. / 360., 1., 0.62, 0.25))
                .flex()
                .items_center()
                .justify_center()
                .child(ui::icon(IconName::Flame, 22., p.accent)),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .items_center()
                .gap(px(6.))
                .child(
                    div()
                        .text_size(px(22.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("What should we build?"),
                )
                .child(ui::label(
                    "flint reads, edits and runs code in this workspace, then verifies its work.",
                    size::MD,
                    p.text_muted,
                )),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .w(px(420.))
                .children(chips),
        )
        .child(
            div()
                .flex()
                .gap(px(16.))
                .child(hint("⌘K", "Commands"))
                .child(hint("⌘N", "New session"))
                .child(hint("⌘O", "Open folder")),
        )
}

fn hint(keys: &str, text: &'static str) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(6.))
        .child(ui::key_hint(keys))
        .child(ui::label(text, size::SM, palette().text_subtle))
}
