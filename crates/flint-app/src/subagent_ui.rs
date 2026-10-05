//! Child conversation navigation. Engines, approvals and storage remain owned
//! by the parent; selecting a child never creates another session or writer.

use flint_agent::TurnEndReason;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::*;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::app::FlintApp;
use crate::session::{Bucket, Session, Status};
use crate::theme::{palette, size};
use crate::ui;
use crate::view_model::{Change, Item, SessionView, SubagentView};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conversation {
    pub parent: u64,
    pub child: Option<String>,
}

impl Conversation {
    pub fn parent(parent: u64) -> Self {
        Self {
            parent,
            child: None,
        }
    }
}

impl SessionView {
    pub fn subagent(&self, id: &str) -> Option<&SubagentView> {
        self.subagents.iter().find(|child| child.session_id == id)
    }

    pub fn subagent_mut(&mut self, id: &str) -> Option<&mut SubagentView> {
        self.subagents
            .iter_mut()
            .find(|child| child.session_id == id)
    }
}

impl SubagentView {
    pub fn matches_query(&self, query: &str) -> bool {
        query.is_empty()
            || [&self.label, &self.model, &self.session_id]
                .iter()
                .any(|text| text.to_lowercase().contains(query))
            || self
                .view
                .items
                .iter()
                .rev()
                .find_map(|item| match item {
                    Item::User(text) | Item::Assistant { text, .. } => Some(text),
                    _ => None,
                })
                .is_some_and(|text| text.to_lowercase().contains(query))
    }

    pub fn status(&self, parent: &SessionView) -> Status {
        let waiting = parent.items.iter().any(|item| match item {
            Item::Approval {
                call_id,
                decision: None,
                ..
            } => self
                .view
                .items
                .iter()
                .any(|item| matches!(item, Item::Tool(call) if &call.call_id == call_id)),
            _ => false,
        });
        if waiting {
            Status::NeedsApproval
        } else if self.view.running || self.queued {
            Status::Running
        } else if matches!(
            self.view.last_reason,
            Some(TurnEndReason::Failed(_) | TurnEndReason::StepLimit)
        ) {
            Status::Failed { seen: !self.unread }
        } else if self.view.last_reason == Some(TurnEndReason::Interrupted) {
            Status::Stopped
        } else if self.unread {
            Status::Unread
        } else {
            Status::Done
        }
    }

    pub fn status_text(&self, parent: &SessionView) -> &'static str {
        match self.status(parent) {
            Status::NeedsApproval => "Needs approval",
            Status::Running if self.queued => "Queued",
            Status::Running => "Running",
            Status::Failed { .. } => "Failed",
            Status::Stopped => "Stopped",
            Status::Unread | Status::Done => "Done",
            Status::Idle | Status::Starting => "Ready",
        }
    }
}

impl FlintApp {
    pub fn conversation_view(&self, target: &Conversation) -> Option<&SessionView> {
        let session = &self.sessions[self.session_index(target.parent)?];
        match &target.child {
            Some(id) => session.view.subagent(id).map(|child| &child.view),
            None => Some(&session.view),
        }
    }

    fn conversation_list(&self, target: &Conversation) -> Option<&ListState> {
        let session = &self.sessions[self.session_index(target.parent)?];
        match &target.child {
            Some(id) => session.subagent_lists.get(id),
            None => Some(&session.list),
        }
    }

    pub fn select_subagent(
        &mut self,
        uid: u64,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(ix) = self.session_index(uid) else {
            return;
        };
        if self.sessions[ix].view.subagent(id).is_none() {
            return;
        }
        self.select_session_with_focus(ix, false, window, cx);
        if let Some(pane) = self.session_workspace.panes.get_mut(&uid)
            && pane.mode == crate::session_workspace::PaneMode::Terminal
        {
            pane.mode = crate::session_workspace::PaneMode::Chat;
        }
        let session = &mut self.sessions[ix];
        session.selected_subagent = Some(id.to_string());
        session.subagents_expanded = true;
        session.view.subagent_mut(id).unwrap().unread = false;
        self.copied = None;
        self.copied_conversation = None;
        self.mention = None;
        self.slash = None;
        self.option_menu = None;
        self.agent_menu = false;
        self.project_menu = None;
        self.help_open = false;
        self.clear_file_preview();
        self.focus.focus(window, cx);
        cx.notify();
    }

    pub fn toggle_conversation_item(
        &mut self,
        target: &Conversation,
        ix: usize,
        cx: &mut Context<Self>,
    ) {
        if let Some(list) = self.conversation_list(target) {
            list.pause_following_tail();
        }
        self.update_conversation(target, |view| view.toggle_expanded(ix), cx);
    }

    pub fn toggle_conversation_work(
        &mut self,
        target: &Conversation,
        ix: usize,
        cx: &mut Context<Self>,
    ) {
        if let Some(list) = self.conversation_list(target) {
            list.pause_following_tail();
        }
        self.update_conversation(target, |view| view.toggle_work(ix), cx);
    }

    pub fn conversation_feedback(
        &mut self,
        target: &Conversation,
        ix: usize,
        positive: bool,
        cx: &mut Context<Self>,
    ) {
        self.update_conversation(target, |view| view.set_feedback(ix, positive), cx);
    }

    fn update_conversation(
        &mut self,
        target: &Conversation,
        update: impl FnOnce(&mut SessionView) -> Change,
        cx: &mut Context<Self>,
    ) {
        let Some(ix) = self.session_index(target.parent) else {
            return;
        };
        let session = &mut self.sessions[ix];
        if let Some(id) = &target.child {
            let Some(child) = session.view.subagent_mut(id) else {
                return;
            };
            let change = update(&mut child.view);
            if let Some(list) = session.subagent_lists.get(id) {
                Session::apply_list(list, change);
            }
        } else {
            let change = update(&mut session.view);
            session.apply(change);
        }
        cx.notify();
    }

    pub fn copy_conversation_answer(
        &mut self,
        target: &Conversation,
        ix: usize,
        text: String,
        cx: &mut Context<Self>,
    ) {
        if self.conversation_view(target).is_none() {
            return;
        }
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        let copied_at = std::time::Instant::now();
        self.copied = Some((ix, copied_at));
        self.copied_conversation = Some(target.clone());
        if let Some(list) = self.conversation_list(target) {
            list.remeasure_items(ix..ix + 1);
        }
        let target = target.clone();
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(1600))
                .await;
            this.update(cx, |app, cx| {
                if app.copied == Some((ix, copied_at))
                    && app.copied_conversation.as_ref() == Some(&target)
                {
                    app.copied = None;
                    app.copied_conversation = None;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }
}

pub(crate) fn sidebar_children(
    app: &FlintApp,
    session: &Session,
    cx: &mut Context<FlintApp>,
) -> AnyElement {
    let p = palette();
    let uid = session.uid;
    let query = app.search_query(cx);
    let expanded = session.subagents_expanded || !query.is_empty();
    let parent_matches = app.parent_matches_query(app.session_index(uid).unwrap(), &query);
    let children = session.view.subagents.iter().filter(|child| {
        (parent_matches || child.matches_query(&query))
            && match app.filter {
                crate::app::SessionFilter::All => true,
                crate::app::SessionFilter::Only(bucket) => {
                    child.status(&session.view).bucket() == bucket
                }
            }
    });
    div()
        .mx(px(20.))
        .mb(px(6.))
        .flex()
        .flex_col()
        .child(
            Button::new(SharedString::from(format!("subagent-disclosure-{uid}")))
                .ghost()
                .label(format!("Subagents ({})", session.view.subagents.len()))
                .icon(ui::icon(
                    if expanded {
                        IconName::ChevronDown
                    } else {
                        IconName::ChevronRight
                    },
                    12.,
                    p.text_subtle,
                ))
                .on_click(cx.listener(move |app, _, _, cx| {
                    if let Some(ix) = app.session_index(uid) {
                        app.sessions[ix].subagents_expanded = !expanded;
                    }
                    cx.notify();
                })),
        )
        .when(expanded, |column| {
            column.children(children.map(|child| {
                let id = child.session_id.clone();
                let focus_id = SharedString::from(format!("subagent-scroll-{uid}-{id}"));
                let status = child.status(&session.view);
                let selected =
                    app.session().uid == uid && session.selected_subagent.as_deref() == Some(&id);
                let color = match status.bucket() {
                    Bucket::NeedsYou => p.warning,
                    Bucket::Working => p.accent,
                    _ => p.text_subtle,
                };
                let tip = format!(
                    "{} · {} · {} · {}",
                    child.label,
                    child.model,
                    child.session_id,
                    child.status_text(&session.view)
                );
                let row = div()
                    .id(SharedString::from(format!("subagent-{uid}-{id}")))
                    .role(Role::Button)
                    .aria_label(format!("Subagent: {tip}"))
                    .tab_index(0)
                    .focus_visible(|style| style.border_1().border_color(p.accent))
                    .min_w_0()
                    .px(px(10.))
                    .py(px(7.))
                    .rounded(px(7.))
                    .flex()
                    .items_center()
                    .gap(px(7.))
                    .cursor_pointer()
                    .when(selected, |row| row.bg(p.raised))
                    .hover(|style| style.bg(p.raised))
                    .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
                    .on_click(cx.listener(move |app, _, window, cx| {
                        app.select_subagent(uid, &id, window, cx)
                    }))
                    .child(if status == Status::Running {
                        ui::spinner(app.now(), 12., color).into_any_element()
                    } else {
                        ui::icon(
                            if status == Status::NeedsApproval {
                                IconName::ShieldAlert
                            } else {
                                IconName::Bot
                            },
                            12.,
                            color,
                        )
                        .into_any_element()
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(size::SM))
                                    .text_color(p.text)
                                    .child(ui::one_line(&child.label)),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(size::XS))
                                    .text_color(color)
                                    .child(format!(
                                        "{} · {}",
                                        child.status_text(&session.view),
                                        ui::one_line(&child.model)
                                    )),
                            ),
                    )
                    .when(child.unread, |row| {
                        row.child(div().size(px(5.)).rounded_full().bg(p.accent))
                    })
                    .test_support();
                crate::sidebar::ScrollableRow {
                    id: focus_id.clone().into(),
                    row: div().id(focus_id).child(row),
                    scroll: app.sidebar_scroll.clone(),
                }
            }))
        })
        .into_any_element()
}

pub(crate) fn render(app: &FlintApp, uid: u64, id: &str, cx: &mut Context<FlintApp>) -> AnyElement {
    let Some(ix) = app.session_index(uid) else {
        return div().into_any_element();
    };
    let session = &app.sessions[ix];
    let Some(child) = session.view.subagent(id) else {
        return div().into_any_element();
    };
    let Some(state) = session.subagent_lists.get(id) else {
        return div().into_any_element();
    };
    let target = Conversation {
        parent: uid,
        child: Some(id.to_string()),
    };
    let list_target = target.clone();
    let p = palette();
    let list = list(
        state.clone(),
        cx.processor(move |app, row, window, cx| {
            app.render_conversation_item(&list_target, row, window, cx)
        }),
    )
    .size_full();
    let latest_id = id.to_string();
    let active = app.session().uid == uid;
    div()
        .id(SharedString::from(format!("subagent-view-{uid}-{id}")))
        .size_full()
        .min_h_0()
        .flex()
        .flex_col()
        .child(
            div()
                .flex_shrink_0()
                .p(px(12.))
                .flex()
                .flex_col()
                .gap(px(4.))
                .child(
                    Button::new(SharedString::from(format!("subagent-back-{uid}")))
                        .ghost()
                        .label("Back to parent")
                        .on_click(cx.listener(move |app, _, window, cx| {
                            if let Some(ix) = app.session_index(uid) {
                                app.select_session(ix, window, cx);
                            }
                        })),
                )
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_size(px(size::BASE))
                        .text_color(p.text)
                        .child(ui::one_line(&child.label)),
                )
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_size(px(size::XS))
                        .text_color(p.text_subtle)
                        .child(ui::one_line(&format!(
                            "{} · {} · {} · Read-only",
                            child.model,
                            child.session_id,
                            child.status_text(&session.view)
                        ))),
                ),
        )
        .child(
            div()
                .id(SharedString::from(format!(
                    "subagent-transcript-{uid}-{id}"
                )))
                .flex_1()
                .min_h_0()
                .relative()
                .child(list)
                .when(!state.is_following_tail(), |area| {
                    area.child(
                        div().absolute().bottom(px(12.)).right(px(16.)).child(
                            Button::new(SharedString::from(format!("subagent-latest-{uid}-{id}")))
                                .outline()
                                .label("Jump to latest")
                                .on_click(cx.listener(move |app, _, _, cx| {
                                    if let Some(ix) = app.session_index(uid)
                                        && let Some(list) =
                                            app.sessions[ix].subagent_lists.get(&latest_id)
                                    {
                                        list.set_follow_mode(FollowMode::Tail);
                                        cx.notify();
                                    }
                                })),
                        ),
                    )
                }),
        )
        .when(active, |column| {
            column.children(
                session
                    .view
                    .pending_approval_ref()
                    .map(|(call_id, kind, summary)| {
                        crate::transcript::pinned_approval(app, call_id, kind, summary, cx)
                    }),
            )
        })
        .test_support()
        .into_any_element()
}

#[cfg(test)]
#[path = "subagent_ui_tests.rs"]
mod tests;
