use super::*;
use flint_app::{chat_content::RowKey, settings::TranscriptWidth, subagent_ui::Conversation};
use gpui_kit::component::text::TextViewState;

fn paint(ui: &Ui, cx: &mut TestAppContext) {
    for _ in 0..3 {
        ui.with(cx, |window, cx| window.render_frame(cx));
        settle(cx);
    }
}

fn row(
    ui: &Ui,
    cx: &mut TestAppContext,
    owner: &Conversation,
) -> (usize, RowKey, Entity<TextViewState>) {
    paint(ui, cx);
    ui.read(cx, |app, _| {
        let view = app.conversation_view(owner).unwrap();
        let ix = view
            .items
            .iter()
            .rposition(|item| matches!(item, Item::Assistant { .. }))
            .unwrap();
        let key = RowKey::new(owner, view.row_identity(ix).unwrap());
        let state = app.chat_content.borrow().state(&key).unwrap();
        (ix, key, state)
    })
}

fn owner(ui: &Ui, cx: &mut TestAppContext) -> Conversation {
    ui.read(cx, |app, _| Conversation::parent(app.session().uid))
}

fn action(key: &RowKey, name: &str, start: usize) -> ElementId {
    key.element_id(&format!("code-{name}-{start}"))
}

fn choose_width(ui: &Ui, cx: &mut TestAppContext, choice: usize) {
    ui.press(cx, "cmd-,");
    paint(ui, cx);
    ui.click(cx, ("settings-chat-width", choice));
    ui.click(cx, "settings-save");
    paint(ui, cx);
}

fn column(
    ui: &Ui,
    cx: &mut TestAppContext,
    owner: &Conversation,
    ix: usize,
) -> Bounds<gpui_kit::Pixels> {
    ui.with(cx, |window, _| {
        window
            .find(ElementId::Name(
                format!(
                    "chat-column-{}-{}-{ix}",
                    owner.parent,
                    owner.child.as_deref().unwrap_or("root")
                )
                .into(),
            ))
            .bounds()
    })
}

#[gpui_kit::test]
fn chat_width_settings_real_control_persists_and_reflows_without_reset(cx: &mut TestAppContext) {
    let options = test_options();
    let ui = open_with(cx, options.clone());
    cx.simulate_window_resize(ui.window, size(px(1800.), px(950.)));
    let engine = running_turn(&ui, cx);
    engine.send(
        cx,
        AgentEvent::TextDelta("Select these words 界🙂\n\n```rust\nlet a = 1;\n```".into()),
    );
    let owner = owner(&ui, cx);
    let (ix, key, state) = row(&ui, cx, &owner);
    assert_eq!(column(&ui, cx, &owner, ix).size.width, px(760.));
    let list = ui.read(cx, |app, _| app.session().list.clone());
    // This short fixture fits at the bottom; pause_following_tail is allowed
    // to re-engage there. Normal mode represents intentionally frozen reading.
    list.set_follow_mode(gpui_kit::FollowMode::Normal);
    ui.with(cx, |window, cx| {
        let bounds = window.find(key.element_id("chat-content")).bounds();
        window.drag(
            point(bounds.left() + px(1.), bounds.top() + px(12.)),
            point(bounds.left() + px(80.), bounds.top() + px(12.)),
            cx,
        );
    });
    let selected = cx
        .read(|cx| state.read(cx).selected_source_range())
        .unwrap();
    let focus = ui.with(cx, |window, cx| window.focused(cx));
    for (choice, width) in [
        (1, TranscriptWidth::Wide),
        (2, TranscriptWidth::Fill),
        (0, TranscriptWidth::Readable),
    ] {
        ui.press(cx, "cmd-,");
        paint(&ui, cx);
        assert_eq!(
            cx.read(|cx| state.read(cx).selected_source_range()),
            Some(selected.clone()),
            "opening Settings cleared the transcript selection"
        );
        ui.click(cx, ("settings-chat-width", choice as usize));
        assert_eq!(
            cx.read(|cx| state.read(cx).selected_source_range()),
            Some(selected.clone()),
            "choosing chat width cleared the transcript selection"
        );
        ui.click(cx, "settings-save");
        paint(&ui, cx);
        assert_eq!(ui.read(cx, |app, _| app.settings.transcript_width), width);
        let current = column(&ui, cx, &owner, ix);
        let composer = ui.with(cx, |window, _| window.find("chat-composer-column").bounds());
        assert_eq!(current.size.width, composer.size.width);
        assert_eq!(current.left(), composer.left());
        if let Some(max) = width.maximum() {
            assert_eq!(current.size.width, px(max));
        } else {
            assert!(current.size.width > px(1040.));
        }
        assert_eq!(row(&ui, cx, &owner).2.entity_id(), state.entity_id());
        assert_eq!(
            cx.read(|cx| state.read(cx).selected_source_range()),
            Some(selected.clone())
        );
        assert!(!list.is_following_tail());
        assert_eq!(ui.with(cx, |window, cx| window.focused(cx)), focus);
        assert_eq!(
            ui.read(cx, |app, _| app.chat_content.borrow().update_counts(&key)),
            Some((0, 0))
        );
    }
    let saved =
        flint_app::settings::Settings::load(options.home.as_ref().unwrap(), &KeySources::none());
    assert_eq!(saved.transcript_width, TranscriptWidth::Readable);
    choose_width(&ui, cx, 1);
    let restarted = open_with(cx, options);
    assert_eq!(
        restarted.read(cx, |app, _| app.settings.transcript_width),
        TranscriptWidth::Wide
    );
    assert!(engine.sent().is_empty());
}

#[gpui_kit::test]
fn settings_modal_preserves_selection_and_resumes_pointer_input_after_each_close_action(
    cx: &mut TestAppContext,
) {
    let ui = open(cx);
    let engine = running_turn(&ui, cx);
    engine.send(
        cx,
        AgentEvent::TextDelta("Select these words, then select different words.".into()),
    );
    let owner = owner(&ui, cx);
    let (_, key, state) = row(&ui, cx, &owner);
    for close in ["settings-save", "settings-cancel", "escape"] {
        ui.with(cx, |window, cx| {
            let bounds = window.find(key.element_id("chat-content")).bounds();
            window.drag(
                point(bounds.left() + px(1.), bounds.top() + px(12.)),
                point(bounds.left() + px(80.), bounds.top() + px(12.)),
                cx,
            );
        });
        let selected = cx
            .read(|cx| state.read(cx).selected_source_range())
            .unwrap();
        ui.press(cx, "secondary-,");
        paint(&ui, cx);
        ui.click(cx, ("settings-chat-width", 1usize));
        assert_eq!(
            cx.read(|cx| state.read(cx).selected_source_range()),
            Some(selected.clone())
        );
        if close == "escape" {
            ui.press(cx, "escape");
        } else {
            ui.click(cx, close);
        }
        paint(&ui, cx);
        assert!(!ui.read(cx, |app, _| app.settings_form.is_some()));
        assert_eq!(
            cx.read(|cx| state.read(cx).selected_source_range()),
            Some(selected.clone())
        );
        ui.with(cx, |window, cx| {
            let bounds = window.find(key.element_id("chat-content")).bounds();
            window.drag(
                point(bounds.left() + px(150.), bounds.top() + px(12.)),
                point(bounds.left() + px(260.), bounds.top() + px(12.)),
                cx,
            );
        });
        let changed = cx
            .read(|cx| state.read(cx).selected_source_range())
            .unwrap();
        assert_ne!(
            selected, changed,
            "{close} did not restore pointer selection"
        );
    }
    assert!(engine.sent().is_empty());
}

#[gpui_kit::test]
fn chat_code_actual_copy_stage_preserves_input_selection_attachments_and_no_dispatch(
    cx: &mut TestAppContext,
) {
    let ui = open(cx);
    let engine = running_turn(&ui, cx);
    let code = "界🙂 `one` ``two`` ```three```\n  trailing  \n";
    engine.send(
        cx,
        AgentEvent::TextDelta(format!("````unrecognized-lang\n{code}\n````")),
    );
    let owner = owner(&ui, cx);
    let (_, key, state) = row(&ui, cx, &owner);
    ui.input(cx, "keep my draft");
    let input = ui.read(cx, |app, _| app.composer.entity_id());
    let image = ui.read(cx, |app, _| app.session().workspace.join("image.png"));
    std::fs::write(&image, b"fixture image").unwrap();
    ui.app.update(cx, |app, cx| {
        app.attachments.push("kept.txt".into());
        app.image_attachments.push(image.clone());
        app.composer
            .update(cx, |input, cx| input.set_selected_range(0..4, cx));
        cx.notify();
    });
    paint(&ui, cx);
    let focus = ui.with(cx, |window, cx| window.focused(cx));
    let before = ui.read(cx, |app, _| app.session().view.items.clone());
    let files = std::fs::read(&image).unwrap();
    ui.click(cx, action(&key, "copy", 0));
    assert_eq!(
        cx.read_from_clipboard().and_then(|item| item.text()),
        Some(code.into())
    );
    ui.click(cx, action(&key, "stage", 0));
    let draft = ui.composer_text(cx);
    assert!(draft.contains("keep"));
    assert!(draft.contains(" my draft"));
    assert!(
        draft.contains(&format!("````unrecognized-lang\n{code}\n````")),
        "{draft:?}"
    );
    assert_eq!(ui.read(cx, |app, _| app.composer.entity_id()), input);
    assert_eq!(
        ui.read(cx, |app, _| app.attachments.clone()),
        vec!["kept.txt"]
    );
    assert_eq!(
        ui.read(cx, |app, _| app.image_attachments.clone()),
        vec![image.clone()]
    );
    assert_eq!(
        ui.read(cx, |app, _| app.session().view.items.clone()),
        before
    );
    assert_eq!(std::fs::read(image).unwrap(), files);
    assert!(engine.sent().is_empty());
    assert_eq!(row(&ui, cx, &owner).2.entity_id(), state.entity_id());
    assert_eq!(ui.with(cx, |window, cx| window.focused(cx)), focus);
    // Real GPUI undo, not set_value/replacing the input entity.
    ui.press(cx, "cmd-z");
    assert_eq!(ui.composer_text(cx), "keep my draft");
}

#[gpui_kit::test]
fn chat_code_stale_receipts_cannot_stage_or_copy_changed_removed_or_other_owner(
    cx: &mut TestAppContext,
) {
    let ui = open(cx);
    let engine = running_turn(&ui, cx);
    engine.send(
        cx,
        AgentEvent::TextDelta("```rust\noriginal 界🙂\n```".into()),
    );
    let first = owner(&ui, cx);
    let (ix, key, _) = row(&ui, cx, &first);
    // Receipt originates from the actual installed native hook, not a fabricated node.
    let receipt = ui.read(cx, |app, _| {
        app.chat_content.borrow().code_action(&key, 0).unwrap()
    });
    ui.press(cx, "cmd-n");
    ui.input(cx, "other draft");
    ui.with(cx, |window, cx| {
        ui.app
            .update(cx, |app, cx| app.stage_code(&receipt, window, cx))
    });
    assert_eq!(ui.composer_text(cx), "other draft");
    ui.click(cx, ("session", 0usize));
    ui.app.update(cx, |app, cx| {
        let change = app.sessions[app.active]
            .view
            .replace_assistant_text(ix, "```bash\nchanged code\n```".into());
        app.sessions[app.active].apply(change);
        cx.notify();
    });
    ui.with(cx, |window, cx| {
        ui.app.update(cx, |app, cx| {
            app.stage_code(&receipt, window, cx);
            cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string("unchanged".into()));
            app.copy_code(&receipt, cx);
        })
    });
    assert!(ui.composer_text(cx).is_empty());
    assert_eq!(
        cx.read_from_clipboard().and_then(|item| item.text()),
        Some("unchanged".into())
    );
    engine.send(
        cx,
        AgentEvent::TurnFinished {
            turn_id: 1,
            reason: TurnEndReason::Completed,
        },
    );
    engine.send(
        cx,
        AgentEvent::SessionStopped {
            history_saved: true,
        },
    );
    drop(engine);
    settle(cx);
    ui.with(cx, |window, cx| {
        ui.app.update(cx, |app, cx| {
            app.delete_session(app.active, window, cx);
            app.stage_code(&receipt, window, cx);
            app.copy_code(&receipt, cx);
        })
    });
    assert!(ui.composer_text(cx).is_empty());
    assert_eq!(
        cx.read_from_clipboard().and_then(|item| item.text()),
        Some("unchanged".into())
    );
}

#[gpui_kit::test]
fn chat_code_child_stage_is_visibly_disabled_and_never_changes_hidden_parent(
    cx: &mut TestAppContext,
) {
    let ui = open(cx);
    let engine = running_turn(&ui, cx);
    engine.send(
        cx,
        AgentEvent::ToolCallStarted {
            call_id: "delegate".into(),
            name: "spawn_agent".into(),
            kind: ToolKind::Other,
            summary: "Child".into(),
            args: json!({"message": "Inspect"}),
        },
    );
    engine.send(
        cx,
        AgentEvent::SubagentStarted {
            call_id: "delegate".into(),
            session_id: "code-child".into(),
            model: "fixture".into(),
        },
    );
    for event in [
        AgentEvent::TurnStarted { turn_id: 1 },
        AgentEvent::TextDelta("```unknown\nchild 界🙂\n```".into()),
    ] {
        engine.send(
            cx,
            AgentEvent::SubagentEvent {
                call_id: "delegate".into(),
                event: Box::new(event),
            },
        );
    }
    ui.input(cx, "parent stays untouched");
    let owner = owner(&ui, cx);
    ui.click(
        cx,
        ElementId::Name(format!("subagent-{}-code-child", owner.parent).into()),
    );
    let child = Conversation {
        parent: owner.parent,
        child: Some("code-child".into()),
    };
    let (_, key, _) = row(&ui, cx, &child);
    ui.with(cx, |window, _| {
        assert_eq!(
            window.find(action(&key, "stage", 0)).label(),
            Some("Stage unavailable: child is read-only")
        );
        assert_eq!(
            window.find(action(&key, "notice", 0)).label(),
            Some("Child read-only: Stage disabled")
        );
    });
    ui.click(cx, action(&key, "stage", 0));
    assert_eq!(ui.composer_text(cx), "parent stays untouched");
    ui.click(cx, action(&key, "copy", 0));
    assert_eq!(
        cx.read_from_clipboard().and_then(|item| item.text()),
        Some("child 界🙂".into())
    );
    assert!(engine.sent().is_empty());
    for choice in [1, 2, 0] {
        choose_width(&ui, cx, choice);
        let (ix, _, _) = row(&ui, cx, &child);
        let available = ui.with(cx, |window, _| {
            window
                .find(ElementId::Name(
                    format!("subagent-view-{}-code-child", child.parent).into(),
                ))
                .bounds()
                .size
                .width
        }) - px(40.);
        let maximum = ui
            .read(cx, |app, _| app.settings.transcript_width.maximum())
            .map(px)
            .unwrap_or(available);
        assert_eq!(
            column(&ui, cx, &child, ix).size.width,
            maximum.min(available)
        );
    }
    assert_eq!(ui.composer_text(cx), "parent stays untouched");
}

#[gpui_kit::test]
fn chat_code_actual_320_420_tiled_panes_keep_headers_scroll_and_composer_in_bounds(
    cx: &mut TestAppContext,
) {
    let (ui, sessions) = session_pane_fixture(cx, 2);
    drag_session_pane(
        &ui,
        cx,
        sessions[1].0,
        sessions[0].0,
        flint_app::docking::Edge::Right,
    );
    cx.simulate_window_resize(ui.window, size(px(1800.), px(950.)));
    let code = format!("界🙂 {}\nlast", "very_long_identifier_".repeat(80));
    for (_, engine) in &sessions {
        engine.send(
            cx,
            AgentEvent::TextDelta(format!("```unknown\n{code}\n```")),
        );
    }
    paint(&ui, cx);
    let left = sessions[0].0;
    ui.click(
        cx,
        ElementId::Name(format!("session-pane-compose-{left}").into()),
    );
    let target = Conversation::parent(left);
    let (ix, key, state) = row(&ui, cx, &target);
    for width in [320., 420.] {
        ui.with(cx, |window, cx| {
            let left_bounds = window
                .find(ElementId::Name(format!("session-pane-{left}").into()))
                .bounds();
            let right_bounds = window
                .find(ElementId::Name(
                    format!("session-pane-{}", sessions[1].0).into(),
                ))
                .bounds();
            window.drag(
                point(
                    (left_bounds.right() + right_bounds.left()) / 2.,
                    left_bounds.center().y,
                ),
                point(left_bounds.left() + px(width), left_bounds.center().y),
                cx,
            );
        });
        paint(&ui, cx);
        let pane = session_pane_bounds(&ui, cx, left);
        assert!((pane.size.width - px(width)).abs() <= px(6.), "{pane:?}");
        let before = ui.with(cx, |window, _| {
            let header = window.find(action(&key, "header", 0)).bounds();
            for id in [
                action(&key, "header", 0),
                action(&key, "copy", 0),
                action(&key, "stage", 0),
                ElementId::Name("chat-composer-column".into()),
                ElementId::Name("composer-toolbar".into()),
                ElementId::Name("send-button".into()),
            ] {
                let bounds = window.find(id).bounds();
                assert!(
                    bounds.left() >= pane.left() && bounds.right() <= pane.right(),
                    "{bounds:?} outside {pane:?}"
                );
            }
            header
        });
        assert_eq!(
            column(&ui, cx, &target, ix).size.width,
            ui.with(cx, |window, _| window
                .find("chat-composer-column")
                .bounds()
                .size
                .width)
        );
        ui.with(cx, |window, cx| {
            window.scroll(
                action(&key, "header", 0),
                ScrollDelta::Pixels(point(px(-600.), px(0.))),
                cx,
            )
        });
        paint(&ui, cx);
        let receipt = ui.read(cx, |app, _| {
            app.chat_content.borrow().code_action(&key, 0).unwrap()
        });
        assert!(
            receipt.horizontal_offset() < 0.,
            "actual native code must scroll horizontally"
        );
        assert_eq!(
            ui.with(cx, |window, _| window
                .find(action(&key, "header", 0))
                .bounds()),
            before
        );
        ui.click(cx, action(&key, "copy", 0));
        assert_eq!(
            cx.read_from_clipboard().and_then(|item| item.text()),
            Some(code.clone())
        );
        ui.click(cx, action(&key, "stage", 0));
        assert!(ui.composer_text(cx).contains(&code));
        assert_eq!(row(&ui, cx, &target).2.entity_id(), state.entity_id());
        assert!(sessions.iter().all(|(_, engine)| engine.sent().is_empty()));
    }
}
