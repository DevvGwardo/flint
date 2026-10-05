//! Blueprint UI tests: one test per user-facing promise, driving the real
//! FlintApp views in a headless window with scripted engine events (channels
//! attached through `FlintApp::attach_engine`, no model involved).
//! `tools/blueprint/interact.py` turns each test into a report check.

use flint_agent::AgentEvent;
use flint_agent::ApprovalDecision;
use flint_agent::ApprovalMode;
use flint_agent::FileDiff;
use flint_agent::Op;
use flint_agent::SessionHandle;
use flint_agent::ToolKind;
use flint_agent::TurnEndReason;
use flint_app::app::FlintApp;
use flint_app::app::Options;
use flint_app::settings::KeySources;
use flint_app::view_model::Item;
use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    AnyWindowHandle, App, AppContext as _, Bounds, ElementId, Entity, Focusable as _, Point,
    ScrollDelta, TestAppContext, Window, WindowBounds, WindowOptions, point, px, size,
};
use serde_json::json;
#[path = "blueprint_ui/command_preview.rs"]
mod command_preview_ui;
#[path = "blueprint_ui/composer_refresh.rs"]
mod composer_refresh_ui;
#[path = "blueprint_ui/event_logging.rs"]
mod event_logging_ui;
#[path = "blueprint_ui/failure_status.rs"]
mod failure_status_ui;
#[path = "blueprint_ui/file_preview.rs"]
mod file_preview_ui;
#[path = "blueprint_ui/general.rs"]
mod general_ui;
mod local_provider;
#[path = "blueprint_ui/menu_overflow.rs"]
mod menu_overflow_ui;
#[path = "blueprint_ui/prompt_queue.rs"]
mod prompt_queue_ui;
#[path = "blueprint_ui/selection_retention.rs"]
mod selection_retention_ui;
#[path = "blueprint_ui/sidebar.rs"]
mod sidebar_ui;
#[path = "blueprint_ui/subagents.rs"]
mod subagent_ui;
#[path = "blueprint_ui/terminal_snapshot.rs"]
mod terminal_snapshot_ui;
#[path = "blueprint_ui/tool_detail.rs"]
mod tool_detail_ui;
#[path = "blueprint_ui/unicode_mention.rs"]
mod unicode_mention_ui;
#[path = "blueprint_ui/worktrees.rs"]
mod worktree_ui;

fn session_pane_fixture(cx: &mut TestAppContext, count: usize) -> (Ui, Vec<(u64, Engine)>) {
    let ui = open_with(cx, terminal_options());
    let mut sessions = Vec::new();
    for ix in 0..count {
        if ix > 0 {
            ui.press(cx, "cmd-n");
        }
        let engine = ui.engine(cx);
        ui.input(cx, &format!("Pane {ix}"));
        ui.press(cx, "enter");
        engine.sent();
        engine.send(
            cx,
            AgentEvent::TerminalStarted {
                terminal_id: "same-id".into(),
                call_id: None,
                label: format!("Terminal {ix}"),
                cwd: None,
            },
        );
        sessions.push((ui.read(cx, |app, _| app.session().uid), engine));
    }
    ui.with(cx, |window, cx| {
        ui.app
            .update(cx, |app, cx| app.select_session(0, window, cx))
    });
    (ui, sessions)
}

fn drag_session_pane(
    ui: &Ui,
    cx: &mut TestAppContext,
    uid: u64,
    target: u64,
    edge: flint_app::docking::Edge,
) {
    use flint_app::docking::Edge;
    let ix = ui.read(cx, |app, _| {
        app.sessions
            .iter()
            .position(|session| session.uid == uid)
            .unwrap()
    });
    ui.with(cx, |window, cx| {
        window.render_frame(cx);
        let from = window.find(("session", ix)).bounds().center();
        let bounds = window
            .try_find(ElementId::Name(format!("session-pane-{target}").into()))
            .map(|element| element.bounds())
            .unwrap_or_else(|| {
                window
                    .find(ElementId::Name("dock-panel-chat".into()))
                    .bounds()
            });
        let center = bounds.center();
        let to = match edge {
            Edge::Left => point(bounds.left() + px(12.), center.y),
            Edge::Right => point(bounds.right() - px(12.), center.y),
            Edge::Top => point(center.x, bounds.top() + px(12.)),
            Edge::Bottom => point(center.x, bounds.bottom() - px(12.)),
        };
        window.drag(from, to, cx);
    });
    settle(cx);
}

fn session_pane_bounds(ui: &Ui, cx: &mut TestAppContext, uid: u64) -> Bounds<gpui_kit::Pixels> {
    ui.with(cx, |window, cx| {
        window.render_frame(cx);
        window
            .find(ElementId::Name(format!("session-pane-{uid}").into()))
            .bounds()
    })
}

#[gpui_kit::test]
fn session_panes_drag_actual_sessions_and_keep_two_terminals_live(cx: &mut TestAppContext) {
    use flint_app::docking::Edge;
    let (ui, sessions) = session_pane_fixture(cx, 2);
    let ids = ui.read(cx, |app, _| {
        app.terminal
            .tabs
            .iter()
            .map(Entity::entity_id)
            .collect::<Vec<_>>()
    });
    drag_session_pane(&ui, cx, sessions[1].0, sessions[0].0, Edge::Right);
    assert!(has(&ui, cx, "session-workspace"));
    assert!(!has(&ui, cx, "dock-panel-terminal"));
    for (uid, engine) in &sessions {
        assert!(has(&ui, cx, &format!("session-pane-{uid}")));
        ui.click(
            cx,
            ElementId::Name(format!("session-pane-mode-{uid}-terminal").into()),
        );
        engine.send(
            cx,
            AgentEvent::TerminalOutput {
                terminal_id: "same-id".into(),
                data: format!("output from {uid}\n"),
                replace: false,
            },
        );
    }
    let left = session_pane_bounds(&ui, cx, sessions[0].0);
    let right = session_pane_bounds(&ui, cx, sessions[1].0);
    assert!(left.right() <= right.left() + px(2.));
    assert_eq!(
        ui.read(cx, |app, _| app
            .terminal
            .tabs
            .iter()
            .map(Entity::entity_id)
            .collect::<Vec<_>>()),
        ids
    );
    for (uid, _) in &sessions {
        let text = ui.read(cx, |app, cx| {
            app.pane_terminal(*uid)
                .unwrap()
                .read(cx)
                .terminal
                .buffer_text()
        });
        assert!(text.contains(&format!("output from {uid}")), "{text:?}");
        assert_eq!(
            ui.read(cx, |app, cx| app
                .pane_terminal(*uid)
                .unwrap()
                .read(cx)
                .session_uid),
            Some(*uid)
        );
    }
}

#[gpui_kit::test]
fn session_panes_split_without_spawning_shells(cx: &mut TestAppContext) {
    use flint_app::docking::Edge;
    let ui = open_with(cx, terminal_options());
    let mut uids = Vec::new();
    for ix in 0..2 {
        if ix > 0 {
            ui.press(cx, "cmd-n");
        }
        let engine = ui.engine(cx);
        ui.input(cx, &format!("Pane {ix}"));
        ui.press(cx, "enter");
        engine.sent();
        uids.push(ui.read(cx, |app, _| app.session().uid));
    }
    ui.with(cx, |window, cx| {
        ui.app
            .update(cx, |app, cx| app.select_session(0, window, cx))
    });
    let tabs = ui.read(cx, |app, _| app.terminal.tabs.len());
    drag_session_pane(&ui, cx, uids[1], uids[0], Edge::Right);
    assert!(has(&ui, cx, "session-workspace"));
    assert_eq!(ui.read(cx, |app, _| app.terminal.tabs.len()), tabs);
    for uid in &uids {
        // An empty terminal half collapses to a strip that opens one on demand.
        assert!(has(&ui, cx, &format!("session-terminal-collapsed-{uid}")));
        assert!(!has(&ui, cx, &format!("session-terminal-{uid}")));
    }
}

#[gpui_kit::test]
fn session_panes_keep_agent_command_terminals_collapsed_until_asked(cx: &mut TestAppContext) {
    use flint_app::docking::Edge;
    let (ui, sessions) = session_pane_fixture(cx, 2);
    drag_session_pane(&ui, cx, sessions[1].0, sessions[0].0, Edge::Right);
    let started = AgentEvent::TerminalStarted {
        terminal_id: "later-command".into(),
        call_id: None,
        label: "later-command".into(),
        cwd: None,
    };
    sessions[0].1.send(cx, started);
    for (uid, _) in &sessions {
        assert!(ui.read(cx, |app, _| app.pane_terminal(*uid).is_some()));
        assert!(has(&ui, cx, &format!("session-terminal-collapsed-{uid}")));
        assert!(!has(&ui, cx, &format!("session-terminal-{uid}")));
    }
    let uid = sessions[0].0;
    ui.click(
        cx,
        ElementId::Name(format!("session-terminal-collapsed-{uid}").into()),
    );
    assert!(has(&ui, cx, &format!("session-terminal-{uid}")));
    assert!(!has(
        &ui,
        cx,
        &format!("session-terminal-{}", sessions[1].0)
    ));
}

#[gpui_kit::test]
fn session_panes_arrange_four_sessions_into_an_even_grid(cx: &mut TestAppContext) {
    use flint_app::docking::Edge;
    let (ui, sessions) = session_pane_fixture(cx, 4);
    for ix in 1..4 {
        drag_session_pane(&ui, cx, sessions[ix].0, sessions[ix - 1].0, Edge::Right);
    }
    for (uid, _) in &sessions {
        ui.click(
            cx,
            ElementId::Name(format!("session-pane-mode-{uid}-terminal").into()),
        );
    }
    ui.click(cx, "session-grid");
    let panes: Vec<_> = sessions
        .iter()
        .map(|(uid, _)| session_pane_bounds(&ui, cx, *uid))
        .collect();
    assert!(panes[0].right() <= panes[1].left() + px(2.));
    assert!(panes[0].bottom() <= panes[2].top() + px(2.));
    assert!(panes[2].right() <= panes[3].left() + px(2.));
    assert!(
        (panes[0].size.width - panes[1].size.width).abs() < px(4.),
        "{panes:?}"
    );
    assert!(
        (panes[0].size.height - panes[2].size.height).abs() < px(4.),
        "{panes:?}"
    );
    let chat = panel_bounds(&ui, cx, flint_app::docking::Panel::Chat);
    for pane in panes {
        assert!(pane.left() >= chat.left() && pane.right() <= chat.right());
        assert!(
            pane.top() >= chat.top() && pane.bottom() <= chat.bottom(),
            "{pane:?} outside {chat:?}"
        );
    }
}

#[gpui_kit::test]
fn session_panes_keep_drafts_and_route_messages_to_the_focused_engine(cx: &mut TestAppContext) {
    use flint_app::docking::Edge;
    let (ui, sessions) = session_pane_fixture(cx, 2);
    ui.input(cx, "first draft");
    let first_composer = ui.read(cx, |app, _| app.composer.entity_id());
    drag_session_pane(&ui, cx, sessions[1].0, sessions[0].0, Edge::Right);
    assert_eq!(ui.composer_text(cx), "");
    ui.input(cx, "second draft");
    let second_composer = ui.read(cx, |app, _| app.composer.entity_id());
    assert_ne!(first_composer, second_composer);
    ui.click(
        cx,
        ElementId::Name(format!("session-pane-compose-{}", sessions[0].0).into()),
    );
    assert_eq!(ui.composer_text(cx), "first draft");
    assert_eq!(
        ui.read(cx, |app, _| app.composer.entity_id()),
        first_composer
    );
    ui.press(cx, "enter");
    assert!(sessions[0].1.sent().is_empty());
    assert_eq!(
        ui.read(cx, |app, _| app.sessions[0].prompt_queue.items[0]
            .text
            .clone()),
        "first draft"
    );
    sessions[0]
        .1
        .send(cx, AgentEvent::TurnStarted { turn_id: 1 });
    sessions[0].1.send(
        cx,
        AgentEvent::TurnFinished {
            turn_id: 1,
            reason: TurnEndReason::Completed,
        },
    );
    let sent = sessions[0].1.sent();
    assert!(
        matches!(sent.as_slice(), [Op::UserMessage(text)] if text == "first draft"),
        "sent {sent:?}; state {:?}",
        ui.read(cx, |app, cx| (
            app.session().uid,
            app.composer.read(cx).value().to_string(),
            app.session().view.running,
            app.store_error.clone()
        ))
    );
    assert!(sessions[1].1.sent().is_empty());
    ui.click(
        cx,
        ElementId::Name(format!("session-pane-compose-{}", sessions[1].0).into()),
    );
    assert_eq!(ui.composer_text(cx), "second draft");
    assert_eq!(
        ui.read(cx, |app, _| app.composer.entity_id()),
        second_composer
    );
}

#[gpui_kit::test]
fn compact_eight_pane_composer_keeps_lower_options_reachable(cx: &mut TestAppContext) {
    use flint_app::docking::Edge;
    use flint_app::session_workspace::{Node, PaneMode};
    fn equal_splits(node: &mut Node<u64>) {
        if let Node::Split {
            sizes,
            first,
            second,
            ..
        } = node
        {
            *sizes = [None, None];
            equal_splits(first);
            equal_splits(second);
        }
    }
    let (ui, sessions) = session_pane_fixture(cx, 8);
    for (_, engine) in &sessions {
        engine.send(
            cx,
            AgentEvent::TurnFinished {
                turn_id: 1,
                reason: TurnEndReason::Completed,
            },
        );
    }
    ui.with(cx, |window, cx| {
        ui.app.update(cx, |app, cx| {
            for ix in 1..sessions.len() {
                assert!(app.split_session_pane(
                    sessions[ix].0,
                    sessions[ix - 1].0,
                    Edge::Right,
                    window,
                    cx,
                ));
            }
            for (uid, _) in &sessions {
                app.set_session_pane_mode(*uid, PaneMode::Chat, window, cx);
            }
            app.arrange_session_grid(cx);
            // An accepted restored layout can contain equal nested splits,
            // yielding a compact upper-left tile instead of uniform cells.
            equal_splits(app.session_workspace.layout.root.as_mut().unwrap());
            app.session_workspace.revision += 1;
            app.effort_supported = true;
            app.focus_session_pane(sessions[0].0, true, window, cx);
        });
    });
    settle(cx);
    ui.with(cx, |window, cx| {
        for _ in 0..3 {
            window.render_frame(cx);
        }
        window.scroll(
            "composer-options",
            ScrollDelta::Pixels(point(px(0.), px(-200.))),
            cx,
        );
    });
    settle(cx);
    ui.with(cx, |window, cx| {
        window.render_frame(cx);
        let card = ui.app.read(cx).popover_room.anchor.get().unwrap();
        let option = window.find("approval-hint").bounds();
        let send = window.find("send").bounds();
        let body = window.find("composer-body").bounds();
        assert!(
            body.size.height >= px(44.),
            "the input is crushed by wrapped controls: {body:?}"
        );
        assert!(
            option.top() >= card.top() && option.bottom() <= card.bottom(),
            "lower option remains clipped: {option:?} outside {card:?}"
        );
        assert!(send.top() >= card.top() && send.bottom() <= card.bottom());
        let uid = ui.app.read(cx).session().uid;
        let pane = window
            .find(ElementId::Name(format!("session-pane-{uid}").into()))
            .bounds();
        assert!(card.top() >= pane.top() && card.bottom() <= pane.bottom());
    });
}

#[gpui_kit::test]
fn one_two_four_and_eight_sessions_keep_concurrent_chat_and_terminal_output_separate(
    cx: &mut TestAppContext,
) {
    use flint_app::docking::Edge;
    for count in [1, 2, 4, 8] {
        let (ui, sessions) = session_pane_fixture(cx, count);
        for ix in 1..count {
            ui.with(cx, |window, cx| {
                ui.app.update(cx, |app, cx| {
                    assert!(app.split_session_pane(
                        sessions[ix].0,
                        sessions[ix - 1].0,
                        Edge::Right,
                        window,
                        cx,
                    ));
                });
            });
        }
        if count > 1 {
            ui.click(cx, "session-grid");
        }
        for (uid, engine) in &sessions {
            engine
                .events
                .try_send(AgentEvent::TurnStarted { turn_id: 1 })
                .unwrap();
            for _ in 0..40 {
                for event in [
                    AgentEvent::TextDelta(format!("chat-{uid} 界 ")),
                    AgentEvent::TerminalOutput {
                        terminal_id: "same-id".into(),
                        data: format!("terminal-{uid} 🙂\r\n"),
                        replace: false,
                    },
                ] {
                    engine.events.try_send(event).unwrap();
                }
            }
            engine
                .events
                .try_send(AgentEvent::TurnFinished {
                    turn_id: 1,
                    reason: TurnEndReason::Completed,
                })
                .unwrap();
        }
        cx.run_until_parked();
        ui.press(cx, "cmd-l");
        ui.input(cx, "independent active draft");
        settle(cx);
        assert_eq!(
            ui.composer_text(cx),
            "independent active draft",
            "{count} panes"
        );
        for (uid, engine) in &sessions {
            assert!(engine.sent().is_empty());
            assert!(ui.read(cx, |app, cx| {
                let session = app
                    .sessions
                    .iter()
                    .find(|session| session.uid == *uid)
                    .unwrap();
                let chat = format!("chat-{uid} 界 ").repeat(40);
                assert!(!session.view.running);
                assert!(
                    session
                        .view
                        .items
                        .iter()
                        .any(|item| matches!(item, Item::Assistant { text, .. } if text == &chat),)
                );
                let terminal = app
                    .terminal
                    .tabs
                    .iter()
                    .find(|view| view.read(cx).session_uid == Some(*uid))
                    .unwrap()
                    .read(cx)
                    .terminal
                    .buffer_text();
                terminal.contains(&format!("terminal-{uid} 🙂"))
                    && sessions.iter().all(|(other, _)| {
                        *other == *uid || !terminal.contains(&format!("terminal-{other} 🙂"))
                    })
            }));
            if count > 1 {
                let bounds = session_pane_bounds(&ui, cx, *uid);
                assert!(bounds.size.width > px(100.) && bounds.size.height > px(100.));
            }
        }
    }
}

#[gpui_kit::test]
fn session_panes_close_without_stopping_sessions_and_preserve_the_remaining_draft(
    cx: &mut TestAppContext,
) {
    use flint_app::docking::Edge;
    let (ui, sessions) = session_pane_fixture(cx, 2);
    ui.input(cx, "keep first draft");
    drag_session_pane(&ui, cx, sessions[1].0, sessions[0].0, Edge::Right);
    let terminal_ids = ui.read(cx, |app, _| {
        app.terminal
            .tabs
            .iter()
            .map(Entity::entity_id)
            .collect::<Vec<_>>()
    });
    ui.click(
        cx,
        ElementId::Name(format!("session-pane-close-{}", sessions[1].0).into()),
    );
    assert!(!has(&ui, cx, "session-workspace"));
    assert_eq!(ui.read(cx, |app, _| app.session().uid), sessions[0].0);
    assert_eq!(ui.composer_text(cx), "keep first draft");
    assert_eq!(ui.read(cx, |app, _| app.sessions.len()), 2);
    assert_eq!(
        ui.read(cx, |app, _| app
            .terminal
            .tabs
            .iter()
            .map(Entity::entity_id)
            .collect::<Vec<_>>()),
        terminal_ids
    );
    for (_, engine) in sessions {
        assert!(
            engine.sent().is_empty(),
            "closing a pane sent an engine operation"
        );
    }
}

#[gpui_kit::test]
fn session_panes_restore_saved_session_ids_modes_and_focus(cx: &mut TestAppContext) {
    use flint_app::docking::Edge;
    let (ui, sessions) = session_pane_fixture(cx, 2);
    let home = ui.read(cx, |app, _| app.home.clone());
    drag_session_pane(&ui, cx, sessions[1].0, sessions[0].0, Edge::Right);
    ui.click(
        cx,
        ElementId::Name(format!("session-pane-mode-{}-terminal", sessions[1].0).into()),
    );
    let saved: serde_json::Value =
        serde_json::from_slice(&std::fs::read(home.join("session-layout.json")).unwrap()).unwrap();
    assert_eq!(saved["version"], 1);
    let active_id = saved["active"].as_str().unwrap();
    let options = Options {
        home: Some(home),
        ..terminal_options()
    };
    let restored = open_with(cx, options);
    assert!(has(&restored, cx, "session-workspace"));
    let restored_uid = restored.read(cx, |app, _| {
        assert_eq!(
            app.session()
                .dir
                .as_ref()
                .unwrap()
                .file_name()
                .unwrap()
                .to_str()
                .unwrap(),
            active_id
        );
        app.session().uid
    });
    assert!(has(
        &restored,
        cx,
        &format!("session-terminal-{restored_uid}")
    ));
    assert!(!has(
        &restored,
        cx,
        &format!("session-transcript-{restored_uid}")
    ));
}

#[gpui_kit::test]
fn session_panes_resize_dividers_and_restore_their_proportions(cx: &mut TestAppContext) {
    use flint_app::docking::Edge;
    let (ui, sessions) = session_pane_fixture(cx, 2);
    let home = ui.read(cx, |app, _| app.home.clone());
    drag_session_pane(&ui, cx, sessions[1].0, sessions[0].0, Edge::Right);
    let before = session_pane_bounds(&ui, cx, sessions[0].0);
    ui.with(cx, |window, cx| {
        let from = point(before.right() + px(1.), before.center().y);
        window.drag(from, from + point(px(110.), px(0.)), cx);
    });
    settle(cx);
    let after = session_pane_bounds(&ui, cx, sessions[0].0);
    assert!(
        after.size.width > before.size.width + px(80.),
        "{before:?} -> {after:?}"
    );
    let saved: serde_json::Value =
        serde_json::from_slice(&std::fs::read(home.join("session-layout.json")).unwrap()).unwrap();
    assert!(
        saved["root"]["sizes"][0].as_f64().unwrap() > saved["root"]["sizes"][1].as_f64().unwrap()
    );
    let again = open_with(
        cx,
        Options {
            home: Some(home),
            ..terminal_options()
        },
    );
    let first = again.read(cx, |app, _| {
        app.sessions
            .iter()
            .find(|session| session.title() == "Pane 0")
            .unwrap()
            .uid
    });
    let restored = session_pane_bounds(&again, cx, first);
    assert!(
        (restored.size.width - after.size.width).abs() <= px(3.),
        "{after:?} -> {restored:?}"
    );
}

#[gpui_kit::test]
fn session_panes_sidebar_selection_replaces_only_the_focused_pane(cx: &mut TestAppContext) {
    use flint_app::docking::Edge;
    let (ui, sessions) = session_pane_fixture(cx, 3);
    drag_session_pane(&ui, cx, sessions[1].0, sessions[0].0, Edge::Right);
    ui.click(cx, ("session", 2usize));
    assert_eq!(ui.read(cx, |app, _| app.session().uid), sessions[2].0);
    assert!(has(&ui, cx, &format!("session-pane-{}", sessions[0].0)));
    assert!(has(&ui, cx, &format!("session-pane-{}", sessions[2].0)));
    assert!(!has(&ui, cx, &format!("session-pane-{}", sessions[1].0)));
    assert!(ui.read(cx, |app, _| app.pane_terminal(sessions[2].0).is_some()));
    ui.click(cx, ("session", 0usize));
    assert!(has(&ui, cx, &format!("session-pane-{}", sessions[2].0)));
    assert_eq!(ui.read(cx, |app, _| app.session().uid), sessions[0].0);
}

#[gpui_kit::test]
fn session_panes_escape_cancels_sidebar_drag_without_interrupting_work(cx: &mut TestAppContext) {
    use gpui_kit::{InputEvent as _, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent};
    let (ui, sessions) = session_pane_fixture(cx, 2);
    sessions[0]
        .1
        .send(cx, AgentEvent::TurnStarted { turn_id: 1 });
    ui.with(cx, |window, cx| {
        window.render_frame(cx);
        let from = window.find(("session", 1usize)).bounds().center();
        window.dispatch_event(
            MouseDownEvent {
                button: MouseButton::Left,
                position: from,
                modifiers: Default::default(),
                click_count: 1,
                first_mouse: false,
            }
            .to_platform_input(),
            cx,
        );
        window.render_frame(cx);
        let to = from + point(px(30.), px(30.));
        window.dispatch_event(
            MouseMoveEvent {
                position: to,
                pressed_button: Some(MouseButton::Left),
                modifiers: Default::default(),
            }
            .to_platform_input(),
            cx,
        );
        window.render_frame(cx);
        assert!(cx.has_active_drag());
        assert!(
            window
                .try_find(ElementId::Name(
                    format!("session-drop-{}-right", sessions[0].0).into()
                ))
                .is_some()
        );
        window.press("escape", cx);
        assert!(!cx.has_active_drag());
        window.dispatch_event(
            MouseUpEvent {
                button: MouseButton::Left,
                position: to,
                modifiers: Default::default(),
                click_count: 1,
            }
            .to_platform_input(),
            cx,
        );
    });
    settle(cx);
    assert!(!has(&ui, cx, "session-workspace"));
    assert!(ui.read(cx, |app, _| app.session().view.running));
    assert!(sessions[0].1.sent().is_empty());
}

#[gpui_kit::test]
fn session_panes_archiving_prunes_the_removed_pane(cx: &mut TestAppContext) {
    use flint_app::docking::Edge;
    let (ui, sessions) = session_pane_fixture(cx, 2);
    drag_session_pane(&ui, cx, sessions[1].0, sessions[0].0, Edge::Right);
    ui.with(cx, |window, cx| {
        ui.app
            .update(cx, |app, cx| app.delete_session(1, window, cx))
    });
    assert!(matches!(sessions[1].1.sent().as_slice(), [Op::Shutdown]));
    sessions[1].1.send(
        cx,
        AgentEvent::SessionStopped {
            history_saved: true,
        },
    );
    sessions[1].1.events.close();
    settle(cx);
    ui.with(cx, |window, cx| {
        ui.app
            .update(cx, |app, cx| app.delete_session(1, window, cx))
    });
    settle(cx);
    assert!(!has(&ui, cx, "session-workspace"));
    assert_eq!(ui.read(cx, |app, _| app.sessions.len()), 1);
    assert_eq!(ui.read(cx, |app, _| app.session().uid), sessions[0].0);
    assert!(ui.read(cx, |app, _| app.pane_terminal(sessions[1].0).is_none()));
    assert!(sessions[0].1.sent().is_empty());
}

#[gpui_kit::test]
fn session_panes_can_move_an_existing_session_to_a_vertical_split(cx: &mut TestAppContext) {
    use flint_app::docking::Edge;
    let (ui, sessions) = session_pane_fixture(cx, 2);
    let ids = ui.read(cx, |app, _| {
        app.terminal
            .tabs
            .iter()
            .map(Entity::entity_id)
            .collect::<Vec<_>>()
    });
    drag_session_pane(&ui, cx, sessions[1].0, sessions[0].0, Edge::Right);
    drag_session_pane(&ui, cx, sessions[1].0, sessions[0].0, Edge::Bottom);
    let top = session_pane_bounds(&ui, cx, sessions[0].0);
    let bottom = session_pane_bounds(&ui, cx, sessions[1].0);
    assert!(top.bottom() <= bottom.top() + px(2.));
    assert!((top.size.width - bottom.size.width).abs() < px(3.));
    assert_eq!(
        ui.read(cx, |app, _| app
            .terminal
            .tabs
            .iter()
            .map(Entity::entity_id)
            .collect::<Vec<_>>()),
        ids
    );
}

#[gpui_kit::test]
fn session_panes_closing_a_terminal_does_not_focus_another_sessions_terminal(
    cx: &mut TestAppContext,
) {
    use flint_app::docking::Edge;
    let (ui, sessions) = session_pane_fixture(cx, 2);
    drag_session_pane(&ui, cx, sessions[1].0, sessions[0].0, Edge::Right);
    ui.click(
        cx,
        ElementId::Name(format!("session-terminal-collapsed-{}", sessions[0].0).into()),
    );
    ui.click(
        cx,
        ElementId::Name(format!("session-terminal-close-{}-0", sessions[0].0).into()),
    );
    assert_eq!(ui.read(cx, |app, _| app.session().uid), sessions[0].0);
    assert!(ui.read(cx, |app, _| app.pane_terminal(sessions[0].0).is_none()));
    assert!(ui.read(cx, |app, _| app.pane_terminal(sessions[1].0).is_some()));
}

#[gpui_kit::test]
fn session_panes_project_switch_replaces_the_focused_pane_and_keeps_its_draft(
    cx: &mut TestAppContext,
) {
    use flint_app::docking::Edge;
    let (ui, sessions) = session_pane_fixture(cx, 2);
    drag_session_pane(&ui, cx, sessions[1].0, sessions[0].0, Edge::Right);
    ui.input(cx, "keep second project's draft");
    let workspace = temp_dir("pane-project");
    ui.app
        .update(cx, |app, cx| app.set_project_folder(workspace, cx));
    let new = ui.read(cx, |app, _| app.session().uid);
    assert!(has(&ui, cx, &format!("session-pane-{new}")));
    assert!(has(&ui, cx, &format!("session-pane-{}", sessions[0].0)));
    assert!(!has(&ui, cx, &format!("session-pane-{}", sessions[1].0)));
    assert_eq!(ui.composer_text(cx), "");
    ui.click(cx, ("session", 1usize));
    assert_eq!(ui.composer_text(cx), "keep second project's draft");
}

#[gpui_kit::test]
fn session_panes_route_keyboard_input_to_two_independent_shells(cx: &mut TestAppContext) {
    use flint_app::docking::Edge;
    let (ui, sessions) = session_pane_fixture(cx, 2);
    drag_session_pane(&ui, cx, sessions[1].0, sessions[0].0, Edge::Right);
    for (uid, _) in &sessions {
        ui.click(
            cx,
            ElementId::Name(format!("session-pane-mode-{uid}-terminal").into()),
        );
        ui.click(
            cx,
            ElementId::Name(format!("session-terminal-new-{uid}").into()),
        );
        assert_eq!(ui.read(cx, |app, _| app.session().uid), *uid);
        ui.input(cx, &format!("printf 'shell-{uid}\\n'"));
        ui.press(cx, "enter");
        let output = wait_for_terminal(&ui, cx, |line| line == format!("shell-{uid}"));
        assert!(
            output.iter().any(|line| line == &format!("shell-{uid}")),
            "{output:?}"
        );
    }
    for (uid, engine) in sessions {
        let text = ui.read(cx, |app, cx| {
            let view = app.pane_terminal(uid).unwrap().read(cx);
            assert!(!view.read_only);
            view.terminal.buffer_text()
        });
        assert!(text.contains(&format!("shell-{uid}")), "{text:?}");
        assert!(engine.sent().is_empty(), "shell input reached the agent");
    }
}

#[gpui_kit::test]
fn session_panes_follow_agent_commands_without_overriding_a_selected_tab(cx: &mut TestAppContext) {
    use flint_app::docking::Edge;
    let (ui, sessions) = session_pane_fixture(cx, 2);
    drag_session_pane(&ui, cx, sessions[1].0, sessions[0].0, Edge::Right);
    let uid = sessions[0].0;
    let engine = &sessions[0].1;
    let started = |id: &str| AgentEvent::TerminalStarted {
        terminal_id: id.into(),
        call_id: None,
        label: id.into(),
        cwd: None,
    };
    engine.send(cx, started("second-command"));
    assert_eq!(
        ui.read(cx, |app, cx| app
            .pane_terminal(uid)
            .unwrap()
            .read(cx)
            .label()),
        "second-command"
    );
    ui.click(
        cx,
        ElementId::Name(format!("session-terminal-collapsed-{uid}").into()),
    );
    ui.click(
        cx,
        ElementId::Name(format!("session-terminal-tab-{uid}-0").into()),
    );
    engine.send(cx, started("third-command"));
    assert_eq!(
        ui.read(cx, |app, cx| app
            .pane_terminal(uid)
            .unwrap()
            .read(cx)
            .label()),
        "Terminal 0"
    );
    let selected = ui.read(cx, |app, _| app.pane_terminal(uid).unwrap().entity_id());
    ui.click(
        cx,
        ElementId::Name(format!("session-terminal-close-{uid}-2").into()),
    );
    assert_eq!(
        ui.read(cx, |app, _| app.pane_terminal(uid).unwrap().entity_id()),
        selected
    );
}

struct Ui {
    window: AnyWindowHandle,
    app: Entity<FlintApp>,
}

struct Engine {
    ops: async_channel::Receiver<Op>,
    events: async_channel::Sender<AgentEvent>,
}

impl Engine {
    /// Sends one event and lets the pump (which coalesces bursts on an 8 ms
    /// timer) deliver it.
    fn send(&self, cx: &mut TestAppContext, event: AgentEvent) {
        self.events.try_send(event).unwrap();
        settle(cx);
    }

    fn sent(&self) -> Vec<Op> {
        std::iter::from_fn(|| self.ops.try_recv().ok()).collect()
    }
}

/// Runs pending work and advances the test clock past the pump's batching
/// delay, twice, so queued events and their redraws all land.
fn settle(cx: &mut TestAppContext) {
    for _ in 0..2 {
        cx.run_until_parked();
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(20));
    }
    cx.run_until_parked();
}

/// A fresh, empty directory under the system temp dir.
fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "flint-ui-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Options isolated from the real machine: a temp workspace, settings home
/// and key file.
fn test_options() -> Options {
    let home = temp_dir("home");
    let key = home.join("api.key");
    std::fs::write(&key, "test-key").unwrap();
    Options {
        workspace: Some(temp_dir("ws")),
        home: Some(home),
        key_path: Some(key),
        // No environment keys and no ~/.fx fallback: independent of this machine.
        key_sources: Some(KeySources::none()),
        engine_poll: true,
        skip_permission_choice: true,
        ..Options::default()
    }
}

fn open(cx: &mut TestAppContext) -> Ui {
    open_with(cx, test_options())
}

fn open_with(cx: &mut TestAppContext, options: Options) -> Ui {
    cx.update(flint_app::init);
    let (window, app) = cx.update(|cx| {
        let bounds = Bounds {
            origin: Point::default(),
            size: options
                .window_size
                .map_or_else(|| size(px(1440.), px(900.)), |(w, h)| size(px(w), px(h))),
        };
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            cx,
            |window, cx| cx.new(|cx| FlintApp::new(options, window, cx)),
        )
        .unwrap()
    });
    cx.run_until_parked();
    Ui { window, app }
}

impl Ui {
    fn with<R>(&self, cx: &mut TestAppContext, f: impl FnOnce(&mut Window, &mut App) -> R) -> R {
        cx.update_window(self.window, |_, window, cx| f(window, cx))
            .unwrap()
    }

    fn press(&self, cx: &mut TestAppContext, key: &str) {
        self.with(cx, |window, cx| window.press(key, cx));
        cx.run_until_parked();
    }

    fn key_cycle(&self, cx: &mut TestAppContext, key: &str) {
        use gpui_kit::InputEvent as _;
        let keystroke = gpui_kit::Keystroke::parse(key).unwrap();
        self.with(cx, |window, cx| {
            window.dispatch_event(
                gpui_kit::KeyDownEvent {
                    keystroke: keystroke.clone(),
                    is_held: false,
                    prefer_character_input: false,
                }
                .to_platform_input(),
                cx,
            );
            window.render_frame(cx);
            window.dispatch_event(gpui_kit::KeyUpEvent { keystroke }.to_platform_input(), cx);
        });
        cx.run_until_parked();
    }

    fn input(&self, cx: &mut TestAppContext, text: &str) {
        self.with(cx, |window, cx| window.input(text, cx));
        cx.run_until_parked();
    }

    fn click(&self, cx: &mut TestAppContext, id: impl Into<ElementId>) {
        let id = id.into();
        self.with(cx, |window, cx| window.click(id, cx));
        cx.run_until_parked();
    }

    fn read<R>(&self, cx: &mut TestAppContext, f: impl FnOnce(&FlintApp, &App) -> R) -> R {
        cx.read(|cx| f(self.app.read(cx), cx))
    }

    /// Attaches scripted engine channels to the active session.
    fn engine(&self, cx: &mut TestAppContext) -> Engine {
        let (ops_tx, ops_rx) = async_channel::unbounded();
        let (events_tx, events_rx) = async_channel::unbounded();
        self.app.update(cx, |app, cx| {
            let ix = app.active;
            app.attach_engine(
                ix,
                SessionHandle {
                    ops: ops_tx,
                    events: events_rx,
                },
                cx,
            );
        });
        Engine {
            ops: ops_rx,
            events: events_tx,
        }
    }

    fn composer_text(&self, cx: &mut TestAppContext) -> String {
        self.read(cx, |app, cx| app.composer.read(cx).value().to_string())
    }

    fn scroll_settings(&self, cx: &mut TestAppContext) {
        self.with(cx, |window, cx| {
            window.scroll(
                "settings-content",
                ScrollDelta::Pixels(point(px(0.), px(-600.))),
                cx,
            )
        });
        cx.run_until_parked();
    }
}

fn tool_started(call_id: &str, kind: ToolKind, summary: &str) -> AgentEvent {
    AgentEvent::ToolCallStarted {
        call_id: call_id.into(),
        name: "tool".into(),
        kind,
        args: json!({}),
        summary: summary.into(),
    }
}

fn tool_finished(call_id: &str, output: &str, diff: Option<FileDiff>) -> AgentEvent {
    AgentEvent::ToolCallFinished {
        call_id: call_id.into(),
        output: output.into(),
        exit_code: Some(0),
        success: true,
        diff,
        duration_ms: 20,
    }
}

/// A complete turn that edits `src/a.rs` and answers.
fn finished_turn(ui: &Ui, cx: &mut TestAppContext, engine: &Engine) {
    ui.input(cx, "fix it");
    ui.press(cx, "enter");
    for event in [
        AgentEvent::TurnStarted { turn_id: 1 },
        AgentEvent::StepStarted {
            turn_id: 1,
            step: 1,
        },
        AgentEvent::ReasoningDelta("look".into()),
        tool_started("r1", ToolKind::Read, "src/a.rs"),
        tool_finished("r1", "fn a() {}", None),
        tool_started("e1", ToolKind::Edit, "src/a.rs"),
        tool_finished(
            "e1",
            "ok",
            Some(FileDiff {
                path: "src/a.rs".into(),
                unified: "@@ -1 +1 @@\n-fn a() {}\n+fn a() -> u8 { 1 }\n".into(),
                added: 1,
                removed: 1,
                created: false,
            }),
        ),
        AgentEvent::TextDelta("Fixed `a`.".into()),
        AgentEvent::TurnFinished {
            turn_id: 1,
            reason: TurnEndReason::Completed,
        },
    ] {
        engine.send(cx, event);
    }
}

#[gpui_kit::test]
fn enter_sends_the_message(cx: &mut TestAppContext) {
    let ui = open(cx);
    let engine = ui.engine(cx);
    ui.input(cx, "hello there");
    ui.press(cx, "enter");
    assert!(matches!(engine.sent().as_slice(), [Op::UserMessage(text)] if text == "hello there"));
    assert_eq!(ui.composer_text(cx), "");
    let first = ui.read(cx, |app, _| app.session().view.items.first().cloned());
    assert_eq!(first, Some(Item::User("hello there".into())));
}

#[gpui_kit::test]
fn shift_enter_adds_a_newline(cx: &mut TestAppContext) {
    let ui = open(cx);
    let engine = ui.engine(cx);
    ui.input(cx, "line one");
    ui.press(cx, "shift-enter");
    ui.input(cx, "line two");
    assert_eq!(ui.composer_text(cx), "line one\nline two");
    assert!(engine.sent().is_empty());
}

#[gpui_kit::test]
fn empty_input_does_not_send(cx: &mut TestAppContext) {
    let ui = open(cx);
    let engine = ui.engine(cx);
    ui.input(cx, "   ");
    ui.press(cx, "enter");
    assert!(engine.sent().is_empty());
    assert!(ui.read(cx, |app, _| app.session().view.items.is_empty()));
}

#[gpui_kit::test]
fn ordinary_session_switching_keeps_drafts_and_attachments_independent(cx: &mut TestAppContext) {
    let (ui, sessions) = session_pane_fixture(cx, 2);
    ui.input(cx, "first session's draft");
    ui.app
        .update(cx, |app, _| app.attachments.push("first.rs".into()));
    ui.with(cx, |window, cx| {
        ui.app
            .update(cx, |app, cx| app.select_session(1, window, cx));
    });
    assert_eq!(ui.composer_text(cx), "");
    assert!(ui.read(cx, |app, _| app.attachments.is_empty()));
    ui.input(cx, "second session's draft");
    ui.with(cx, |window, cx| {
        ui.app
            .update(cx, |app, cx| app.select_session(0, window, cx));
    });
    assert_eq!(ui.composer_text(cx), "first session's draft");
    assert_eq!(ui.read(cx, |app, _| app.attachments.clone()), ["first.rs"]);
    assert!(sessions.iter().all(|(_, engine)| engine.sent().is_empty()));
}

#[gpui_kit::test]
fn terminal_output_returns_only_to_its_own_session_outside_tiled_mode(cx: &mut TestAppContext) {
    let (ui, sessions) = session_pane_fixture(cx, 2);
    ui.app.update(cx, |app, cx| {
        let uid = sessions[0].0;
        let cwd = app.sessions[0].workspace.clone();
        let terminal = cx.new(|cx| {
            flint_app::term_view::TermView::new(
                flint_term::Terminal::detached(flint_term::Size {
                    cols: 80,
                    rows: 16,
                    cell_width: 8,
                    cell_height: 18,
                }),
                cwd,
                false,
                true,
                cx,
            )
        });
        terminal.update(cx, |view, _| {
            view.session_uid = Some(uid);
            view.terminal.feed(b"controlled first-session output\r\n");
        });
        app.terminal.tabs.push(terminal);
        app.terminal.active = app.terminal.tabs.len() - 1;
        app.terminal.open = true;
    });
    ui.with(cx, |window, cx| {
        ui.app
            .update(cx, |app, cx| app.select_session(1, window, cx));
    });
    ui.click(cx, "terminal-send");
    assert!(sessions[0].1.sent().is_empty());
    assert!(ui.read(cx, |app, _| {
        app.sessions[0].prompt_queue.items[0]
            .message
            .contains("controlled first-session output")
    }));
    sessions[0]
        .1
        .send(cx, AgentEvent::TurnStarted { turn_id: 1 });
    sessions[0].1.send(
        cx,
        AgentEvent::TurnFinished {
            turn_id: 1,
            reason: TurnEndReason::Completed,
        },
    );
    assert!(
        sessions[1].1.sent().is_empty(),
        "another session received terminal contents"
    );
    assert!(matches!(sessions[0].1.sent().as_slice(),
        [Op::UserMessage(message)] if message.contains("controlled first-session output")));
}

#[gpui_kit::test]
fn acp_option_changes_wait_for_ack_and_closed_channels_keep_confirmed_values(
    cx: &mut TestAppContext,
) {
    let (ui, engine) = acp_session(cx);
    ui.app
        .update(cx, |app, cx| app.set_session_option("model", "opus", cx));
    assert_eq!(
        ui.read(cx, |app, _| app.session_slots().model.unwrap().current),
        "default"
    );
    assert!(
        matches!(engine.sent().as_slice(), [Op::SetSessionOption { value, .. }] if value == "opus")
    );
    engine.ops.close();
    ui.app.update(cx, |app, cx| {
        app.set_session_option("mode", "acceptEdits", cx)
    });
    assert_eq!(
        ui.read(cx, |app, _| app.session_slots().mode.unwrap().current),
        "default"
    );
    assert!(ui.read(cx, |app, _| {
        app.store_error
            .as_ref()
            .is_some_and(|error| error.contains("setting"))
    }));
}

#[gpui_kit::test]
fn flooded_engine_yields_to_typing_between_ordered_batches(cx: &mut TestAppContext) {
    let ui = open(cx);
    let engine = ui.engine(cx);
    engine
        .events
        .try_send(AgentEvent::TurnStarted { turn_id: 1 })
        .unwrap();
    for _ in 0..600 {
        engine
            .events
            .try_send(AgentEvent::TextDelta("界".into()))
            .unwrap();
    }
    engine
        .events
        .try_send(AgentEvent::TurnFinished {
            turn_id: 1,
            reason: TurnEndReason::Completed,
        })
        .unwrap();
    cx.run_until_parked();
    assert!(
        ui.read(cx, |app, _| app.session().view.running),
        "a flooded queue must yield before consuming the entire turn"
    );
    ui.input(cx, "typing stays independent");
    assert_eq!(ui.composer_text(cx), "typing stays independent");
    settle(cx);
    assert!(!ui.read(cx, |app, _| app.session().view.running));
    assert!(ui.read(cx, |app, _| {
        app.session().view.items.iter().any(
            |item| matches!(item, Item::Assistant { text, streaming: false } if text == &"界".repeat(600)),
        )
    }));
}

#[gpui_kit::test]
fn image_only_message_reaches_engine_and_is_visible(cx: &mut TestAppContext) {
    let ui = open(cx);
    let engine = ui.engine(cx);
    let image = temp_dir("image").join("screen.png");
    std::fs::write(&image, b"\x89PNG\r\n\x1a\npixels").unwrap();
    ui.app.update(cx, |app, cx| {
        app.add_image_attachment(image);
        cx.notify();
    });
    assert_eq!(
        ui.read(cx, |app, _| (
            app.image_attachments.len(),
            app.store_error.clone()
        )),
        (1, None)
    );
    assert!(ui.with(cx, |window, cx| {
        window.render_frame(cx);
        window.try_find(("remove-image", 0usize)).is_some()
    }));
    ui.press(cx, "enter");
    assert!(ui.read(cx, |app, _| app.image_attachments.is_empty()));
    let sent = engine.sent();
    let [Op::UserMessageWithImages { text, images }] = sent.as_slice() else {
        panic!("expected an image prompt");
    };
    assert!(text.is_empty());
    assert_eq!(images[0].mime_type, "image/png");
    assert_eq!(images[0].name, "screen.png");
    assert_eq!(
        ui.read(cx, |app, _| app.session().view.items.first().cloned()),
        Some(Item::User("[Image: screen.png]".into()))
    );
}

#[gpui_kit::test]
fn changed_image_keeps_composer_draft_on_send_failure(cx: &mut TestAppContext) {
    let ui = open(cx);
    let engine = ui.engine(cx);
    let image = temp_dir("image").join("screen.png");
    std::fs::write(&image, b"\x89PNG\r\n\x1a\npixels").unwrap();
    ui.app.update(cx, |app, cx| {
        app.add_image_attachment(image.clone());
        cx.notify();
    });
    std::fs::write(&image, b"not an image").unwrap();
    ui.input(cx, "What is this?");
    ui.press(cx, "enter");
    assert!(engine.sent().is_empty());
    assert_eq!(ui.composer_text(cx), "What is this?");
    assert!(ui.read(cx, |app, _| !app.image_attachments.is_empty()));
}

fn copy_test_image(cx: &mut TestAppContext) {
    cx.update(|cx| {
        cx.write_to_clipboard(gpui_kit::ClipboardItem::new_image(
            &gpui_kit::Image::from_bytes(
                gpui_kit::ImageFormat::Png,
                b"\x89PNG\r\n\x1a\npixels".to_vec(),
            ),
        ));
    });
}

#[gpui_kit::test]
fn clipboard_image_paste_preserves_text_and_sends_an_image(cx: &mut TestAppContext) {
    let ui = open(cx);
    let engine = ui.engine(cx);
    ui.input(cx, "What is this?");
    ui.press(cx, "cmd-a");
    copy_test_image(cx);
    ui.press(cx, "cmd-v");
    settle(cx);
    assert_eq!(ui.composer_text(cx), "What is this?");
    assert_eq!(ui.read(cx, |app, _| app.pending_image_pastes), 0);
    let path = ui.read(cx, |app, _| {
        assert_eq!(app.image_attachments.len(), 1);
        assert_eq!(app.store_error, None);
        app.image_attachments[0].clone()
    });
    assert!(path.exists());
    assert!(has(&ui, cx, "composer-attachments"));
    ui.press(cx, "enter");
    let sent = engine.sent();
    assert!(
        matches!(sent.as_slice(), [Op::UserMessageWithImages { text, images }]
            if text == "What is this?" && images.len() == 1 && images[0].mime_type == "image/png")
    );
    assert!(
        !path.exists(),
        "temporary image must be removed after encoding"
    );
}

#[gpui_kit::test]
fn removing_a_pasted_image_deletes_only_its_temporary_file(cx: &mut TestAppContext) {
    let ui = open(cx);
    copy_test_image(cx);
    ui.press(cx, "cmd-v");
    settle(cx);
    let path = ui.read(cx, |app, _| app.image_attachments[0].clone());
    ui.click(cx, ("remove-image", 0usize));
    assert!(!path.exists());
    assert!(ui.read(cx, |app, _| app.image_attachments.is_empty()));

    let original = temp_dir("copied-image").join("original.png");
    std::fs::write(&original, b"\x89PNG\r\n\x1a\npixels").unwrap();
    cx.update(|cx| {
        cx.write_to_clipboard(gpui_kit::ClipboardItem {
            entries: vec![gpui_kit::ClipboardEntry::ExternalPaths(
                gpui_kit::ExternalPaths(vec![original.clone()].into()),
            )],
        });
    });
    ui.press(cx, "cmd-v");
    assert_eq!(
        ui.read(cx, |app, _| app.image_attachments.clone()),
        vec![original.clone()]
    );
    ui.click(cx, ("remove-image", 0usize));
    assert!(
        original.exists(),
        "copied original files must not be deleted"
    );
}

#[gpui_kit::test]
fn clipboard_image_limit_and_invalid_data_keep_the_draft(cx: &mut TestAppContext) {
    let ui = open(cx);
    ui.input(cx, "draft");
    copy_test_image(cx);
    for _ in 0..4 {
        ui.press(cx, "cmd-v");
        settle(cx);
    }
    assert_eq!(ui.read(cx, |app, _| app.image_attachments.len()), 4);
    ui.press(cx, "cmd-v");
    assert_eq!(ui.read(cx, |app, _| app.image_attachments.len()), 4);
    assert!(ui.read(cx, |app, _| {
        app.store_error.as_deref().unwrap().contains("four")
    }));
    ui.click(cx, ("remove-image", 0usize));
    cx.update(|cx| {
        cx.write_to_clipboard(gpui_kit::ClipboardItem::new_image(
            &gpui_kit::Image::from_bytes(gpui_kit::ImageFormat::Png, b"invalid".to_vec()),
        ));
    });
    ui.press(cx, "cmd-v");
    settle(cx);
    assert_eq!(ui.read(cx, |app, _| app.image_attachments.len()), 3);
    assert!(ui.read(cx, |app, _| {
        app.store_error
            .as_deref()
            .unwrap()
            .contains("supported image")
    }));
    assert_eq!(ui.composer_text(cx), "draft");
}

#[gpui_kit::test]
fn text_clipboard_paste_still_replaces_selection_and_undoes(cx: &mut TestAppContext) {
    let ui = open(cx);
    ui.input(cx, "original");
    ui.press(cx, "cmd-a");
    cx.update(|cx| {
        cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(
            "replacement\ntext".into(),
        ));
    });
    ui.press(cx, "cmd-v");
    assert_eq!(ui.composer_text(cx), "replacement\ntext");
    assert!(ui.read(cx, |app, _| app.image_attachments.is_empty()));
    ui.press(cx, "cmd-z");
    assert_eq!(ui.composer_text(cx), "original");
}

#[gpui_kit::test]
fn sending_waits_until_clipboard_image_preparation_finishes(cx: &mut TestAppContext) {
    let ui = open(cx);
    let engine = ui.engine(cx);
    ui.input(cx, "draft");
    ui.app.update(cx, |app, cx| {
        app.pending_image_pastes = 1;
        cx.notify();
    });
    ui.press(cx, "enter");
    assert!(engine.sent().is_empty());
    assert_eq!(ui.composer_text(cx), "draft");
    ui.app.update(cx, |app, cx| {
        app.pending_image_pastes = 0;
        cx.notify();
    });
    ui.press(cx, "enter");
    assert!(matches!(engine.sent().as_slice(), [Op::UserMessage(text)] if text == "draft"));
}

#[gpui_kit::test]
fn delayed_clipboard_image_does_not_attach_to_another_session(cx: &mut TestAppContext) {
    let ui = open(cx);
    ui.input(cx, "draft");
    copy_test_image(cx);
    ui.with(cx, |window, cx| {
        window.press("cmd-v", cx);
        ui.app.update(cx, |app, cx| {
            assert_eq!(app.pending_image_pastes, 1);
            let workspace = app.session().workspace.clone();
            app.sessions
                .push(flint_app::session::Session::new(99_999, workspace));
            app.select_session(app.sessions.len() - 1, window, cx);
        });
    });
    settle(cx);
    assert!(ui.read(cx, |app, _| app.image_attachments.is_empty()));
    assert_eq!(ui.read(cx, |app, _| app.pending_image_pastes), 0);
    assert_eq!(ui.composer_text(cx), "");
    ui.with(cx, |window, cx| {
        ui.app
            .update(cx, |app, cx| app.select_session(0, window, cx));
    });
    assert_eq!(ui.composer_text(cx), "draft");
    assert!(ui.read(cx, |app, _| app.image_attachments.is_empty()));
}

#[gpui_kit::test]
fn concurrent_clipboard_pastes_respect_the_four_image_limit(cx: &mut TestAppContext) {
    let ui = open(cx);
    copy_test_image(cx);
    ui.with(cx, |window, cx| {
        for _ in 0..5 {
            window.press("cmd-v", cx);
        }
        assert_eq!(ui.app.read(cx).pending_image_pastes, 4);
    });
    settle(cx);
    assert_eq!(ui.read(cx, |app, _| app.image_attachments.len()), 4);
    assert_eq!(ui.read(cx, |app, _| app.pending_image_pastes), 0);
}

#[gpui_kit::test]
fn short_window_caps_composer_popover(cx: &mut TestAppContext) {
    let ui = open_with(
        cx,
        Options {
            window_size: Some((520., 480.)),
            ..test_options()
        },
    );
    ui.app.update(cx, |app, cx| {
        app.help_open = true;
        cx.notify();
    });
    ui.with(cx, |window, cx| {
        window.render_frame(cx);
        let help = window
            .find(ElementId::Name("composer-popover".into()))
            .bounds();
        let composer = window
            .find(ElementId::Name("composer-toolbar".into()))
            .bounds();
        assert!(help.size.height <= px(480. * 0.42 + 1.));
        assert!(
            help.origin.y >= px(36.),
            "popover {help:?} must clear the title bar"
        );
        assert!(help.origin.y + help.size.height <= px(480.));
        assert!(
            composer.origin.y + composer.size.height <= px(480.),
            "send controls must remain reachable"
        );
    });
}

#[gpui_kit::test]
fn composer_is_focused_on_launch(cx: &mut TestAppContext) {
    let ui = open(cx);
    let app = ui.app.clone();
    let focused = ui.with(cx, |window, cx| {
        app.read(cx).composer.focus_handle(cx).is_focused(window)
    });
    assert!(focused);
}

#[gpui_kit::test]
fn shift_tab_cycles_the_approval_mode(cx: &mut TestAppContext) {
    let ui = open(cx);
    assert_eq!(
        ui.read(cx, |app, _| app.approval),
        ApprovalMode::AskForChanges
    );
    ui.press(cx, "shift-tab");
    assert_eq!(ui.read(cx, |app, _| app.approval), ApprovalMode::Auto);
    ui.press(cx, "shift-tab");
    assert_eq!(
        ui.read(cx, |app, _| app.approval),
        ApprovalMode::AskForChanges
    );
}

#[gpui_kit::test]
fn cmd_k_opens_the_palette_and_escape_closes_it(cx: &mut TestAppContext) {
    let ui = open(cx);
    ui.press(cx, "cmd-k");
    assert!(ui.read(cx, |app, _| app.palette.is_some()));
    ui.press(cx, "escape");
    assert!(ui.read(cx, |app, _| app.palette.is_none()));
}

#[gpui_kit::test]
fn palette_enter_runs_the_selected_command(cx: &mut TestAppContext) {
    let ui = open(cx);
    ui.press(cx, "cmd-k");
    ui.input(cx, "changes panel");
    ui.press(cx, "enter");
    assert!(ui.read(cx, |app, _| app.changes_open));
    assert!(ui.read(cx, |app, _| app.palette.is_none()));
}

#[gpui_kit::test]
fn cmd_j_toggles_the_changes_panel(cx: &mut TestAppContext) {
    let ui = open(cx);
    ui.press(cx, "cmd-j");
    assert!(ui.read(cx, |app, _| app.changes_open));
    ui.press(cx, "cmd-j");
    assert!(ui.read(cx, |app, _| !app.changes_open));
}

#[gpui_kit::test]
fn cmd_n_creates_a_session(cx: &mut TestAppContext) {
    let ui = open(cx);
    let _engine = ui.engine(cx);
    ui.input(cx, "first task");
    ui.press(cx, "enter");
    ui.press(cx, "cmd-n");
    assert_eq!(
        ui.read(cx, |app, _| (app.sessions.len(), app.active)),
        (2, 1)
    );
}

#[gpui_kit::test]
fn stop_and_cmd_period_send_interrupt(cx: &mut TestAppContext) {
    let ui = open(cx);
    let engine = ui.engine(cx);
    ui.input(cx, "long task");
    ui.press(cx, "enter");
    engine.send(cx, AgentEvent::TurnStarted { turn_id: 1 });
    engine.sent();
    ui.press(cx, "cmd-.");
    assert!(matches!(engine.sent().as_slice(), [Op::Interrupt]));
    ui.click(cx, "stop");
    assert!(matches!(engine.sent().as_slice(), [Op::Interrupt]));
}

#[gpui_kit::test]
fn approval_buttons_send_the_right_op(cx: &mut TestAppContext) {
    for (button, decision) in [
        ("approve-button", ApprovalDecision::Approve),
        ("always-button", ApprovalDecision::ApproveAlways),
        ("deny-button", ApprovalDecision::Deny),
    ] {
        let ui = open(cx);
        let engine = ui.engine(cx);
        ui.input(cx, "deploy");
        ui.press(cx, "enter");
        engine.send(cx, AgentEvent::TurnStarted { turn_id: 1 });
        engine.send(
            cx,
            AgentEvent::ApprovalRequested {
                call_id: "c1".into(),
                kind: ToolKind::Command,
                summary: "rm -rf build".into(),
            },
        );
        engine.sent();
        ui.click(cx, button);
        if decision == ApprovalDecision::ApproveAlways {
            assert!(
                engine.sent().is_empty(),
                "broad approval needs confirmation"
            );
            assert!(ui.read(cx, |app, _| app.approval_confirm.is_some()));
            ui.click(cx, button);
        }
        let sent = engine.sent();
        assert!(
            matches!(sent.as_slice(), [Op::Approval { call_id, decision: d }] if call_id == "c1" && *d == decision),
            "{button}: {sent:?}"
        );
    }
}

#[gpui_kit::test]
fn worked_for_expands_and_collapses(cx: &mut TestAppContext) {
    let ui = open(cx);
    let engine = ui.engine(cx);
    finished_turn(&ui, cx, &engine);
    let header = ui.read(cx, |app, _| app.session().view.turns[0].header.unwrap());
    ui.click(cx, ("worked", header));
    assert!(ui.read(cx, |app, _| app.session().view.turns[0].expanded));
    ui.click(cx, ("worked", header));
    assert!(ui.read(cx, |app, _| !app.session().view.turns[0].expanded));
}

#[gpui_kit::test]
fn clicking_a_tool_row_expands_its_output(cx: &mut TestAppContext) {
    let ui = open(cx);
    let engine = ui.engine(cx);
    ui.input(cx, "run tests");
    ui.press(cx, "enter");
    engine.send(cx, AgentEvent::TurnStarted { turn_id: 1 });
    engine.send(cx, tool_started("c1", ToolKind::Command, "npm test"));
    engine.send(
        cx,
        tool_finished("c1", "line 1\nline 2\nline 3\nline 4", None),
    );
    let ix = ui.read(cx, |app, _| app.session().view.items.len() - 1);
    ui.click(cx, ("tool", ix));
    let expanded = ui.read(cx, |app, _| match &app.session().view.items[ix] {
        Item::Tool(call) => call.expanded,
        _ => false,
    });
    assert!(expanded);
}

#[gpui_kit::test]
fn multiline_command_rows_do_not_overlap_and_expand_the_full_command(cx: &mut TestAppContext) {
    for success in [true, false] {
        let ui = open(cx);
        let engine = running_turn(&ui, cx);
        let command = "cat <<'END'\nfirst line\nsecond line\nEND";
        for (id, text) in [
            ("before", "echo before"),
            ("multi", command),
            ("after", "echo after"),
        ] {
            engine.send(cx, tool_started(id, ToolKind::Command, text));
            engine.send(
                cx,
                AgentEvent::ToolCallFinished {
                    call_id: id.into(),
                    output: "result".into(),
                    exit_code: Some(if success { 0 } else { 1 }),
                    success,
                    diff: None,
                    duration_ms: 1,
                },
            );
        }
        let indices = ui.read(cx, |app, _| {
            app.session()
                .view
                .items
                .iter()
                .enumerate()
                .filter_map(|(ix, item)| matches!(item, Item::Tool(_)).then_some(ix))
                .collect::<Vec<_>>()
        });
        ui.with(cx, |window, cx| {
            window.render_frame(cx);
            for pair in indices.windows(2) {
                let before = window.find(("tool", pair[0])).bounds();
                let after = window.find(("tool", pair[1])).bounds();
                assert!(
                    before.bottom() <= after.top(),
                    "{before:?} overlaps {after:?}"
                );
            }
            for ix in &indices {
                let header = window.find(("tool-header", *ix)).bounds();
                let summary = window.find(("tool-summary", *ix)).bounds();
                assert!(summary.size.height <= px(26.), "{summary:?}");
                assert!(
                    summary.bottom() <= header.bottom(),
                    "{summary:?} overflows {header:?}"
                );
            }
            assert!(window.try_find(("tool-command", indices[1])).is_none());
        });
        ui.click(cx, ("tool", indices[1]));
        ui.with(cx, |window, cx| {
            window.render_frame(cx);
            let command = window.find(("tool-command", indices[1])).bounds();
            assert!(command.size.height >= px(4. * 21.), "{command:?}");
        });
        assert!(
            ui.read(cx, |app, _| matches!(&app.session().view.items[indices[1]],
            Item::Tool(call) if call.summary == command))
        );
    }
}

#[gpui_kit::test]
fn multiline_running_command_stays_inside_the_compact_task_tray(cx: &mut TestAppContext) {
    let ui = open_with(
        cx,
        Options {
            window_size: Some((900., 560.)),
            ..test_options()
        },
    );
    let engine = running_turn(&ui, cx);
    let command = "cat <<'END'\nfirst line\nsecond line\nEND";
    engine.send(cx, tool_started("multi", ToolKind::Command, command));
    ui.with(cx, |window, cx| {
        window.render_frame(cx);
        let row = window.find("running-command-multi").bounds();
        let summary = window.find("running-command-summary-multi").bounds();
        assert!(summary.size.height <= px(26.), "{summary:?}");
        assert!(
            summary.bottom() <= row.bottom(),
            "{summary:?} overflows {row:?}"
        );
        assert!(
            summary.right() <= row.right(),
            "{summary:?} overflows {row:?}"
        );
    });
    assert!(
        !ui.read(cx, |app, _| app.session().activity())
            .contains('\n')
    );
    assert!(ui.read(cx, |app, _| {
        app.session()
            .view
            .items
            .iter()
            .any(|item| matches!(item, Item::Tool(call) if call.summary == command))
    }));
}

#[gpui_kit::test]
fn multiline_approval_summary_stays_in_its_header(cx: &mut TestAppContext) {
    let ui = open(cx);
    let engine = running_turn(&ui, cx);
    let command = "cat <<'END'\nfirst line\nsecond line\nEND";
    engine.send(cx, tool_started("multi", ToolKind::Command, command));
    engine.send(
        cx,
        AgentEvent::ApprovalRequested {
            call_id: "multi".into(),
            kind: ToolKind::Command,
            summary: command.into(),
        },
    );
    ui.with(cx, |window, cx| {
        window.render_frame(cx);
        let summary = window.find("approval-summary").bounds();
        assert!(summary.size.height <= px(26.), "{summary:?}");
    });
}

#[gpui_kit::test]
fn review_opens_the_changes_panel_on_that_file(cx: &mut TestAppContext) {
    let ui = open(cx);
    let engine = ui.engine(cx);
    finished_turn(&ui, cx, &engine);
    let end = ui.read(cx, |app, _| app.session().view.turns[0].end.unwrap());
    ui.click(cx, ("review-button", end));
    assert_eq!(
        ui.read(cx, |app, _| (app.changes_open, app.selected_change)),
        (true, Some(0))
    );
}

#[gpui_kit::test]
fn switching_sessions_keeps_a_background_session_running(cx: &mut TestAppContext) {
    let ui = open(cx);
    let engine = ui.engine(cx);
    ui.input(cx, "background work");
    ui.press(cx, "enter");
    engine.send(cx, AgentEvent::TurnStarted { turn_id: 1 });
    ui.press(cx, "cmd-n");
    assert_eq!(ui.read(cx, |app, _| app.active), 1);
    engine.send(cx, tool_started("c1", ToolKind::Command, "make"));
    engine.send(
        cx,
        AgentEvent::TurnFinished {
            turn_id: 1,
            reason: TurnEndReason::Completed,
        },
    );
    let (items, unread) = ui.read(cx, |app, _| {
        (app.sessions[0].view.items.len(), app.sessions[0].unread)
    });
    assert!(
        items >= 3,
        "background session kept folding events: {items} items"
    );
    assert!(unread, "finished background session is marked unread");
    ui.click(cx, ("session", 0usize));
    assert_eq!(
        ui.read(cx, |app, _| (app.active, app.sessions[0].unread)),
        (0, false)
    );
}

#[gpui_kit::test]
fn transcript_follows_output_until_scrolled_up(cx: &mut TestAppContext) {
    let ui = open(cx);
    let engine = ui.engine(cx);
    ui.input(cx, "stream");
    ui.press(cx, "enter");
    engine.send(cx, AgentEvent::TurnStarted { turn_id: 1 });
    for n in 0..60 {
        engine.send(
            cx,
            tool_started(
                &format!("c{n}"),
                ToolKind::Read,
                &format!("src/file_{n}.rs"),
            ),
        );
    }
    ui.with(cx, |window, cx| window.render_frame(cx));
    assert!(ui.read(cx, |app, _| app.session().list.is_following_tail()));
    ui.with(cx, |window, cx| {
        window.scroll(
            "transcript",
            ScrollDelta::Pixels(point(px(0.), px(400.))),
            cx,
        )
    });
    cx.run_until_parked();
    assert!(ui.read(cx, |app, _| !app.session().list.is_following_tail()));
    let latest = format!(
        "transcript-latest-{}",
        ui.read(cx, |app, _| app.session().uid)
    );
    assert!(has(&ui, cx, &latest));
    engine.send(
        cx,
        AgentEvent::TextDelta("New output respects scrolling".into()),
    );
    assert!(ui.read(cx, |app, _| !app.session().list.is_following_tail()));
    ui.click(cx, ElementId::Name(latest.into()));
    assert!(ui.read(cx, |app, _| app.session().list.is_following_tail()));
}

// ---- Part B fixes -------------------------------------------------------

fn approval_requested(engine: &Engine, cx: &mut TestAppContext, call_id: &str) {
    engine.send(
        cx,
        AgentEvent::ApprovalRequested {
            call_id: call_id.into(),
            kind: ToolKind::Command,
            summary: "npm publish".into(),
        },
    );
}

fn running_turn(ui: &Ui, cx: &mut TestAppContext) -> Engine {
    let engine = ui.engine(cx);
    ui.input(cx, "do the thing");
    ui.press(cx, "enter");
    engine.send(cx, AgentEvent::TurnStarted { turn_id: 1 });
    engine.sent();
    engine
}

#[gpui_kit::test]
fn pending_approval_is_pinned_just_above_the_composer(cx: &mut TestAppContext) {
    let ui = open(cx);
    let engine = running_turn(&ui, cx);
    approval_requested(&engine, cx, "c1");
    let (card, stop) = ui.with(cx, |window, _| {
        (
            window.find("approval-card").bounds(),
            window.find("stop").bounds(),
        )
    });
    assert!(
        card.bottom() <= stop.top(),
        "card {card:?} must sit above the composer {stop:?}"
    );
    assert!(
        card.top() > px(450.),
        "card is in the lower half, near the composer: {card:?}"
    );
}

#[gpui_kit::test]
fn y_a_n_answer_an_approval_from_an_empty_composer(cx: &mut TestAppContext) {
    for (key, decision) in [
        ("y", ApprovalDecision::Approve),
        ("a", ApprovalDecision::ApproveAlways),
        ("n", ApprovalDecision::Deny),
    ] {
        let ui = open(cx);
        let engine = running_turn(&ui, cx);
        approval_requested(&engine, cx, "c1");
        ui.press(cx, key);
        if decision == ApprovalDecision::ApproveAlways {
            assert!(engine.sent().is_empty(), "A must not grant broad approval");
            ui.click(cx, "always-button");
        }
        let sent = engine.sent();
        assert!(
            matches!(sent.as_slice(), [Op::Approval { decision: d, .. }] if *d == decision),
            "{key}: {sent:?}"
        );
        assert_eq!(ui.composer_text(cx), "", "{key} must not be typed");
    }
}

#[gpui_kit::test]
fn typing_y_with_text_in_the_composer_does_not_approve(cx: &mut TestAppContext) {
    let ui = open(cx);
    let engine = running_turn(&ui, cx);
    approval_requested(&engine, cx, "c1");
    ui.input(cx, "wait, why");
    assert!(engine.sent().is_empty());
    assert!(ui.read(cx, |app, _| app.session().view.pending_approval().is_some()));
}

#[gpui_kit::test]
fn cmd_enter_approves_the_pending_request(cx: &mut TestAppContext) {
    let ui = open(cx);
    let engine = running_turn(&ui, cx);
    approval_requested(&engine, cx, "c1");
    ui.press(cx, "cmd-enter");
    assert!(matches!(
        engine.sent().as_slice(),
        [Op::Approval {
            decision: ApprovalDecision::Approve,
            ..
        }]
    ));
}

#[gpui_kit::test]
fn large_pending_approval_preserves_requests_and_keyboard_routes_oldest(cx: &mut TestAppContext) {
    for window_size in [(1440., 900.), (1000., 560.)] {
        let ui = open_with(
            cx,
            Options {
                window_size: Some(window_size),
                ..test_options()
            },
        );
        let engine = running_turn(&ui, cx);
        let summary = format!("{}\nORIGINAL_BODY", "界🙂".repeat(4096));
        engine.send(
            cx,
            AgentEvent::ApprovalRequested {
                call_id: "first-borrowed".into(),
                kind: ToolKind::Command,
                summary: summary.clone(),
            },
        );
        approval_requested(&engine, cx, "second-borrowed");
        ui.input(cx, "wait, why");
        assert!(engine.sent().is_empty());
        assert_eq!(ui.composer_text(cx), "wait, why");
        ui.with(cx, |window, cx| {
            window.render_frame(cx);
            let card = window.find("approval-card").bounds();
            let header = window.find("approval-summary").bounds();
            assert!(header.left() >= card.left() && header.right() <= card.right());
        });
        ui.press(cx, "cmd-enter");
        assert!(matches!(engine.sent().as_slice(),
            [Op::Approval { call_id, decision: ApprovalDecision::Approve }] if call_id == "first-borrowed"));
        ui.read(cx, |app, _| {
            assert!(app.session().view.items.iter().any(|item| matches!(item,
                Item::Approval {summary: stored, decision: Some(ApprovalDecision::Approve), ..} if stored == &summary)));
            assert_eq!(app.session().view.pending_approval().unwrap().0, "second-borrowed");
        });
        ui.press(cx, "cmd-enter");
        assert!(matches!(engine.sent().as_slice(),
            [Op::Approval { call_id, decision: ApprovalDecision::Approve }] if call_id == "second-borrowed"));
        ui.read(cx, |app, _| {
            assert!(app.session().view.pending_approval().is_none());
            assert!(
                app.session()
                    .prompt_queue
                    .items
                    .iter()
                    .any(|prompt| prompt.text == "wait, why")
            );
        });
    }
}

#[gpui_kit::test]
fn background_approval_shows_needs_approval_in_the_sidebar(cx: &mut TestAppContext) {
    let ui = open(cx);
    let engine = running_turn(&ui, cx);
    ui.press(cx, "cmd-n");
    approval_requested(&engine, cx, "c1");
    let status = ui.read(cx, |app, _| app.sessions[0].status());
    assert_eq!(status, flint_app::session::Status::NeedsApproval);
}

/// A workspace with two source files and a gitignored secret.
fn mention_workspace() -> std::path::PathBuf {
    let ws = temp_dir("mention");
    std::fs::create_dir_all(ws.join("src")).unwrap();
    std::fs::write(ws.join("src/main.rs"), "fn main() { println!(\"hi\"); }\n").unwrap();
    std::fs::write(ws.join("src/lib.rs"), "pub fn lib() {}\n").unwrap();
    std::fs::write(ws.join(".gitignore"), "secret.txt\n").unwrap();
    std::fs::write(ws.join("secret.txt"), "password\n").unwrap();
    ws
}

#[gpui_kit::test]
fn mention_indexing_yields_and_ignores_a_changed_workspace(cx: &mut TestAppContext) {
    let source = mention_workspace();
    let destination = temp_dir("mention-destination");
    std::fs::write(destination.join("destination.rs"), "fn destination() {}\n").unwrap();
    let ui = open_with(
        cx,
        Options {
            workspace: Some(source),
            ..test_options()
        },
    );
    ui.with(cx, |window, cx| {
        ui.app.update(cx, |app, cx| {
            app.open_mention_picker(window, cx);
            assert!(
                app.mention.as_ref().unwrap().results.is_empty(),
                "indexing must yield instead of walking the filesystem on the UI thread"
            );
            app.set_project_folder(destination.clone(), cx);
        });
    });
    settle(cx);
    assert!(
        ui.read(cx, |app, _| app.mention.is_none()),
        "a project change closes the old picker; late results must not reopen it"
    );
    ui.with(cx, |window, cx| {
        ui.app
            .update(cx, |app, cx| app.open_mention_picker(window, cx));
    });
    settle(cx);
    assert_eq!(
        ui.read(cx, |app, _| app.mention.as_ref().unwrap().results.clone()),
        vec!["destination.rs".to_string()]
    );
}

#[gpui_kit::test]
fn changing_project_after_a_turn_keeps_the_previous_sessions_draft(cx: &mut TestAppContext) {
    let ui = open(cx);
    let _engine = running_turn(&ui, cx);
    ui.input(cx, "old session draft");
    ui.with(cx, |_, cx| {
        ui.app
            .update(cx, |app, _| app.attachments.push("old.rs".into()));
    });
    let destination = temp_dir("draft-project-destination");
    ui.with(cx, |_, cx| {
        ui.app
            .update(cx, |app, cx| app.set_project_folder(destination, cx));
    });
    settle(cx);
    assert_eq!(ui.composer_text(cx), "");
    assert!(ui.read(cx, |app, _| app.attachments.is_empty()));
    ui.with(cx, |window, cx| {
        ui.app
            .update(cx, |app, cx| app.select_session(0, window, cx));
    });
    settle(cx);
    assert_eq!(ui.composer_text(cx), "old session draft");
    assert_eq!(
        ui.read(cx, |app, _| app.attachments.clone()),
        vec!["old.rs".to_string()]
    );
}

#[gpui_kit::test]
fn at_opens_a_file_picker_and_picking_attaches_the_file(cx: &mut TestAppContext) {
    let ui = open_with(
        cx,
        Options {
            workspace: Some(mention_workspace()),
            ..test_options()
        },
    );
    let engine = ui.engine(cx);
    ui.input(cx, "explain @mai");
    let results = ui.read(cx, |app, _| app.mention.as_ref().map(|m| m.results.clone()));
    assert_eq!(
        results.as_ref().and_then(|r| r.first()).map(String::as_str),
        Some("src/main.rs")
    );
    ui.press(cx, "enter");
    assert_eq!(ui.composer_text(cx), "explain @src/main.rs ");
    assert_eq!(
        ui.read(cx, |app, _| app.attachments.clone()),
        vec!["src/main.rs".to_string()]
    );
    ui.press(cx, "enter");
    let sent = engine.sent();
    let [Op::UserMessage(message)] = sent.as_slice() else {
        panic!("expected one message, got {sent:?}");
    };
    assert!(message.starts_with("explain @src/main.rs"));
    assert!(message.contains(
        "[Attached file src/main.rs (1 lines)]\n```\nfn main() { println!(\"hi\"); }\n```"
    ));
}

#[gpui_kit::test]
fn the_file_picker_respects_gitignore(cx: &mut TestAppContext) {
    let ui = open_with(
        cx,
        Options {
            workspace: Some(mention_workspace()),
            ..test_options()
        },
    );
    ui.input(cx, "@secret");
    let results = ui.read(cx, |app, _| app.mention.as_ref().map(|m| m.results.clone()));
    assert_eq!(results, Some(vec![]));
}

#[gpui_kit::test]
fn plus_opens_the_same_file_picker(cx: &mut TestAppContext) {
    let ui = open_with(
        cx,
        Options {
            workspace: Some(mention_workspace()),
            ..test_options()
        },
    );
    // "+" opens the project menu; its last row is "Attach file…".
    ui.click(cx, "attach");
    let attach = ui.read(cx, |app, _| app.project_items().len() - 1);
    ui.click(cx, ("project-item", attach));
    let menu = ui.read(cx, |app, _| {
        app.mention.as_ref().map(|m| (m.inline, m.results.len()))
    });
    assert_eq!(
        menu,
        Some((false, 2)),
        "src/main.rs and src/lib.rs; secret.txt is gitignored"
    );
}

#[gpui_kit::test]
fn slash_opens_a_keyboard_navigable_command_menu(cx: &mut TestAppContext) {
    let ui = open(cx);
    ui.input(cx, "/");
    assert!(ui.read(cx, |app, _| app.slash.is_some()));
    ui.press(cx, "down");
    ui.press(cx, "down");
    ui.press(cx, "enter");
    assert!(
        ui.read(cx, |app, _| app.settings_form.is_some()),
        "third command is /model"
    );
    assert_eq!(ui.composer_text(cx), "");
}

#[gpui_kit::test]
fn slash_review_opens_the_changes_panel(cx: &mut TestAppContext) {
    let ui = open(cx);
    ui.input(cx, "/rev");
    ui.press(cx, "enter");
    assert!(ui.read(cx, |app, _| app.changes_open && app.slash.is_none()));
}

#[gpui_kit::test]
fn settings_save_to_config_and_are_read_on_startup(cx: &mut TestAppContext) {
    let options = test_options();
    let home = options.home.clone().unwrap();
    let ui = open_with(cx, options.clone());
    ui.press(cx, "cmd-,");
    assert!(ui.read(cx, |app, _| app.settings_form.is_some()));
    ui.with(cx, |window, cx| {
        ui.app.update(cx, |app, cx| {
            let input = app.settings_form.as_ref().unwrap().subagent_model.clone();
            input.update(cx, |input, cx| input.set_value("child-model", window, cx));
        });
    });
    ui.scroll_settings(cx);
    ui.click(cx, ("settings-approval", 0usize));
    ui.click(cx, ("settings-effort", 2usize));
    ui.click(cx, "settings-save");
    assert!(ui.read(cx, |app, _| app.settings_form.is_none()));
    let saved = std::fs::read_to_string(home.join("config.toml")).unwrap();
    assert!(
        saved.contains("approval = \"auto\"")
            && saved.contains("effort = \"high\"")
            && saved.contains("subagent_model = \"child-model\""),
        "{saved}"
    );
    let again = open_with(cx, options);
    assert_eq!(
        again.read(cx, |app, _| app.settings.subagent_model.clone()),
        "child-model"
    );
    assert_eq!(
        again.read(cx, |app, _| (app.approval, app.effort)),
        (ApprovalMode::Auto, Some(flint_agent::ReasoningEffort::High))
    );
}

#[gpui_kit::test]
fn subagent_cards_expand_child_activity_and_route_approvals(cx: &mut TestAppContext) {
    let options = test_options();
    let ui = open_with(cx, options.clone());
    let engine = ui.engine(cx);
    ui.input(cx, "research with a subagent");
    ui.press(cx, "enter");
    engine.sent();
    engine.send(cx, AgentEvent::TurnStarted { turn_id: 1 });
    engine.send(
        cx,
        AgentEvent::ToolCallStarted {
            call_id: "delegate".into(),
            name: "spawn_agent".into(),
            kind: ToolKind::Other,
            args: json!({"label": "Research", "message": "Inspect the project."}),
            summary: "Research".into(),
        },
    );
    engine.send(
        cx,
        AgentEvent::SubagentStarted {
            call_id: "delegate".into(),
            session_id: "agent-1".into(),
            model: "child-model".into(),
        },
    );
    for event in [
        AgentEvent::TurnStarted { turn_id: 1 },
        AgentEvent::TextDelta("Child findings.".into()),
        tool_started("agent-1:edit", ToolKind::Edit, "a.txt"),
    ] {
        engine.send(
            cx,
            AgentEvent::SubagentEvent {
                call_id: "delegate".into(),
                event: Box::new(event),
            },
        );
    }
    let row = ui.read(cx, |app, _| {
        app.session()
            .view
            .items
            .iter()
            .position(|item| {
                matches!(item,
                    Item::Tool(call) if call.call_id == "delegate"
                )
            })
            .unwrap()
    });
    ui.click(cx, ("tool", row));
    assert!(ui.read(cx, |app, _| matches!(&app.session().view.items[row],
        Item::Tool(call) if call.expanded && call.subagent.as_ref().unwrap().model == "child-model"
    )));
    engine.send(
        cx,
        AgentEvent::ApprovalRequested {
            call_id: "agent-1:edit".into(),
            kind: ToolKind::Edit,
            summary: "a.txt".into(),
        },
    );
    ui.click(cx, "deny-button");
    assert!(
        matches!(engine.sent().as_slice(), [Op::Approval { call_id, decision }]
        if call_id == "agent-1:edit" && *decision == ApprovalDecision::Deny)
    );
    engine.send(
        cx,
        AgentEvent::SubagentEvent {
            call_id: "delegate".into(),
            event: Box::new(AgentEvent::TurnFinished {
                turn_id: 1,
                reason: TurnEndReason::Completed,
            }),
        },
    );
    engine.send(
        cx,
        tool_finished(
            "delegate",
            r#"{"session_id":"agent-1","output":"Child findings."}"#,
            None,
        ),
    );
    engine.send(cx, AgentEvent::TextDelta("Parent answer.".into()));
    engine.send(
        cx,
        AgentEvent::TurnFinished {
            turn_id: 1,
            reason: TurnEndReason::Completed,
        },
    );
    let again = open_with(cx, options);
    assert!(again.read(cx, |app, _| app.sessions.iter().any(|session| {
        session.view.items.iter().any(|item| {
            matches!(item, Item::Tool(call)
                if call.subagent.as_ref().is_some_and(|child|
                    child.model == "child-model" && session.view.subagent(&child.session_id).is_some_and(|child| !child.view.running))
            )
        })
    })));
}

#[gpui_kit::test]
fn escape_closes_settings_without_saving(cx: &mut TestAppContext) {
    let options = test_options();
    let home = options.home.clone().unwrap();
    let ui = open_with(cx, options);
    ui.press(cx, "cmd-,");
    ui.scroll_settings(cx);
    ui.click(cx, ("settings-approval", 0usize));
    ui.press(cx, "escape");
    assert!(ui.read(cx, |app, _| app.settings_form.is_none()
        && app.approval == ApprovalMode::AskForChanges));
    assert!(!home.join("config.toml").exists());
}

#[gpui_kit::test]
fn settings_tab_and_reverse_tab_stay_in_sheet_and_restore_focus(cx: &mut TestAppContext) {
    let ui = open(cx);
    ui.press(cx, "cmd-,");
    ui.press(cx, "tab");
    assert!(ui.with(cx, |window, cx| {
        ui.app
            .read(cx)
            .settings_form
            .as_ref()
            .unwrap()
            .subagent_model
            .focus_handle(cx)
            .is_focused(window)
    }));
    ui.press(cx, "shift-tab");
    assert!(ui.with(cx, |window, cx| {
        ui.app
            .read(cx)
            .settings_form
            .as_ref()
            .unwrap()
            .model
            .focus_handle(cx)
            .is_focused(window)
    }));
    for key in ["tab", "shift-tab"] {
        for _ in 0..35 {
            ui.press(cx, key);
            assert!(
                ui.with(cx, |window, cx| {
                    ui.app
                        .read(cx)
                        .settings_form
                        .as_ref()
                        .unwrap()
                        .focus
                        .contains_focused(window, cx)
                }),
                "focus escaped settings"
            );
        }
    }
    assert_eq!(
        ui.read(cx, |app, _| app.approval),
        ApprovalMode::AskForChanges
    );
    ui.press(cx, "escape");
    assert!(ui.with(cx, |window, cx| {
        ui.app.read(cx).composer.focus_handle(cx).is_focused(window)
    }));
}

#[gpui_kit::test]
fn palette_and_sessions_drawer_contain_keyboard_focus(cx: &mut TestAppContext) {
    let ui = open_with(
        cx,
        Options {
            window_size: Some((900., 560.)),
            ..test_options()
        },
    );
    ui.press(cx, "cmd-k");
    for key in ["tab", "shift-tab"] {
        for _ in 0..4 {
            ui.press(cx, key);
            assert!(ui.with(cx, |window, cx| {
                gpui_kit::base::active_focus_trap(window, cx).is_some()
            }));
        }
    }
    ui.press(cx, "escape");
    assert!(ui.read(cx, |app, _| app.palette.is_none()));
    ui.click(cx, "sessions-control");
    for key in ["tab", "shift-tab"] {
        for _ in 0..18 {
            ui.press(cx, key);
            assert!(ui.with(cx, |window, cx| {
                gpui_kit::base::active_focus_trap(window, cx).is_some()
            }));
        }
    }
    ui.press(cx, "escape");
    assert!(ui.read(cx, |app, _| !app.session_drawer));
    assert!(ui.with(cx, |window, cx| {
        window.focused(cx).is_some()
            && !ui.app.read(cx).search.focus_handle(cx).is_focused(window)
            && gpui_kit::base::active_focus_trap(window, cx).is_none()
    }));
}

#[gpui_kit::test]
fn archive_confirmation_contains_focus_and_escape_does_not_stop_work(cx: &mut TestAppContext) {
    let ui = open(cx);
    let engine = running_turn(&ui, cx);
    ui.with(cx, |window, cx| {
        ui.app
            .update(cx, |app, cx| app.delete_session(0, window, cx))
    });
    for key in ["tab", "shift-tab"] {
        for _ in 0..6 {
            ui.press(cx, key);
            assert!(ui.with(cx, |window, cx| {
                gpui_kit::base::active_focus_trap(window, cx).is_some()
            }));
        }
    }
    ui.press(cx, "escape");
    assert!(ui.read(cx, |app, _| app.archive_confirm.is_none()
        && app.session().view.running));
    assert!(engine.sent().is_empty());
    assert!(ui.with(cx, |window, cx| {
        ui.app.read(cx).composer.focus_handle(cx).is_focused(window)
    }));
}

#[gpui_kit::test]
fn approval_details_and_broad_confirmation_are_keyboard_operable(cx: &mut TestAppContext) {
    let ui = open(cx);
    let engine = running_turn(&ui, cx);
    approval_requested(&engine, cx, "fixture-approval");
    ui.click(cx, "approval-preview-toggle");
    ui.key_cycle(cx, "enter");
    assert!(ui.read(cx, |app, _| app.approval_preview.is_none()));
    ui.key_cycle(cx, "space");
    assert!(ui.read(cx, |app, _| app.approval_preview.is_some()));
    ui.press(cx, "escape");
    assert!(ui.read(cx, |app, _| app.approval_preview.is_none()
        && app.session().view.running));
    ui.press(cx, "shift-tab");
    // Reverse navigation outside the composer must not change defaults.
    assert_eq!(
        ui.read(cx, |app, _| app.approval),
        ApprovalMode::AskForChanges
    );
    ui.click(cx, "always-button");
    ui.press(cx, "escape");
    assert!(ui.read(cx, |app, _| app.approval_confirm.is_none()
        && app.session().view.pending_approvals == 1));
    assert!(engine.sent().is_empty());
}

#[gpui_kit::test]
fn header_keyboard_keydown_and_keyup_toggle_only_once(cx: &mut TestAppContext) {
    let ui = open(cx);
    for _ in 0..25 {
        if ui.with(cx, |window, cx| {
            window.render_frame(cx);
            window.find("changes").focused() == Some(true)
        }) {
            break;
        }
        ui.press(cx, "tab");
    }
    assert!(ui.with(cx, |window, _| window.find("changes").focused()
        == Some(true)));
    assert!(!ui.read(cx, |app, _| app.changes_open));
    ui.key_cycle(cx, "enter");
    assert!(ui.read(cx, |app, _| app.changes_open));
    ui.key_cycle(cx, "space");
    assert!(!ui.read(cx, |app, _| app.changes_open));
}

#[gpui_kit::test]
fn palette_to_settings_returns_to_a_live_composer(cx: &mut TestAppContext) {
    let ui = open(cx);
    ui.press(cx, "cmd-k");
    ui.input(cx, "Settings");
    ui.press(cx, "enter");
    assert!(ui.read(cx, |app, _| app.settings_form.is_some()
        && app.palette.is_none()));
    ui.press(cx, "escape");
    assert!(ui.with(cx, |window, cx| {
        ui.app.read(cx).composer.focus_handle(cx).is_focused(window)
    }));
    ui.input(cx, "fixture");
    assert_eq!(ui.composer_text(cx), "fixture");
}

#[gpui_kit::test]
fn concurrent_session_shutdown_blocks_submissions_and_retry_for_each_session(
    cx: &mut TestAppContext,
) {
    let ui = open(cx);
    let first = ui.engine(cx);
    ui.input(cx, "first fixture");
    ui.press(cx, "enter");
    first.sent();
    ui.press(cx, "cmd-n");
    let second = ui.engine(cx);
    ui.input(cx, "second fixture");
    ui.press(cx, "enter");
    second.sent();
    ui.with(cx, |window, cx| {
        ui.app.update(cx, |app, cx| {
            app.delete_session(0, window, cx);
            app.delete_session(1, window, cx);
            app.select_session(0, window, cx);
        })
    });
    assert!(matches!(first.sent().as_slice(), [Op::Shutdown]));
    assert!(matches!(second.sent().as_slice(), [Op::Shutdown]));
    let count = ui.read(cx, |app, _| app.sessions[0].view.items.len());
    ui.input(cx, "must remain unsent");
    ui.press(cx, "enter");
    ui.app.update(cx, |app, cx| app.retry(cx));
    assert!(first.sent().is_empty());
    assert_eq!(ui.composer_text(cx), "must remain unsent");
    assert_eq!(
        ui.read(cx, |app, _| app.sessions[0].view.items.len()),
        count
    );
}

#[gpui_kit::test]
fn failed_shutdown_and_failed_undo_preserve_history(cx: &mut TestAppContext) {
    let options = test_options();
    let ui = open_with(cx, options.clone());
    let engine = ui.engine(cx);
    ui.input(cx, "fixture");
    ui.press(cx, "enter");
    let dir = ui.read(cx, |app, _| app.session().dir.clone().unwrap());
    engine.ops.close();
    ui.with(cx, |window, cx| {
        ui.app
            .update(cx, |app, cx| app.delete_session(0, window, cx))
    });
    assert!(dir.exists() && !ui.read(cx, |app, _| app.session().stopping));
    assert!(ui.read(cx, |app, _| {
        app.store_error
            .as_deref()
            .is_some_and(|s| s.contains("stop session engine"))
    }));
    drop(engine);
    settle(cx);
    ui.with(cx, |window, cx| {
        ui.app
            .update(cx, |app, cx| app.delete_session(0, window, cx))
    });
    let archive = home_archive(&options, &dir);
    let events = std::fs::read(archive.join("events.jsonl")).unwrap();
    std::fs::create_dir(&dir).unwrap();
    std::fs::write(dir.join("sentinel"), "fixture").unwrap();
    ui.with(cx, |window, cx| {
        ui.app.update(cx, |app, cx| app.undo_archive(window, cx))
    });
    assert!(ui.read(cx, |app, _| app.archived_session.is_some()
        && app.store_error.is_some()));
    assert_eq!(std::fs::read(archive.join("events.jsonl")).unwrap(), events);
    assert_eq!(
        std::fs::read_to_string(dir.join("sentinel")).unwrap(),
        "fixture"
    );
}

#[gpui_kit::test]
fn first_run_explicit_choice_survives_restart_without_reconfiguring_active_engine(
    cx: &mut TestAppContext,
) {
    for mode in [ApprovalMode::AskForChanges, ApprovalMode::Auto] {
        let options = Options {
            skip_permission_choice: false,
            ..test_options()
        };
        let ui = open_with(cx, options.clone());
        assert!(ui.read(cx, |app, _| app.permission_choice_open));
        assert_eq!(
            ui.read(cx, |app, _| app.approval),
            ApprovalMode::AskForChanges
        );
        ui.app.update(cx, |app, _| {
            app.sessions[0].native_approval = Some(ApprovalMode::AskForChanges);
        });
        for key in ["tab", "shift-tab"] {
            for _ in 0..5 {
                ui.press(cx, key);
                assert!(ui.with(cx, |window, cx| {
                    gpui_kit::base::active_focus_trap(window, cx).is_some()
                }));
            }
        }
        ui.click(
            cx,
            if mode == ApprovalMode::Auto {
                "first-run-auto"
            } else {
                "first-run-ask"
            },
        );
        assert!(ui.read(cx, |app, _| !app.permission_choice_open
            && app.sessions[0].native_approval == Some(ApprovalMode::AskForChanges)));
        let again = open_with(cx, options);
        assert!(again.read(cx, |app, _| !app.permission_choice_open
            && app.approval == mode));
    }
}

#[gpui_kit::test]
fn first_run_cancel_keeps_ask_and_reprompts_after_restart(cx: &mut TestAppContext) {
    let options = Options {
        skip_permission_choice: false,
        ..test_options()
    };
    let ui = open_with(cx, options.clone());
    ui.press(cx, "escape");
    assert!(ui.read(cx, |app, _| !app.permission_choice_open
        && app.approval == ApprovalMode::AskForChanges));
    assert!(!options.home.as_ref().unwrap().join("config.toml").exists());
    // Creating history after cancellation must not misclassify this home as legacy.
    let engine = ui.engine(cx);
    ui.input(cx, "local fixture");
    ui.press(cx, "enter");
    drop(engine);
    settle(cx);
    let after_history = open_with(cx, options.clone());
    assert!(after_history.read(cx, |app, _| app.permission_choice_open));
    // Incidental preference saves must not silently complete onboarding.
    ui.app
        .update(cx, |app, _| app.settings.save(&app.home).unwrap());
    let again = open_with(cx, options);
    assert!(again.read(cx, |app, _| app.permission_choice_open));
}

#[gpui_kit::test]
fn existing_preferences_and_legacy_installs_do_not_get_first_run_choice(cx: &mut TestAppContext) {
    for approval in ["auto", "ask", "unknown"] {
        let options = Options {
            skip_permission_choice: false,
            ..test_options()
        };
        std::fs::write(
            options.home.as_ref().unwrap().join("config.toml"),
            format!("approval = \"{approval}\"\n"),
        )
        .unwrap();
        let ui = open_with(cx, options);
        assert!(!ui.read(cx, |app, _| app.permission_choice_open));
        assert_eq!(
            ui.read(cx, |app, _| app.approval),
            if approval == "auto" {
                ApprovalMode::Auto
            } else {
                ApprovalMode::AskForChanges
            }
        );
    }
    let options = Options {
        skip_permission_choice: false,
        ..test_options()
    };
    std::fs::create_dir(options.home.as_ref().unwrap().join("sessions")).unwrap();
    assert!(!open_with(cx, options).read(cx, |app, _| app.permission_choice_open));
}

#[gpui_kit::test]
fn first_run_save_failure_keeps_choice_visible_and_permissions_safe(cx: &mut TestAppContext) {
    let options = Options {
        skip_permission_choice: false,
        ..test_options()
    };
    let ui = open_with(cx, options.clone());
    std::fs::create_dir(options.home.as_ref().unwrap().join("config.toml")).unwrap();
    ui.click(cx, "first-run-auto");
    assert!(ui.read(cx, |app, _| app.permission_choice_open
        && app.permission_choice_error.is_some()
        && app.approval == ApprovalMode::AskForChanges));
    ui.click(cx, "first-run-cancel");
    assert!(!ui.read(cx, |app, _| app.permission_choice_open));
}

#[gpui_kit::test]
fn sessions_are_saved_and_listed_after_a_restart(cx: &mut TestAppContext) {
    let options = test_options();
    let ui = open_with(cx, options.clone());
    let engine = ui.engine(cx);
    ui.input(cx, "persist me");
    ui.press(cx, "enter");
    for event in [
        AgentEvent::TurnStarted { turn_id: 1 },
        AgentEvent::TextDelta("Saved.".into()),
        AgentEvent::TurnFinished {
            turn_id: 1,
            reason: TurnEndReason::Completed,
        },
    ] {
        engine.send(cx, event);
    }
    let again = open_with(cx, options);
    let restored = again.read(cx, |app, _| {
        app.sessions
            .iter()
            .find(|s| s.title() == "persist me")
            .map(|s| (s.status(), s.view.items.len(), s.dir.is_some()))
    });
    assert_eq!(restored, Some((flint_app::session::Status::Done, 3, true)));
}

#[gpui_kit::test]
fn rename_archive_and_undo_from_the_session_context_menu(cx: &mut TestAppContext) {
    let options = test_options();
    let ui = open_with(cx, options.clone());
    let engine = ui.engine(cx);
    ui.input(cx, "rename me");
    ui.press(cx, "enter");
    drop(engine);
    settle(cx);
    let dir = ui.read(cx, |app, _| app.session().dir.clone().unwrap());
    ui.with(cx, |window, cx| window.right_click(("session", 0usize), cx));
    cx.run_until_parked();
    ui.click(cx, ("rename-session", 0usize));
    assert!(ui.read(cx, |app, _| app.renaming.is_some()));
    ui.press(cx, "cmd-a");
    ui.input(cx, "Better title");
    ui.press(cx, "enter");
    assert_eq!(
        ui.read(cx, |app, _| app.sessions[0].title()),
        "Better title"
    );
    let meta = std::fs::read_to_string(dir.join("meta.json")).unwrap();
    assert!(meta.contains("Better title"), "{meta}");
    ui.with(cx, |window, cx| window.right_click(("session", 0usize), cx));
    cx.run_until_parked();
    ui.click(cx, ("delete-session", 0usize));
    assert!(ui.read(cx, |app, _| {
        app.sessions.iter().all(|s| s.title() != "Better title")
    }));
    assert!(!dir.exists(), "archiving removes it from the active list");
    assert!(
        home_archive(&options, &dir).exists(),
        "history remains archived"
    );
    ui.click(cx, "undo-archive");
    assert!(dir.exists(), "undo restores the original history");
    assert_eq!(ui.read(cx, |app, _| app.session().title()), "Better title");
}

fn home_archive(options: &Options, dir: &std::path::Path) -> std::path::PathBuf {
    options
        .home
        .as_ref()
        .unwrap()
        .join("archive")
        .join(dir.file_name().unwrap())
}

fn seed_sidebar_sessions(ui: &Ui, cx: &mut TestAppContext, count: usize) {
    ui.app.update(cx, |app, cx| {
        for ix in 1..count {
            let mut session =
                flint_app::session::Session::new(50_000 + ix as u64, app.workspace.clone());
            let change = session.view.push_user(format!("Sidebar task {ix}"));
            session.apply(change);
            session.touched = std::time::UNIX_EPOCH + std::time::Duration::from_secs(ix as u64);
            app.sessions.push(session);
        }
        cx.notify();
    });
    ui.with(cx, |window, cx| window.render_frame(cx));
}

#[gpui_kit::test]
fn sidebar_session_scrollbar_supports_wheel_track_and_drag_without_selecting(
    cx: &mut TestAppContext,
) {
    let ui = open(cx);
    seed_sidebar_sessions(&ui, cx, 40);
    assert!(ui.read(cx, |app, _| app.sidebar_scroll.max_offset().y > px(1000.)));
    ui.with(cx, |window, cx| {
        window.scroll(
            "session-list",
            ScrollDelta::Pixels(point(px(0.), px(-100.))),
            cx,
        );
    });
    let wheel = ui.read(cx, |app, _| app.sidebar_scroll.offset().y);
    assert!(wheel < px(0.));
    ui.with(cx, |window, cx| {
        let bounds = window.find("session-list").bounds();
        window.click_at(
            "session-list",
            point(bounds.size.width - px(6.), bounds.size.height - px(8.)),
            cx,
        );
    });
    let track = ui.read(cx, |app, _| app.sidebar_scroll.offset().y);
    assert!(track < wheel, "scrollbar track must move the session list");
    ui.with(cx, |window, cx| {
        let bounds = window.find("session-list").bounds();
        window.drag(
            point(bounds.right() - px(6.), bounds.bottom() - px(8.)),
            point(bounds.right() - px(6.), bounds.top() + px(8.)),
            cx,
        );
        window.simulate_next_frame(cx);
        window.render_frame(cx);
    });
    let dragged = ui.read(cx, |app, _| app.sidebar_scroll.offset().y);
    assert!(dragged > track, "dragging the thumb must scroll up");
    assert_eq!(ui.read(cx, |app, _| app.active), 0);
    ui.with(cx, |window, cx| {
        window.render_frame(cx);
        window.render_frame(cx);
    });
    assert_eq!(ui.read(cx, |app, _| app.sidebar_scroll.offset().y), dragged);
}

#[gpui_kit::test]
fn sidebar_archive_scrollbar_keeps_restore_and_navigation_reachable(cx: &mut TestAppContext) {
    let options = test_options();
    for ix in 0..16 {
        let dir = options
            .home
            .as_ref()
            .unwrap()
            .join("archive")
            .join(format!("saved-{ix:02}"));
        flint_app::store::write_meta(
            &dir,
            &flint_app::store::Meta {
                id: format!("saved-{ix:02}"),
                title: Some(format!("Archived {ix} {}", "long-title-".repeat(20))),
                workspace: options.workspace.clone().unwrap(),
                created_at: ix,
                updated_at: ix,
                agent: flint_agent::AgentKind::Flint,
                general: false,
            },
        )
        .unwrap();
        flint_app::store::append(
            &dir,
            &flint_app::store::Logged::User(format!("Archived task {ix}")),
        )
        .unwrap();
    }
    let ui = open_with(
        cx,
        Options {
            window_size: Some((900., 560.)),
            ..options
        },
    );
    ui.click(cx, "sessions-control");
    seed_sidebar_sessions(&ui, cx, 20);
    ui.with(cx, |window, cx| {
        window.render_frame(cx);
        let sidebar = window.find("sidebar").bounds();
        let sessions = window.find("session-list").bounds();
        let archive = window.find("archive-list").bounds();
        let settings = window.find("settings").bounds();
        assert!(sessions.size.height > px(80.));
        assert!(archive.size.height <= px(112.));
        assert!(sessions.bottom() <= archive.top());
        assert!(archive.bottom() <= settings.top());
        assert!(settings.bottom() <= sidebar.bottom());
        // Archives list a page at a time; "Show more" holds back the rest.
        for ix in 0..10usize {
            let row = window.find(("restore-archive", ix)).bounds();
            assert!(row.right() <= archive.right() - px(12.));
        }
        assert!(window.try_find(("restore-archive", 10usize)).is_none());
        window.scroll(
            "archive-list",
            ScrollDelta::Pixels(point(px(0.), px(-34.))),
            cx,
        );
    });
    let wheel = ui.read(cx, |app, _| app.archive_scroll.offset().y);
    assert!(wheel < px(0.));
    let sessions_offset = ui.read(cx, |app, _| app.sidebar_scroll.offset());
    ui.with(cx, |window, cx| {
        let bounds = window.find("archive-list").bounds();
        window.click_at(
            "archive-list",
            point(bounds.size.width - px(6.), bounds.size.height - px(4.)),
            cx,
        );
    });
    assert!(ui.read(cx, |app, _| app.archive_scroll.offset().y < wheel));
    assert_eq!(
        ui.read(cx, |app, _| app.sidebar_scroll.offset()),
        sessions_offset
    );
    ui.with(cx, |window, cx| {
        let scroll = ui.app.read(cx).archive_scroll.clone();
        scroll.scroll_to_bottom();
        window.render_frame(cx);
    });
    ui.with(cx, |window, cx| {
        let scroll = ui.app.read(cx).archive_scroll.clone();
        scroll.scroll_to_bottom();
        window.render_frame(cx);
    });
    ui.click(cx, "show-more-archives");
    assert!(!has(&ui, cx, "show-more-archives"));
    ui.with(cx, |window, cx| {
        let scroll = ui.app.read(cx).archive_scroll.clone();
        scroll.scroll_to_bottom();
        window.render_frame(cx);
    });
    ui.click(cx, ("restore-archive", 15usize));
    assert_eq!(ui.read(cx, |app, _| app.archives.len()), 15);
    assert!(ui.read(cx, |app, _| {
        app.session().title().starts_with("Archived 0 ")
    }));
    assert!(!ui.read(cx, |app, _| app.session_drawer));
}

#[gpui_kit::test]
fn sidebar_long_workspace_titles_and_statuses_stay_inside_the_panel(cx: &mut TestAppContext) {
    for width in [900., 1000.] {
        let ui = open_with(
            cx,
            Options {
                window_size: Some((width, 560.)),
                ..test_options()
            },
        );
        ui.app.update(cx, |app, cx| {
            app.sessions[0].workspace = app.workspace.join("long-workspace-name-".repeat(10));
            let change = app.sessions[0]
                .view
                .push_user("very-long-title-".repeat(40));
            app.sessions[0].apply(change);
            app.sessions[0].agent = flint_agent::AgentKind::ClaudeCode;
            cx.notify();
        });
        if width < 1000. {
            ui.click(cx, "sessions-control");
        }
        ui.with(cx, |window, cx| {
            window.render_frame(cx);
            let panel = window.find("sidebar").bounds();
            let actions = window.find(("session-actions", 0usize)).bounds();
            for id in [
                ("workspace-label", 0usize),
                ("sidebar-title", 0usize),
                ("sidebar-subtitle", 0usize),
            ] {
                let bounds = window.find(id).bounds();
                assert!(bounds.size.width > px(20.), "{id:?}: {bounds:?}");
                assert!(
                    bounds.left() >= panel.left() && bounds.right() <= panel.right() - px(12.),
                    "{id:?}: {bounds:?} outside {panel:?}"
                );
            }
            let title = window.find(("sidebar-title", 0usize)).bounds();
            assert!(title.right() <= actions.left());
            assert_eq!(actions.size, size(px(24.), px(24.)));
        });
    }
}

#[gpui_kit::test]
fn sidebar_search_and_empty_filters_recover_and_reset_scroll(cx: &mut TestAppContext) {
    use flint_app::app::SessionFilter;
    use flint_app::session::Bucket;
    let ui = open(cx);
    seed_sidebar_sessions(&ui, cx, 30);
    ui.with(cx, |window, cx| {
        window.scroll(
            "session-list",
            ScrollDelta::Pixels(point(px(0.), px(-300.))),
            cx,
        );
    });
    assert!(ui.read(cx, |app, _| app.sidebar_scroll.offset().y < px(0.)));
    ui.click(cx, "session-search");
    ui.input(cx, "  SIDEBAR TASK 12  ");
    assert_eq!(ui.read(cx, |app, cx| app.visible_sessions(cx)), vec![12]);
    assert_eq!(ui.read(cx, |app, _| app.sidebar_scroll.offset().y), px(0.));
    ui.press(cx, "cmd-a");
    ui.input(cx, "no-such-session");
    assert!(has(&ui, cx, "session-list-empty"));
    ui.click(cx, "reset-session-filter");
    assert_eq!(ui.read(cx, |app, cx| app.visible_sessions(cx).len()), 30);
    assert!(ui.with(cx, |window, cx| {
        ui.app.read(cx).search.focus_handle(cx).is_focused(window)
    }));
    ui.app.update(cx, |app, cx| {
        app.set_filter(SessionFilter::Only(Bucket::Working), cx)
    });
    assert!(has(&ui, cx, "session-list-empty"));
    ui.click(cx, "reset-session-filter");
    assert_eq!(ui.read(cx, |app, _| app.filter), SessionFilter::All);
    ui.click(cx, "session-search");
    let workspace = ui.read(cx, |app, _| app.workspace.to_string_lossy().to_uppercase());
    ui.input(cx, &format!("  {workspace}  "));
    assert_eq!(ui.read(cx, |app, cx| app.visible_sessions(cx).len()), 30);
    ui.app.update(cx, |app, cx| {
        app.set_filter(SessionFilter::Only(Bucket::Ready), cx)
    });
    ui.press(cx, "cmd-n");
    assert_eq!(ui.read(cx, |app, _| app.filter), SessionFilter::All);
    assert_eq!(
        ui.read(cx, |app, cx| app.search.read(cx).value().to_string()),
        ""
    );
    assert!(ui.read(cx, |app, cx| app.visible_sessions(cx).contains(&app.active)));
}

#[gpui_kit::test]
fn sidebar_needs_you_filter_holds_work_waiting_for_approval(cx: &mut TestAppContext) {
    use flint_app::app::SessionFilter;
    use flint_app::session::Bucket;
    let ui = open(cx);
    let engine = running_turn(&ui, cx);
    ui.press(cx, "cmd-n");
    approval_requested(&engine, cx, "sidebar-approval");
    // Waiting on an approval is "Needs you", not "Working".
    let counts = ui.read(cx, |app, cx| app.bucket_counts(cx));
    assert_eq!(counts.of(SessionFilter::Only(Bucket::NeedsYou)), 1);
    assert_eq!(counts.of(SessionFilter::Only(Bucket::Working)), 0);
    assert_eq!(counts.of(SessionFilter::All), 2);
    ui.click(cx, ("session-filter", 1usize));
    assert_eq!(
        ui.read(cx, |app, _| app.filter),
        SessionFilter::Only(Bucket::NeedsYou)
    );
    assert_eq!(ui.read(cx, |app, cx| app.visible_sessions(cx)), vec![0]);
    ui.click(cx, ("session", 0usize));
    assert!(ui.read(cx, |app, _| app.session().view.pending_approvals == 1));
}

#[gpui_kit::test]
fn sidebar_search_matches_the_last_message(cx: &mut TestAppContext) {
    let ui = open(cx);
    seed_sidebar_sessions(&ui, cx, 3);
    ui.app.update(cx, |app, cx| {
        let change = app.sessions[2]
            .view
            .push_user("Where did the Zebra config go?".to_string());
        app.sessions[2].apply(change);
        cx.notify();
    });
    ui.click(cx, "session-search");
    ui.input(cx, "zebra");
    assert_eq!(ui.read(cx, |app, cx| app.visible_sessions(cx)), vec![2]);
}

#[gpui_kit::test]
fn sidebar_groups_by_project_status_or_agent_and_counts_hidden_sessions(cx: &mut TestAppContext) {
    use flint_app::app::{SessionFilter, SessionGrouping};
    use flint_app::session::Bucket;
    use flint_app::session_groups::GroupKey;
    let ui = open(cx);
    seed_sidebar_sessions(&ui, cx, 4);
    ui.app.update(cx, |app, cx| {
        app.sessions[1].agent = flint_agent::AgentKind::ClaudeCode;
        app.sessions[2].agent = flint_agent::AgentKind::ClaudeCode;
        app.sessions[3].unread = true;
        let change = app.sessions[0].view.push_user("active".to_string());
        app.sessions[0].apply(change);
        cx.notify();
    });
    let groups =
        |ui: &Ui, cx: &mut TestAppContext| ui.read(cx, |app, cx| app.session_groups(cx).groups);
    // One workspace: one project group, with every session in it.
    let by_project = groups(&ui, cx);
    assert_eq!(by_project.len(), 1);
    assert_eq!(by_project[0].rows.len(), 4);
    assert!(matches!(by_project[0].key, GroupKey::Project(..)));

    ui.click(cx, "session-grouping");
    assert_eq!(ui.read(cx, |app, _| app.grouping), SessionGrouping::Status);
    let by_status = groups(&ui, cx);
    let labels: Vec<_> = by_status.iter().map(|g| g.label.as_str()).collect();
    // Unread work sorts under "Needs you", ahead of the rest.
    assert_eq!(labels, ["Needs you", "Inactive"]);
    assert_eq!(by_status[0].rows, vec![3]);

    ui.click(cx, "session-grouping");
    assert_eq!(ui.read(cx, |app, _| app.grouping), SessionGrouping::Agent);
    let by_agent = groups(&ui, cx);
    let mut sizes: Vec<_> = by_agent
        .iter()
        .map(|g| (g.label.clone(), g.rows.len()))
        .collect();
    sizes.sort();
    assert_eq!(
        sizes,
        [("Claude Code".to_string(), 2), ("flint".to_string(), 2)]
    );

    // A filter that hides part of a heading says so in the heading's count.
    ui.app.update(cx, |app, cx| {
        app.set_filter(SessionFilter::Only(Bucket::NeedsYou), cx)
    });
    let filtered = groups(&ui, cx);
    assert_eq!(filtered.len(), 1);
    assert_eq!((filtered[0].rows.len(), filtered[0].total), (1, 2));

    ui.press(cx, "cmd-shift-g");
    assert_eq!(ui.read(cx, |app, _| app.grouping), SessionGrouping::Project);
}

#[gpui_kit::test]
fn sidebar_groups_every_worktree_of_a_repository_together(cx: &mut TestAppContext) {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    let linked = tmp.path().join("repo-feature");
    let other = tmp.path().join("other");
    let admin = repo.join(".git/worktrees/repo-feature");
    std::fs::create_dir_all(&admin).unwrap();
    std::fs::create_dir_all(&linked).unwrap();
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(admin.join("commondir"), "../..\n").unwrap();
    std::fs::write(repo.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
    std::fs::write(admin.join("HEAD"), "ref: refs/heads/feature\n").unwrap();
    std::fs::write(
        linked.join(".git"),
        format!("gitdir: {}\n", admin.display()),
    )
    .unwrap();
    let ui = open(cx);
    seed_sidebar_sessions(&ui, cx, 4);
    ui.app.update(cx, |app, cx| {
        app.sessions[0].workspace = repo.clone();
        app.sessions[1].workspace = linked.clone();
        app.sessions[2].workspace = other.clone();
        app.sessions[3].workspace = repo.clone();
        cx.notify();
    });
    let groups = ui.read(cx, |app, cx| app.session_groups(cx).groups);
    let mut labels: Vec<_> = groups
        .iter()
        .map(|g| (g.label.clone(), g.rows.len()))
        .collect();
    labels.sort();
    assert_eq!(labels, [("other".to_string(), 1), ("repo".to_string(), 3)]);
    // The row for the worktree shows its own branch.
    assert!(ui.with(cx, |window, cx| {
        window.render_frame(cx);
        window.try_find(("sidebar-details", 1usize)).is_some()
    }));
    assert_eq!(
        flint_app::project::branch(&linked).as_deref(),
        Some("feature")
    );
}

#[gpui_kit::test]
fn sidebar_lists_a_page_of_sessions_then_shows_more(cx: &mut TestAppContext) {
    use flint_app::app::SESSION_PAGE;
    let ui = open(cx);
    seed_sidebar_sessions(&ui, cx, SESSION_PAGE + 15);
    let groups = ui.read(cx, |app, cx| app.session_groups(cx));
    let shown: usize = groups.groups.iter().map(|g| g.rows.len()).sum();
    assert_eq!((shown, groups.more), (SESSION_PAGE, 15));
    assert_eq!(groups.groups[0].total, SESSION_PAGE + 15);
    // Searching still reaches a session on a later page.
    ui.click(cx, "session-search");
    ui.input(cx, "sidebar task 6");
    assert_eq!(
        ui.read(cx, |app, cx| app.visible_sessions(cx).len()),
        // 6 and 60..=64
        6
    );
    ui.press(cx, "cmd-a");
    ui.press(cx, "backspace");
    assert!(has(&ui, cx, "show-more-sessions"));
    ui.with(cx, |window, cx| {
        let scroll = ui.app.read(cx).sidebar_scroll.clone();
        scroll.scroll_to_bottom();
        window.render_frame(cx);
    });
    ui.click(cx, "show-more-sessions");
    let groups = ui.read(cx, |app, cx| app.session_groups(cx));
    assert_eq!(groups.more, 0);
    assert_eq!(
        groups.groups.iter().map(|g| g.rows.len()).sum::<usize>(),
        SESSION_PAGE + 15
    );
    assert!(!has(&ui, cx, "show-more-sessions"));
}

#[gpui_kit::test]
fn sidebar_paging_always_keeps_the_active_session_listed(cx: &mut TestAppContext) {
    use flint_app::app::SESSION_PAGE;
    let ui = open(cx);
    seed_sidebar_sessions(&ui, cx, SESSION_PAGE + 5);
    // Make the active session the oldest, so recency alone would page it out.
    ui.app.update(cx, |app, cx| {
        app.sessions[0].touched = std::time::UNIX_EPOCH;
        cx.notify();
    });
    let groups = ui.read(cx, |app, cx| app.session_groups(cx));
    let rows: Vec<usize> = groups.groups.iter().flat_map(|g| g.rows.clone()).collect();
    assert!(rows.contains(&0));
    assert_eq!(rows.len(), SESSION_PAGE + 1);
    assert_eq!(groups.more, 4);
}

#[gpui_kit::test]
fn sidebar_session_actions_work_with_mouse_and_keyboard_without_selecting(cx: &mut TestAppContext) {
    let ui = open(cx);
    seed_sidebar_sessions(&ui, cx, 2);
    ui.click(cx, ("session-actions", 1usize));
    assert_eq!(
        ui.read(cx, |app, _| (app.active, app.session_menu)),
        (0, Some(1))
    );
    let actions_focus = ui.with(cx, |window, cx| window.focused(cx));
    assert!(actions_focus.is_some());
    ui.with(cx, |window, cx| window.render_frame(cx));
    assert_eq!(ui.with(cx, |window, cx| window.focused(cx)), actions_focus);
    ui.key_cycle(cx, "space");
    assert_eq!(
        ui.read(cx, |app, _| (app.active, app.session_menu)),
        (0, None)
    );
    ui.key_cycle(cx, "enter");
    assert_eq!(
        ui.read(cx, |app, _| (app.active, app.session_menu)),
        (0, Some(1))
    );
    ui.click(cx, ("rename-session", 1usize));
    assert_eq!(ui.read(cx, |app, _| app.active), 0);
    ui.click(cx, ("rename-input", 1usize));
    assert!(ui.read(cx, |app, _| app.renaming.is_some()));
    assert_eq!(ui.read(cx, |app, _| app.active), 0);
    ui.press(cx, "cmd-a");
    ui.input(cx, "Renamed background task");
    ui.press(cx, "enter");
    assert_eq!(
        ui.read(cx, |app, _| app.sessions[1].title()),
        "Renamed background task"
    );
    assert_eq!(ui.read(cx, |app, _| app.active), 0);
}

#[gpui_kit::test]
fn sidebar_rename_blur_does_not_steal_focus_and_obsolete_inputs_cannot_cancel_it(
    cx: &mut TestAppContext,
) {
    let ui = open(cx);
    ui.with(cx, |window, _| window.activate_window());
    cx.run_until_parked();
    ui.with(cx, |window, cx| {
        ui.app.update(cx, |app, cx| app.start_rename(0, window, cx))
    });
    cx.run_until_parked();
    let old = ui.read(cx, |app, _| app.renaming.as_ref().unwrap().1.clone());
    ui.with(cx, |window, cx| {
        ui.app.update(cx, |app, cx| app.start_rename(0, window, cx))
    });
    cx.run_until_parked();
    old.update(cx, |_, cx| {
        cx.emit(gpui_kit::component::input::InputEvent::Blur)
    });
    cx.run_until_parked();
    assert!(ui.read(cx, |app, _| app.renaming.is_some()));
    ui.click(cx, "session-search");
    assert!(ui.read(cx, |app, _| app.renaming.is_none()));
    assert!(ui.with(cx, |window, cx| {
        ui.app.read(cx).search.focus_handle(cx).is_focused(window)
    }));
    ui.input(cx, "search stays focused");
    assert_eq!(
        ui.read(cx, |app, cx| app.search.read(cx).value().to_string()),
        "search stays focused"
    );
}

#[gpui_kit::test]
fn sidebar_escape_dismisses_actions_and_rename_before_the_drawer(cx: &mut TestAppContext) {
    let ui = open_with(
        cx,
        Options {
            window_size: Some((900., 560.)),
            ..test_options()
        },
    );
    let engine = running_turn(&ui, cx);
    engine.sent();
    ui.click(cx, "sessions-control");
    ui.click(cx, ("session-actions", 0usize));
    ui.press(cx, "escape");
    assert!(ui.read(cx, |app, _| app.session_menu.is_none()
        && app.session_drawer));
    ui.click(cx, ("session-actions", 0usize));
    ui.click(cx, ("rename-session", 0usize));
    ui.press(cx, "cmd-a");
    ui.input(cx, "cancel this title");
    ui.press(cx, "escape");
    assert!(ui.read(cx, |app, _| app.renaming.is_none() && app.session_drawer));
    assert!(ui.with(cx, |window, cx| {
        ui.app.read(cx).search.focus_handle(cx).is_focused(window)
    }));
    ui.click(cx, ("session-actions", 0usize));
    ui.click(cx, ("rename-session", 0usize));
    ui.press(cx, "cmd-a");
    ui.input(cx, "Committed title");
    ui.press(cx, "enter");
    assert_eq!(
        ui.read(cx, |app, _| app.session().title()),
        "Committed title"
    );
    assert!(ui.with(cx, |window, cx| {
        ui.app.read(cx).search.focus_handle(cx).is_focused(window)
    }));
    ui.press(cx, "escape");
    assert!(!ui.read(cx, |app, _| app.session_drawer));
    ui.key_cycle(cx, "space");
    assert!(
        ui.read(cx, |app, _| app.session_drawer),
        "focus must return to the drawer opener"
    );
    assert!(
        engine.sent().is_empty(),
        "sidebar Escape must not interrupt work"
    );
}

#[gpui_kit::test]
fn sidebar_palette_rename_is_visible_and_focused_when_narrow_and_filtered(cx: &mut TestAppContext) {
    let ui = open_with(
        cx,
        Options {
            window_size: Some((900., 560.)),
            ..test_options()
        },
    );
    seed_sidebar_sessions(&ui, cx, 30);
    ui.click(cx, "sessions-control");
    ui.click(cx, "session-search");
    ui.input(cx, "no matching title");
    ui.press(cx, "escape");
    ui.press(cx, "cmd-k");
    ui.input(cx, "Rename session");
    ui.press(cx, "enter");
    assert!(ui.read(cx, |app, _| app.palette.is_none() && app.session_drawer));
    assert!(ui.with(cx, |window, cx| {
        window.render_frame(cx);
        window.try_find(("rename-input", 0usize)).is_some()
    }));
    assert!(ui.with(cx, |window, cx| {
        let app = ui.app.read(cx);
        app.renaming
            .as_ref()
            .unwrap()
            .1
            .focus_handle(cx)
            .is_focused(window)
    }));
    ui.press(cx, "cmd-a");
    ui.input(cx, "Renamed from palette");
    ui.press(cx, "enter");
    assert_eq!(
        ui.read(cx, |app, _| app.session().title()),
        "Renamed from palette"
    );
    assert!(ui.read(cx, |app, _| app.renaming.is_none()));
}

#[gpui_kit::test]
fn sidebar_switching_and_new_sessions_cancel_rename_without_changing_titles(
    cx: &mut TestAppContext,
) {
    let ui = open(cx);
    seed_sidebar_sessions(&ui, cx, 2);
    ui.with(cx, |window, cx| {
        ui.app.update(cx, |app, cx| app.start_rename(1, window, cx))
    });
    ui.press(cx, "cmd-a");
    ui.input(cx, "Do not save this");
    ui.click(cx, ("session", 0usize));
    assert!(ui.read(cx, |app, _| app.renaming.is_none()));
    assert_eq!(
        ui.read(cx, |app, _| app.sessions[1].title()),
        "Sidebar task 1"
    );
    ui.with(cx, |window, cx| {
        ui.app.update(cx, |app, cx| app.start_rename(1, window, cx))
    });
    ui.press(cx, "cmd-n");
    assert!(ui.read(cx, |app, _| app.renaming.is_none()));
    assert!(ui.with(cx, |window, cx| {
        ui.app.read(cx).composer.focus_handle(cx).is_focused(window)
    }));
}

#[gpui_kit::test]
fn sidebar_archive_cancels_rename_before_session_indices_shift(cx: &mut TestAppContext) {
    let ui = open(cx);
    seed_sidebar_sessions(&ui, cx, 3);
    ui.with(cx, |window, cx| {
        ui.app.update(cx, |app, cx| app.start_rename(1, window, cx))
    });
    ui.press(cx, "cmd-a");
    ui.input(cx, "Wrong session title");
    ui.with(cx, |window, cx| {
        ui.app
            .update(cx, |app, cx| app.delete_session(0, window, cx))
    });
    assert!(ui.read(cx, |app, _| app.renaming.is_none()));
    ui.with(cx, |window, cx| {
        ui.app.update(cx, |app, cx| app.commit_rename(window, cx))
    });
    assert_eq!(
        ui.read(cx, |app, _| app.sessions[0].title()),
        "Sidebar task 1"
    );
    assert_eq!(
        ui.read(cx, |app, _| app.sessions[1].title()),
        "Sidebar task 2"
    );
}

#[gpui_kit::test]
fn sidebar_closing_drawer_cancels_rename_and_preserves_the_title(cx: &mut TestAppContext) {
    let ui = open_with(
        cx,
        Options {
            window_size: Some((900., 560.)),
            ..test_options()
        },
    );
    ui.click(cx, "sessions-control");
    ui.click(cx, ("session-actions", 0usize));
    ui.click(cx, ("rename-session", 0usize));
    let title = ui.read(cx, |app, _| app.session().title());
    ui.press(cx, "cmd-a");
    ui.input(cx, "Discarded edit");
    ui.click(cx, "hide-sidebar");
    assert!(ui.read(cx, |app, _| !app.session_drawer && app.renaming.is_none()));
    assert_eq!(ui.read(cx, |app, _| app.session().title()), title);
    ui.key_cycle(cx, "space");
    assert!(ui.read(cx, |app, _| app.session_drawer));
}

#[gpui_kit::test]
fn sidebar_offscreen_rename_scrolls_into_view_once_and_preserves_manual_scrolling(
    cx: &mut TestAppContext,
) {
    let ui = open_with(
        cx,
        Options {
            window_size: Some((1000., 700.)),
            ..test_options()
        },
    );
    seed_sidebar_sessions(&ui, cx, 40);
    ui.with(cx, |window, cx| {
        ui.app.update(cx, |app, cx| app.start_rename(1, window, cx));
        window.render_frame(cx);
        for _ in 0..3 {
            window.simulate_next_frame(cx);
            window.render_frame(cx);
        }
        let editor = window.find(("rename-input", 1usize)).bounds();
        let list = window.find("session-list").bounds();
        assert!(
            editor.top() >= list.top() && editor.bottom() <= list.bottom(),
            "{editor:?} outside {list:?}"
        );
        window.scroll(
            "session-list",
            ScrollDelta::Pixels(point(px(0.), px(100.))),
            cx,
        );
    });
    let offset = ui.read(cx, |app, _| app.sidebar_scroll.offset());
    ui.with(cx, |window, cx| {
        for _ in 0..3 {
            window.simulate_next_frame(cx);
            window.render_frame(cx);
        }
    });
    assert_eq!(ui.read(cx, |app, _| app.sidebar_scroll.offset()), offset);
}

#[gpui_kit::test]
fn sidebar_cancelled_rename_cannot_apply_a_delayed_scroll(cx: &mut TestAppContext) {
    let ui = open(cx);
    seed_sidebar_sessions(&ui, cx, 40);
    ui.with(cx, |window, cx| {
        ui.app.update(cx, |app, cx| app.start_rename(1, window, cx));
        window.render_frame(cx);
        window.simulate_next_frame(cx);
        ui.app.update(cx, |app, cx| app.cancel_rename(window, cx));
        window.render_frame(cx);
    });
    let offset = ui.read(cx, |app, _| app.sidebar_scroll.offset());
    ui.with(cx, |window, cx| {
        for _ in 0..3 {
            window.simulate_next_frame(cx);
            window.render_frame(cx);
        }
    });
    assert!(ui.read(cx, |app, _| app.renaming.is_none()));
    assert_eq!(ui.read(cx, |app, _| app.sidebar_scroll.offset()), offset);
}

#[gpui_kit::test]
fn sidebar_tab_navigation_scrolls_offscreen_sessions_into_view(cx: &mut TestAppContext) {
    let ui = open(cx);
    seed_sidebar_sessions(&ui, cx, 40);
    ui.click(cx, "session-search");
    // Only the filter strip's "All" chip precedes the list; grouping is
    // beside Settings in the footer.
    for _ in 0..42 {
        ui.press(cx, "tab");
        ui.with(cx, |window, cx| {
            window.render_frame(cx);
            window.simulate_next_frame(cx);
            window.render_frame(cx);
        });
    }
    assert_eq!(ui.read(cx, |app, _| app.active), 0);
    ui.with(cx, |window, cx| {
        window.render_frame(cx);
        let row = window.find(("session", 20usize)).bounds();
        let list = window.find("session-list").bounds();
        assert!(
            row.top() >= list.top() && row.bottom() <= list.bottom(),
            "{row:?} outside {list:?}"
        );
    });
    ui.key_cycle(cx, "enter");
    assert_eq!(ui.read(cx, |app, _| app.active), 20);
}

#[gpui_kit::test]
fn sidebar_actions_tab_to_rename_and_accept_without_selecting_the_session(cx: &mut TestAppContext) {
    let ui = open(cx);
    seed_sidebar_sessions(&ui, cx, 2);
    ui.click(cx, ("session-actions", 1usize));
    ui.press(cx, "tab");
    ui.key_cycle(cx, "enter");
    assert_eq!(
        ui.read(cx, |app, _| app.renaming.as_ref().map(|(ix, _)| *ix)),
        Some(1)
    );
    assert_eq!(ui.read(cx, |app, _| app.active), 0);
    ui.press(cx, "cmd-a");
    ui.input(cx, "Keyboard title");
    ui.press(cx, "enter");
    assert_eq!(
        ui.read(cx, |app, _| app.sessions[1].title()),
        "Keyboard title"
    );
}

#[gpui_kit::test]
fn sidebar_auto_collapse_does_not_leave_an_invisible_rename_editor(cx: &mut TestAppContext) {
    let ui = open(cx);
    ui.with(cx, |window, cx| {
        ui.app.update(cx, |app, cx| app.start_rename(0, window, cx))
    });
    cx.simulate_window_resize(ui.window, size(px(900.), px(560.)));
    cx.run_until_parked();
    ui.with(cx, |window, cx| window.render_frame(cx));
    assert!(ui.read(cx, |app, _| app.renaming.is_none() || app.session_drawer));
    ui.press(cx, "escape");
    ui.press(cx, "escape");
    assert!(ui.with(cx, |window, cx| window.focused(cx).is_some()));
}

fn wait_for(ui: &Ui, cx: &mut TestAppContext, ready: impl Fn(&FlintApp) -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
    while !ui.read(cx, |app, _| ready(app)) {
        assert!(
            std::time::Instant::now() < deadline,
            "local engine condition timed out"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
        settle(cx);
    }
}

#[gpui_kit::test]
fn real_engine_archive_drains_approval_and_history_then_undo_and_restart_recover(
    cx: &mut TestAppContext,
) {
    let provider = local_provider::LocalProvider::approval();
    let options = test_options();
    let ui = open_with(cx, options.clone());
    ui.app.update(cx, |app, _| {
        app.settings.base_url = provider.url.clone();
        app.settings.model = "fixture-model".into();
    });
    ui.input(cx, "local fixture");
    ui.press(cx, "enter");
    wait_for(&ui, cx, |app| app.sessions[0].view.pending_approvals == 1);
    let dir = ui.read(cx, |app, _| app.sessions[0].dir.clone().unwrap());
    let archive = home_archive(&options, &dir);
    ui.app.update(cx, |app, _| {
        app.sessions[0]
            .ops
            .as_ref()
            .unwrap()
            .try_send(Op::UserMessage("queued fixture".into()))
            .unwrap();
    });
    ui.with(cx, |window, cx| {
        ui.app
            .update(cx, |app, cx| app.delete_session(0, window, cx))
    });
    assert!(dir.exists() && !archive.exists());
    assert!(ui.read(cx, |app, _| app.session().view.running));
    ui.with(cx, |window, cx| {
        ui.app
            .update(cx, |app, cx| app.delete_session(0, window, cx))
    });
    wait_for(&ui, cx, |app| {
        !app.sessions[0].view.running && app.sessions[0].ops.is_none()
    });
    assert_eq!(
        ui.read(cx, |app, _| app.sessions[0].view.pending_approvals),
        0
    );
    assert!(
        !options
            .workspace
            .as_ref()
            .unwrap()
            .join("fixture.txt")
            .exists()
    );
    ui.press(cx, "cmd-n");
    let history = std::fs::read(dir.join("history.json")).unwrap();
    assert_eq!(
        provider
            .completion_requests
            .load(std::sync::atomic::Ordering::Relaxed),
        1
    );
    let events = std::fs::read(dir.join("events.jsonl")).unwrap();
    ui.with(cx, |window, cx| {
        ui.app
            .update(cx, |app, cx| app.delete_session(0, window, cx))
    });
    assert!(archive.exists() && !dir.exists());
    assert_eq!(
        std::fs::read(archive.join("history.json")).unwrap(),
        history
    );
    ui.with(cx, |window, cx| {
        ui.app.update(cx, |app, cx| app.undo_archive(window, cx))
    });
    assert_eq!(std::fs::read(dir.join("events.jsonl")).unwrap(), events);
    assert_eq!(std::fs::read(dir.join("history.json")).unwrap(), history);
    assert!(ui.read(cx, |app, _| app.session().ops.is_none()));
    ui.with(cx, |window, cx| {
        ui.app
            .update(cx, |app, cx| app.delete_session(app.active, window, cx))
    });
    let again = open_with(cx, options);
    assert!(again.read(cx, |app, _| app.archived_session.is_none()
        && app.archives.len() == 1));
    again.with(cx, |window, cx| {
        again
            .app
            .update(cx, |app, cx| app.restore_archive(0, window, cx))
    });
    assert_eq!(std::fs::read(dir.join("history.json")).unwrap(), history);
    assert_eq!(std::fs::read(dir.join("events.jsonl")).unwrap(), events);
    assert!(again.read(cx, |app, _| !app.session().view.running
        && app.session().view.pending_approvals == 0));
}

#[gpui_kit::test]
fn real_engine_after_undo_resumes_saved_history_without_old_broad_permission(
    cx: &mut TestAppContext,
) {
    let provider = local_provider::LocalProvider::approval();
    let options = test_options();
    let ui = open_with(cx, options);
    ui.app.update(cx, |app, _| {
        app.settings.base_url = provider.url.clone();
        app.settings.model = "fixture-model".into();
    });
    ui.input(cx, "fixture");
    ui.press(cx, "enter");
    wait_for(&ui, cx, |app| app.session().view.pending_approvals == 1);
    ui.app
        .update(cx, |app, _| app.sessions[0].native_allow_all = true);
    for _ in 0..2 {
        ui.with(cx, |window, cx| {
            ui.app
                .update(cx, |app, cx| app.delete_session(0, window, cx))
        });
    }
    wait_for(&ui, cx, |app| app.session().ops.is_none());
    ui.with(cx, |window, cx| {
        ui.app.update(cx, |app, cx| {
            app.delete_session(0, window, cx);
            app.undo_archive(window, cx);
        })
    });
    ui.input(cx, "continuation fixture");
    ui.press(cx, "enter");
    wait_for(&ui, cx, |app| app.session().view.pending_approvals == 1);
    assert!(ui.read(cx, |app, _| app.session().view.turn_id == 2
        && !app.session().native_allow_all
        && app.session().native_approval == Some(ApprovalMode::AskForChanges)));
    ui.app.update(cx, |app, _| {
        app.session()
            .ops
            .as_ref()
            .unwrap()
            .try_send(Op::Shutdown)
            .unwrap();
    });
    wait_for(&ui, cx, |app| app.session().ops.is_none());
}

#[gpui_kit::test]
fn real_engine_failed_history_flush_keeps_session_unarchived(cx: &mut TestAppContext) {
    let provider = local_provider::LocalProvider::approval();
    let options = test_options();
    let ui = open_with(cx, options.clone());
    ui.app.update(cx, |app, _| {
        app.settings.base_url = provider.url.clone();
        app.settings.model = "fixture-model".into();
    });
    ui.input(cx, "local fixture");
    ui.press(cx, "enter");
    wait_for(&ui, cx, |app| app.session().view.pending_approvals == 1);
    let dir = ui.read(cx, |app, _| app.session().dir.clone().unwrap());
    std::fs::create_dir(dir.join("history.json")).unwrap();
    for _ in 0..2 {
        ui.with(cx, |window, cx| {
            ui.app
                .update(cx, |app, cx| app.delete_session(0, window, cx))
        });
    }
    wait_for(&ui, cx, |app| !app.session().view.running);
    ui.with(cx, |window, cx| {
        ui.app
            .update(cx, |app, cx| app.delete_session(0, window, cx))
    });
    wait_for(&ui, cx, |app| app.session().ops.is_none());
    ui.with(cx, |window, cx| {
        ui.app
            .update(cx, |app, cx| app.delete_session(0, window, cx))
    });
    assert!(dir.exists() && !home_archive(&options, &dir).exists());
    assert!(ui.read(cx, |app, _| {
        app.store_error
            .as_deref()
            .is_some_and(|s| s.contains("not saved"))
    }));
}

#[gpui_kit::test]
fn late_events_and_failed_ui_persistence_are_saved_before_archive(cx: &mut TestAppContext) {
    let options = test_options();
    let ui = open_with(cx, options.clone());
    let engine = ui.engine(cx);
    ui.input(cx, "local fixture");
    ui.press(cx, "enter");
    engine.sent();
    let dir = ui.read(cx, |app, _| app.session().dir.clone().unwrap());
    let log = dir.join("events.jsonl");
    let original = std::fs::read(&log).unwrap();
    std::fs::rename(&log, dir.join("saved-events.jsonl")).unwrap();
    std::fs::create_dir(&log).unwrap();
    engine.send(cx, AgentEvent::TextDelta("late fixture event".into()));
    ui.with(cx, |window, cx| {
        ui.app
            .update(cx, |app, cx| app.delete_session(0, window, cx))
    });
    drop(engine);
    settle(cx);
    ui.with(cx, |window, cx| {
        ui.app
            .update(cx, |app, cx| app.delete_session(0, window, cx))
    });
    assert!(dir.exists() && ui.read(cx, |app, _| app.archived_session.is_none()));
    std::fs::remove_dir(&log).unwrap();
    std::fs::rename(dir.join("saved-events.jsonl"), &log).unwrap();
    ui.with(cx, |window, cx| {
        ui.app
            .update(cx, |app, cx| app.delete_session(0, window, cx))
    });
    let bytes = std::fs::read(home_archive(&options, &dir).join("events.jsonl")).unwrap();
    assert!(bytes.starts_with(&original));
    assert_eq!(
        String::from_utf8(bytes)
            .unwrap()
            .matches("late fixture event")
            .count(),
        1
    );
}

#[gpui_kit::test]
fn failed_background_persistence_blocks_new_turns_until_storage_recovers(cx: &mut TestAppContext) {
    let ui = open(cx);
    let engine = ui.engine(cx);
    ui.input(cx, "first fixture");
    ui.press(cx, "enter");
    engine.sent();
    let dir = ui.read(cx, |app, _| app.session().dir.clone().unwrap());
    let log = dir.join("events.jsonl");
    std::fs::rename(&log, dir.join("saved-events.jsonl")).unwrap();
    std::fs::create_dir(&log).unwrap();
    engine.send(cx, AgentEvent::TextDelta("retained fixture".into()));
    ui.app.update(cx, |app, _| {
        assert!(app.sessions[0].flush_records().is_err());
    });
    wait_for(&ui, cx, |app| app.sessions[0].stopping);
    assert!(matches!(engine.sent().as_slice(), [Op::Shutdown]));
    drop(engine);
    settle(cx);
    let next_engine = ui.engine(cx);
    let count = ui.read(cx, |app, _| app.session().view.items.len());
    ui.input(cx, "second fixture");
    ui.press(cx, "enter");
    assert_eq!(ui.composer_text(cx), "second fixture");
    assert!(next_engine.sent().is_empty());
    assert_eq!(ui.read(cx, |app, _| app.session().view.items.len()), count);
    assert!(ui.read(cx, |app, _| {
        app.store_error
            .as_deref()
            .is_some_and(|error| error.contains("Fix storage"))
    }));
    std::fs::remove_dir(&log).unwrap();
    std::fs::rename(dir.join("saved-events.jsonl"), &log).unwrap();
    ui.press(cx, "enter");
    assert!(matches!(next_engine.sent().as_slice(),
        [Op::UserMessage(message)] if message == "second fixture"));
    ui.app
        .update(cx, |app, _| app.sessions[0].flush_records().unwrap());
    let saved = std::fs::read_to_string(log).unwrap();
    assert_eq!(saved.matches("retained fixture").count(), 1);
    assert_eq!(saved.matches("second fixture").count(), 1);
}

#[gpui_kit::test]
fn quitting_drains_all_accepted_session_records(cx: &mut TestAppContext) {
    use flint_app::store::Logged;
    let ui = open(cx);
    let _engine = ui.engine(cx);
    ui.input(cx, "quit fixture");
    ui.press(cx, "enter");
    let dir = ui.read(cx, |app, _| app.session().dir.clone().unwrap());
    ui.app.update(cx, |app, _| {
        for n in 0..4_000 {
            app.sessions[0]
                .log(Logged::User(format!("queued-{n}")))
                .unwrap();
        }
    });
    cx.quit();
    let saved: Vec<Logged> = std::fs::read_to_string(dir.join("events.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let queued: Vec<&str> = saved
        .iter()
        .filter_map(|record| match record {
            Logged::User(message) if message.starts_with("queued-") => Some(message.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(queued.len(), 4_000);
    for (n, message) in queued.iter().enumerate() {
        assert_eq!(*message, format!("queued-{n}"));
    }
}

#[gpui_kit::test]
fn archive_failure_preserves_the_session_and_reports_it(cx: &mut TestAppContext) {
    let options = test_options();
    let ui = open_with(cx, options.clone());
    let engine = ui.engine(cx);
    ui.input(cx, "keep me");
    ui.press(cx, "enter");
    drop(engine);
    settle(cx);
    let dir = ui.read(cx, |app, _| app.session().dir.clone().unwrap());
    let collision = home_archive(&options, &dir);
    std::fs::create_dir_all(&collision).unwrap();
    std::fs::write(collision.join("untouched"), "user data").unwrap();
    ui.with(cx, |window, cx| window.right_click(("session", 0usize), cx));
    cx.run_until_parked();
    ui.click(cx, ("delete-session", 0usize));
    assert!(ui.read(cx, |app, _| {
        app.session().title() == "keep me"
            && app
                .store_error
                .as_deref()
                .is_some_and(|e| e.contains("archive"))
    }));
    assert_eq!(
        std::fs::read_to_string(collision.join("untouched")).unwrap(),
        "user data"
    );
    assert!(dir.exists());
}

#[gpui_kit::test]
fn archiving_running_work_needs_confirmation(cx: &mut TestAppContext) {
    let ui = open(cx);
    let engine = running_turn(&ui, cx);
    ui.with(cx, |window, cx| window.right_click(("session", 0usize), cx));
    cx.run_until_parked();
    ui.click(cx, ("delete-session", 0usize));
    assert!(ui.read(cx, |app, _| app.archive_confirm == Some(app.session().uid)));
    assert!(engine.sent().is_empty());
    ui.click(cx, "archive-stop");
    assert!(matches!(engine.sent().as_slice(), [Op::Shutdown]));
    assert!(ui.read(cx, |app, _| app.archived_session.is_none()));
    engine.send(
        cx,
        AgentEvent::TurnFinished {
            turn_id: 1,
            reason: TurnEndReason::Interrupted,
        },
    );
    ui.click(cx, ("delete-session", 0usize));
    assert!(engine.sent().is_empty(), "shutdown must not be sent twice");
    assert!(ui.read(cx, |app, _| app.archived_session.is_none()));
    drop(engine);
    settle(cx);
    ui.click(cx, ("delete-session", 0usize));
    assert!(ui.read(cx, |app, _| app.archived_session.is_some()));
}

#[gpui_kit::test]
fn collapsed_sidebar_reopens_docked_below_the_window_buttons(cx: &mut TestAppContext) {
    let ui = open(cx);
    for _ in 0..2 {
        ui.click(cx, "hide-sidebar");
        assert!(ui.read(cx, |app, _| !app.sidebar_open && !app.sidebar_visible));
        ui.with(cx, |window, _| {
            assert!(window.find("sessions-control").bounds().top() >= px(36.));
        });
        ui.click(cx, "sessions-control");
        assert!(ui.read(cx, |app, _| {
            app.sidebar_open && app.sidebar_visible && !app.session_drawer
        }));
        ui.with(cx, |window, _| {
            assert!(window.find("hide-sidebar").bounds().top() >= px(36.));
            assert!(window.try_find("sessions-drawer").is_none());
        });
    }
}

#[gpui_kit::test]
fn collapsed_sidebar_reopens_docked_in_tiled_sessions(cx: &mut TestAppContext) {
    use flint_app::docking::Edge;
    let (ui, sessions) = session_pane_fixture(cx, 2);
    drag_session_pane(&ui, cx, sessions[1].0, sessions[0].0, Edge::Right);
    ui.click(cx, "hide-sidebar");
    ui.with(cx, |window, _| {
        assert!(window.find("session-workspace-sidebar").bounds().top() >= px(36.));
    });
    ui.click(cx, "session-workspace-sidebar");
    assert!(ui.read(cx, |app, _| {
        app.sidebar_open && app.sidebar_visible && !app.session_drawer
    }));
    assert!(has(&ui, cx, "session-workspace"));
}

#[gpui_kit::test]
fn compact_sessions_drawer_stays_below_the_window_buttons(cx: &mut TestAppContext) {
    let ui = open_with(
        cx,
        Options {
            window_size: Some((900., 560.)),
            ..test_options()
        },
    );
    ui.click(cx, "sessions-control");
    ui.with(cx, |window, _| {
        for id in [
            "sessions-drawer-backdrop",
            "sessions-drawer",
            "hide-sidebar",
        ] {
            let bounds = window.find(id).bounds();
            assert!(bounds.top() >= px(36.), "{id}: {bounds:?}");
            assert!(bounds.bottom() <= px(560.), "{id}: {bounds:?}");
        }
    });
    ui.click(cx, "hide-sidebar");
    assert!(ui.read(cx, |app, _| !app.session_drawer));
    ui.click(cx, "sessions-control");
    assert!(ui.read(cx, |app, _| app.session_drawer));
    ui.press(cx, "escape");
    assert!(ui.read(cx, |app, _| !app.session_drawer));
    assert!(ui.with(cx, |window, cx| {
        gpui_kit::base::active_focus_trap(window, cx).is_none()
    }));
}

#[gpui_kit::test]
fn sessions_remain_reachable_at_narrow_window_sizes(cx: &mut TestAppContext) {
    for (width, height) in [(900., 560.), (1000., 700.), (1440., 900.)] {
        let ui = open_with(
            cx,
            Options {
                window_size: Some((width, height)),
                ..test_options()
            },
        );
        if width < 1000. {
            assert!(ui.with(cx, |window, _| {
                window.try_find("sessions-control").is_some()
            }));
            ui.click(cx, "sessions-control");
            assert!(ui.with(cx, |window, _| window.try_find("sessions-drawer").is_some()));
            ui.press(cx, "escape");
            assert!(ui.read(cx, |app, _| !app.session_drawer));
        } else {
            assert!(ui.read(cx, |app, _| app.sidebar_visible));
        }
    }
}

#[gpui_kit::test]
fn a_narrow_docked_chat_collapses_the_sidebar_even_in_a_large_window(cx: &mut TestAppContext) {
    use flint_app::docking::Node;
    let ui = open(cx);
    ui.app.update(cx, |app, cx| {
        app.changes_open = true;
        if let Node::Split { second, .. } = &mut app.dock_layout.root
            && let Node::Split { sizes, .. } = second.as_mut()
        {
            *sizes = [Some(320.), Some(820.)];
        }
        cx.notify();
    });
    ui.with(cx, |window, cx| window.render_frame(cx));
    assert!(ui.read(cx, |app, _| !app.sidebar_visible));
    assert!(ui.with(cx, |window, _| {
        window.try_find("sessions-control").is_some()
    }));
    ui.click(cx, "sessions-control");
    assert!(ui.with(cx, |window, _| window.try_find("sessions-drawer").is_some()));
}

#[gpui_kit::test]
fn saving_new_model_does_not_relabel_an_active_native_engine(cx: &mut TestAppContext) {
    let ui = open(cx);
    ui.app.update(cx, |app, cx| {
        app.sessions[app.active].native_model = Some("active-model".into());
        cx.notify();
    });
    ui.press(cx, "cmd-,");
    ui.with(cx, |window, cx| {
        ui.app.update(cx, |app, cx| {
            let model = app.settings_form.as_ref().unwrap().model.clone();
            model.update(cx, |input, cx| input.set_value("future-model", window, cx));
        });
    });
    ui.click(cx, "settings-save");
    assert_eq!(
        ui.read(cx, |app, _| app.agent_label(flint_agent::AgentKind::Flint)),
        "flint · active-model"
    );
    assert_eq!(
        ui.read(cx, |app, _| app.settings.model.clone()),
        "future-model"
    );
}

#[gpui_kit::test]
fn settings_validate_unsaved_credential_source(cx: &mut TestAppContext) {
    let ui = open(cx);
    ui.press(cx, "cmd-,");
    ui.with(cx, |window, cx| {
        ui.app.update(cx, |app, cx| {
            let env = app.settings_form.as_ref().unwrap().api_key_env.clone();
            env.update(cx, |input, cx| {
                input.set_value("not a variable", window, cx)
            });
        });
    });
    ui.click(cx, "settings-save");
    assert!(ui.read(cx, |app, _| {
        app.settings_form.as_ref().unwrap().error.as_deref()
            == Some("Enter an environment variable name, not a key value.")
    }));
    assert!(ui.read(cx, |app, _| app.settings.api_key_env.is_empty()));
}

fn set_probe_draft(ui: &Ui, cx: &mut TestAppContext, url: &str, model: &str) {
    ui.with(cx, |window, cx| {
        ui.app.update(cx, |app, cx| {
            let form = app.settings_form.as_ref().unwrap();
            form.base_url
                .update(cx, |input, cx| input.set_value(url, window, cx));
            form.model
                .update(cx, |input, cx| input.set_value(model, window, cx));
        })
    });
    cx.run_until_parked();
}

#[gpui_kit::test]
fn connection_test_is_explicit_read_only_and_uses_unsaved_draft(cx: &mut TestAppContext) {
    use std::sync::atomic::Ordering;
    let provider = local_provider::LocalProvider::approval();
    let options = test_options();
    let ui = open_with(
        cx,
        Options {
            key_path: None,
            ..options.clone()
        },
    );
    ui.press(cx, "cmd-,");
    set_probe_draft(&ui, cx, &provider.url, "fixture-model");
    assert_eq!(provider.model_requests.load(Ordering::Relaxed), 0);
    ui.click(cx, "test-provider-connection");
    wait_for(&ui, cx, |app| {
        app.settings_form.as_ref().unwrap().probe_result.is_some()
    });
    let result = ui.read(cx, |app, _| {
        app.settings_form
            .as_ref()
            .unwrap()
            .probe_result
            .clone()
            .unwrap()
    });
    assert_eq!(
        provider.model_requests.load(Ordering::Relaxed),
        1,
        "GET /models request count"
    );
    assert_eq!(
        provider.completion_requests.load(Ordering::Relaxed),
        0,
        "no completion request"
    );
    assert!(
        result.reachable
            && result.model_listed == Some(true)
            && result.credentials_accepted.is_none(),
        "{result:?}"
    );
    assert!(result.message.contains("without credentials"));
    assert_eq!(provider.model_requests.load(Ordering::Relaxed), 1);
    assert_eq!(provider.completion_requests.load(Ordering::Relaxed), 0);
    assert!(ui.read(cx, |app, _| app.settings.base_url != provider.url));
    assert!(!options.home.as_ref().unwrap().join("config.toml").exists());
    set_probe_draft(&ui, cx, &provider.url, "changed-model");
    assert!(ui.read(cx, |app, _| {
        app.settings_form.as_ref().unwrap().probe_result.is_none()
    }));
}

#[gpui_kit::test]
fn connection_test_cancellation_draft_edits_and_sheet_close_discard_results(
    cx: &mut TestAppContext,
) {
    let provider = local_provider::LocalProvider::listing(std::time::Duration::from_millis(120));
    let ui = open(cx);
    ui.press(cx, "cmd-,");
    set_probe_draft(&ui, cx, &provider.url, "fixture-model");
    ui.click(cx, "test-provider-connection");
    ui.click(cx, "cancel-provider-test");
    assert!(ui.read(cx, |app, _| {
        app.settings_form.as_ref().unwrap().probe.is_none()
    }));
    ui.click(cx, "test-provider-connection");
    set_probe_draft(&ui, cx, &provider.url, "edited-model");
    assert!(ui.read(cx, |app, _| {
        app.settings_form.as_ref().unwrap().probe.is_none()
    }));
    ui.click(cx, "test-provider-connection");
    ui.press(cx, "escape");
    ui.press(cx, "cmd-,");
    std::thread::sleep(std::time::Duration::from_millis(160));
    settle(cx);
    assert!(ui.read(cx, |app, _| {
        let form = app.settings_form.as_ref().unwrap();
        form.probe.is_none() && form.probe_result.is_none()
    }));
}

#[gpui_kit::test]
fn narrow_settings_keep_validation_and_actions_visible(cx: &mut TestAppContext) {
    let ui = open_with(
        cx,
        Options {
            window_size: Some((900., 560.)),
            ..test_options()
        },
    );
    ui.press(cx, "cmd-,");
    ui.with(cx, |window, cx| {
        ui.app.update(cx, |app, cx| {
            let model = app.settings_form.as_ref().unwrap().model.clone();
            model.update(cx, |input, cx| input.set_value("", window, cx));
        });
    });
    ui.click(cx, "settings-save");
    assert!(ui.read(cx, |app, _| {
        app.settings_form.as_ref().unwrap().error.is_some()
    }));
    ui.with(cx, |window, cx| {
        window.render_frame(cx);
        for id in ["settings-error", "settings-save", "settings-cancel"] {
            let bounds = window.find(ElementId::Name(id.into())).bounds();
            assert!(
                bounds.top() >= px(0.) && bounds.bottom() <= px(560.),
                "{id} is outside the 900x560 window: {bounds:?}"
            );
        }
    });
    ui.click(cx, "settings-cancel");
    assert!(ui.read(cx, |app, _| app.settings_form.is_none()));
    ui.press(cx, "cmd-,");
    ui.scroll_settings(cx);
    ui.click(cx, "settings-save");
    assert!(ui.read(cx, |app, _| app.settings_form.is_none()));
}

#[gpui_kit::test]
fn escape_cancels_broad_approval_without_stopping_work(cx: &mut TestAppContext) {
    let ui = open(cx);
    let engine = running_turn(&ui, cx);
    approval_requested(&engine, cx, "c1");
    ui.press(cx, "a");
    assert!(ui.read(cx, |app, _| app.approval_confirm.is_some()));
    ui.press(cx, "escape");
    assert!(ui.read(cx, |app, _| app.approval_confirm.is_none()));
    assert!(engine.sent().is_empty());
}

#[gpui_kit::test]
fn the_effort_chip_sends_set_reasoning_effort(cx: &mut TestAppContext) {
    let ui = open(cx);
    let engine = running_turn(&ui, cx);
    ui.click(cx, "effort-chip");
    assert!(matches!(
        engine.sent().as_slice(),
        [Op::SetReasoningEffort(Some(
            flint_agent::ReasoningEffort::High
        ))]
    ));
}

#[gpui_kit::test]
fn the_effort_chip_hides_when_the_endpoint_ignores_it(cx: &mut TestAppContext) {
    let ui = open(cx);
    let engine = running_turn(&ui, cx);
    engine.send(
        cx,
        AgentEvent::Error(
            "This model endpoint doesn't accept reasoning_effort; continuing without it.".into(),
        ),
    );
    assert!(!ui.read(cx, |app, _| app.effort_supported));
    assert!(ui.with(cx, |window, _| window.try_find("effort-chip").is_none()));
}

#[gpui_kit::test]
fn clicking_a_guard_nudge_expands_its_message(cx: &mut TestAppContext) {
    let ui = open(cx);
    let engine = running_turn(&ui, cx);
    engine.send(
        cx,
        AgentEvent::HarnessNudge {
            reason: flint_agent::NudgeReason::Verify,
            message: "You modified files but haven't run the tests. Run them and continue.".into(),
        },
    );
    let ix = ui.read(cx, |app, _| app.session().view.items.len() - 1);
    ui.click(cx, ("nudge", ix));
    let expanded = ui.read(cx, |app, _| {
        matches!(
            app.session().view.items[ix],
            Item::Nudge { expanded: true, .. }
        )
    });
    assert!(expanded);
}

#[gpui_kit::test]
fn context_compaction_shows_a_row(cx: &mut TestAppContext) {
    let ui = open(cx);
    let engine = running_turn(&ui, cx);
    engine.send(
        cx,
        AgentEvent::ContextCompacted {
            before_tokens: 92_000,
            after_tokens: 41_000,
        },
    );
    let last = ui.read(cx, |app, _| app.session().view.items.last().cloned());
    assert_eq!(
        last,
        Some(Item::Compacted {
            before_tokens: 92_000,
            after_tokens: 41_000
        })
    );
}

#[gpui_kit::test]
fn changes_show_one_combined_diff_per_file(cx: &mut TestAppContext) {
    use flint_app::session::combined;
    let ws = temp_dir("combined");
    let original = "one\ntwo\nthree\n";
    let v1 = "one\nTWO\nthree\n";
    let v2 = "one\nTWO\nthree\nfour\n";
    std::fs::write(ws.join("a.txt"), original).unwrap();
    let ui = open_with(
        cx,
        Options {
            workspace: Some(ws.clone()),
            ..test_options()
        },
    );
    let engine = running_turn(&ui, cx);
    for (n, (before, after)) in [(original, v1), (v1, v2)].into_iter().enumerate() {
        std::fs::write(ws.join("a.txt"), after).unwrap();
        let (unified, added, removed) = combined(before, after, "a.txt");
        let id = format!("e{n}");
        engine.send(cx, tool_started(&id, ToolKind::Edit, "a.txt"));
        engine.send(
            cx,
            tool_finished(
                &id,
                "ok",
                Some(FileDiff {
                    path: "a.txt".into(),
                    unified,
                    added,
                    removed,
                    created: false,
                }),
            ),
        );
    }
    engine.send(
        cx,
        AgentEvent::TurnFinished {
            turn_id: 1,
            reason: TurnEndReason::Completed,
        },
    );
    let (expected, added, removed) = combined(original, v2, "a.txt");
    wait_for(&ui, cx, |app| {
        app.session().view.changes[0].combined.is_some()
    });
    let file = ui.read(cx, |app, _| app.session().view.changes[0].clone());
    assert_eq!(
        (file.combined, file.added, file.removed),
        (Some(expected), added, removed)
    );
    let turn = ui.read(cx, |app, _| {
        (
            app.session().view.turns[0].added,
            app.session().view.turns[0].removed,
        )
    });
    assert_eq!(turn, (2, 1), "the files-changed card counts the net change");
}

#[gpui_kit::test]
fn combined_changes_survive_restart_continued_edits_and_archive_recovery(cx: &mut TestAppContext) {
    use flint_app::session::combined;
    use flint_app::store::{self, Logged, Meta};
    let options = test_options();
    let workspace = options.workspace.as_ref().unwrap();
    let dir = options.home.as_ref().unwrap().join("sessions/saved-edits");
    store::write_meta(
        &dir,
        &Meta {
            id: "saved-edits".into(),
            title: Some("Saved edits".into()),
            workspace: workspace.clone(),
            created_at: 1,
            updated_at: 2,
            agent: flint_agent::AgentKind::Flint,
            general: false,
        },
    )
    .unwrap();
    let original = "one\ntwo\n";
    let intermediate = "one\nTWO\n";
    let current = "one\nTHREE\n";
    std::fs::write(workspace.join("a.txt"), current).unwrap();
    for (n, (before, after)) in [(original, intermediate), (intermediate, current)]
        .into_iter()
        .enumerate()
    {
        let (unified, added, removed) = combined(before, after, "a.txt");
        for record in [
            Logged::User(format!("edit {n}")),
            Logged::Event(AgentEvent::TurnStarted {
                turn_id: n as u64 + 1,
            }),
            Logged::Event(tool_started(&format!("e{n}"), ToolKind::Edit, "a.txt")),
            Logged::Event(tool_finished(
                &format!("e{n}"),
                "ok",
                Some(FileDiff {
                    path: "a.txt".into(),
                    unified,
                    added,
                    removed,
                    created: false,
                }),
            )),
            Logged::Event(AgentEvent::TurnFinished {
                turn_id: n as u64 + 1,
                reason: TurnEndReason::Completed,
            }),
        ] {
            store::append(&dir, &record).unwrap();
        }
    }
    // The last turn made no edits; recovery still needs earlier changed files.
    store::append(&dir, &Logged::Event(AgentEvent::TurnStarted { turn_id: 3 })).unwrap();
    store::append(
        &dir,
        &Logged::Event(AgentEvent::TurnFinished {
            turn_id: 3,
            reason: TurnEndReason::Completed,
        }),
    )
    .unwrap();
    let ui = open_with(cx, options.clone());
    ui.with(cx, |window, cx| {
        ui.app
            .update(cx, |app, cx| app.select_session(1, window, cx));
    });
    wait_for(&ui, cx, |app| {
        app.session().view.changes[0].combined.is_some()
    });
    let expected = combined(original, current, "a.txt");
    assert_eq!(
        ui.read(cx, |app, _| {
            let file = &app.session().view.changes[0];
            (file.combined.clone().unwrap(), file.added, file.removed)
        }),
        expected
    );
    assert_eq!(
        ui.read(cx, |app, _| app
            .session()
            .view
            .turns
            .iter()
            .map(|turn| { (turn.added, turn.removed, turn.file_stats.len()) })
            .collect::<Vec<_>>()),
        [(1, 1, 1), (1, 1, 1), (0, 0, 0)],
        "restored summaries retain each turn's own net counts"
    );
    let engine = ui.engine(cx);
    engine.send(cx, AgentEvent::TurnStarted { turn_id: 4 });
    std::fs::write(workspace.join("a.txt"), original).unwrap();
    let (unified, added, removed) = combined(current, original, "a.txt");
    engine.send(cx, tool_started("revert", ToolKind::Edit, "a.txt"));
    engine.send(
        cx,
        tool_finished(
            "revert",
            "ok",
            Some(FileDiff {
                path: "a.txt".into(),
                unified,
                added,
                removed,
                created: false,
            }),
        ),
    );
    engine.send(
        cx,
        AgentEvent::TurnFinished {
            turn_id: 4,
            reason: TurnEndReason::Completed,
        },
    );
    wait_for(&ui, cx, |app| {
        app.session().view.changes[0].combined.is_some()
    });
    assert_eq!(
        ui.read(cx, |app, _| {
            let file = &app.session().view.changes[0];
            (file.added, file.removed)
        }),
        (0, 0),
        "continued edits use the original session contents"
    );
    drop(engine);
    settle(cx);
    ui.with(cx, |window, cx| {
        ui.app
            .update(cx, |app, cx| app.delete_session(app.active, window, cx));
    });
    assert!(home_archive(&options, &dir).exists());
    let again = open_with(cx, options);
    again.with(cx, |window, cx| {
        again
            .app
            .update(cx, |app, cx| app.restore_archive(0, window, cx));
    });
    wait_for(&again, cx, |app| {
        app.session().view.changes[0].combined.is_some()
    });
    assert_eq!(
        again.read(cx, |app, _| {
            let file = &app.session().view.changes[0];
            (file.added, file.removed)
        }),
        (0, 0),
        "archive recovery also retains the full edit history"
    );
}

#[gpui_kit::test]
fn the_changes_panel_lists_files_only_when_there_are_several(cx: &mut TestAppContext) {
    let ui = open(cx);
    let engine = running_turn(&ui, cx);
    let edit = |n: usize, path: &str| {
        tool_finished(
            &format!("e{n}"),
            "ok",
            Some(FileDiff {
                path: path.into(),
                unified: format!("--- a/{path}\n+++ b/{path}\n@@ -1 +1 @@\n-a\n+b\n"),
                added: 1,
                removed: 1,
                created: false,
            }),
        )
    };
    engine.send(cx, tool_started("e0", ToolKind::Edit, "x.rs"));
    engine.send(cx, edit(0, "x.rs"));
    ui.press(cx, "cmd-j");
    assert!(ui.with(cx, |window, _| {
        window.try_find(("change", 0usize)).is_none()
    }));
    assert!(ui.with(cx, |window, _| {
        window.try_find("open-file-preview").is_some()
    }));
    engine.send(cx, tool_started("e1", ToolKind::Edit, "y.rs"));
    engine.send(cx, edit(1, "y.rs"));
    assert!(ui.with(cx, |window, _| {
        window.try_find(("change", 1usize)).is_some()
    }));
}

#[gpui_kit::test]
fn large_diffs_render_visible_rows_and_keep_offscreen_long_lines_scrollable(
    cx: &mut TestAppContext,
) {
    let ui = open_with(
        cx,
        Options {
            diff_test: Some(10_000),
            window_size: Some((900., 560.)),
            ..test_options()
        },
    );
    ui.with(cx, |window, cx| {
        window.render_frame(cx);
        assert!(window.try_find(("diff-row", 1usize)).is_some());
        assert!(window.try_find(("diff-row", 5000usize)).is_none());
    });
    let scroll = ui.read(cx, |app, _| app.change_diff_scroll.clone());
    assert!(scroll.0.borrow().base_handle.max_offset().x > px(10_000.));
    assert!(scroll.0.borrow().base_handle.max_offset().y > px(100_000.));
    scroll.scroll_to_item_strict(9999, gpui_kit::ScrollStrategy::Bottom);
    ui.with(cx, |window, cx| {
        window.render_frame(cx);
        assert!(window.try_find(("diff-row", 9999usize)).is_some());
        assert!(window.try_find(("diff-row", 1usize)).is_none());
    });
    let before = scroll.0.borrow().base_handle.offset();
    scroll
        .0
        .borrow()
        .base_handle
        .set_offset(point(px(-2000.), before.y));
    ui.with(cx, |window, cx| window.render_frame(cx));
    assert_eq!(scroll.0.borrow().base_handle.offset().x, px(-2000.));
    assert_eq!(
        ui.read(cx, |app, _| app.session().view.changes[0].added),
        10_000
    );
}

#[gpui_kit::test]
fn switching_changed_files_resets_diff_scroll_and_updates_content(cx: &mut TestAppContext) {
    let ui = open(cx);
    let engine = running_turn(&ui, cx);
    for (id, path, content) in [("e0", "a.rs", "first"), ("e1", "b.rs", "second")] {
        engine.send(cx, tool_started(id, ToolKind::Edit, path));
        engine.send(
            cx,
            tool_finished(
                id,
                "ok",
                Some(FileDiff {
                    path: path.into(),
                    unified: format!("@@ -0,0 +1,100 @@\n{}", format!("+{content}\n").repeat(100)),
                    added: 100,
                    removed: 0,
                    created: true,
                }),
            ),
        );
    }
    ui.press(cx, "cmd-j");
    let scroll = ui.read(cx, |app, _| app.change_diff_scroll.clone());
    scroll.scroll_to_item_strict(80, gpui_kit::ScrollStrategy::Top);
    ui.with(cx, |window, cx| window.render_frame(cx));
    assert!(scroll.0.borrow().base_handle.offset().y < px(-500.));
    ui.click(cx, ("change", 1usize));
    ui.with(cx, |window, cx| window.render_frame(cx));
    assert_eq!(ui.read(cx, |app, _| app.selected_change), Some(1));
    assert_eq!(
        scroll.0.borrow().base_handle.offset(),
        point(px(0.), px(0.))
    );
    assert!(ui.with(cx, |window, _| {
        window.try_find(("diff-row", 1usize)).is_some()
    }));
}

#[gpui_kit::test]
fn diff_selection_copies_multiple_rows_and_survives_redraw(cx: &mut TestAppContext) {
    let ui = open(cx);
    let engine = running_turn(&ui, cx);
    engine.send(cx, tool_started("edit", ToolKind::Edit, "a.rs"));
    engine.send(
        cx,
        tool_finished(
            "edit",
            "ok",
            Some(FileDiff {
                path: "a.rs".into(),
                unified: "@@ -0,0 +1,3 @@\n+alpha\n+\tbeta\n+gamma\n".into(),
                added: 3,
                removed: 0,
                created: true,
            }),
        ),
    );
    ui.press(cx, "cmd-j");
    ui.with(cx, |window, cx| {
        window.render_frame(cx);
        let first = window.find(("diff-row", 1usize)).bounds();
        let last = window.find(("diff-row", 3usize)).bounds();
        let from = point(first.left() + px(130.), first.center().y);
        let to = point(first.left() + px(250.), last.center().y);
        window.drag(from, to, cx);
        window.render_frame(cx);
        assert_eq!(
            gpui_kit::base::TextSelection::selected_text(window, cx),
            "alpha\n\tbeta\ngamma"
        );
    });
    ui.app.update(cx, |_, cx| cx.notify());
    settle(cx);
    ui.with(cx, |window, cx| {
        window.render_frame(cx);
        assert_eq!(
            gpui_kit::base::TextSelection::selected_text(window, cx),
            "alpha\n\tbeta\ngamma"
        );
    });
    ui.press(cx, "cmd-c");
    assert_eq!(
        cx.read_from_clipboard().and_then(|item| item.text()),
        Some("alpha\n\tbeta\ngamma".into())
    );
}

#[gpui_kit::test]
fn diff_selection_preserves_blank_lines_after_scrolling_out_of_view(cx: &mut TestAppContext) {
    let ui = open(cx);
    let engine = running_turn(&ui, cx);
    let mut unified = "@@ -0,0 +1,1000 @@\n+alpha\n+\n+gamma\n".to_string();
    unified.push_str(&"+other\n".repeat(997));
    engine.send(cx, tool_started("edit", ToolKind::Edit, "a.rs"));
    engine.send(
        cx,
        tool_finished(
            "edit",
            "ok",
            Some(FileDiff {
                path: "a.rs".into(),
                unified,
                added: 1000,
                removed: 0,
                created: true,
            }),
        ),
    );
    ui.press(cx, "cmd-j");
    ui.with(cx, |window, cx| {
        window.render_frame(cx);
        let first = window.find(("diff-row", 1usize)).bounds();
        let last = window.find(("diff-row", 3usize)).bounds();
        window.drag(
            point(first.left() + px(130.), first.center().y),
            point(first.left() + px(250.), last.center().y),
            cx,
        );
        window.render_frame(cx);
        assert_eq!(
            gpui_kit::base::TextSelection::selected_text(window, cx),
            "alpha\n\ngamma"
        );
    });
    let scroll = ui.read(cx, |app, _| app.change_diff_scroll.clone());
    scroll.scroll_to_item_strict(800, gpui_kit::ScrollStrategy::Top);
    ui.with(cx, |window, cx| {
        window.render_frame(cx);
        assert!(window.try_find(("diff-row", 1usize)).is_none());
        assert_eq!(
            gpui_kit::base::TextSelection::selected_text(window, cx),
            "alpha\n\ngamma"
        );
    });
    ui.press(cx, "cmd-c");
    assert_eq!(
        cx.read_from_clipboard().and_then(|item| item.text()),
        Some("alpha\n\ngamma".into())
    );
}

#[gpui_kit::test]
fn diff_selection_supports_partial_unicode_and_native_word_selection(cx: &mut TestAppContext) {
    use gpui_kit::{InputEvent as _, MouseButton, MouseDownEvent, MouseUpEvent};
    let ui = open(cx);
    let engine = running_turn(&ui, cx);
    let text = "alpha 界 beta gamma";
    engine.send(cx, tool_started("edit", ToolKind::Edit, "a.rs"));
    engine.send(
        cx,
        tool_finished(
            "edit",
            "ok",
            Some(FileDiff {
                path: "a.rs".into(),
                unified: format!("@@ -0,0 +1 @@\n+{text}\n"),
                added: 1,
                removed: 0,
                created: true,
            }),
        ),
    );
    ui.press(cx, "cmd-j");
    ui.with(cx, |window, cx| {
        window.render_frame(cx);
        let row = window.find(("diff-row", 1usize)).bounds();
        let run = gpui_kit::TextRun {
            len: text.len(),
            font: gpui_kit::font(flint_app::theme::MONO_FONT),
            color: gpui_kit::black(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let layout = window.text_system().shape_line(
            text.into(),
            px(flint_app::theme::size::SM),
            &[run],
            None,
        );
        let at = |index| {
            point(
                row.left() + px(130.5) + layout.x_for_index(index),
                row.center().y,
            )
        };
        window.drag(at(6), at(12), cx);
        window.render_frame(cx);
        assert_eq!(
            gpui_kit::base::TextSelection::selected_text(window, cx),
            &text[6..12]
        );
        let position = at(12);
        for event in [
            MouseDownEvent {
                button: MouseButton::Left,
                position,
                modifiers: Default::default(),
                click_count: 2,
                first_mouse: false,
            }
            .to_platform_input(),
            MouseUpEvent {
                button: MouseButton::Left,
                position,
                modifiers: Default::default(),
                click_count: 2,
            }
            .to_platform_input(),
        ] {
            window.dispatch_event(event, cx);
        }
        window.render_frame(cx);
        assert_eq!(
            gpui_kit::base::TextSelection::selected_text(window, cx),
            "beta"
        );
    });
    ui.press(cx, "cmd-c");
    assert_eq!(
        cx.read_from_clipboard().and_then(|item| item.text()),
        Some("beta".into())
    );
}

#[gpui_kit::test]
fn diff_selection_copies_partial_text_after_horizontal_scrolling(cx: &mut TestAppContext) {
    let ui = open(cx);
    let engine = running_turn(&ui, cx);
    let text = format!(
        "{}target text{}",
        "prefix ".repeat(80),
        " suffix".repeat(80)
    );
    engine.send(cx, tool_started("edit", ToolKind::Edit, "a.rs"));
    engine.send(
        cx,
        tool_finished(
            "edit",
            "ok",
            Some(FileDiff {
                path: "a.rs".into(),
                unified: format!("@@ -0,0 +1 @@\n+{text}\n"),
                added: 1,
                removed: 0,
                created: true,
            }),
        ),
    );
    ui.press(cx, "cmd-j");
    let scroll = ui.read(cx, |app, _| app.change_diff_scroll.clone());
    ui.with(cx, |window, cx| {
        window.render_frame(cx);
        let layout = window.text_system().shape_line(
            text.clone().into(),
            px(flint_app::theme::size::SM),
            &[gpui_kit::TextRun {
                len: text.len(),
                font: gpui_kit::font(flint_app::theme::MONO_FONT),
                color: gpui_kit::black(),
                background_color: None,
                underline: None,
                strikethrough: None,
            }],
            None,
        );
        let start = text.find("target").unwrap();
        scroll
            .0
            .borrow()
            .base_handle
            .set_offset(point(-layout.x_for_index(start), px(0.)));
        window.render_frame(cx);
        let row = window.find(("diff-row", 1usize)).bounds();
        let from = point(
            row.left() + px(130.) + layout.x_for_index(start),
            row.center().y,
        );
        let to = point(
            from.x + layout.x_for_index(start + 11) - layout.x_for_index(start),
            from.y,
        );
        window.drag(from, to, cx);
        window.render_frame(cx);
        assert_eq!(
            gpui_kit::base::TextSelection::selected_text(window, cx),
            "target text"
        );
    });
    ui.press(cx, "cmd-c");
    assert_eq!(
        cx.read_from_clipboard().and_then(|item| item.text()),
        Some("target text".into())
    );
}

#[gpui_kit::test]
fn diff_selection_spans_recycled_rows_and_copies_in_reverse(cx: &mut TestAppContext) {
    use gpui_kit::{InputEvent as _, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent};
    let ui = open(cx);
    let engine = running_turn(&ui, cx);
    let lines: Vec<String> = (0..1000)
        .map(|n| {
            if n % 7 == 0 {
                String::new()
            } else {
                format!("row {n:04} 界")
            }
        })
        .collect();
    engine.send(cx, tool_started("edit", ToolKind::Edit, "a.rs"));
    engine.send(
        cx,
        tool_finished(
            "edit",
            "ok",
            Some(FileDiff {
                path: "a.rs".into(),
                unified: format!(
                    "@@ -0,0 +1,1000 @@\n{}\n",
                    lines
                        .iter()
                        .map(|line| format!("+{line}"))
                        .collect::<Vec<_>>()
                        .join("\n")
                ),
                added: 1000,
                removed: 0,
                created: true,
            }),
        ),
    );
    ui.press(cx, "cmd-j");
    let scroll = ui.read(cx, |app, _| app.change_diff_scroll.clone());
    ui.with(cx, |window, cx| {
        window.render_frame(cx);
        let row = window.find(("diff-row", 2usize)).bounds();
        window.dispatch_event(
            MouseDownEvent {
                button: MouseButton::Left,
                position: point(row.left() + px(130.), row.center().y),
                modifiers: Default::default(),
                click_count: 1,
                first_mouse: false,
            }
            .to_platform_input(),
            cx,
        );
        window.render_frame(cx);
        scroll.scroll_to_item_strict(800, gpui_kit::ScrollStrategy::Top);
        window.render_frame(cx);
        assert!(window.try_find(("diff-row", 2usize)).is_none());
        let row = window.find(("diff-row", 803usize)).bounds();
        let end = point(row.left() + px(300.), row.center().y);
        window.dispatch_event(
            MouseMoveEvent {
                position: end,
                pressed_button: Some(MouseButton::Left),
                modifiers: Default::default(),
            }
            .to_platform_input(),
            cx,
        );
        window.dispatch_event(
            MouseUpEvent {
                button: MouseButton::Left,
                position: end,
                modifiers: Default::default(),
                click_count: 1,
            }
            .to_platform_input(),
            cx,
        );
        window.render_frame(cx);
        assert_eq!(
            gpui_kit::base::TextSelection::selected_text(window, cx),
            lines[1..803].join("\n")
        );
        let row = window.find(("diff-row", 803usize)).bounds();
        window.dispatch_event(
            MouseDownEvent {
                button: MouseButton::Left,
                position: point(row.left() + px(300.), row.center().y),
                modifiers: Default::default(),
                click_count: 1,
                first_mouse: false,
            }
            .to_platform_input(),
            cx,
        );
        window.render_frame(cx);
        scroll.scroll_to_item_strict(0, gpui_kit::ScrollStrategy::Top);
        window.render_frame(cx);
        let row = window.find(("diff-row", 2usize)).bounds();
        let end = point(row.left() + px(130.), row.center().y);
        window.dispatch_event(
            MouseMoveEvent {
                position: end,
                pressed_button: Some(MouseButton::Left),
                modifiers: Default::default(),
            }
            .to_platform_input(),
            cx,
        );
        window.dispatch_event(
            MouseUpEvent {
                button: MouseButton::Left,
                position: end,
                modifiers: Default::default(),
                click_count: 1,
            }
            .to_platform_input(),
            cx,
        );
        window.render_frame(cx);
        assert_eq!(
            gpui_kit::base::TextSelection::selected_text(window, cx),
            lines[1..803].join("\n")
        );
    });
}

#[gpui_kit::test]
fn changing_diff_content_does_not_reuse_selection_from_the_previous_file(cx: &mut TestAppContext) {
    let ui = open(cx);
    let engine = running_turn(&ui, cx);
    for (id, path, content) in [("e0", "a.rs", "alpha"), ("e1", "b.rs", "bravo")] {
        engine.send(cx, tool_started(id, ToolKind::Edit, path));
        engine.send(
            cx,
            tool_finished(
                id,
                "ok",
                Some(FileDiff {
                    path: path.into(),
                    unified: format!("@@ -0,0 +1 @@\n+{content}\n"),
                    added: 1,
                    removed: 0,
                    created: true,
                }),
            ),
        );
    }
    ui.press(cx, "cmd-j");
    ui.with(cx, |window, cx| {
        window.render_frame(cx);
        let row = window.find(("diff-row", 1usize)).bounds();
        window.drag(
            point(row.left() + px(130.), row.center().y),
            point(row.left() + px(240.), row.center().y),
            cx,
        );
        window.render_frame(cx);
        assert_eq!(
            gpui_kit::base::TextSelection::selected_text(window, cx),
            "alpha"
        );
    });
    ui.app.update(cx, |app, cx| app.select_change(1, cx));
    settle(cx);
    ui.with(cx, |window, cx| {
        window.render_frame(cx);
        assert_eq!(gpui_kit::base::TextSelection::selected_text(window, cx), "");
    });
}

#[gpui_kit::test]
fn the_header_title_uses_the_available_width(cx: &mut TestAppContext) {
    let ui = open(cx);
    let _engine = ui.engine(cx);
    ui.input(
        cx,
        &"Refactor the session store so restore is incremental ".repeat(4),
    );
    ui.press(cx, "enter");
    let width = ui.with(cx, |window, _| {
        window.find("session-title").bounds().size.width
    });
    assert!(width > px(380.), "title width {width:?}");
}

#[gpui_kit::test]
fn the_welcome_tip_is_plain_language_and_its_dismissal_is_remembered(cx: &mut TestAppContext) {
    let options = test_options();
    let ui = open_with(cx, options.clone());
    assert!(ui.with(cx, |window, _| window.try_find("welcome-tip").is_some()));
    ui.click(cx, "dismiss-tip");
    let again = open_with(cx, options);
    assert!(again.with(cx, |window, _| window.try_find("welcome-tip").is_none()));
}

#[gpui_kit::test]
fn copy_shows_copied_and_fills_the_clipboard(cx: &mut TestAppContext) {
    let ui = open(cx);
    let engine = ui.engine(cx);
    finished_turn(&ui, cx, &engine);
    let end = ui.read(cx, |app, _| app.session().view.turns[0].end.unwrap());
    ui.click(cx, ("copy-button", end));
    assert!(ui.read(cx, |app, _| app.copied.is_some_and(|(row, _)| row == end)));
    let clip = cx.read_from_clipboard().and_then(|item| item.text());
    assert_eq!(clip.as_deref(), Some("Fixed `a`."));
}

#[gpui_kit::test]
fn a_missing_key_shows_a_card_that_opens_settings(cx: &mut TestAppContext) {
    let ui = open_with(
        cx,
        Options {
            key_path: Some(temp_dir("nokey").join("api.key")),
            ..test_options()
        },
    );
    ui.input(cx, "hello");
    ui.press(cx, "enter");
    let (ix, message) = ui.read(cx, |app, _| {
        let view = &app.session().view;
        view.items
            .iter()
            .enumerate()
            .find_map(|(ix, item)| match item {
                Item::Error(message) => Some((ix, message.clone())),
                _ => None,
            })
            .unwrap()
    });
    assert!(message.starts_with("No API key found"), "{message}");
    assert!(!message.contains("test-key"));
    ui.click(cx, ("error-settings", ix));
    assert!(ui.read(cx, |app, _| app.settings_form.is_some()));
}

#[gpui_kit::test]
fn a_failed_engine_start_keeps_the_submitted_prompt_after_restart(cx: &mut TestAppContext) {
    let options = Options {
        key_path: Some(temp_dir("missing-key").join("api.key")),
        ..test_options()
    };
    let ui = open_with(cx, options.clone());
    ui.input(cx, "Keep this prompt even when setup is incomplete");
    ui.press(cx, "enter");
    ui.app.update(cx, |app, _| {
        app.sessions[0].flush_records().unwrap();
    });
    let restored = open_with(cx, options);
    assert!(restored.read(cx, |app, _| {
        app.sessions.iter().any(|session| {
            session.view.items.iter().any(|item| {
                matches!(item, Item::User(text) if text == "Keep this prompt even when setup is incomplete")
            })
        })
    }));
}

#[gpui_kit::test]
fn an_unreachable_endpoint_shows_a_card_with_retry(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let options = test_options();
    let home = options.home.clone().unwrap();
    std::fs::write(
        home.join("config.toml"),
        "model = \"test-model\"\nbase_url = \"http://127.0.0.1:9/v1\"\n",
    )
    .unwrap();
    let ui = open_with(cx, options);
    ui.input(cx, "hello");
    ui.press(cx, "enter");
    let started = std::time::Instant::now();
    let error = loop {
        settle(cx);
        let error = ui.read(cx, |app, _| {
            app.session()
                .view
                .items
                .iter()
                .enumerate()
                .find_map(|(ix, item)| match item {
                    Item::Error(message) => Some((ix, message.clone())),
                    _ => None,
                })
        });
        if let Some(error) = error {
            break error;
        }
        assert!(
            started.elapsed() < std::time::Duration::from_secs(30),
            "no error from the engine"
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    };
    assert!(
        ui.with(cx, |window, _| window
            .try_find(("error-retry", error.0))
            .is_some()),
        "retry offered for: {}",
        error.1
    );
    assert!(ui.with(cx, |window, _| {
        window.try_find(("error-settings", error.0)).is_some()
    }));
}

#[gpui_kit::test]
fn a_model_error_offers_retry_which_resends(cx: &mut TestAppContext) {
    let ui = open(cx);
    let engine = running_turn(&ui, cx);
    engine.send(
        cx,
        AgentEvent::Error("HTTP 400: model `nope` does not exist".into()),
    );
    engine.send(
        cx,
        AgentEvent::TurnFinished {
            turn_id: 1,
            reason: TurnEndReason::Failed("HTTP 400".into()),
        },
    );
    let ix = ui.read(cx, |app, _| {
        app.session()
            .view
            .items
            .iter()
            .position(|item| matches!(item, Item::Error(_)))
            .unwrap()
    });
    ui.click(cx, ("error-retry", ix));
    assert!(matches!(engine.sent().as_slice(), [Op::UserMessage(text)] if text == "do the thing"));
}

#[gpui_kit::test]
fn files_changed_counts_survive_edits_landing_before_the_ui_reads_them(cx: &mut TestAppContext) {
    use flint_app::session::combined;
    let ws = temp_dir("race");
    let original = "def area_of_rect(w, h):\n    return w * h\n\nx = area_of_rect(1, 2)\n";
    let v1 = "def rectangle_area(w, h):\n    return w * h\n\nx = area_of_rect(1, 2)\n";
    let v2 = "def rectangle_area(w, h):\n    return w * h\n\nx = rectangle_area(1, 2)\n";
    // The engine already applied both edits before the UI sees the first one.
    std::fs::write(ws.join("geo.py"), v2).unwrap();
    let ui = open_with(
        cx,
        Options {
            workspace: Some(ws),
            ..test_options()
        },
    );
    let engine = running_turn(&ui, cx);
    for (n, (before, after)) in [(original, v1), (v1, v2)].into_iter().enumerate() {
        let (unified, added, removed) = combined(before, after, "geo.py");
        let id = format!("e{n}");
        engine
            .events
            .try_send(tool_started(&id, ToolKind::Edit, "geo.py"))
            .unwrap();
        engine
            .events
            .try_send(tool_finished(
                &id,
                "ok",
                Some(FileDiff {
                    path: "geo.py".into(),
                    unified,
                    added,
                    removed,
                    created: false,
                }),
            ))
            .unwrap();
    }
    engine.send(
        cx,
        AgentEvent::TurnFinished {
            turn_id: 1,
            reason: TurnEndReason::Completed,
        },
    );
    wait_for(&ui, cx, |app| {
        app.session().view.changes[0].combined.is_some()
    });
    let turn = ui.read(cx, |app, _| {
        (
            app.session().view.turns[0].added,
            app.session().view.turns[0].removed,
        )
    });
    assert_eq!(
        turn,
        (2, 2),
        "net change is two lines replaced, as git diff reports"
    );
}

#[gpui_kit::test]
fn agent_picker_sets_a_fresh_session_and_opens_a_new_one_after_start(cx: &mut TestAppContext) {
    use flint_agent::AgentKind;
    let ui = open(cx);
    // A fresh session takes the picked agent; the chip shows it.
    ui.click(cx, "model-chip");
    assert!(ui.read(cx, |app, _| app.agent_menu));
    ui.click(cx, ("agent-item", 1usize));
    assert_eq!(
        ui.read(cx, |app, _| (
            app.agent_menu,
            app.sessions.len(),
            app.session().agent
        )),
        (false, 1, AgentKind::ClaudeCode)
    );
    assert_eq!(
        ui.read(cx, |app, _| app.agent_label(app.session().agent)),
        "Claude Code"
    );

    // Once it has started, the session keeps its agent; /agent opens a new one.
    let _engine = ui.engine(cx);
    ui.input(cx, "first task");
    ui.press(cx, "enter");
    ui.input(cx, "/agent codex");
    ui.press(cx, "escape");
    ui.press(cx, "enter");
    let agents = ui.read(cx, |app, _| {
        (
            app.sessions.iter().map(|s| s.agent).collect::<Vec<_>>(),
            app.active,
        )
    });
    assert_eq!(agents, (vec![AgentKind::ClaudeCode, AgentKind::Codex], 1));
    assert_eq!(ui.composer_text(cx), "");
}

#[gpui_kit::test]
fn droid_is_selectable_and_its_options_and_session_survive_restart(cx: &mut TestAppContext) {
    use flint_agent::AgentKind;
    let options = test_options();
    let ui = open_with(cx, options.clone());
    ui.click(cx, "model-chip");
    let index = flint_app::agents::AGENTS
        .iter()
        .position(|kind| *kind == AgentKind::Droid)
        .unwrap();
    ui.click(cx, ("agent-item", index));
    assert_eq!(ui.read(cx, |app, _| app.session().agent), AgentKind::Droid);
    assert_eq!(
        ui.read(cx, |app, _| app.agent_label(app.session().agent)),
        "Droid"
    );
    let engine = ui.engine(cx);
    ui.input(cx, "Droid task");
    ui.press(cx, "enter");
    let sent = engine.sent();
    assert!(
        matches!(sent.as_slice(), [Op::UserMessage(text)] if text == "Droid task"),
        "sent {sent:?}; draft {:?}; error {:?}",
        ui.composer_text(cx),
        ui.read(cx, |app, _| app.store_error.clone())
    );
    engine.send(cx, agent_options("default"));
    assert!(has(&ui, cx, "option-model"));
    ui.click(cx, "option-model");
    ui.press(cx, "down");
    ui.press(cx, "enter");
    assert!(
        matches!(engine.sent().as_slice(), [Op::SetSessionOption { id, value }]
        if id == "model" && value == "opus")
    );
    for event in [
        AgentEvent::TurnStarted { turn_id: 1 },
        AgentEvent::TextDelta("Droid answer.".into()),
        AgentEvent::TurnFinished {
            turn_id: 1,
            reason: TurnEndReason::Completed,
        },
    ] {
        engine.send(cx, event);
    }
    let again = open_with(cx, options);
    assert!(again.read(cx, |app, _| app.sessions.iter().any(
        |session| session.title() == "Droid task" && session.agent == AgentKind::Droid
    )));
    ui.input(cx, "/agent flint");
    ui.press(cx, "enter");
    ui.input(cx, "/agent droid");
    ui.press(cx, "enter");
    assert_eq!(ui.read(cx, |app, _| app.session().agent), AgentKind::Droid);
    assert_eq!(ui.composer_text(cx), "");
}

#[gpui_kit::test]
fn command_palette_opens_a_droid_session(cx: &mut TestAppContext) {
    let ui = open(cx);
    ui.press(cx, "cmd-k");
    ui.input(cx, "New Droid session");
    ui.press(cx, "enter");
    assert_eq!(
        ui.read(cx, |app, _| app.session().agent),
        flint_agent::AgentKind::Droid
    );
    assert!(ui.read(cx, |app, _| app.palette.is_none()));
}

fn agent_options(mode: &str) -> AgentEvent {
    use flint_agent::OptionChoice;
    use flint_agent::SessionOption;
    let option =
        |id: &str, category: Option<&str>, current: &str, choices: &[(&str, &str)]| SessionOption {
            id: id.to_string(),
            name: id.to_string(),
            description: None,
            category: category.map(str::to_string),
            current: current.to_string(),
            choices: choices
                .iter()
                .map(|(value, name)| OptionChoice {
                    value: (*value).to_string(),
                    name: (*name).to_string(),
                    description: Some(format!("about {name}")),
                })
                .collect(),
        };
    AgentEvent::SessionOptions(vec![
        option(
            "mode",
            Some("mode"),
            mode,
            &[
                ("default", "Manual"),
                ("acceptEdits", "Accept Edits"),
                ("plan", "Plan"),
                ("bypassPermissions", "Bypass"),
            ],
        ),
        option(
            "model",
            Some("model"),
            "default",
            &[("default", "Default"), ("opus", "Opus")],
        ),
        option(
            "effort",
            Some("thought_level"),
            "high",
            &[("low", "Low"), ("high", "High")],
        ),
        option(
            "fast",
            Some("model_config"),
            "off",
            &[("on", "On"), ("off", "Off")],
        ),
        option(
            "agent",
            None,
            "default",
            &[("default", "Default"), ("reviewer", "Reviewer")],
        ),
    ])
}

/// A Claude Code session with scripted options attached.
fn acp_session(cx: &mut TestAppContext) -> (Ui, Engine) {
    let ui = open(cx);
    ui.app.update(cx, |app, _| {
        app.sessions[0].agent = flint_agent::AgentKind::ClaudeCode
    });
    let engine = ui.engine(cx);
    engine.send(cx, agent_options("default"));
    (ui, engine)
}

fn has(ui: &Ui, cx: &mut TestAppContext, id: &str) -> bool {
    let id = id.to_string();
    ui.with(cx, move |window, cx| {
        window.render_frame(cx);
        window.try_find(ElementId::Name(id.into())).is_some()
    })
}

#[gpui_kit::test]
fn agent_option_chips_render_and_replace_auto_run(cx: &mut TestAppContext) {
    let (ui, _engine) = acp_session(cx);
    for id in [
        "option-model",
        "option-reasoning",
        "option-mode",
        "option-fast",
        "option-more",
    ] {
        assert!(has(&ui, cx, id), "{id} missing");
    }
    assert!(!has(&ui, cx, "approval-hint"));
    assert!(!has(&ui, cx, "effort-chip"));
}

#[gpui_kit::test]
fn option_menus_send_set_session_option(cx: &mut TestAppContext) {
    let (ui, engine) = acp_session(cx);
    // Keyboard: open the model menu, move down, Enter.
    ui.click(cx, "option-model");
    assert!(has(&ui, cx, "option-menu"));
    ui.press(cx, "down");
    ui.press(cx, "enter");
    assert!(
        matches!(engine.sent().as_slice(), [Op::SetSessionOption { id, value }] if id == "model" && value == "opus")
    );
    assert!(!has(&ui, cx, "option-menu"));
    // Mouse: reasoning menu, first row.
    ui.click(cx, "option-reasoning");
    ui.click(cx, ("option-item", 0usize));
    assert!(
        matches!(engine.sent().as_slice(), [Op::SetSessionOption { id, value }] if id == "effort" && value == "low")
    );
    // Fast is a toggle.
    ui.click(cx, "option-fast");
    assert!(
        matches!(engine.sent().as_slice(), [Op::SetSessionOption { id, value }] if id == "fast" && value == "on")
    );
    // More lists the remaining options; picking one opens its choices.
    ui.click(cx, "option-more");
    ui.click(cx, ("option-item", 0usize));
    ui.click(cx, ("option-item", 1usize));
    assert!(
        matches!(engine.sent().as_slice(), [Op::SetSessionOption { id, value }] if id == "agent" && value == "reviewer")
    );
}

#[gpui_kit::test]
fn shift_tab_cycles_the_agents_mode(cx: &mut TestAppContext) {
    let (ui, engine) = acp_session(cx);
    ui.press(cx, "shift-tab");
    assert!(
        matches!(engine.sent().as_slice(), [Op::SetSessionOption { id, value }] if id == "mode" && value == "acceptEdits")
    );
    engine.send(cx, agent_options("plan"));
    // From Plan it wraps to Manual, skipping Bypass.
    ui.press(cx, "shift-tab");
    assert!(
        matches!(engine.sent().as_slice(), [Op::SetSessionOption { id, value }] if id == "mode" && value == "default")
    );
    // flint's auto-run is untouched.
    assert_eq!(
        ui.read(cx, |app, _| app.approval),
        ApprovalMode::AskForChanges
    );
}

#[gpui_kit::test]
fn long_option_dropdown_scrolls_to_current_and_keyboard_selected_rows(cx: &mut TestAppContext) {
    let ui = open_with(
        cx,
        Options {
            window_size: Some((900., 560.)),
            ..test_options()
        },
    );
    ui.app.update(cx, |app, _| {
        app.sessions[0].agent = flint_agent::AgentKind::ClaudeCode
    });
    let engine = ui.engine(cx);
    engine.send(
        cx,
        AgentEvent::SessionOptions(vec![flint_agent::SessionOption {
            id: "model".into(),
            name: "Model".into(),
            category: Some("model".into()),
            description: None,
            current: "model-30".into(),
            choices: (0..40)
                .map(|n| flint_agent::OptionChoice {
                    value: format!("model-{n}"),
                    name: format!("Model {n}"),
                    description: Some(format!("Description for model {n}")),
                })
                .collect(),
        }]),
    );
    ui.click(cx, "option-model");
    let visible = |ui: &Ui, cx: &mut TestAppContext, n| {
        ui.with(cx, |window, cx| {
            window.render_frame(cx);
            window.simulate_next_frame(cx);
            window.render_frame(cx);
            let viewport = window
                .find(ElementId::Name("option-menu-rows".into()))
                .bounds();
            let row = window.find(("option-item", n)).bounds();
            assert!(
                row.origin.y >= viewport.origin.y - px(1.),
                "{row:?} above {viewport:?}"
            );
            assert!(
                row.bottom() <= viewport.bottom() + px(1.),
                "{row:?} below {viewport:?}"
            );
        });
    };
    visible(&ui, cx, 30usize);
    assert_popover_inside_chat(&ui, cx);
    assert!(ui.read(cx, |app, _| app.menu_scroll.max_offset().y > px(500.)));
    for _ in 0..9 {
        ui.press(cx, "down");
    }
    visible(&ui, cx, 39usize);
    ui.press(cx, "enter");
    assert!(
        matches!(engine.sent().as_slice(), [Op::SetSessionOption { id, value }]
        if id == "model" && value == "model-39")
    );
    assert_eq!(
        ui.read(cx, |app, _| app.session().options[0].current.clone()),
        "model-30",
        "a request is not a confirmed setting"
    );
    let confirmed = ui.read(cx, |app, _| {
        let mut options = app.session().options.clone();
        options[0].current = "model-39".into();
        options
    });
    engine.send(cx, AgentEvent::SessionOptions(confirmed));
    ui.click(cx, "option-model");
    visible(&ui, cx, 39usize);
    for _ in 0..39 {
        ui.press(cx, "up");
    }
    visible(&ui, cx, 0usize);
    assert_eq!(ui.read(cx, |app, _| app.menu_scroll.offset().y), px(0.));
    ui.with(cx, |window, cx| {
        window.scroll(
            "option-menu-rows",
            ScrollDelta::Pixels(point(px(0.), px(-100.))),
            cx,
        );
    });
    let wheel_offset = ui.read(cx, |app, _| app.menu_scroll.offset().y);
    assert!(wheel_offset < px(0.));
    ui.with(cx, |window, cx| {
        let viewport = window.find("option-menu-rows").bounds();
        window.click_at(
            "option-menu-rows",
            point(viewport.size.width - px(6.), viewport.size.height - px(8.)),
            cx,
        );
    });
    let track_offset = ui.read(cx, |app, _| app.menu_scroll.offset().y);
    assert!(
        track_offset < wheel_offset,
        "scrollbar track must scroll the menu"
    );
    assert_eq!(
        ui.read(cx, |app, _| app.option_menu.as_ref().unwrap().selected),
        0
    );
    assert!(
        engine.sent().is_empty(),
        "scrolling must not choose a model"
    );
    ui.with(cx, |window, cx| {
        window.simulate_next_frame(cx);
        window.render_frame(cx);
        window.render_frame(cx);
    });
    assert_eq!(
        ui.read(cx, |app, _| app.menu_scroll.offset().y),
        track_offset,
        "redrawing must preserve manual scrolling"
    );
}

/// A Claude Code session whose model option has `count` choices.
fn long_model_session(cx: &mut TestAppContext, options: Options, count: usize) -> (Ui, Engine) {
    let ui = open_with(cx, options);
    ui.app.update(cx, |app, _| {
        app.sessions[0].agent = flint_agent::AgentKind::ClaudeCode
    });
    let engine = ui.engine(cx);
    engine.send(
        cx,
        AgentEvent::SessionOptions(vec![flint_agent::SessionOption {
            id: "model".into(),
            name: "Model".into(),
            category: Some("model".into()),
            description: None,
            current: "model-0".into(),
            choices: (0..count)
                .map(|n| flint_agent::OptionChoice {
                    value: format!("model-{n}"),
                    name: format!("Model {n}"),
                    description: Some(format!("Description for model {n}")),
                })
                .collect(),
        }]),
    );
    (ui, engine)
}

/// Asserts the open composer popover lies inside the chat panel, and that
/// the panel also contains the composer it belongs to.
fn assert_popover_inside_chat(ui: &Ui, cx: &mut TestAppContext) {
    ui.with(cx, |window, cx| {
        // The first frame measures the room; the second uses it.
        window.render_frame(cx);
        window.simulate_next_frame(cx);
        window.render_frame(cx);
        let panel = window.find("dock-panel-chat").bounds();
        let popover = window.find("composer-popover").bounds();
        assert!(
            popover.origin.y >= panel.origin.y - px(1.),
            "popover {popover:?} rises above the chat panel {panel:?}"
        );
        assert!(
            popover.bottom() <= panel.bottom() + px(1.),
            "popover {popover:?} runs past the bottom of the chat panel {panel:?}"
        );
        assert!(
            popover.size.height >= px(80.),
            "popover {popover:?} is too short to use"
        );
    });
}

#[gpui_kit::test]
fn long_model_menu_fits_below_the_welcome_composer(cx: &mut TestAppContext) {
    // A tall window opens welcome-screen menus below the composer.
    let (ui, _engine) = long_model_session(cx, test_options(), 60);
    ui.click(cx, "option-model");
    assert_popover_inside_chat(&ui, cx);
}

#[gpui_kit::test]
fn long_model_menu_fits_the_chat_panel_beside_a_terminal_dock(cx: &mut TestAppContext) {
    let (_dir, options) = docking_options();
    let (ui, _engine) = long_model_session(
        cx,
        Options {
            open_terminal: true,
            ..options
        },
        60,
    );
    ui.click(cx, "option-model");
    assert_popover_inside_chat(&ui, cx);
}

#[gpui_kit::test]
fn long_model_menu_fits_a_conversation_beside_a_terminal_dock(cx: &mut TestAppContext) {
    let (_dir, options) = docking_options();
    let (ui, _engine) = long_model_session(
        cx,
        Options {
            open_terminal: true,
            window_size: Some((1200., 640.)),
            ..options
        },
        60,
    );
    ui.press(cx, "cmd-l");
    ui.input(cx, "hello");
    ui.press(cx, "enter");
    assert!(ui.read(cx, |app, _| !app.session().view.items.is_empty()));
    ui.click(cx, "option-model");
    assert_popover_inside_chat(&ui, cx);
}

#[gpui_kit::test]
fn keyboard_keeps_the_selected_slash_command_in_view(cx: &mut TestAppContext) {
    let ui = open_with(
        cx,
        Options {
            window_size: Some((900., 420.)),
            ..test_options()
        },
    );
    ui.press(cx, "cmd-l");
    ui.input(cx, "/");
    let last = flint_app::slash::COMMANDS.len() - 1;
    for _ in 0..last {
        ui.press(cx, "down");
    }
    ui.with(cx, |window, cx| {
        window.render_frame(cx);
        window.simulate_next_frame(cx);
        window.render_frame(cx);
        let rows = window.find("slash-menu-rows").bounds();
        let row = window.find(("slash-item", last)).bounds();
        assert!(
            row.origin.y >= rows.origin.y - px(1.),
            "{row:?} above {rows:?}"
        );
        assert!(
            row.bottom() <= rows.bottom() + px(1.),
            "{row:?} below {rows:?}"
        );
    });
    assert_popover_inside_chat(&ui, cx);
}

#[gpui_kit::test]
fn command_palette_fits_a_short_window(cx: &mut TestAppContext) {
    let ui = open_with(
        cx,
        Options {
            window_size: Some((900., 420.)),
            open_palette: true,
            ..test_options()
        },
    );
    ui.with(cx, |window, cx| {
        window.render_frame(cx);
        let palette = window.find("palette-focus-trap").bounds();
        assert!(
            palette.bottom() <= px(420.),
            "palette {palette:?} runs past the bottom of a 420 px window"
        );
    });
}

#[gpui_kit::test]
fn plus_menu_lists_folders_and_picking_one_sets_the_workspace(cx: &mut TestAppContext) {
    use flint_app::project_menu::ProjectItem;
    let ui = open(cx);
    let other = temp_dir("other-project");
    // A saved session elsewhere makes that folder a recent one.
    ui.app.update(cx, |app, _| {
        app.sessions
            .push(flint_app::session::Session::new(9_999, other.clone()));
    });
    ui.click(cx, "attach");
    let items = ui.read(cx, |app, _| app.project_items());
    assert_eq!(
        items,
        vec![
            ProjectItem::OpenFolder,
            ProjectItem::Recent(other.clone()),
            ProjectItem::Worktrees,
            ProjectItem::General,
            ProjectItem::AttachImage,
            ProjectItem::AttachFile,
        ]
    );
    for n in 0..5usize {
        assert!(ui.with(cx, move |w, _| w.try_find(("project-item", n)).is_some()));
    }
    // An unstarted session moves to the chosen folder.
    ui.click(cx, ("project-item", 1usize));
    assert_eq!(
        ui.read(cx, |app, _| (
            app.session().workspace.clone(),
            app.sessions.len(),
            app.project_menu
        )),
        (other.clone(), 2, None)
    );

    // A started session stays put; the folder opens in a new session.
    let _engine = ui.engine(cx);
    ui.input(cx, "first task");
    ui.press(cx, "enter");
    let first = ui.read(cx, |app, _| app.sessions[1].workspace.clone());
    let elsewhere = temp_dir("elsewhere");
    ui.app
        .update(cx, |app, cx| app.set_project_folder(elsewhere.clone(), cx));
    assert_eq!(
        ui.read(cx, |app, _| (
            app.sessions.len(),
            app.session().workspace.clone(),
            app.sessions[1].workspace.clone()
        )),
        (3, elsewhere, first)
    );
}

#[gpui_kit::test]
fn welcome_and_composer_fit_long_workspace_names(cx: &mut TestAppContext) {
    let mut options = test_options();
    let workspace = options
        .workspace
        .as_ref()
        .unwrap()
        .join("long-project-name-".repeat(10));
    std::fs::create_dir_all(&workspace).unwrap();
    options.workspace = Some(workspace);
    options.window_size = Some((900., 560.));
    let ui = open_with(cx, options);
    ui.with(cx, |window, cx| {
        window.render_frame(cx);
        for id in [
            "welcome-folder",
            "composer-toolbar",
            "composer-options",
            "send",
        ] {
            let bounds = window.find(ElementId::Name(id.into())).bounds();
            assert!(
                bounds.left() >= px(0.) && bounds.right() <= px(900.),
                "{id}: {bounds:?}"
            );
            assert!(
                bounds.top() >= px(0.) && bounds.bottom() <= px(560.),
                "{id}: {bounds:?}"
            );
        }
        let toolbar = window.find("composer-toolbar").bounds();
        let options = window.find("composer-options").bounds();
        assert!(
            options.top() >= toolbar.bottom(),
            "{toolbar:?} overlaps {options:?}"
        );
    });
}

#[gpui_kit::test]
fn welcome_folder_chip_opens_the_project_menu(cx: &mut TestAppContext) {
    let ui = open(cx);
    ui.click(cx, "welcome-folder");
    assert!(has(&ui, cx, "project-menu"));
    ui.press(cx, "escape");
    assert!(!has(&ui, cx, "project-menu"));
}

#[gpui_kit::test]
fn open_picker_does_not_push_the_logo_into_the_header(cx: &mut TestAppContext) {
    let ui = open(cx);
    let bounds = |ui: &Ui, cx: &mut TestAppContext, id: &'static str| {
        ui.with(cx, move |window, cx| {
            window.render_frame(cx);
            window.find(ElementId::Name(id.into())).bounds()
        })
    };
    let header = bounds(&ui, cx, "header");
    let before = bounds(&ui, cx, "welcome-logo");
    ui.click(cx, "attach");
    let attach = ui.read(cx, |app, _| app.project_items().len() - 1);
    ui.click(cx, ("project-item", attach));
    assert!(ui.read(cx, |app, _| app.mention.is_some()));
    let after = bounds(&ui, cx, "welcome-logo");
    assert_eq!(after.origin, before.origin, "the picker moved the hero");
    assert!(
        after.origin.y >= header.origin.y + header.size.height,
        "logo {after:?} overlaps header {header:?}"
    );
}

#[gpui_kit::test]
fn starting_agent_shows_a_status_line_and_greyed_chips(cx: &mut TestAppContext) {
    let ui = open(cx);
    ui.app.update(cx, |app, _| {
        app.sessions[0].agent = flint_agent::AgentKind::Codex
    });
    let engine = ui.engine(cx);
    assert!(ui.read(cx, |app, _| app.session().agent_starting()));
    assert!(has(&ui, cx, "agent-starting"));
    assert!(ui.with(cx, |w, _| {
        w.try_find(("option-placeholder", 0usize)).is_some()
    }));
    engine.send(cx, agent_options("default"));
    assert!(!has(&ui, cx, "agent-starting"));
    assert!(has(&ui, cx, "option-model"));
}

#[gpui_kit::test]
fn slash_model_and_mode_open_the_agents_menus(cx: &mut TestAppContext) {
    use flint_app::session_options::MenuTarget;
    let (ui, _engine) = acp_session(cx);
    ui.input(cx, "/model");
    ui.press(cx, "enter");
    assert_eq!(
        ui.read(cx, |app, _| app
            .option_menu
            .as_ref()
            .map(|m| m.target.clone())),
        Some(MenuTarget::Option("model".into()))
    );
    ui.press(cx, "escape");
    ui.input(cx, "/mode");
    ui.press(cx, "enter");
    assert_eq!(
        ui.read(cx, |app, _| app
            .option_menu
            .as_ref()
            .map(|m| m.target.clone())),
        Some(MenuTarget::Option("mode".into()))
    );
}

fn terminal_options() -> Options {
    Options {
        terminal_command: Some(("/bin/sh".into(), Vec::new())),
        terminal_poll: true,
        ..test_options()
    }
}

/// Waits (real time) until the active terminal shows a line satisfying `pred`.
fn wait_for_terminal(ui: &Ui, cx: &mut TestAppContext, pred: impl Fn(&str) -> bool) -> Vec<String> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        settle(cx);
        let lines = ui.read(cx, |app, cx| {
            app.terminal
                .active_view()
                .map(|v| v.read(cx).terminal.snapshot().text_lines())
                .unwrap_or_default()
        });
        if lines.iter().any(|l| pred(l)) || std::time::Instant::now() > deadline {
            return lines;
        }
        std::thread::sleep(std::time::Duration::from_millis(30));
    }
}

#[gpui_kit::test]
fn ctrl_backtick_toggles_a_terminal_in_the_workspace(cx: &mut TestAppContext) {
    let ui = open_with(cx, terminal_options());
    assert!(!has(&ui, cx, "terminal-panel"));
    ui.press(cx, "ctrl-`");
    let (open, tabs, cwd, workspace) = ui.read(cx, |app, cx| {
        (
            app.terminal.open,
            app.terminal.tabs.len(),
            app.terminal.active_view().map(|v| v.read(cx).cwd.clone()),
            app.session().workspace.clone(),
        )
    });
    assert_eq!((open, tabs, cwd), (true, 1, Some(workspace)));
    assert!(has(&ui, cx, "terminal-panel"));
    ui.click(cx, "terminal-new");
    assert_eq!(ui.read(cx, |app, _| app.terminal.tabs.len()), 2);
    ui.press(cx, "ctrl-`");
    assert!(!has(&ui, cx, "terminal-panel"));
}

#[gpui_kit::test]
fn typing_in_the_terminal_runs_commands(cx: &mut TestAppContext) {
    let ui = open_with(cx, terminal_options());
    ui.press(cx, "ctrl-`");
    settle(cx);
    ui.input(cx, "echo hi-$((40+2))");
    ui.press(cx, "enter");
    let lines = wait_for_terminal(&ui, cx, |l| l == "hi-42");
    assert!(lines.iter().any(|l| l == "hi-42"), "{lines:?}");
    // Escape and Shift+Tab go to the shell, not to flint.
    let approval = ui.read(cx, |app, _| app.approval);
    ui.press(cx, "shift-tab");
    assert_eq!(ui.read(cx, |app, _| app.approval), approval);
}

#[gpui_kit::test]
fn terminal_copy_and_paste(cx: &mut TestAppContext) {
    let ui = open_with(cx, terminal_options());
    ui.press(cx, "ctrl-`");
    settle(cx);
    ui.input(cx, "echo copy-me");
    ui.press(cx, "enter");
    let lines = wait_for_terminal(&ui, cx, |l| l == "copy-me");
    let row = lines
        .iter()
        .position(|l| l == "copy-me")
        .expect("output row");
    ui.read(cx, |app, cx| {
        let view = app.terminal.active_view().expect("tab").read(cx);
        view.terminal.start_selection(row, 0, false, 1);
        view.terminal.update_selection(row, 6, true);
    });
    ui.press(cx, "cmd-c");
    let copied = cx.read(|cx| cx.read_from_clipboard().and_then(|item| item.text()));
    assert_eq!(copied.as_deref(), Some("copy-me"));

    cx.update(|cx| {
        cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(
            "echo pasted-$((1+1))".into(),
        ))
    });
    ui.press(cx, "cmd-v");
    ui.press(cx, "enter");
    let lines = wait_for_terminal(&ui, cx, |l| l == "pasted-2");
    assert!(lines.iter().any(|l| l == "pasted-2"), "{lines:?}");
}

/// A running command card (the transcript's second row, after the message).
fn running_command(ui: &Ui, cx: &mut TestAppContext, engine: &Engine, command: &str) -> usize {
    ui.input(cx, "run it");
    ui.press(cx, "enter");
    engine.send(cx, AgentEvent::TurnStarted { turn_id: 1 });
    engine.send(cx, tool_started("c1", ToolKind::Command, command));
    engine.sent();
    ui.read(cx, |app, _| {
        app.session()
            .view
            .items
            .iter()
            .position(|item| matches!(item, Item::Tool(_)))
            .expect("tool row")
    })
}

#[gpui_kit::test]
fn a_command_card_opens_in_a_terminal(cx: &mut TestAppContext) {
    let ui = open_with(cx, terminal_options());
    let engine = ui.engine(cx);
    let row = running_command(&ui, cx, &engine, "npm test");
    assert!(!has(&ui, cx, "terminal-panel"));
    ui.click(cx, ("tool-terminal", row));
    let (open, tabs, read_only) = ui.read(cx, |app, cx| {
        (
            app.terminal.open,
            app.terminal.tabs.len(),
            app.terminal
                .active_view()
                .map(|view| view.read(cx).read_only),
        )
    });
    assert_eq!((open, tabs, read_only), (true, 1, Some(false)));
    // The command is on the new shell's prompt, ready to run.
    let lines = wait_for_terminal(&ui, cx, |l| l.contains("npm test"));
    assert!(lines.iter().any(|l| l.contains("npm test")), "{lines:?}");
}

#[gpui_kit::test]
fn a_command_card_sends_its_output_to_the_agent(cx: &mut TestAppContext) {
    let ui = open_with(cx, terminal_options());
    let engine = ui.engine(cx);
    let row = running_command(&ui, cx, &engine, "npm test");
    engine.send(cx, tool_finished("c1", "2 tests failed\n", None));
    engine.sent();
    ui.click(cx, ("tool-send", row));
    assert!(engine.sent().is_empty());
    assert!(ui.read(cx, |app, _| {
        app.session().prompt_queue.items[0]
            .message
            .contains("2 tests failed")
    }));
    engine.send(
        cx,
        AgentEvent::TurnFinished {
            turn_id: 1,
            reason: TurnEndReason::Completed,
        },
    );
    assert!(
        matches!(
            engine.sent().as_slice(),
            [Op::UserMessage(message)]
                if message.contains("2 tests failed") && message.contains("npm test")
        ),
        "output not sent back"
    );
    // It shows in the transcript as a user message; the output itself is
    // what was sent, in a fenced block.
    let last = ui.read(cx, |app, _| app.session().view.items.last().cloned());
    assert!(
        matches!(&last, Some(Item::User(text)) if text.contains("output of `npm test`")),
        "{last:?}"
    );
}

#[gpui_kit::test]
fn an_agents_command_mirrors_into_a_read_only_tab(cx: &mut TestAppContext) {
    let ui = open_with(cx, terminal_options());
    let engine = ui.engine(cx);
    let row = running_command(&ui, cx, &engine, "npm test");
    engine.send(
        cx,
        AgentEvent::TerminalStarted {
            terminal_id: "t1".into(),
            call_id: Some("c1".into()),
            label: "claude: npm test".into(),
            cwd: None,
        },
    );
    // Mirrored, but not in the way until the card asks for it.
    assert_eq!(
        ui.read(cx, |app, _| (app.terminal.open, app.terminal.tabs.len())),
        (false, 1)
    );
    engine.send(
        cx,
        AgentEvent::TerminalOutput {
            terminal_id: "t1".into(),
            data: "2 tests failed\n".into(),
            replace: false,
        },
    );
    ui.click(cx, ("tool-terminal", row));
    assert!(ui.read(cx, |app, _| app.terminal.open));
    let lines = wait_for_terminal(&ui, cx, |l| l == "2 tests failed");
    assert!(lines.iter().any(|l| l == "2 tests failed"), "{lines:?}");
    // A whole-output update replaces the screen instead of appending.
    engine.send(
        cx,
        AgentEvent::TerminalOutput {
            terminal_id: "t1".into(),
            data: "all green\n".into(),
            replace: true,
        },
    );
    let lines = wait_for_terminal(&ui, cx, |l| l == "all green");
    assert!(!lines.iter().any(|l| l == "2 tests failed"), "{lines:?}");
    engine.send(
        cx,
        AgentEvent::TerminalExited {
            terminal_id: "t1".into(),
            exit_code: Some(1),
        },
    );
    let label = ui.read(cx, |app, cx| {
        app.terminal
            .active_view()
            .map(|view| view.read(cx).label())
            .unwrap_or_default()
    });
    assert_eq!(label, "claude: npm test (exit 1)");
}

#[gpui_kit::test]
fn multiline_agent_terminal_tabs_preserve_the_command_without_header_overflow(
    cx: &mut TestAppContext,
) {
    let ui = open_with(cx, terminal_options());
    let engine = ui.engine(cx);
    let command = "cat <<'END'\nfirst line\nsecond line\nEND";
    running_command(&ui, cx, &engine, command);
    let label = format!("droid: {command}");
    engine.send(
        cx,
        AgentEvent::TerminalStarted {
            terminal_id: "multi".into(),
            call_id: Some("c1".into()),
            label: label.clone(),
            cwd: None,
        },
    );
    ui.click(cx, "terminal");
    ui.with(cx, |window, cx| {
        window.render_frame(cx);
        let tab = window.find(("terminal-tab", 0usize)).bounds();
        let summary = window.find(("terminal-tab-summary", 0usize)).bounds();
        assert!(summary.size.height <= px(26.), "{summary:?}");
        assert!(
            summary.bottom() <= tab.bottom(),
            "{summary:?} overflows {tab:?}"
        );
    });
    assert_eq!(
        ui.read(cx, |app, cx| app
            .terminal
            .active_view()
            .unwrap()
            .read(cx)
            .fixed_label
            .clone()),
        Some(label)
    );
    engine.send(
        cx,
        AgentEvent::TerminalExited {
            terminal_id: "multi".into(),
            exit_code: Some(7),
        },
    );
    assert_eq!(
        ui.read(cx, |app, cx| app
            .terminal
            .active_view()
            .unwrap()
            .read(cx)
            .label()),
        "droid: cat <<'END' … (exit 7)"
    );
}

#[gpui_kit::test]
fn a_mirrored_command_tab_is_never_typed_into(cx: &mut TestAppContext) {
    let ui = open_with(cx, terminal_options());
    let engine = ui.engine(cx);
    running_command(&ui, cx, &engine, "npm test");
    engine.send(
        cx,
        AgentEvent::TerminalStarted {
            terminal_id: "t1".into(),
            call_id: Some("c1".into()),
            label: "claude: npm test".into(),
            cwd: None,
        },
    );
    ui.click(cx, "terminal");
    assert!(ui.read(cx, |app, _| app.terminal.open));
    ui.input(cx, "echo nope");
    ui.press(cx, "enter");
    let lines = ui.read(cx, |app, cx| {
        app.terminal
            .active_view()
            .map(|view| view.read(cx).terminal.snapshot().text_lines())
            .unwrap_or_default()
    });
    assert!(!lines.iter().any(|l| l.contains("nope")), "{lines:?}");
    // Its tab offers no "send to agent": there is nothing to type.
    assert!(!has(&ui, cx, "terminal-send"));
}

#[gpui_kit::test]
fn the_sidebar_opens_the_terminal_dock(cx: &mut TestAppContext) {
    let ui = open_with(cx, terminal_options());
    assert!(!ui.read(cx, |app, _| app.terminal.open));
    ui.click(cx, "terminal");
    assert!(ui.read(cx, |app, _| app.terminal.open));
    assert_eq!(ui.read(cx, |app, _| app.terminal.tabs.len()), 1);
    ui.click(cx, "terminal");
    assert!(!ui.read(cx, |app, _| app.terminal.open));
}

/// Own a unique home for persisted-layout tests, including parallel runs.
fn docking_options() -> (tempfile::TempDir, Options) {
    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let key = dir.path().join("api.key");
    std::fs::write(&key, "test-key").unwrap();
    let options = Options {
        workspace: Some(workspace),
        home: Some(dir.path().join("home")),
        key_path: Some(key),
        key_sources: Some(KeySources::none()),
        terminal_command: Some(("/bin/sh".into(), Vec::new())),
        terminal_poll: true,
        engine_poll: true,
        skip_permission_choice: true,
        ..Options::default()
    };
    (dir, options)
}

fn dock(
    ui: &Ui,
    cx: &mut TestAppContext,
    panel: flint_app::docking::Panel,
    target: flint_app::docking::Panel,
    edge: flint_app::docking::Edge,
) {
    use flint_app::docking::Edge;
    ui.with(cx, |window, cx| {
        window.render_frame(cx);
        let from = window
            .find(ElementId::Name(
                format!("dock-handle-{}", panel.id()).into(),
            ))
            .bounds()
            .center();
        let bounds = window
            .find(ElementId::Name(
                format!("dock-panel-{}", target.id()).into(),
            ))
            .bounds();
        let center = bounds.center();
        let to = match edge {
            Edge::Left => point(bounds.left() + px(12.), center.y),
            Edge::Right => point(bounds.right() - px(12.), center.y),
            Edge::Top => point(center.x, bounds.top() + px(12.)),
            Edge::Bottom => point(center.x, bounds.bottom() - px(12.)),
        };
        window.drag(from, to, cx);
    });
    settle(cx);
}

fn panel_bounds(
    ui: &Ui,
    cx: &mut TestAppContext,
    panel: flint_app::docking::Panel,
) -> Bounds<gpui_kit::Pixels> {
    ui.with(cx, |window, cx| {
        window.render_frame(cx);
        window
            .find(ElementId::Name(format!("dock-panel-{}", panel.id()).into()))
            .bounds()
    })
}

#[gpui_kit::test]
fn docking_moves_all_four_panels_without_restarting_their_contents(cx: &mut TestAppContext) {
    use flint_app::docking::{Edge, Panel};
    let (_dir, options) = docking_options();
    let ui = open_with(
        cx,
        Options {
            open_changes: true,
            open_terminal: true,
            ..options
        },
    );
    ui.press(cx, "cmd-l");
    ui.input(cx, "keep this draft");
    let before = ui.read(cx, |app, _| {
        (
            app.session().uid,
            app.composer.entity_id(),
            app.terminal.active_view().unwrap().entity_id(),
        )
    });
    for (panel, target, edge) in [
        (Panel::Changes, Panel::Chat, Edge::Top),
        (Panel::Sidebar, Panel::Chat, Edge::Right),
        (Panel::Chat, Panel::Changes, Edge::Bottom),
        (Panel::Terminal, Panel::Chat, Edge::Left),
    ] {
        let mut expected = ui.read(cx, |app, _| app.dock_layout.clone());
        expected.move_panel(panel, target, edge);
        dock(&ui, cx, panel, target, edge);
        assert_eq!(ui.read(cx, |app, _| app.dock_layout.clone()), expected);
        let moving = panel_bounds(&ui, cx, panel);
        let target = panel_bounds(&ui, cx, target);
        match edge {
            Edge::Left => assert!(moving.right() <= target.left() + px(2.)),
            Edge::Right => assert!(moving.left() >= target.right() - px(2.)),
            Edge::Top => assert!(moving.bottom() <= target.top() + px(2.)),
            Edge::Bottom => assert!(moving.top() >= target.bottom() - px(2.)),
        }
        assert!(!ui.with(cx, |_, cx| cx.has_active_drag()));
    }
    assert_eq!(
        ui.read(cx, |app, _| (
            app.session().uid,
            app.composer.entity_id(),
            app.terminal.active_view().unwrap().entity_id(),
        )),
        before
    );
    assert_eq!(ui.composer_text(cx), "keep this draft");
    assert!(!has(&ui, cx, "dock-target-chat-left"));
}

#[gpui_kit::test]
fn docking_survives_panel_toggles_and_restart_and_can_be_reset(cx: &mut TestAppContext) {
    use flint_app::docking::{Edge, Layout, Panel};
    let (_dir, options) = docking_options();
    let ui = open_with(cx, options.clone());
    ui.press(cx, "ctrl-`");
    dock(&ui, cx, Panel::Terminal, Panel::Chat, Edge::Right);
    let layout = ui.read(cx, |app, _| app.dock_layout.clone());
    let terminal = ui.read(cx, |app, _| app.terminal.active_view().unwrap().entity_id());
    ui.press(cx, "ctrl-`");
    assert!(!has(&ui, cx, "terminal-panel"));
    ui.press(cx, "ctrl-`");
    assert_eq!(
        ui.read(cx, |app, _| app.terminal.active_view().unwrap().entity_id()),
        terminal
    );
    assert_eq!(ui.read(cx, |app, _| app.dock_layout.clone()), layout);
    let again = open_with(cx, options);
    assert_eq!(again.read(cx, |app, _| app.dock_layout.clone()), layout);
    assert!(
        panel_bounds(&again, cx, Panel::Terminal).left()
            >= panel_bounds(&again, cx, Panel::Chat).right() - px(2.)
    );
    again.press(cx, "cmd-k");
    again.input(cx, "Reset panel layout");
    again.press(cx, "enter");
    assert_eq!(
        again.read(cx, |app, _| app.dock_layout.clone()),
        Layout::default()
    );
    assert!(
        panel_bounds(&again, cx, Panel::Terminal).top()
            >= panel_bounds(&again, cx, Panel::Chat).bottom() - px(2.)
    );
}

#[gpui_kit::test]
fn dropping_outside_a_dock_target_does_not_change_the_layout(cx: &mut TestAppContext) {
    let (_dir, options) = docking_options();
    let ui = open_with(cx, options);
    let before = ui.read(cx, |app, _| app.dock_layout.clone());
    ui.with(cx, |window, cx| {
        window.render_frame(cx);
        let from = window.find("dock-handle-sidebar").bounds().center();
        let to = window.find("dock-panel-chat").bounds().center();
        window.drag(from, to, cx);
    });
    settle(cx);
    assert_eq!(ui.read(cx, |app, _| app.dock_layout.clone()), before);
    assert!(!has(&ui, cx, "dock-target-chat-left"));
}

#[gpui_kit::test]
fn docking_escape_cancels_the_drag_without_interrupting_a_running_turn(cx: &mut TestAppContext) {
    use gpui_kit::{InputEvent as _, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent};
    let (_dir, options) = docking_options();
    let ui = open_with(cx, options);
    let engine = ui.engine(cx);
    ui.input(cx, "keep working");
    ui.press(cx, "enter");
    engine.send(cx, AgentEvent::TurnStarted { turn_id: 1 });
    engine.sent();
    let before = ui.read(cx, |app, _| app.dock_layout.clone());
    ui.with(cx, |window, cx| {
        window.render_frame(cx);
        let from = window.find("dock-handle-sidebar").bounds().center();
        window.dispatch_event(
            MouseDownEvent {
                button: MouseButton::Left,
                position: from,
                modifiers: Default::default(),
                click_count: 1,
                first_mouse: false,
            }
            .to_platform_input(),
            cx,
        );
        window.render_frame(cx);
        let to = from + point(px(30.), px(30.));
        window.dispatch_event(
            MouseMoveEvent {
                position: to,
                pressed_button: Some(MouseButton::Left),
                modifiers: Default::default(),
            }
            .to_platform_input(),
            cx,
        );
        window.render_frame(cx);
        assert!(cx.has_active_drag());
        assert!(window.try_find("dock-target-chat-left").is_some());
        window.press("escape", cx);
        assert!(!cx.has_active_drag());
        window.dispatch_event(
            MouseUpEvent {
                button: MouseButton::Left,
                position: to,
                modifiers: Default::default(),
                click_count: 1,
            }
            .to_platform_input(),
            cx,
        );
        window.render_frame(cx);
    });
    settle(cx);
    assert_eq!(ui.read(cx, |app, _| app.dock_layout.clone()), before);
    assert!(ui.read(cx, |app, _| app.session().view.running));
    assert!(engine.sent().is_empty());
    assert!(!has(&ui, cx, "dock-target-chat-left"));
}

#[gpui_kit::test]
fn docking_split_resize_is_saved_and_restored(cx: &mut TestAppContext) {
    use flint_app::docking::Panel;
    let (_dir, options) = docking_options();
    let options = Options {
        open_terminal: true,
        ..options
    };
    let ui = open_with(cx, options.clone());
    let before = panel_bounds(&ui, cx, Panel::Terminal);
    ui.with(cx, |window, cx| {
        let from = point(before.center().x, before.top());
        window.drag(from, from - point(px(0.), px(80.)), cx);
    });
    settle(cx);
    let after = panel_bounds(&ui, cx, Panel::Terminal);
    assert!(
        after.size.height > before.size.height + px(50.),
        "{before:?} -> {after:?}"
    );
    let saved = ui.read(cx, |app, _| app.dock_layout.clone());
    let again = open_with(cx, options);
    assert_eq!(again.read(cx, |app, _| app.dock_layout.clone()), saved);
    let restored = panel_bounds(&again, cx, Panel::Terminal);
    assert!((restored.size.height - after.size.height).abs() <= px(2.));
}

#[gpui_kit::test]
fn docking_nested_splits_keep_every_panel_reachable_after_resizing(cx: &mut TestAppContext) {
    use flint_app::docking::{Edge, Panel};
    let (_dir, options) = docking_options();
    let ui = open_with(
        cx,
        Options {
            open_changes: true,
            open_terminal: true,
            ..options
        },
    );
    dock(&ui, cx, Panel::Terminal, Panel::Chat, Edge::Right);
    dock(&ui, cx, Panel::Changes, Panel::Terminal, Edge::Right);
    dock(&ui, cx, Panel::Sidebar, Panel::Changes, Edge::Right);
    let chat = panel_bounds(&ui, cx, Panel::Chat);
    ui.with(cx, |window, cx| {
        let from = point(chat.right(), chat.center().y);
        window.drag(from, from + point(px(1200.), px(0.)), cx);
    });
    settle(cx);
    let mut previous = None;
    for panel in [Panel::Chat, Panel::Terminal, Panel::Changes, Panel::Sidebar] {
        let bounds = panel_bounds(&ui, cx, panel);
        assert!(bounds.size.width >= px(99.), "{panel:?}: {bounds:?}");
        assert!(bounds.right() <= px(1441.), "{panel:?}: {bounds:?}");
        if let Some(right) = previous {
            assert!(bounds.left() >= right - px(2.), "{panel:?}: {bounds:?}");
        }
        previous = Some(bounds.right());
        assert!(has(&ui, cx, &format!("dock-handle-{}", panel.id())));
    }
}

#[gpui_kit::test]
fn sidebar_rows_hold_still_while_streaming_and_track_progress(cx: &mut TestAppContext) {
    let ui = open(cx);
    let engine = running_turn(&ui, cx);
    let touched = ui.read(cx, |app, _| app.session().touched);
    std::thread::sleep(std::time::Duration::from_millis(5));
    engine.send(cx, AgentEvent::ReasoningDelta("planning".into()));
    engine.send(cx, AgentEvent::TextDelta("streaming".into()));
    // Streaming doesn't count as new activity: rows don't reorder per token.
    assert_eq!(ui.read(cx, |app, _| app.session().touched), touched);
    let line = ui.read(cx, |app, _| app.session().status_line(app.now()).0);
    assert!(line.starts_with("Writing… · "), "{line}");
    assert!(ui.with(cx, |window, cx| {
        window.render_frame(cx);
        window.try_find(("sidebar-when", 0usize)).is_some()
    }));
    engine.send(
        cx,
        AgentEvent::TurnFinished {
            turn_id: 1,
            reason: TurnEndReason::Failed("provider returned 500".into()),
        },
    );
    assert!(ui.read(cx, |app, _| app.session().touched) > touched);
    assert_eq!(
        ui.read(cx, |app, _| app.session().status()),
        flint_app::session::Status::Failed { seen: true }
    );
}

#[gpui_kit::test]
fn sidebar_grouping_is_remembered_across_launches(cx: &mut TestAppContext) {
    use flint_app::app::SessionGrouping;
    let options = test_options();
    let ui = open_with(cx, options.clone());
    assert_eq!(ui.read(cx, |app, _| app.grouping), SessionGrouping::Project);
    ui.app
        .update(cx, |app, cx| app.set_grouping(SessionGrouping::Agent, cx));
    let again = open_with(cx, options);
    assert_eq!(
        again.read(cx, |app, _| app.grouping),
        SessionGrouping::Agent
    );
}

#[gpui_kit::test]
fn a_hidden_sidebar_still_shows_sessions_that_need_you(cx: &mut TestAppContext) {
    let ui = open_with(
        cx,
        Options {
            window_size: Some((900., 800.)),
            ..test_options()
        },
    );
    assert!(ui.read(cx, |app, _| !app.sidebar_visible));
    let engine = running_turn(&ui, cx);
    // Another session is on screen; the first one works in the background.
    ui.press(cx, "cmd-n");
    assert!(has(&ui, cx, "sessions-working"));
    assert!(!has(&ui, cx, "sessions-waiting"));
    approval_requested(&engine, cx, "background-approval");
    assert!(has(&ui, cx, "sessions-waiting"));
    assert!(!has(&ui, cx, "sessions-working"));
}

#[gpui_kit::test]
fn sidebar_filter_chips_stay_in_place_when_counts_change(cx: &mut TestAppContext) {
    let ui = open(cx);
    seed_sidebar_sessions(&ui, cx, 3);
    let chips = |ui: &Ui, cx: &mut TestAppContext| {
        ui.with(cx, |window, cx| {
            window.render_frame(cx);
            (0..4usize)
                .map(|ix| window.try_find(("session-filter", ix)).map(|e| e.bounds()))
                .collect::<Vec<_>>()
        })
    };
    let before = chips(&ui, cx);
    assert!(before.iter().all(Option::is_some), "{before:?}");
    let _engine = running_turn(&ui, cx);
    assert_eq!(chips(&ui, cx), before);
}
