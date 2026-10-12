//! A local overview, derived from loaded sessions, never invented service/Git metrics.

use std::time::{SystemTime, UNIX_EPOCH};

use gpui_kit::assets::IconName;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::app::{FlintApp, SessionFilter};
use crate::session::{Bucket, Session, Tone};
use crate::theme::{palette, size};
use crate::ui;

#[derive(Default, Debug, PartialEq, Eq)]
struct Activity {
    daily: [usize; 14],
    finished_turns: usize,
    queued: usize,
}

fn activity(sessions: &[Session], now: SystemTime) -> Activity {
    let today = now.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() / 86_400;
    let mut result = Activity::default();
    for session in sessions {
        result.queued += session.prompt_queue.items.len();
        result.finished_turns += session.view.finished_turns;
        if session.view.items.is_empty() {
            continue;
        }
        let day = session
            .touched
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            / 86_400;
        if let Some(age) = today.checked_sub(day).filter(|age| *age < 14) {
            result.daily[13 - age as usize] += 1;
        }
    }
    result
}

fn card(
    id: &'static str,
    icon: IconName,
    title: &'static str,
    count: Option<usize>,
) -> Stateful<Div> {
    let p = palette();
    div()
        .id(id)
        .w_full()
        .min_w_0()
        .min_h(px(130.))
        .p(px(14.))
        .rounded(px(12.))
        .border_1()
        .border_color(p.border.opacity(0.7))
        .bg(p.surface.opacity(0.72))
        .flex()
        .flex_col()
        .gap(px(8.))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(7.))
                .child(ui::icon(icon, 14., p.text_subtle))
                .child(ui::label(title, size::XS, p.text_muted))
                .when_some(count, |heading, count| {
                    heading.child(ui::pill(count.to_string(), p.text_subtle, p.raised))
                }),
        )
}

fn empty(title: &'static str, detail: &'static str) -> Div {
    let p = palette();
    div()
        .min_h(px(64.))
        .flex()
        .flex_col()
        .justify_center()
        .gap(px(5.))
        .child(ui::label(title, size::SM, p.text_muted))
        .child(ui::label(detail, size::XS, p.text_subtle))
}

fn session_row(
    session: &Session,
    kind: &'static str,
    app: &FlintApp,
    cx: &Context<FlintApp>,
) -> impl IntoElement {
    let p = palette();
    let uid = session.uid;
    let (status, tone) = session.status_line(app.now());
    let color = match tone {
        Tone::Muted => p.text_subtle,
        Tone::Warning => p.warning,
        Tone::Danger => p.danger,
    };
    div()
        .id(SharedString::from(format!("welcome-{kind}-{uid}")))
        .role(gpui_kit::Role::Button)
        .aria_label(format!("Open conversation: {}", session.title()))
        .tab_index(0)
        .focus_visible(|style| style.bg(p.raised).border_color(p.info))
        .min_w_0()
        .h(px(44.))
        .rounded(px(7.))
        .px(px(5.))
        .flex()
        .items_center()
        .gap(px(9.))
        .cursor_pointer()
        .hover(|style| style.bg(p.raised))
        .on_click(cx.listener(move |app, _, window, cx| {
            if let Some(ix) = app.session_index(uid) {
                app.select_session(ix, window, cx);
            }
        }))
        .child(div().size(px(5.)).rounded_full().bg(color).flex_shrink_0())
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(ui::label(ui::one_line(&session.title()), size::SM, p.text).truncate())
                .child(ui::label(ui::one_line(&status), size::XS, color).truncate()),
        )
        .child(ui::icon(IconName::ArrowUpRight, 12., p.text_subtle))
        .test_support()
}

/// The UI shows two rows, so keep only the two newest instead of sorting all
/// conversations on every redraw. Equal timestamps keep session-list order.
fn recent_sessions(sessions: &[Session]) -> (usize, Vec<&Session>) {
    let mut count = 0;
    let mut newest: Vec<&Session> = Vec::with_capacity(3);
    for session in sessions
        .iter()
        .filter(|session| !session.view.items.is_empty())
    {
        count += 1;
        let position = newest
            .iter()
            .position(|old| session.touched > old.touched)
            .unwrap_or(newest.len());
        if position < 2 {
            newest.insert(position, session);
            newest.truncate(2);
        }
    }
    (count, newest)
}

pub fn render(app: &FlintApp, columns: usize, cx: &mut Context<FlintApp>) -> impl IntoElement {
    let p = palette();
    let working: Vec<_> = app
        .sessions
        .iter()
        .filter(|session| session.status().bucket() == Bucket::Working)
        .collect();
    let waiting = app
        .sessions
        .iter()
        .filter(|session| session.status().bucket() == Bucket::NeedsYou)
        .count();
    let (recent_count, recent) = recent_sessions(&app.sessions);
    let stats = activity(&app.sessions, SystemTime::now());
    let total: usize = stats.daily.iter().sum();
    let peak = stats.daily.iter().copied().max().unwrap_or(0).max(1);

    let working_card = card(
        "welcome-working",
        IconName::Sparkles,
        "Working now",
        Some(working.len()),
    )
    .when(working.is_empty(), |card| {
        card.child(empty(
            "A little quiet, in a good way.",
            "Start a task above. Live agent work appears here.",
        ))
    })
    .children(
        working
            .iter()
            .take(2)
            .map(|session| session_row(session, "working", app, cx)),
    )
    .when(waiting > 0, |card| {
        card.child(
            div()
                .id("welcome-needs-you")
                .role(gpui_kit::Role::Button)
                .aria_label(format!("Open {waiting} conversations that need you"))
                .tab_index(0)
                .focus_visible(|style| style.border_1().border_color(p.warning))
                .cursor_pointer()
                .text_size(px(size::XS))
                .text_color(p.warning)
                .hover(|style| style.underline())
                .on_click(cx.listener(|app, _, window, cx| {
                    app.open_sessions(window, cx);
                    app.set_filter(SessionFilter::Only(Bucket::NeedsYou), cx);
                }))
                .child(format!("{waiting} need your attention"))
                .test_support(),
        )
    })
    .test_support();

    let recent_card = card(
        "welcome-recent",
        IconName::MessageSquare,
        "Recent conversations",
        Some(recent_count),
    )
    .when(recent.is_empty(), |card| {
        card.child(empty(
            "Your next idea starts here.",
            "Conversations you start will be a click away.",
        ))
    })
    .children(
        recent
            .iter()
            .take(2)
            .map(|session| session_row(session, "recent", app, cx)),
    )
    .test_support();

    let activity_card = card(
        "welcome-activity",
        IconName::ListTree,
        "Session activity",
        None,
    )
    .child(
        div()
            .flex()
            .items_baseline()
            .gap(px(8.))
            .child(ui::label(total.to_string(), 26., p.text).font_weight(FontWeight::MEDIUM))
            .child(ui::label(
                "conversations active · 14 days",
                size::XS,
                p.text_subtle,
            )),
    )
    .child(
        div()
            .id("welcome-activity-chart")
            .aria_label("Daily count of loaded conversations, based on their last activity")
            .tooltip(|window, cx| {
                gpui_kit::component::tooltip::Tooltip::new(
                    "Loaded conversations grouped by their last activity day (UTC). \
                     Each conversation is counted once; this is not a commits or turns chart.",
                )
                .build(window, cx)
            })
            .h(px(32.))
            .w_full()
            .flex()
            .items_end()
            .gap(px(4.))
            .children(stats.daily.iter().enumerate().map(|(day, count)| {
                div()
                    .id(("welcome-activity-day", day))
                    .flex_1()
                    .h(px(2. + 30. * *count as f32 / peak as f32))
                    .rounded(px(2.))
                    .bg(if *count == 0 {
                        p.raised
                    } else {
                        p.info.opacity(0.8)
                    })
                    .test_support()
            }))
            .test_support(),
    )
    .child(
        div()
            .flex()
            .justify_between()
            .child(ui::label("13 days ago", size::XS, p.text_subtle))
            .child(ui::label("Today", size::XS, p.text_subtle)),
    )
    .test_support();

    let workspace = app.session();
    let workspace_card = card(
        "welcome-workspace-overview",
        IconName::Folder,
        "Your workspace",
        None,
    )
    .child(
        div()
            .min_w_0()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(ui::icon(
                if workspace.general {
                    IconName::Sparkles
                } else {
                    IconName::FolderOpen
                },
                15.,
                p.info,
            ))
            .child(ui::label(workspace.workspace_label(), size::SM, p.text).truncate()),
    )
    .child(ui::label(
        if workspace.general {
            "A private folder for the things you create."
        } else {
            "Files and tools use this project's folder."
        },
        size::XS,
        p.text_subtle,
    ))
    .child(
        div()
            .flex()
            .flex_wrap()
            .gap(px(12.))
            .child(ui::label(
                format!("{} turns finished", stats.finished_turns),
                size::XS,
                p.text_subtle,
            ))
            .child(ui::label(
                format!("{} queued", stats.queued),
                size::XS,
                p.text_subtle,
            )),
    )
    .child(
        div()
            .id("welcome-terminals")
            .role(gpui_kit::Role::Button)
            .aria_label("Open terminal dock")
            .tab_index(0)
            .focus_visible(|style| style.border_1().border_color(p.info))
            .cursor_pointer()
            .flex()
            .items_center()
            .gap(px(6.))
            .on_click(cx.listener(|app, _, window, cx| {
                if !app.terminal.open {
                    app.toggle_terminal(window, cx);
                }
            }))
            .child(ui::icon(IconName::SquareTerminal, 13., p.text_subtle))
            .child(ui::label(
                if app.terminal.tabs.is_empty() {
                    "Open a terminal".into()
                } else {
                    format!(
                        "{} terminal{} available",
                        app.terminal.tabs.len(),
                        if app.terminal.tabs.len() == 1 {
                            ""
                        } else {
                            "s"
                        }
                    )
                },
                size::XS,
                p.text_muted,
            ))
            .test_support(),
    )
    .test_support();

    div()
        .id("welcome-dashboard")
        .w_full()
        .flex()
        .flex_col()
        .gap(px(12.))
        .child(
            div()
                .w_full()
                .flex()
                .when(columns == 1, |row| row.flex_col())
                .gap(px(12.))
                .child(div().flex_1().min_w_0().child(working_card))
                .child(div().flex_1().min_w_0().child(recent_card)),
        )
        .child(
            div()
                .w_full()
                .flex()
                .when(columns == 1, |row| row.flex_col())
                .gap(px(12.))
                .child(div().flex_1().min_w_0().child(activity_card))
                .child(div().flex_1().min_w_0().child(workspace_card)),
        )
        .test_support()
}

#[cfg(test)]
mod tests {
    use super::{Activity, activity, recent_sessions};
    use crate::prompt_queue::Prompt;
    use crate::session::Session;
    use flint_agent::{AgentEvent, TurnEndReason};
    use std::time::{Duration, UNIX_EPOCH};

    #[test]
    fn recent_selection_matches_stable_sort_and_retains_the_total() {
        for count in [0, 1, 2, 3, 50] {
            let mut sessions: Vec<_> = (0..count)
                .map(|uid| {
                    let mut session = Session::new(uid, "/fixture".into());
                    session.touched = UNIX_EPOCH + Duration::from_secs((uid * 7) % 5);
                    if uid % 4 != 0 {
                        session.view.push_user(format!("Conversation {uid}"));
                    }
                    session
                })
                .collect();
            let mut reference: Vec<_> = sessions
                .iter()
                .filter(|session| !session.view.items.is_empty())
                .collect();
            reference.sort_by_key(|session| std::cmp::Reverse(session.touched));
            let (total, actual) = recent_sessions(&sessions);
            assert_eq!(total, reference.len());
            assert_eq!(
                actual.iter().map(|session| session.uid).collect::<Vec<_>>(),
                reference
                    .iter()
                    .take(2)
                    .map(|session| session.uid)
                    .collect::<Vec<_>>()
            );
            sessions.clear();
            assert_eq!(recent_sessions(&sessions).0, 0);
        }
    }

    #[test]
    fn activity_is_real_bounded_and_ignores_empty_or_future_conversations() {
        let now = UNIX_EPOCH + Duration::from_secs(20 * 86_400);
        let mut session = Session::new(1, "/fixture".into());
        session.touched = now;
        assert_eq!(activity(&[session], now), Activity::default());
        let mut sessions: Vec<_> = [0, 3, 13, 14]
            .into_iter()
            .enumerate()
            .map(|(ix, age)| {
                let mut session = Session::new(ix as u64, "/fixture".into());
                session.view.push_user("fixture".into());
                session.touched = now - Duration::from_secs(age * 86_400);
                session
            })
            .collect();
        let mut future = Session::new(99, "/fixture".into());
        future.view.push_user("future".into());
        future.touched = now + Duration::from_secs(86_400);
        sessions.push(future);
        let stats = activity(&sessions, now);
        assert_eq!(stats.daily.iter().sum::<usize>(), 3);
        assert_eq!(stats.daily[13], 1);
        assert_eq!(stats.daily[10], 1);
        assert_eq!(stats.daily[0], 1);
        assert_eq!(stats.finished_turns, 0);
    }

    #[test]
    fn activity_counts_finished_turns_and_real_pending_prompts_not_in_progress_work() {
        let now = UNIX_EPOCH + Duration::from_secs(20 * 86_400);
        let mut finished = Session::new(1, "/fixture".into());
        finished.view.push_user("Finished fixture".into());
        finished
            .view
            .fold(AgentEvent::TurnStarted { turn_id: 1 }, Duration::ZERO);
        finished.view.fold(
            AgentEvent::TurnFinished {
                turn_id: 1,
                reason: TurnEndReason::Completed,
            },
            Duration::ZERO,
        );
        finished
            .view
            .fold(AgentEvent::TurnStarted { turn_id: 2 }, Duration::ZERO);
        finished.touched = now;
        let mut queued = Session::new(2, "/fixture".into());
        queued.prompt_queue.paused = true;
        for text in ["First", "Second"] {
            queued
                .prompt_queue
                .items
                .push_back(Prompt::new(text.into(), text.into(), vec![]));
        }
        let mut expected = Activity {
            finished_turns: 1,
            queued: 2,
            ..Activity::default()
        };
        expected.daily[13] = 1;
        assert_eq!(activity(&[finished, queued], now), expected);
    }
}
