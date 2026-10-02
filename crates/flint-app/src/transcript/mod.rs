//! The center column: the empty state (greeting with a centered composer), or
//! the transcript with the composer docked below it.
//!
//! Rows render by their turn [`Role`]: a running turn is a compact live
//! activity stream; a finished turn collapses its work behind "Worked for …",
//! promotes the final answer, and ends with the files-changed card.

mod approval;
mod empty;
mod errors;
mod rows;
mod turn;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::app::FlintApp;
use crate::theme::palette;
use crate::view_model::Item;
use crate::view_model::Role;

pub use approval::pinned as pinned_approval;
pub use turn::file_icon;

/// Readable line length for the transcript.
pub const COLUMN_WIDTH: f32 = 760.;

pub fn render_main(
    app: &FlintApp,
    window: &mut Window,
    cx: &mut Context<FlintApp>,
) -> impl IntoElement {
    let session = app.session();
    if session.view.items.is_empty() {
        return empty::render(app, window, cx).into_any_element();
    }
    div()
        .size_full()
        .flex()
        .flex_col()
        .bg(palette().bg)
        .child(
            div()
                .id("transcript")
                .flex_1()
                .min_h_0()
                .child(
                    list(
                        session.list.clone(),
                        cx.processor(|this, ix, window, cx| this.render_item(ix, window, cx)),
                    )
                    .size_full(),
                )
                .test_support(),
        )
        .child(
            div()
                .flex_shrink_0()
                .flex()
                .justify_center()
                .px(px(32.))
                .pb(px(22.))
                .pt(px(8.))
                .child(crate::composer::render(app, cx)),
        )
        .into_any_element()
}

impl FlintApp {
    fn render_item(
        &mut self,
        ix: usize,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let view = &self.session().view;
        let Some(item) = view.items.get(ix) else {
            return div().into_any_element();
        };
        let role = view.role(ix);
        if role == Role::Hidden {
            return div().into_any_element();
        }
        let now = self.now();
        let (top, body): (f32, AnyElement) = match role {
            Role::Answer => (22., turn::answer(ix, item)),
            Role::Summary => match view.turn_at(ix) {
                Some(info) => {
                    let copied = self.copied.is_some_and(|(row, _)| row == ix);
                    (24., turn::summary(ix, item, info, view, copied, cx))
                }
                None => (0., div().into_any_element()),
            },
            Role::WorkHeader => {
                let info = view.turn_at(ix).cloned().unwrap_or_default();
                let header = turn::worked_line(ix, &info, cx);
                let body = div()
                    .flex()
                    .flex_col()
                    .child(header)
                    .when(info.expanded, |col| {
                        col.child(div().pt(px(16.)).child(rows::render(ix, item, now, cx)))
                    })
                    .into_any_element();
                (24., body)
            }
            Role::Plain | Role::Live | Role::Work => {
                (row_spacing(item), rows::render(ix, item, now, cx))
            }
            Role::Hidden => unreachable!("handled above"),
        };
        let last = ix + 1 == view.items.len();
        div()
            .w_full()
            .flex()
            .justify_center()
            .px(px(40.))
            .pt(px(if ix == 0 { 32. } else { top }))
            .when(last, |row| row.pb(px(32.)))
            .child(div().w_full().max_w(px(COLUMN_WIDTH)).child(body))
            .into_any_element()
    }
}

/// Vertical rhythm between rows: generous around prose, tight between tool
/// one-liners so a burst of activity reads as one block.
fn row_spacing(item: &Item) -> f32 {
    match item {
        Item::User(_) => 28.,
        Item::Assistant { .. } => 20.,
        Item::Thinking { .. } => 18.,
        Item::Tool(_) | Item::Repair { .. } => 10.,
        Item::Nudge { .. } | Item::Compacted { .. } => 12.,
        Item::Approval { .. } | Item::Error(_) => 18.,
        Item::TurnSummary { .. } => 24.,
    }
}
