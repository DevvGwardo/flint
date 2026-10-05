use super::*;

fn running(cx: &mut TestAppContext) -> (Ui, Engine) {
    let ui = open(cx);
    let engine = ui.engine(cx);
    ui.input(cx, "first task");
    ui.press(cx, "enter");
    engine.sent();
    engine.send(cx, AgentEvent::TurnStarted { turn_id: 1 });
    (ui, engine)
}

fn ids(ui: &Ui, cx: &mut TestAppContext) -> Vec<u64> {
    ui.read(cx, |app, _| {
        app.session()
            .prompt_queue
            .items
            .iter()
            .map(|prompt| prompt.id)
            .collect()
    })
}

fn row_id(action: &str, id: u64) -> ElementId {
    ElementId::Name(format!("queue-{action}-{id}").into())
}

#[gpui_kit::test]
fn prompt_queue_waits_for_turns_and_keeps_fifo_before_turn_started(cx: &mut TestAppContext) {
    let ui = open(cx);
    let engine = ui.engine(cx);
    ui.input(cx, "first");
    ui.press(cx, "enter");
    assert!(matches!(engine.sent().as_slice(), [Op::UserMessage(text)] if text == "first"));
    ui.input(cx, "second");
    ui.press(cx, "enter");
    ui.input(cx, "third");
    ui.click(cx, "send");
    assert!(engine.sent().is_empty());
    assert_eq!(ids(&ui, cx).len(), 2);
    assert_eq!(
        ui.read(cx, |app, _| app
            .session()
            .view
            .items
            .iter()
            .filter(|item| matches!(item, Item::User(_)))
            .count()),
        1
    );
    engine.send(cx, AgentEvent::TurnStarted { turn_id: 1 });
    engine.send(
        cx,
        AgentEvent::TurnFinished {
            turn_id: 1,
            reason: TurnEndReason::Completed,
        },
    );
    assert!(matches!(engine.sent().as_slice(), [Op::UserMessage(text)] if text == "second"));
    assert_eq!(ids(&ui, cx).len(), 1);
    // A late duplicate cannot advance the next pending prompt.
    engine.send(
        cx,
        AgentEvent::TurnFinished {
            turn_id: 1,
            reason: TurnEndReason::Completed,
        },
    );
    assert!(engine.sent().is_empty());
    engine.send(cx, AgentEvent::TurnStarted { turn_id: 2 });
    engine.send(
        cx,
        AgentEvent::TurnFinished {
            turn_id: 2,
            reason: TurnEndReason::Completed,
        },
    );
    assert!(matches!(engine.sent().as_slice(), [Op::UserMessage(text)] if text == "third"));
    assert!(ids(&ui, cx).is_empty());
}

#[gpui_kit::test]
fn prompt_queue_reorder_edit_remove_and_pause_are_real_controls(cx: &mut TestAppContext) {
    let (ui, engine) = running(cx);
    for prompt in ["second", "third", "fourth"] {
        ui.input(cx, prompt);
        ui.press(cx, "enter");
    }
    let before = ids(&ui, cx);
    ui.click(cx, "queue-toggle");
    ui.click(cx, row_id("up", before[1]));
    assert_eq!(ids(&ui, cx), [before[1], before[0], before[2]]);
    ui.click(cx, row_id("edit", before[1]));
    ui.press(cx, "cmd-a");
    ui.input(cx, "edited third");
    ui.click(cx, "queue-edit-save");
    assert_eq!(
        ui.read(cx, |app, _| app.session().prompt_queue.items[0]
            .text
            .clone()),
        "edited third"
    );
    ui.click(cx, row_id("remove", before[2]));
    assert_eq!(ids(&ui, cx).len(), 2);
    ui.click(cx, "queue-pause");
    engine.send(
        cx,
        AgentEvent::TurnFinished {
            turn_id: 1,
            reason: TurnEndReason::Completed,
        },
    );
    assert!(engine.sent().is_empty());
    ui.click(cx, "queue-pause");
    assert!(matches!(engine.sent().as_slice(), [Op::UserMessage(text)] if text == "edited third"));
    assert_eq!(ids(&ui, cx), [before[0]]);
}

#[gpui_kit::test]
fn prompt_queue_native_steering_waits_for_acceptance_and_keeps_other_prompts(
    cx: &mut TestAppContext,
) {
    let (ui, engine) = running(cx);
    for prompt in ["later task", "steer this task"] {
        ui.input(cx, prompt);
        ui.press(cx, "enter");
    }
    let before = ids(&ui, cx);
    ui.click(cx, "queue-toggle");
    ui.click(cx, row_id("steer", before[1]));
    assert!(
        matches!(engine.sent().as_slice(), [Op::SteerMessage { id, text, .. }] if *id == before[1] && text == "steer this task")
    );
    assert_eq!(ids(&ui, cx), before);
    assert!(ui.read(cx, |app, _| app.session().view.running));
    engine.send(cx, AgentEvent::SteeringAccepted { id: before[1] });
    assert_eq!(ids(&ui, cx), [before[0]]);
    assert!(ui.read(cx, |app, _| app.session().view.items.iter().any(
        |item| matches!(item, Item::User(text) if text == "steer this task")
    )));
    assert_eq!(
        ui.read(cx, |app, _| app.session().list.item_count()),
        ui.read(cx, |app, _| app.session().view.items.len())
    );
    engine.send(
        cx,
        AgentEvent::TurnFinished {
            turn_id: 1,
            reason: TurnEndReason::Completed,
        },
    );
    assert!(matches!(engine.sent().as_slice(), [Op::UserMessage(text)] if text == "later task"));
}

#[gpui_kit::test]
fn prompt_queue_acp_steering_interrupts_then_sends_the_selected_prompt(cx: &mut TestAppContext) {
    let (ui, engine) = running(cx);
    ui.app.update(cx, |app, _| {
        app.sessions[app.active].agent = flint_agent::AgentKind::Droid
    });
    ui.input(cx, "ACP follow-up");
    ui.press(cx, "enter");
    let id = ids(&ui, cx)[0];
    ui.click(cx, "queue-toggle");
    ui.click(cx, row_id("steer", id));
    assert!(matches!(engine.sent().as_slice(), [Op::Interrupt]));
    assert_eq!(ids(&ui, cx), [id]);
    engine.send(
        cx,
        AgentEvent::TurnFinished {
            turn_id: 1,
            reason: TurnEndReason::Interrupted,
        },
    );
    assert!(matches!(engine.sent().as_slice(), [Op::UserMessage(text)] if text == "ACP follow-up"));
    assert!(ids(&ui, cx).is_empty());
}

#[gpui_kit::test]
fn prompt_queue_stop_and_failure_pause_without_losing_work(cx: &mut TestAppContext) {
    let (ui, engine) = running(cx);
    ui.input(cx, "keep queued");
    ui.press(cx, "enter");
    ui.click(cx, "stop");
    assert!(matches!(engine.sent().as_slice(), [Op::Interrupt]));
    engine.send(
        cx,
        AgentEvent::TurnFinished {
            turn_id: 1,
            reason: TurnEndReason::Interrupted,
        },
    );
    assert!(engine.sent().is_empty());
    assert_eq!(ids(&ui, cx).len(), 1);
    assert!(ui.read(cx, |app, _| app.session().prompt_queue.paused));
    ui.click(cx, "queue-pause");
    assert!(matches!(engine.sent().as_slice(), [Op::UserMessage(text)] if text == "keep queued"));
}

#[gpui_kit::test]
fn prompt_queue_snapshots_files_and_images_and_restores_paused(cx: &mut TestAppContext) {
    let (ui, engine) = running(cx);
    let workspace = ui.read(cx, |app, _| app.session().workspace.clone());
    let home = ui.read(cx, |app, _| app.home.clone());
    let dir = ui.read(cx, |app, _| app.session().dir.clone().unwrap());
    let file = workspace.join("fixture.txt");
    let png = workspace.join("fixture.png");
    std::fs::write(&file, "frozen file content").unwrap();
    std::fs::write(&png, b"\x89PNG\r\n\x1a\nsnapshot").unwrap();
    ui.app.update(cx, |app, _| {
        app.attachments.push("fixture.txt".into());
        app.add_image_attachment(png.clone());
    });
    ui.input(cx, "use these attachments");
    ui.press(cx, "enter");
    let saved = ui.read(cx, |app, _| app.session().prompt_queue.items[0].clone());
    std::fs::write(&file, "later file change").unwrap();
    std::fs::remove_file(&png).unwrap();
    assert!(saved.message.contains("frozen file content"));
    assert!(!saved.images[0].data.is_empty());
    let restored = open_with(
        cx,
        Options {
            home: Some(home),
            workspace: Some(workspace),
            ..test_options()
        },
    );
    let snapshot = restored.read(cx, |app, _| {
        let session = app
            .sessions
            .iter()
            .find(|session| session.dir.as_ref() == Some(&dir))
            .unwrap();
        (
            session.prompt_queue.items[0].clone(),
            session.prompt_queue.paused,
            session.ops.is_none(),
        )
    });
    assert_eq!(snapshot, (saved, true, true));
    assert!(engine.sent().is_empty());
}

#[gpui_kit::test]
fn prompt_queue_background_sessions_dispatch_only_their_own_work(cx: &mut TestAppContext) {
    let (ui, first) = running(cx);
    ui.input(cx, "first session follow-up");
    ui.press(cx, "enter");
    ui.press(cx, "cmd-n");
    let second = ui.engine(cx);
    ui.input(cx, "second task");
    ui.press(cx, "enter");
    second.sent();
    second.send(cx, AgentEvent::TurnStarted { turn_id: 1 });
    ui.input(cx, "second follow-up");
    ui.press(cx, "enter");
    first.send(
        cx,
        AgentEvent::TurnFinished {
            turn_id: 1,
            reason: TurnEndReason::Completed,
        },
    );
    assert!(
        matches!(first.sent().as_slice(), [Op::UserMessage(text)] if text == "first session follow-up")
    );
    assert!(second.sent().is_empty());
    assert_eq!(
        ui.read(cx, |app, _| app.session().prompt_queue.items[0]
            .text
            .clone()),
        "second follow-up"
    );
}

#[gpui_kit::test]
fn prompt_queue_full_or_unwritable_queue_keeps_the_draft(cx: &mut TestAppContext) {
    let (ui, engine) = running(cx);
    let dir = ui.read(cx, |app, _| app.session().dir.clone().unwrap());
    std::fs::create_dir(dir.join("prompt-queue.json")).unwrap();
    ui.input(cx, "must not disappear");
    ui.press(cx, "enter");
    assert_eq!(
        ui.read(cx, |app, cx| app.composer.read(cx).value().to_string()),
        "must not disappear"
    );
    assert!(ids(&ui, cx).is_empty());
    assert!(engine.sent().is_empty());
    assert!(ui.read(cx, |app, _| app.session().prompt_queue.error.is_some()));
}

#[gpui_kit::test]
fn prompt_queue_steer_draft_button_and_keyboard_preserve_the_running_turn(cx: &mut TestAppContext) {
    let (ui, engine) = running(cx);
    ui.input(cx, "new direction");
    ui.click(cx, "steer-draft");
    let id = ids(&ui, cx)[0];
    assert!(
        matches!(engine.sent().as_slice(), [Op::SteerMessage { text, .. }] if text == "new direction")
    );
    ui.input(cx, "another direction");
    assert_eq!(
        ui.composer_text(cx),
        "another direction",
        "draft was not focused before the shortcut"
    );
    ui.press(cx, "cmd-enter");
    assert!(engine.sent().is_empty());
    assert_eq!(
        ui.composer_text(cx),
        "another direction",
        "shortcut cleared draft; queue {:?}",
        ids(&ui, cx)
    );
    assert_eq!(ids(&ui, cx), [id]);
    engine.send(cx, AgentEvent::SteeringAccepted { id });
    ui.press(cx, "cmd-enter");
    assert!(
        matches!(engine.sent().as_slice(), [Op::SteerMessage { text, .. }] if text == "another direction")
    );
    assert!(ui.read(cx, |app, _| app.session().view.running));
}

#[gpui_kit::test]
fn prompt_queue_editing_defers_dispatch_until_the_saved_edit_is_ready(cx: &mut TestAppContext) {
    let (ui, engine) = running(cx);
    ui.input(cx, "pending instruction");
    ui.press(cx, "enter");
    let id = ids(&ui, cx)[0];
    ui.click(cx, "queue-toggle");
    ui.click(cx, row_id("edit", id));
    engine.send(
        cx,
        AgentEvent::TurnFinished {
            turn_id: 1,
            reason: TurnEndReason::Completed,
        },
    );
    assert!(engine.sent().is_empty());
    ui.press(cx, "cmd-a");
    ui.input(cx, "saved instruction");
    ui.click(cx, "queue-edit-save");
    assert!(
        matches!(engine.sent().as_slice(), [Op::UserMessage(text)] if text == "saved instruction")
    );
}

#[gpui_kit::test]
fn prompt_queue_compact_panes_keep_input_and_queue_controls_inside_their_panel(
    cx: &mut TestAppContext,
) {
    use flint_app::docking::Edge;
    use flint_app::session_workspace::PaneMode;
    let (ui, sessions) = session_pane_fixture(cx, 4);
    for (_, engine) in &sessions {
        engine.send(cx, AgentEvent::TurnStarted { turn_id: 1 });
    }
    ui.input(cx, "compact follow-up");
    ui.press(cx, "enter");
    ui.with(cx, |window, cx| {
        ui.app.update(cx, |app, cx| {
            for ix in 1..sessions.len() {
                app.split_session_pane(sessions[ix].0, sessions[ix - 1].0, Edge::Right, window, cx);
            }
            app.arrange_session_grid(cx);
            for (uid, _) in &sessions {
                app.set_session_pane_mode(*uid, PaneMode::Chat, window, cx);
            }
            app.focus_session_pane(sessions[0].0, true, window, cx);
        });
    });
    settle(cx);
    ui.with(cx, |window, cx| {
        for _ in 0..3 {
            window.render_frame(cx);
        }
        let card = ui.app.read(cx).popover_room.anchor.get().unwrap();
        for id in ["send", "queue-toggle", "stop"] {
            let bounds = window.find(ElementId::Name(id.into())).bounds();
            assert!(
                bounds.left() >= card.left() && bounds.right() <= card.right(),
                "{id}: {bounds:?} outside {card:?}"
            );
            assert!(
                bounds.top() >= card.top() && bounds.bottom() <= card.bottom(),
                "{id}: {bounds:?} outside {card:?}"
            );
        }
        assert!(window.find("composer-body").bounds().size.height >= px(44.));
    });
    ui.click(cx, "queue-toggle");
    assert!(has(&ui, cx, "prompt-queue"));
    assert_popover_inside_chat(&ui, cx);
    ui.press(cx, "escape");
    assert!(!ui.read(cx, |app, _| app.queue_popover));
}

#[gpui_kit::test]
fn prompt_queue_failed_turns_keep_pending_prompts_paused(cx: &mut TestAppContext) {
    let (ui, engine) = running(cx);
    ui.input(cx, "review after failure");
    ui.press(cx, "enter");
    engine.send(
        cx,
        AgentEvent::TurnFinished {
            turn_id: 1,
            reason: TurnEndReason::Failed("fixture failure".into()),
        },
    );
    assert!(engine.sent().is_empty());
    assert!(ui.read(cx, |app, _| app.session().prompt_queue.paused));
    assert_eq!(ids(&ui, cx).len(), 1);
}

#[gpui_kit::test]
fn prompt_queue_short_windows_use_a_popover_with_visible_steering_controls(
    cx: &mut TestAppContext,
) {
    let (ui, _) = running(cx);
    ui.input(cx, "first follow-up");
    ui.press(cx, "enter");
    ui.input(cx, "second follow-up");
    ui.press(cx, "enter");
    let id = ids(&ui, cx)[0];
    cx.simulate_window_resize(ui.window, size(px(900.), px(560.)));
    settle(cx);
    ui.with(cx, |window, cx| {
        for _ in 0..3 {
            window.render_frame(cx);
        }
    });
    assert!(!has(&ui, cx, "prompt-queue"));
    ui.click(cx, "queue-toggle");
    assert_popover_inside_chat(&ui, cx);
    ui.with(cx, |window, cx| {
        window.render_frame(cx);
        let queue = window.find("prompt-queue").bounds();
        let steer = window.find(row_id("steer", id)).bounds();
        assert!(
            steer.top() >= queue.top() && steer.bottom() <= queue.bottom(),
            "steering is clipped: {steer:?} outside {queue:?}"
        );
        assert!(steer.left() >= queue.left() && steer.right() <= queue.right());
    });
}

#[gpui_kit::test]
fn long_queued_preview_fits_and_editing_and_sending_keep_the_complete_request(
    cx: &mut TestAppContext,
) {
    for window_size in [(1440., 900.), (900., 560.)] {
        let ui = open_with(
            cx,
            Options {
                window_size: Some(window_size),
                ..test_options()
            },
        );
        let engine = running_turn(&ui, cx);
        let text = format!("{}\nORIGINAL_BODY", "界🙂".repeat(2048));
        let message = format!("{text}\n\n[Captured context]\nFROZEN_ATTACHMENT");
        let id = ui.app.update(cx, |app, cx| {
            let queue = &mut app.sessions[app.active].prompt_queue;
            queue.paused = true;
            let id = queue
                .enqueue(flint_app::prompt_queue::Prompt::new(
                    text.clone(),
                    message.clone(),
                    Vec::new(),
                ))
                .unwrap();
            cx.notify();
            id
        });
        settle(cx);
        if !has(&ui, cx, "prompt-queue") {
            ui.click(cx, "queue-toggle");
        }
        ui.with(cx, |window, cx| {
            window.render_frame(cx);
            let preview = window.find(row_id("preview", id)).bounds();
            let detail = window.find(row_id("detail", id)).bounds();
            let queue = window.find("prompt-queue").bounds();
            assert!(preview.left() >= queue.left() && preview.right() <= queue.right());
            assert!(preview.size.height > px(0.) && preview.size.height < px(40.));
            assert!(detail.left() >= queue.left() && detail.right() <= queue.right());
            assert!(detail.size.height > px(0.) && detail.size.height < px(30.));
        });
        ui.click(cx, row_id("edit", id));
        ui.read(cx, |app, cx| {
            assert_eq!(
                app.queue_edit
                    .as_ref()
                    .unwrap()
                    .input
                    .read(cx)
                    .value()
                    .as_str(),
                text
            );
            assert_eq!(app.session().prompt_queue.items[0].message, message);
        });
        ui.press(cx, "enter");
        ui.read(cx, |app, _| {
            assert!(app.queue_edit.is_none());
            assert_eq!(app.session().prompt_queue.items[0].text, text);
            assert_eq!(app.session().prompt_queue.items[0].message, message);
        });
        ui.click(cx, row_id("steer", id));
        assert!(
            matches!(engine.sent().as_slice(), [Op::SteerMessage { id: sent, text, .. }] if *sent == id && text == &message)
        );
        assert_eq!(ids(&ui, cx), [id]);
    }
}

#[gpui_kit::test]
fn long_queue_editor_pointer_actions_stay_visible_and_keep_the_complete_request(
    cx: &mut TestAppContext,
) {
    for window_size in [(1440., 900.), (1000., 700.), (900., 560.)] {
        let ui = open_with(
            cx,
            Options {
                window_size: Some(window_size),
                ..test_options()
            },
        );
        let engine = running_turn(&ui, cx);
        let text = format!("{}\n{}", "界🙂".repeat(256), "line\n".repeat(12))
            .trim()
            .to_string();
        let message = format!("{text}\n\n[Captured context]\nFROZEN_ATTACHMENT");
        let id = ui.app.update(cx, |app, cx| {
            let queue = &mut app.sessions[app.active].prompt_queue;
            queue.paused = true;
            let id = queue
                .enqueue(flint_app::prompt_queue::Prompt::new(
                    text.clone(),
                    message.clone(),
                    Vec::new(),
                ))
                .unwrap();
            cx.notify();
            id
        });
        settle(cx);
        if !has(&ui, cx, "prompt-queue") {
            ui.click(cx, "queue-toggle");
        }
        ui.click(cx, row_id("edit", id));
        ui.with(cx, |window, cx| {
            window.render_frame(cx);
            let queue = window.find("prompt-queue").bounds();
            let rows = window.find("queue-rows").bounds();
            let footer = window.find("queue-edit-footer").bounds();
            assert!(rows.bottom() <= footer.top());
            for action in ["queue-edit-save", "queue-edit-cancel"] {
                let button = window.find(action).bounds();
                eprintln!("queue_editor_geometry window={window_size:?} action={action} queue={queue:?} rows={rows:?} button={button:?}");
                assert!(button.left() >= queue.left() && button.right() <= queue.right());
                assert!(
                    button.top() >= queue.top() && button.bottom() <= queue.bottom(),
                    "{action} clipped: {button:?} outside {queue:?}"
                );
                assert!(button.bottom() <= px(window_size.1));
            }
        });
        ui.click(cx, "queue-edit-save");
        ui.read(cx, |app, _| {
            assert!(
                app.queue_edit.is_none(),
                "pointer save did not exit edit mode"
            );
            assert_eq!(app.session().prompt_queue.items[0].text, text);
            assert_eq!(app.session().prompt_queue.items[0].message, message);
        });
        if !has(&ui, cx, "prompt-queue") {
            ui.click(cx, "queue-toggle");
        }
        ui.click(cx, row_id("edit", id));
        ui.press(cx, "cmd-a");
        ui.input(cx, " ");
        ui.click(cx, "queue-edit-save");
        assert!(has(&ui, cx, "queue-error"));
        ui.read(cx, |app, _| {
            assert!(app.queue_edit.is_some());
            assert_eq!(app.session().prompt_queue.items[0].text, text);
            assert_eq!(app.session().prompt_queue.items[0].message, message);
        });
        ui.with(cx, |window, cx| {
            window.render_frame(cx);
            let queue = window.find("prompt-queue").bounds();
            let cancel = window.find("queue-edit-cancel").bounds();
            assert!(cancel.top() >= queue.top() && cancel.bottom() <= queue.bottom());
        });
        ui.click(cx, "queue-edit-cancel");
        ui.read(cx, |app, _| {
            assert!(
                app.queue_edit.is_none(),
                "pointer cancel did not exit edit mode"
            );
            assert_eq!(app.session().prompt_queue.items[0].text, text);
            assert_eq!(app.session().prompt_queue.items[0].message, message);
            assert!(app.session().prompt_queue.paused);
        });
        assert!(engine.sent().is_empty());
    }
}
