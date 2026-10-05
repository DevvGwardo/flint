use super::*;

fn delegate(engine: &Engine, cx: &mut TestAppContext, call: &str, id: &str, prompt: &str) {
    engine.send(
        cx,
        AgentEvent::ToolCallStarted {
            call_id: call.into(),
            name: "spawn_agent".into(),
            kind: ToolKind::Other,
            summary: "Audit permissions".into(),
            args: json!({"label": "Audit permissions", "message": prompt}),
        },
    );
    engine.send(
        cx,
        AgentEvent::SubagentStarted {
            call_id: call.into(),
            session_id: id.into(),
            model: "audit-model".into(),
        },
    );
}

fn child_event(engine: &Engine, cx: &mut TestAppContext, call: &str, event: AgentEvent) {
    engine.send(
        cx,
        AgentEvent::SubagentEvent {
            call_id: call.into(),
            event: Box::new(event),
        },
    );
}

fn fixture(cx: &mut TestAppContext) -> (Ui, Engine, u64) {
    let ui = open(cx);
    let engine = running_turn(&ui, cx);
    let uid = ui.read(cx, |app, _| app.session().uid);
    delegate(&engine, cx, "delegate", "agent-1", "Inspect permissions.");
    child_event(
        &engine,
        cx,
        "delegate",
        AgentEvent::TurnStarted { turn_id: 1 },
    );
    (ui, engine, uid)
}

fn child_row(uid: u64) -> ElementId {
    ElementId::Name(format!("subagent-{uid}-agent-1").into())
}

#[gpui_kit::test]
fn subagent_sidebar_opens_full_history_without_engine_or_draft_changes(cx: &mut TestAppContext) {
    let (ui, engine, uid) = fixture(cx);
    for n in 0..100 {
        engine
            .events
            .try_send(AgentEvent::SubagentEvent {
                call_id: "delegate".into(),
                event: Box::new(tool_started(
                    &format!("agent-1:read-{n}"),
                    ToolKind::Read,
                    "a.txt",
                )),
            })
            .unwrap();
    }
    settle(cx);
    ui.input(cx, "Parent draft stays here");
    let parent_rows = ui.read(cx, |app, _| app.session().view.items.len());
    ui.click(cx, child_row(uid));
    assert!(has(&ui, cx, &format!("subagent-view-{uid}-agent-1")));
    assert!(!has(&ui, cx, "composer"));
    assert_eq!(ui.read(cx, |app, _| app.sessions.len()), 1);
    assert_eq!(
        ui.read(cx, |app, _| app.session().subagent_lists["agent-1"]
            .item_count()),
        101
    );
    assert_eq!(
        ui.read(cx, |app, _| app.session().view.items.len()),
        parent_rows
    );
    assert_eq!(ui.composer_text(cx), "Parent draft stays here");
    ui.with(cx, |window, cx| {
        ui.app.update(cx, |app, cx| app.submit(window, cx))
    });
    assert!(
        engine.sent().is_empty(),
        "viewing a child must not send the hidden parent draft"
    );
    ui.click(cx, ElementId::Name(format!("subagent-back-{uid}").into()));
    assert_eq!(ui.composer_text(cx), "Parent draft stays here");
    assert!(ui.read(cx, |app, _| app.session().selected_subagent.is_none()));
}

#[gpui_kit::test]
fn subagent_sidebar_resume_and_restart_keep_one_complete_conversation(cx: &mut TestAppContext) {
    let options = test_options();
    let ui = open_with(cx, options.clone());
    let engine = running_turn(&ui, cx);
    for (call, turn, prompt, answer) in [
        ("first", 1, "Inspect.", "First findings."),
        ("resume", 2, "Verify.", "Second findings."),
    ] {
        delegate(&engine, cx, call, "agent-1", prompt);
        child_event(&engine, cx, call, AgentEvent::TurnStarted { turn_id: turn });
        child_event(&engine, cx, call, AgentEvent::TextDelta(answer.into()));
        child_event(
            &engine,
            cx,
            call,
            AgentEvent::TurnFinished {
                turn_id: turn,
                reason: TurnEndReason::Completed,
            },
        );
        engine.send(cx, tool_finished(call, "done", None));
    }
    engine.send(
        cx,
        AgentEvent::TurnFinished {
            turn_id: 1,
            reason: TurnEndReason::Completed,
        },
    );
    ui.app
        .update(cx, |app, _| app.sessions[0].flush_records().unwrap());
    let again = open_with(cx, options);
    let uid = again.read(cx, |app, _| {
        let parent = app
            .sessions
            .iter()
            .find(|session| !session.view.subagents.is_empty())
            .unwrap();
        assert_eq!(parent.view.subagents.len(), 1);
        assert_eq!(parent.view.subagents[0].view.turns.len(), 2);
        assert!(parent.ops.is_none());
        parent.uid
    });
    again.click(cx, child_row(uid));
    assert_eq!(
        again.read(cx, |app, _| app.session().subagent_lists["agent-1"]
            .item_count()),
        6
    );
    assert!(has(&again, cx, &format!("subagent-view-{uid}-agent-1")));
}

#[gpui_kit::test]
fn subagent_sidebar_approval_is_visible_and_answers_parent_engine(cx: &mut TestAppContext) {
    let (ui, engine, uid) = fixture(cx);
    child_event(
        &engine,
        cx,
        "delegate",
        tool_started("agent-1:edit", ToolKind::Edit, "a.txt"),
    );
    ui.click(cx, child_row(uid));
    engine.send(
        cx,
        AgentEvent::ApprovalRequested {
            call_id: "agent-1:edit".into(),
            kind: ToolKind::Edit,
            summary: "a.txt".into(),
        },
    );
    assert_eq!(
        ui.read(cx, |app, _| app.session().view.subagents[0]
            .status(&app.session().view)),
        flint_app::session::Status::NeedsApproval
    );
    assert!(has(&ui, cx, "deny-button"));
    ui.click(cx, "deny-button");
    assert!(
        matches!(engine.sent().as_slice(), [Op::Approval { call_id, decision: ApprovalDecision::Deny }] if call_id == "agent-1:edit")
    );
    assert_eq!(
        ui.read(cx, |app, _| app.session().view.pending_approvals),
        0
    );
    ui.press(cx, "escape");
    assert!(ui.read(cx, |app, _| app.session().selected_subagent.is_none()));
    assert!(
        engine.sent().is_empty(),
        "Escape returns to parent, not interrupt"
    );
}

#[gpui_kit::test]
fn subagent_sidebar_tool_controls_cannot_mutate_parent_rows(cx: &mut TestAppContext) {
    let (ui, engine, uid) = fixture(cx);
    child_event(
        &engine,
        cx,
        "delegate",
        AgentEvent::ToolCallStarted {
            call_id: "agent-1:command".into(),
            name: "run_command".into(),
            kind: ToolKind::Command,
            summary: "printf hello".into(),
            args: json!({"command": "printf hello"}),
        },
    );
    let before = ui.read(cx, |app, _| app.session().view.items.clone());
    ui.click(cx, child_row(uid));
    ui.click(cx, ("tool", 1usize));
    assert!(ui.read(cx, |app, _| matches!(&app.session().view.subagents[0].view.items[1], Item::Tool(call) if call.expanded)));
    assert_eq!(
        ui.read(cx, |app, _| app.session().view.items.clone()),
        before
    );
    assert!(!ui.with(cx, |window, cx| {
        window.render_frame(cx);
        window.try_find(("tool-terminal", 1usize)).is_some()
            || window.try_find(("tool-send", 1usize)).is_some()
    }));
    assert!(engine.sent().is_empty());
}

#[gpui_kit::test]
fn subagent_sidebar_search_reveals_collapsed_children(cx: &mut TestAppContext) {
    let (ui, _engine, uid) = fixture(cx);
    ui.click(
        cx,
        ElementId::Name(format!("subagent-disclosure-{uid}").into()),
    );
    assert!(!has(&ui, cx, &format!("subagent-{uid}-agent-1")));
    ui.click(cx, "session-search");
    ui.input(cx, "audit-model");
    assert!(has(&ui, cx, &format!("subagent-{uid}-agent-1")));
    assert_eq!(ui.read(cx, |app, cx| app.visible_sessions(cx)), vec![0]);
    ui.click(cx, child_row(uid));
    assert!(has(&ui, cx, &format!("subagent-view-{uid}-agent-1")));
}

#[gpui_kit::test]
fn subagent_sidebar_narrow_drawer_opens_readable_child_view(cx: &mut TestAppContext) {
    let (ui, _engine, uid) = fixture(cx);
    cx.simulate_window_resize(ui.window, size(px(900.), px(560.)));
    settle(cx);
    ui.click(cx, "sessions-control");
    ui.click(cx, child_row(uid));
    assert!(!ui.read(cx, |app, _| app.session_drawer));
    ui.with(cx, |window, cx| {
        window.render_frame(cx);
        let child = window
            .find(ElementId::Name(
                format!("subagent-view-{uid}-agent-1").into(),
            ))
            .bounds();
        let back = window
            .find(ElementId::Name(format!("subagent-back-{uid}").into()))
            .bounds();
        assert!(back.top() >= child.top() && back.bottom() <= child.bottom());
        assert!(back.right() <= child.right());
    });
}

#[gpui_kit::test]
fn subagent_sidebar_finished_work_copy_and_feedback_are_child_scoped(cx: &mut TestAppContext) {
    let (ui, engine, uid) = fixture(cx);
    child_event(
        &engine,
        cx,
        "delegate",
        tool_started("agent-1:read", ToolKind::Read, "a.txt"),
    );
    child_event(
        &engine,
        cx,
        "delegate",
        tool_finished("agent-1:read", "contents", None),
    );
    child_event(
        &engine,
        cx,
        "delegate",
        AgentEvent::TextDelta("Child answer.".into()),
    );
    child_event(
        &engine,
        cx,
        "delegate",
        AgentEvent::TurnFinished {
            turn_id: 1,
            reason: TurnEndReason::Completed,
        },
    );
    let before = ui.read(cx, |app, _| app.session().view.items.clone());
    ui.click(cx, child_row(uid));
    let (header, end) = ui.read(cx, |app, _| {
        let turn = &app.session().view.subagents[0].view.turns[0];
        (turn.header.unwrap(), turn.end.unwrap())
    });
    ui.click(cx, ("worked", header));
    assert!(ui.read(cx, |app, _| {
        app.session().view.subagents[0].view.turns[0].expanded
    }));
    ui.click(cx, ("copy-button", end));
    assert_eq!(
        cx.read_from_clipboard()
            .and_then(|item| item.text())
            .as_deref(),
        Some("Child answer.")
    );
    assert_eq!(
        ui.read(cx, |app, _| app
            .copied_conversation
            .as_ref()
            .unwrap()
            .child
            .clone()),
        Some("agent-1".into())
    );
    ui.click(cx, ("up", end));
    assert_eq!(
        ui.read(cx, |app, _| app.session().view.subagents[0].view.turns[0]
            .feedback),
        Some(true)
    );
    assert_eq!(
        ui.read(cx, |app, _| app.session().view.items.clone()),
        before
    );
    assert_eq!(
        ui.read(cx, |app, _| app.session().view.turns[0].feedback),
        None
    );
    ui.click(cx, ElementId::Name(format!("subagent-back-{uid}").into()));
    assert!(ui.read(cx, |app, _| app.copied.is_none()));
}

#[gpui_kit::test]
fn subagent_sidebar_same_child_id_in_two_parents_is_isolated(cx: &mut TestAppContext) {
    let (ui, first_engine, first_uid) = fixture(cx);
    child_event(
        &first_engine,
        cx,
        "delegate",
        AgentEvent::TextDelta("First parent child.".into()),
    );
    ui.input(cx, "First parent draft");
    ui.press(cx, "cmd-n");
    let second_engine = running_turn(&ui, cx);
    let second_uid = ui.read(cx, |app, _| app.session().uid);
    delegate(&second_engine, cx, "delegate", "agent-1", "Second task.");
    child_event(
        &second_engine,
        cx,
        "delegate",
        AgentEvent::TurnStarted { turn_id: 1 },
    );
    child_event(
        &second_engine,
        cx,
        "delegate",
        AgentEvent::TextDelta("Second parent child.".into()),
    );
    ui.click(cx, child_row(first_uid));
    assert_eq!(ui.read(cx, |app, _| app.session().uid), first_uid);
    assert_eq!(ui.composer_text(cx), "First parent draft");
    assert!(ui.read(cx, |app, _| {
        app.session().view.subagents[0].view.items.iter().any(
            |item| matches!(item, Item::Assistant { text, .. } if text == "First parent child."),
        )
    }));
    ui.click(cx, child_row(second_uid));
    assert_eq!(ui.read(cx, |app, _| app.session().uid), second_uid);
    assert!(ui.read(cx, |app, _| {
        app.session().view.subagents[0].view.items.iter().any(
            |item| matches!(item, Item::Assistant { text, .. } if text == "Second parent child."),
        )
    }));
    assert!(first_engine.sent().is_empty() && second_engine.sent().is_empty());
}

#[gpui_kit::test]
fn subagent_sidebar_focus_composer_returns_to_parent_and_keeps_draft(cx: &mut TestAppContext) {
    let (ui, engine, uid) = fixture(cx);
    ui.input(cx, "Unsent draft");
    ui.click(cx, child_row(uid));
    ui.press(cx, "cmd-l");
    assert!(ui.read(cx, |app, _| app.session().selected_subagent.is_none()));
    assert_eq!(ui.composer_text(cx), "Unsent draft");
    assert!(ui.with(cx, |window, cx| {
        ui.app
            .read(cx)
            .composer
            .read(cx)
            .focus_handle(cx)
            .is_focused(window)
    }));
    assert!(engine.sent().is_empty());
}

#[gpui_kit::test]
fn subagent_sidebar_in_terminal_tile_opens_child_chat(cx: &mut TestAppContext) {
    use flint_app::docking::Edge;
    use flint_app::session_workspace::PaneMode;
    let (ui, sessions) = session_pane_fixture(cx, 2);
    let uid = sessions[0].0;
    delegate(&sessions[0].1, cx, "delegate", "agent-1", "Inspect.");
    drag_session_pane(&ui, cx, sessions[1].0, uid, Edge::Right);
    ui.click(
        cx,
        ElementId::Name(format!("session-pane-mode-{uid}-terminal").into()),
    );
    ui.click(cx, child_row(uid));
    assert_eq!(
        ui.read(cx, |app, _| app.session_workspace.panes[&uid].mode),
        PaneMode::Chat
    );
    assert!(has(&ui, cx, &format!("subagent-view-{uid}-agent-1")));
    assert_eq!(ui.read(cx, |app, _| app.sessions.len()), 2);
}

#[gpui_kit::test]
fn subagent_sidebar_scroll_and_unread_are_independent(cx: &mut TestAppContext) {
    let (ui, engine, uid) = fixture(cx);
    for n in 0..100 {
        engine
            .events
            .try_send(AgentEvent::SubagentEvent {
                call_id: "delegate".into(),
                event: Box::new(AgentEvent::Error(format!("Earlier diagnostic {n}"))),
            })
            .unwrap();
    }
    settle(cx);
    delegate(&engine, cx, "other", "agent-2", "Other task.");
    child_event(&engine, cx, "other", AgentEvent::TurnStarted { turn_id: 1 });
    ui.click(cx, child_row(uid));
    ui.app.update(cx, |app, cx| {
        app.session().subagent_lists["agent-1"].pause_following_tail();
        app.session().subagent_lists["agent-1"].scroll_to(gpui_kit::ListOffset {
            item_ix: 0,
            offset_in_item: px(0.),
        });
        cx.notify();
    });
    assert!(has(&ui, cx, &format!("subagent-latest-{uid}-agent-1")));
    child_event(
        &engine,
        cx,
        "other",
        AgentEvent::TurnFinished {
            turn_id: 1,
            reason: TurnEndReason::Completed,
        },
    );
    child_event(
        &engine,
        cx,
        "delegate",
        AgentEvent::TurnFinished {
            turn_id: 1,
            reason: TurnEndReason::Completed,
        },
    );
    assert!(ui.read(cx, |app, _| {
        app.session().view.subagent("agent-2").unwrap().unread
    }));
    assert!(!ui.read(cx, |app, _| {
        app.session().view.subagent("agent-1").unwrap().unread
    }));
    ui.click(
        cx,
        ElementId::Name(format!("subagent-{uid}-agent-2").into()),
    );
    assert!(ui.read(cx, |app, _| {
        app.session().subagent_lists["agent-2"].is_following_tail()
    }));
    ui.click(cx, child_row(uid));
    assert!(!ui.read(cx, |app, _| {
        app.session().subagent_lists["agent-1"].is_following_tail()
    }));
    ui.click(
        cx,
        ElementId::Name(format!("subagent-latest-{uid}-agent-1").into()),
    );
    assert!(ui.read(cx, |app, _| {
        app.session().subagent_lists["agent-1"].is_following_tail()
    }));
}

#[gpui_kit::test]
fn subagent_sidebar_keyboard_reveals_offscreen_children(cx: &mut TestAppContext) {
    let (ui, engine, uid) = fixture(cx);
    for n in 2..=30 {
        delegate(
            &engine,
            cx,
            &format!("call-{n}"),
            &format!("agent-{n}"),
            "Inspect.",
        );
    }
    ui.click(cx, "session-search");
    for _ in 0..25 {
        ui.press(cx, "tab");
        ui.with(cx, |window, cx| {
            window.render_frame(cx);
            window.simulate_next_frame(cx);
            window.render_frame(cx);
        });
    }
    ui.with(cx, |window, cx| {
        window.render_frame(cx);
        let row = window
            .find(ElementId::Name(format!("subagent-{uid}-agent-20").into()))
            .bounds();
        let viewport = window.find("session-list").bounds();
        assert!(
            row.top() >= viewport.top() && row.bottom() <= viewport.bottom(),
            "{row:?} outside {viewport:?}"
        );
    });
    ui.key_cycle(cx, "enter");
    assert_eq!(
        ui.read(cx, |app, _| app.session().selected_subagent.clone()),
        Some("agent-20".into())
    );
}
