use super::*;

#[gpui_kit::test]
fn composer_slash_matching_preserves_navigation_and_large_unmatched_drafts(
    cx: &mut TestAppContext,
) {
    for dimensions in [(1440., 900.), (900., 560.)] {
        let ui = open_with(
            cx,
            Options {
                window_size: Some(dimensions),
                ..test_options()
            },
        );
        let engine = ui.engine(cx);
        for draft in ["/MO".to_string(), format!("/{}", "界🙂".repeat(8192))] {
            let (prefix, last) = draft.split_at(draft.char_indices().last().unwrap().0);
            ui.with(cx, |window, cx| {
                ui.app.update(cx, |app, cx| {
                    app.composer.update(cx, |state, cx| {
                        state.set_value(prefix.to_string(), window, cx);
                        state.focus(window, cx);
                    });
                });
            });
            ui.press(cx, "cmd-down");
            ui.input(cx, last);
            settle(cx);
            assert!(ui.read(cx, |app, _| app.slash.is_some()));
            ui.press(cx, "down");
            let expected = usize::from(draft == "/MO");
            assert_eq!(
                ui.read(cx, |app, _| app.slash.as_ref().unwrap().selected),
                expected
            );
            ui.press(cx, "up");
            assert_eq!(
                ui.read(cx, |app, _| app.slash.as_ref().unwrap().selected),
                0
            );
            assert_eq!(ui.composer_text(cx), draft);
            assert!(engine.sent().is_empty());
            ui.press(cx, "escape");
            assert!(ui.read(cx, |app, _| app.slash.is_none()));
            assert_eq!(ui.composer_text(cx), draft);
        }
    }
}

#[gpui_kit::test]
fn composer_approval_letters_require_raw_empty_and_keep_whitespace_drafts(cx: &mut TestAppContext) {
    for dimensions in [(1440., 900.), (900., 560.)] {
        for (key, decision) in [
            ("y", ApprovalDecision::Approve),
            ("a", ApprovalDecision::ApproveAlways),
            ("n", ApprovalDecision::Deny),
        ] {
            for draft in [
                String::new(),
                " ".into(),
                "\u{2003}\u{3000}".into(),
                "\u{200b}".into(),
                format!("{} original draft", "界🙂".repeat(1024)),
            ] {
                let ui = open_with(
                    cx,
                    Options {
                        window_size: Some(dimensions),
                        ..test_options()
                    },
                );
                let engine = running_turn(&ui, cx);
                approval_requested(&engine, cx, "raw-empty-fixture");
                ui.with(cx, |window, cx| {
                    ui.app.update(cx, |app, cx| {
                        app.composer.update(cx, |state, cx| {
                            state.set_value(draft.clone(), window, cx);
                            state.focus(window, cx);
                        });
                        cx.notify();
                    });
                });
                ui.press(cx, "cmd-down");
                ui.press(cx, key);
                if draft.is_empty() {
                    if decision == ApprovalDecision::ApproveAlways {
                        assert!(engine.sent().is_empty());
                        ui.click(cx, "always-button");
                    }
                    assert!(
                        matches!(engine.sent().as_slice(), [Op::Approval { decision: actual, .. }] if *actual == decision)
                    );
                    assert_eq!(ui.composer_text(cx), "");
                } else {
                    assert!(engine.sent().is_empty());
                    assert!(ui.read(cx, |app, _| app.session().view.pending_approval().is_some()));
                    assert_eq!(ui.composer_text(cx), format!("{draft}{key}"));
                }
            }
        }
    }
}

#[gpui_kit::test]
fn composer_eligibility_keeps_unicode_blank_drafts_unsent_and_complete_text(
    cx: &mut TestAppContext,
) {
    for dimensions in [(1440., 900.), (900., 560.)] {
        let ui = open_with(
            cx,
            Options {
                window_size: Some(dimensions),
                ..test_options()
            },
        );
        let engine = ui.engine(cx);
        for blank in ["", " \t\r\n", "\u{85}\u{a0}\u{2003}\u{2028}\u{3000}"] {
            let blank = blank.repeat(512);
            ui.with(cx, |window, cx| {
                ui.app.update(cx, |app, cx| {
                    app.composer
                        .update(cx, |state, cx| state.set_value(blank.clone(), window, cx));
                    cx.notify();
                });
                window.render_frame(cx);
            });
            ui.click(cx, "send");
            assert!(engine.sent().is_empty());
            assert_eq!(ui.composer_text(cx), blank);
        }
        let draft = format!("{}\0界🙂{}", " \u{2003}".repeat(1024), "tail".repeat(1024));
        ui.with(cx, |window, cx| {
            ui.app.update(cx, |app, cx| {
                app.composer
                    .update(cx, |state, cx| state.set_value(draft.clone(), window, cx));
                cx.notify();
            });
            window.render_frame(cx);
        });
        assert_eq!(ui.composer_text(cx), draft);
        ui.click(cx, "send");
        assert!(
            matches!(engine.sent().as_slice(), [Op::UserMessage(text)] if text == draft.trim())
        );
    }
}

#[gpui_kit::test]
fn composer_task_projection_keeps_order_completion_queue_and_stop(cx: &mut TestAppContext) {
    for dimensions in [(1440., 900.), (900., 560.)] {
        let ui = open_with(
            cx,
            Options {
                window_size: Some(dimensions),
                ..test_options()
            },
        );
        let engine = ui.engine(cx);
        ui.input(cx, "Parallel work");
        ui.press(cx, "enter");
        engine.sent();
        engine.send(cx, AgentEvent::TurnStarted { turn_id: 1 });
        for ix in 0..9 {
            engine.send(
                cx,
                tool_started(
                    &format!("task-{ix}"),
                    ToolKind::Command,
                    &format!("job {ix} 界🙂"),
                ),
            );
        }
        engine.send(cx, tool_started("read", ToolKind::Read, "not a command"));
        assert!(has(&ui, cx, "composer-task-tray"));
        ui.with(cx, |window, cx| {
            window.render_frame(cx);
            let first = window.find("running-command-task-0").bounds();
            let second = window.find("running-command-task-1").bounds();
            assert!(first.top() < second.top());
            for ix in 0..9 {
                assert!(
                    window
                        .try_find(ElementId::Name(format!("running-command-task-{ix}").into()))
                        .is_some()
                );
            }
            assert!(window.try_find("running-command-read").is_none());
        });
        engine.send(cx, tool_finished("task-0", "completed", None));
        assert!(!has(&ui, cx, "running-command-task-0"));
        assert!(has(&ui, cx, "running-command-task-8"));
        ui.input(cx, "Follow-up 界🙂");
        ui.click(cx, "send");
        assert!(engine.sent().is_empty());
        assert_eq!(
            ui.read(cx, |app, _| app.session().prompt_queue.items.len()),
            1
        );
        ui.click(cx, "stop");
        assert!(matches!(engine.sent().as_slice(), [Op::Interrupt]));
        assert!(ui.read(cx, |app, _| app.session().prompt_queue.paused));
        engine.send(
            cx,
            AgentEvent::TurnFinished {
                turn_id: 1,
                reason: TurnEndReason::Interrupted,
            },
        );
        assert!(!has(&ui, cx, "composer-task-tray"));
    }
}

#[gpui_kit::test]
fn composer_demo_keeps_running_controls_in_compact_queue_scenes(cx: &mut TestAppContext) {
    for dimensions in [(1440., 900.), (900., 560.)] {
        let ui = open_with(
            cx,
            Options {
                window_size: Some(dimensions),
                demo: true,
                demo_instant: true,
                demo_queue: true,
                demo_stop: Some(27),
                ..test_options()
            },
        );
        settle(cx);
        assert!(ui.read(cx, |app, _| app.session().view.running));
        assert_eq!(
            ui.read(cx, |app, _| app.session().view.running_commands().len()),
            1
        );
        for id in ["composer-task-tray", "stop", "send"] {
            assert!(has(&ui, cx, id), "{dimensions:?}: {id} missing");
            ui.with(cx, |window, cx| {
                window.render_frame(cx);
                let bounds = window.find(id).bounds();
                assert!(bounds.size.width > px(0.) && bounds.size.height > px(0.));
                assert!(
                    bounds.bottom() <= px(dimensions.1) && bounds.right() <= px(dimensions.0),
                    "{dimensions:?}: {id} bounds {bounds:?}"
                );
            });
        }
    }
}

#[gpui_kit::test]
fn composer_refresh_keeps_hints_and_primary_actions_in_a_short_window(cx: &mut TestAppContext) {
    let ui = open_with(
        cx,
        Options {
            window_size: Some((900., 560.)),
            ..test_options()
        },
    );
    let engine = ui.engine(cx);
    assert!(has(&ui, cx, "composer-shortcuts"));
    ui.click(cx, "send");
    assert!(engine.sent().is_empty());
    ui.input(cx, "A useful prompt");
    ui.click(cx, "send");
    assert!(
        matches!(engine.sent().as_slice(), [Op::UserMessage(text)] if text == "A useful prompt")
    );
    assert_eq!(ui.composer_text(cx), "");
    ui.with(cx, |window, cx| {
        window.render_frame(cx);
        let send = window.find("send").bounds();
        let hints = window.find("composer-shortcuts").bounds();
        assert!(send.size.height >= px(32.));
        assert!(send.right() <= px(900.) && send.bottom() <= px(560.));
        assert!(hints.right() <= px(900.) && hints.bottom() <= px(560.));
    });
}

#[gpui_kit::test]
fn composer_refresh_keeps_queue_stop_and_task_output_separate(cx: &mut TestAppContext) {
    let ui = open(cx);
    let engine = ui.engine(cx);
    ui.input(cx, "First task");
    ui.press(cx, "enter");
    engine.sent();
    engine.send(cx, AgentEvent::TurnStarted { turn_id: 1 });
    engine.send(cx, tool_started("command", ToolKind::Command, "cargo test"));
    engine.send(
        cx,
        AgentEvent::ToolOutputDelta {
            call_id: "command".into(),
            chunk: "checking\nlatest output\n".into(),
        },
    );
    assert!(has(&ui, cx, "composer-task-tray"));
    ui.input(cx, "Follow-up");
    ui.click(cx, "send");
    assert!(engine.sent().is_empty());
    assert_eq!(
        ui.read(cx, |app, _| app.session().prompt_queue.items.len()),
        1
    );
    ui.click(cx, "stop");
    assert!(matches!(engine.sent().as_slice(), [Op::Interrupt]));
    assert!(ui.read(cx, |app, _| app.session().prompt_queue.paused));
}

#[gpui_kit::test]
fn composer_query_reads_keep_large_unicode_drafts_mentions_and_slash_selection(
    cx: &mut TestAppContext,
) {
    for dimensions in [(1440., 900.), (900., 560.)] {
        let ui = open_with(
            cx,
            Options {
                window_size: Some(dimensions),
                ..test_options()
            },
        );
        let workspace = ui.read(cx, |app, _| app.session().workspace.clone());
        std::fs::write(workspace.join("guide.md"), "# Fixture guide").unwrap();
        let base = format!(
            "{}{}",
            "Keep the full original context.\n".repeat(256),
            "界🙂 ".repeat(256)
        );
        let draft = format!("{base} @gu");
        ui.with(cx, |window, cx| {
            ui.app.update(cx, |app, cx| {
                app.composer.update(cx, |state, cx| {
                    state.set_value(base.clone(), window, cx);
                    state.focus(window, cx);
                });
            });
        });
        settle(cx);
        ui.press(cx, "cmd-down");
        ui.input(cx, " @gu");
        assert!(has(&ui, cx, "mention-menu"));
        assert_eq!(ui.composer_text(cx), draft);
        assert_eq!(
            ui.read(cx, |app, _| app.mention.as_ref().unwrap().query.clone()),
            "gu"
        );
        ui.press(cx, "enter");
        let expected = format!("{base} @guide.md ");
        assert_eq!(ui.composer_text(cx), expected);
        assert!(!has(&ui, cx, "mention-menu"));
        assert_eq!(
            ui.read(cx, |app, _| app.attachments.clone()),
            vec!["guide.md"]
        );
        let engine = ui.engine(cx);
        ui.click(cx, "send");
        let sent = engine.sent();
        assert!(
            matches!(sent.as_slice(), [Op::UserMessage(text)] if text.contains(expected.trim_end()) && text.contains("# Fixture guide")),
            "{sent:?}"
        );
        assert_eq!(ui.composer_text(cx), "");
        ui.input(cx, "/mode");
        assert!(has(&ui, cx, "slash-menu"));
        ui.press(cx, "enter");
        assert!(!has(&ui, cx, "slash-menu"));
        assert_eq!(ui.composer_text(cx), "");
        assert_eq!(cx.opened_url(), None);
    }
}
