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

use gpui_kit::component::button::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::app::FlintApp;
use crate::subagent_ui::Conversation;
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
    if let Some(id) = &session.selected_subagent {
        return crate::subagent_ui::render(app, session.uid, id, cx);
    }
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
                .relative()
                .child(
                    list(
                        session.list.clone(),
                        cx.processor(|this, ix, window, cx| this.render_item(ix, window, cx)),
                    )
                    .size_full(),
                )
                .when(!session.list.is_following_tail(), |area| {
                    area.child(jump_to_latest(session.uid, cx))
                })
                .test_support(),
        )
        .child(
            div()
                .flex_shrink_0()
                .flex()
                .justify_center()
                .px(px(if window.viewport_size().width < px(600.) {
                    16.
                } else {
                    32.
                }))
                .pb(px(22.))
                .pt(px(8.))
                .child(crate::composer::render(app, window, cx)),
        )
        .into_any_element()
}

/// The session's list is independent; only the focused tile owns the composer.
pub fn render_session_pane(
    app: &FlintApp,
    uid: u64,
    window: &mut Window,
    cx: &mut Context<FlintApp>,
) -> AnyElement {
    let Some(ix) = app.session_index(uid) else {
        return div().into_any_element();
    };
    let session = &app.sessions[ix];
    if let Some(id) = &session.selected_subagent {
        return crate::subagent_ui::render(app, uid, id, cx);
    }
    let active = app.session().uid == uid;
    let p = palette();
    let content = if session.view.items.is_empty() {
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .p(px(16.))
            .child(crate::ui::label(
                "Ready when you are",
                crate::theme::size::SM,
                p.text_subtle,
            ))
            .into_any_element()
    } else {
        list(
            session.list.clone(),
            cx.processor(move |this, ix, window, cx| this.render_session_item(uid, ix, window, cx)),
        )
        .size_full()
        .into_any_element()
    };
    div()
        .id(SharedString::from(format!("session-transcript-{uid}")))
        .size_full()
        .min_h_0()
        .relative()
        .flex()
        .flex_col()
        .bg(p.bg)
        .child(div().flex_1().min_h_0().relative().child(content).when(
            !session.view.items.is_empty() && !session.list.is_following_tail(),
            |area| area.child(jump_to_latest(uid, cx)),
        ))
        .when(active, |pane| {
            pane.child(
                div()
                    .flex_shrink_0()
                    .px(px(12.))
                    .pt(px(6.))
                    .pb(px(12.))
                    .child(crate::composer::render(app, window, cx)),
            )
            .child(crate::popover::probe(&app.popover_room, |room| &room.area))
        })
        .when(!active, |pane| {
            pane.child(
                div()
                    .id(SharedString::from(format!("session-pane-compose-{uid}")))
                    .role(gpui_kit::Role::Button)
                    .aria_label("Focus this session's composer")
                    .tab_index(0)
                    .h(px(38.))
                    .flex_shrink_0()
                    .mx(px(12.))
                    .mb(px(10.))
                    .px(px(12.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(p.border)
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .justify_between()
                    .hover(|style| style.bg(p.raised))
                    .focus_visible(|style| style.border_color(p.accent))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |app, _, window, cx| {
                            app.focus_session_pane(uid, true, window, cx);
                            cx.stop_propagation();
                        }),
                    )
                    .on_click(cx.listener(move |app, _, window, cx| {
                        app.focus_session_pane(uid, true, window, cx)
                    }))
                    .child(crate::ui::label(
                        if session.status() == crate::session::Status::NeedsApproval {
                            "Approval waiting, click to respond"
                        } else {
                            "Click to message this session"
                        },
                        crate::theme::size::XS,
                        p.text_subtle,
                    ))
                    .child(crate::ui::icon(
                        gpui_kit::assets::IconName::ArrowUpRight,
                        12.,
                        p.text_subtle,
                    ))
                    .test_support(),
            )
        })
        .test_support()
        .into_any_element()
}

/// Explicitly re-engage GPUI's tail mode after intentional scrolling.
fn jump_to_latest(uid: u64, cx: &mut Context<FlintApp>) -> AnyElement {
    div()
        .absolute()
        .bottom(px(12.))
        .right(px(16.))
        .child(
            Button::new(SharedString::from(format!("transcript-latest-{uid}")))
                .outline()
                .label("Jump to latest")
                .tooltip("Return to the bottom and follow new output")
                .on_click(cx.listener(move |app, _, _, cx| {
                    if let Some(ix) = app.session_index(uid) {
                        app.sessions[ix].list.set_follow_mode(FollowMode::Tail);
                        cx.notify();
                    }
                })),
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
        self.render_session_item(self.session().uid, ix, _window, cx)
    }

    fn render_session_item(
        &mut self,
        uid: u64,
        ix: usize,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.render_conversation_item(&Conversation::parent(uid), ix, _window, cx)
    }

    pub(crate) fn render_conversation_item(
        &mut self,
        target: &Conversation,
        ix: usize,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(session_ix) = self.session_index(target.parent) else {
            return div().into_any_element();
        };
        let Some(view) = self.conversation_view(target) else {
            return div().into_any_element();
        };
        let Some(item) = view.items.get(ix) else {
            return div().into_any_element();
        };
        let role = view.role(ix);
        if role == Role::Hidden {
            return div().into_any_element();
        }
        let now = self.now();
        let workspace = &self.sessions[session_ix].workspace;
        let (top, body): (f32, AnyElement) = match role {
            Role::Answer => (22., turn::answer(ix, item, workspace, cx)),
            Role::Summary => match view.turn_at(ix) {
                Some(info) => {
                    let copied = self.copied_conversation.as_ref() == Some(target)
                        && self.copied.is_some_and(|(row, _)| row == ix);
                    (24., turn::summary(target, ix, item, info, view, copied, cx))
                }
                None => (0., div().into_any_element()),
            },
            Role::WorkHeader => {
                let info = view.turn_at(ix).cloned().unwrap_or_default();
                let header = turn::worked_line(target, ix, &info, cx);
                let body = div()
                    .flex()
                    .flex_col()
                    .child(header)
                    .when(info.expanded, |col| {
                        col.child(div().pt(px(16.)).child(rows::render(
                            target,
                            ix,
                            item,
                            now,
                            &self.settings.base_url,
                            workspace,
                            cx,
                        )))
                    })
                    .into_any_element();
                (24., body)
            }
            Role::Plain | Role::Live | Role::Work => (
                row_spacing(item),
                rows::render(
                    target,
                    ix,
                    item,
                    now,
                    &self.settings.base_url,
                    workspace,
                    cx,
                ),
            ),
            Role::Hidden => unreachable!("handled above"),
        };
        let last = ix + 1 == view.items.len();
        div()
            .w_full()
            .flex()
            .justify_center()
            .px(px(20.))
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
        Item::Nudge { .. } | Item::Compacted { .. } | Item::Reverted { .. } => 12.,
        Item::Approval { .. } | Item::Error(_) => 18.,
        Item::TurnSummary { .. } => 24.,
    }
}
