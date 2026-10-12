use super::*;

use flint_app::chat_content::{
    FALLBACK_SOURCE_BYTES, MAX_RICH_SOURCE_BYTES, MAX_UNPINNED_ENTRIES, MAX_UNPINNED_SOURCE_BYTES,
    RowKey,
};
use flint_app::subagent_ui::Conversation;
use gpui_kit::component::text::TextViewState;

fn paint(ui: &Ui, cx: &mut TestAppContext) {
    for _ in 0..3 {
        ui.with(cx, |window, cx| window.render_frame(cx));
        settle(cx);
    }
}

fn target(ui: &Ui, cx: &mut TestAppContext) -> Conversation {
    ui.read(cx, |app, _| Conversation::parent(app.session().uid))
}

fn key(ui: &Ui, cx: &mut TestAppContext, target: &Conversation, row: usize) -> RowKey {
    ui.read(cx, |app, _| {
        RowKey::new(
            target,
            app.conversation_view(target)
                .unwrap()
                .row_identity(row)
                .unwrap(),
        )
    })
}

fn text_state(ui: &Ui, cx: &mut TestAppContext, key: &RowKey) -> Entity<TextViewState> {
    paint(ui, cx);
    ui.read(cx, |app, _| {
        app.chat_content
            .borrow()
            .state(key)
            .expect("actual transcript retained text")
    })
}

fn rendered(state: &Entity<TextViewState>, cx: &TestAppContext) -> String {
    cx.read(|cx| state.read(cx).rendered_text().as_str().to_string())
}

fn finish(engine: &Engine, cx: &mut TestAppContext, turn_id: u64) {
    engine.send(
        cx,
        AgentEvent::TurnFinished {
            turn_id,
            reason: TurnEndReason::Completed,
        },
    );
}

fn click_link(ui: &Ui, cx: &mut TestAppContext, row: usize) {
    ui.with(cx, |window, cx| {
        window.click_at(("assistant-text", row), point(px(8.), px(12.)), cx);
    });
    settle(cx);
}

#[gpui_kit::test]
fn retained_markdown_streams_unicode_partial_fences_tables_and_promotes_same_entity(
    cx: &mut TestAppContext,
) {
    let ui = open(cx);
    let engine = running_turn(&ui, cx);
    engine.send(cx, AgentEvent::TextDelta("Hello **界".into()));
    let owner = target(&ui, cx);
    let row = ui.read(cx, |app, _| app.session().view.items.len() - 1);
    let row_key = key(&ui, cx, &owner, row);
    let state = text_state(&ui, cx, &row_key);
    for delta in [
        "🙂**\n\n```rust\nlet ",
        "x = 1;\n``",
        "`\n\n| Name | Value |\n",
        "| --- | --- |\n| 👩",
        "\u{200d}💻 | 42 |\n",
    ] {
        engine.send(cx, AgentEvent::TextDelta(delta.into()));
        paint(&ui, cx);
        assert_eq!(text_state(&ui, cx, &row_key).entity_id(), state.entity_id());
    }
    let text = rendered(&state, cx);
    assert!(text.contains("Hello 界🙂"), "{text:?}");
    assert!(text.contains("let x = 1;"), "{text:?}");
    assert!(text.contains("👩\u{200d}💻"), "{text:?}");
    assert!(!text.contains('\u{fffd}'));
    let counts = ui.read(cx, |app, _| {
        app.chat_content.borrow().update_counts(&row_key).unwrap()
    });
    assert_eq!(counts, (5, 0));
    finish(&engine, cx, 1);
    paint(&ui, cx);
    assert!(has(
        &ui,
        cx,
        &format!("chat-content-{}-root-{}", owner.parent, row_key.row)
    ));
    assert_eq!(text_state(&ui, cx, &row_key).entity_id(), state.entity_id());
    assert_eq!(rendered(&state, cx), text);
    assert_eq!(
        ui.read(cx, |app, _| app
            .chat_content
            .borrow()
            .update_counts(&row_key)),
        Some(counts)
    );
    assert_ne!(
        ui.read(cx, |app, _| app.session().view.outcome()),
        flint_agent::TaskOutcome::Accepted
    );
}

#[gpui_kit::test]
fn retained_markdown_real_selection_copy_survives_append_and_terminal_promotion(
    cx: &mut TestAppContext,
) {
    let ui = open(cx);
    let engine = running_turn(&ui, cx);
    engine.send(cx, AgentEvent::TextDelta("alpha bravo".into()));
    let owner = target(&ui, cx);
    let row = ui.read(cx, |app, _| app.session().view.items.len() - 1);
    let row_key = key(&ui, cx, &owner, row);
    let state = text_state(&ui, cx, &row_key);
    ui.with(cx, |window, cx| {
        let bounds = window.find(row_key.element_id("chat-content")).bounds();
        window.drag(
            point(bounds.left() + px(1.), bounds.top() + px(12.)),
            point(bounds.left() + px(42.), bounds.top() + px(12.)),
            cx,
        );
        window.render_frame(cx);
    });
    let selected = cx.read(|cx| state.read(cx).selected_text());
    assert!(
        !selected.is_empty(),
        "pointer selection must target actual rendered text"
    );
    let source_range = cx
        .read(|cx| state.read(cx).selected_source_range())
        .unwrap();
    engine.send(cx, AgentEvent::TextDelta(" charlie".into()));
    paint(&ui, cx);
    assert_eq!(cx.read(|cx| state.read(cx).selected_text()), selected);
    assert_eq!(
        cx.read(|cx| state.read(cx).selected_source_range()),
        Some(source_range.clone())
    );
    finish(&engine, cx, 1);
    paint(&ui, cx);
    assert_eq!(cx.read(|cx| state.read(cx).selected_text()), selected);
    assert_eq!(
        cx.read(|cx| state.read(cx).selected_source_range()),
        Some(source_range)
    );
    ui.press(cx, "cmd-c");
    // GPUI's standard TextView Copy trims its block-ending newline.
    assert_eq!(
        cx.read_from_clipboard().and_then(|item| item.text()),
        Some(selected.trim().into())
    );
    assert_eq!(text_state(&ui, cx, &row_key).entity_id(), state.entity_id());
}

#[gpui_kit::test]
fn retained_markdown_batch_delivery_appends_once_and_explicit_replacement_replaces_once(
    cx: &mut TestAppContext,
) {
    let ui = open(cx);
    let engine = running_turn(&ui, cx);
    engine.send(cx, AgentEvent::TextDelta("first".into()));
    let owner = target(&ui, cx);
    let row = ui.read(cx, |app, _| app.session().view.items.len() - 1);
    let row_key = key(&ui, cx, &owner, row);
    let state = text_state(&ui, cx, &row_key);
    for delta in [" **second", "界", "🙂**"] {
        engine
            .events
            .try_send(AgentEvent::TextDelta(delta.into()))
            .unwrap();
    }
    settle(cx);
    paint(&ui, cx);
    assert!(rendered(&state, cx).contains("first second界🙂"));
    assert_eq!(
        ui.read(cx, |app, _| app
            .chat_content
            .borrow()
            .update_counts(&row_key)),
        Some((1, 0))
    );
    ui.app.update(cx, |app, cx| {
        let change = app.sessions[app.active]
            .view
            .replace_assistant_text(row, "first replacement".into());
        app.sessions[app.active].apply(change);
        cx.notify();
    });
    paint(&ui, cx);
    assert_eq!(rendered(&state, cx).trim(), "first replacement");
    assert_eq!(text_state(&ui, cx, &row_key).entity_id(), state.entity_id());
    assert_eq!(
        ui.read(cx, |app, _| app
            .chat_content
            .borrow()
            .update_counts(&row_key)),
        Some((1, 1))
    );
    engine.send(cx, AgentEvent::TextDelta(" tail".into()));
    paint(&ui, cx);
    assert_eq!(rendered(&state, cx).trim(), "first replacement tail");
    assert_eq!(
        ui.read(cx, |app, _| app
            .chat_content
            .borrow()
            .update_counts(&row_key)),
        Some((2, 1))
    );
}

fn child(engine: &Engine, cx: &mut TestAppContext, event: AgentEvent) {
    engine.send(
        cx,
        AgentEvent::SubagentEvent {
            call_id: "delegate".into(),
            event: Box::new(event),
        },
    );
}

#[gpui_kit::test]
fn retained_markdown_child_and_parent_rows_never_share_entities_or_content(
    cx: &mut TestAppContext,
) {
    let ui = open(cx);
    let engine = running_turn(&ui, cx);
    let owner = target(&ui, cx);
    engine.send(
        cx,
        AgentEvent::ToolCallStarted {
            call_id: "delegate".into(),
            name: "spawn_agent".into(),
            kind: ToolKind::Other,
            summary: "Child".into(),
            args: json!({"message":"child request"}),
        },
    );
    engine.send(
        cx,
        AgentEvent::SubagentStarted {
            call_id: "delegate".into(),
            session_id: "parent".into(),
            model: "fixture".into(),
        },
    );
    child(&engine, cx, AgentEvent::TurnStarted { turn_id: 1 });
    child(&engine, cx, AgentEvent::TextDelta("child **界**".into()));
    engine.send(cx, AgentEvent::TextDelta("parent text".into()));
    let parent_row = ui.read(cx, |app, _| app.session().view.items.len() - 1);
    let parent_key = key(&ui, cx, &owner, parent_row);
    let parent_state = text_state(&ui, cx, &parent_key);
    ui.click(
        cx,
        ElementId::Name(format!("subagent-{}-parent", owner.parent).into()),
    );
    let child_target = Conversation {
        parent: owner.parent,
        child: Some("parent".into()),
    };
    let child_key = key(&ui, cx, &child_target, 1);
    let child_state = text_state(&ui, cx, &child_key);
    assert_ne!(parent_state.entity_id(), child_state.entity_id());
    assert!(rendered(&child_state, cx).contains("child 界"));
    child(&engine, cx, AgentEvent::TextDelta("🙂".into()));
    child(
        &engine,
        cx,
        AgentEvent::TurnFinished {
            turn_id: 1,
            reason: TurnEndReason::Completed,
        },
    );
    paint(&ui, cx);
    assert_eq!(
        text_state(&ui, cx, &child_key).entity_id(),
        child_state.entity_id()
    );
    assert!(rendered(&child_state, cx).contains("child 界🙂"));
    assert_eq!(rendered(&parent_state, cx).trim(), "parent text");
    ui.click(
        cx,
        ElementId::Name(format!("subagent-back-{}", owner.parent).into()),
    );
    paint(&ui, cx);
    assert_eq!(
        text_state(&ui, cx, &parent_key).entity_id(),
        parent_state.entity_id()
    );
}

#[gpui_kit::test]
fn retained_markdown_replay_and_session_switches_keep_independent_content(cx: &mut TestAppContext) {
    let options = test_options();
    let ui = open_with(cx, options.clone());
    let engine = running_turn(&ui, cx);
    engine.send(cx, AgentEvent::TextDelta("saved **界🙂**".into()));
    finish(&engine, cx, 1);
    let owner = target(&ui, cx);
    let row = ui.read(cx, |app, _| {
        app.session().view.turns[0].final_answer.unwrap()
    });
    let saved_key = key(&ui, cx, &owner, row);
    let state = text_state(&ui, cx, &saved_key);
    ui.app.update(cx, |app, _| {
        app.sessions[app.active].flush_records().unwrap();
        write_native_counter_fixture(app.session().dir.as_ref().unwrap(), 1);
    });
    ui.press(cx, "cmd-n");
    let second = running_turn(&ui, cx);
    second.send(cx, AgentEvent::TextDelta("other conversation".into()));
    paint(&ui, cx);
    let other = target(&ui, cx);
    let other_row = ui.read(cx, |app, _| app.session().view.items.len() - 1);
    let other_key = key(&ui, cx, &other, other_row);
    assert_ne!(
        text_state(&ui, cx, &other_key).entity_id(),
        state.entity_id()
    );
    ui.with(cx, |window, cx| {
        ui.app
            .update(cx, |app, cx| app.select_session(0, window, cx))
    });
    assert_eq!(
        text_state(&ui, cx, &saved_key).entity_id(),
        state.entity_id()
    );
    let replay = open_with(cx, options);
    let replay_ix = replay.read(cx, |app, _| {
        app.sessions
            .iter()
            .position(|s| {
                s.view.items.iter().any(
                    |item| matches!(item, Item::Assistant { text, .. } if text == "saved **界🙂**"),
                )
            })
            .unwrap()
    });
    replay.with(cx, |window, cx| {
        replay
            .app
            .update(cx, |app, cx| app.select_session(replay_ix, window, cx))
    });
    let replay_target = target(&replay, cx);
    let replay_row = replay.read(cx, |app, _| {
        app.session().view.turns[0].final_answer.unwrap()
    });
    let replay_key = key(&replay, cx, &replay_target, replay_row);
    assert_eq!(replay_key.row, saved_key.row);
    assert_eq!(
        rendered(&text_state(&replay, cx, &replay_key), cx).trim(),
        "saved 界🙂"
    );
    assert!(replay.read(cx, |app, _| app.session().ops.is_none()));
}

#[gpui_kit::test]
fn retained_markdown_oversize_fallback_is_bounded_literal_and_copy_keeps_full_source(
    cx: &mut TestAppContext,
) {
    let ui = open(cx);
    let engine = running_turn(&ui, cx);
    engine.send(cx, AgentEvent::TextDelta("small **start**".into()));
    let owner = target(&ui, cx);
    let row = ui.read(cx, |app, _| app.session().view.items.len() - 1);
    let row_key = key(&ui, cx, &owner, row);
    let state = text_state(&ui, cx, &row_key);
    engine.send(
        cx,
        AgentEvent::TextDelta(format!(
            "\n![blocked](file:///tmp/no-read)\n{}",
            "界".repeat(MAX_RICH_SOURCE_BYTES / 3 + 1)
        )),
    );
    paint(&ui, cx);
    assert_eq!(text_state(&ui, cx, &row_key).entity_id(), state.entity_id());
    let preview = rendered(&state, cx);
    assert!(
        preview.contains("small **start**"),
        "oversize content must be literal"
    );
    assert!(preview.contains("![blocked](file:///tmp/no-read)"));
    assert!(preview.len() <= FALLBACK_SOURCE_BYTES + 1);
    assert!(!preview.contains('\u{fffd}'));
    cx.simulate_window_resize(ui.window, size(px(900.), px(560.)));
    paint(&ui, cx);
    ui.click(cx, row_key.element_id("chat-source-copy"));
    let full = ui.read(cx, |app, _| match &app.session().view.items[row] {
        Item::Assistant { text, .. } => text.clone(),
        _ => panic!("assistant"),
    });
    assert!(full.len() > MAX_RICH_SOURCE_BYTES);
    assert_eq!(
        cx.read_from_clipboard().and_then(|item| item.text()),
        Some(full)
    );
    engine.send(cx, AgentEvent::TextDelta(" last".into()));
    paint(&ui, cx);
    assert_eq!(rendered(&state, cx), preview);
    assert_eq!(
        ui.read(cx, |app, _| app
            .chat_content
            .borrow()
            .update_counts(&row_key)),
        Some((0, 1))
    );
    ui.app.update(cx, |app, cx| {
        let change = app.sessions[app.active]
            .view
            .replace_assistant_text(row, "after fallback **界🙂**".into());
        app.sessions[app.active].apply(change);
        cx.notify();
    });
    paint(&ui, cx);
    assert_eq!(rendered(&state, cx).trim(), "after fallback 界🙂");
    assert_eq!(text_state(&ui, cx, &row_key).entity_id(), state.entity_id());
    assert!(!has(
        &ui,
        cx,
        &format!("chat-source-copy-{}-root-{}", owner.parent, row_key.row)
    ));
    assert_eq!(
        ui.read(cx, |app, _| app
            .chat_content
            .borrow()
            .update_counts(&row_key)),
        Some((0, 2))
    );
}

#[gpui_kit::test]
fn retained_markdown_resource_policy_blocks_images_references_html_and_all_implicit_urls(
    cx: &mut TestAppContext,
) {
    use gpui_kit::http_client::{FakeHttpClient, Response};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let ui = open(cx);
    let requests = Arc::new(AtomicUsize::new(0));
    let count = requests.clone();
    cx.update(|cx| {
        cx.set_http_client(FakeHttpClient::create(move |_| {
            count.fetch_add(1, Ordering::SeqCst);
            async {
                Ok(Response::builder()
                    .status(404)
                    .body(Default::default())
                    .unwrap())
            }
        }))
    });
    let urls = [
        "https://invalid.example/no-network.png",
        "http://name:sentinel@invalid.example/credential.png",
        "file:///tmp/no-read.png",
        "/tmp/absolute-no-read.png",
        "relative-no-read.png",
        "data:image/svg+xml;base64,PHN2Zy8+",
        "javascript:alert(1)",
    ];
    let mut source = String::new();
    for url in urls {
        source.push_str(&format!(
            "![direct]({url})\n\n![ref][r{}]\n\n[r{}]: {url}\n\n",
            source.len(),
            source.len()
        ));
        source.push_str(&format!(
            "<div><img src=\"{url}\" onerror=\"alert(1)\"></div>\n\n"
        ));
        source.push_str(&format!("inline <img src='{url}'> end\n\n"));
    }
    source.push_str("<iframe src='https://invalid.example/frame'></iframe>\n\n<script>throw new Error('bad')</script>\n\n<a href='file:///tmp/no-read'>HTML link</a>\n\n");
    let engine = running_turn(&ui, cx);
    engine.send(cx, AgentEvent::TextDelta(source.clone()));
    finish(&engine, cx, 1);
    let owner = target(&ui, cx);
    let row = ui.read(cx, |app, _| {
        app.session().view.turns[0].final_answer.unwrap()
    });
    let row_key = key(&ui, cx, &owner, row);
    let state = text_state(&ui, cx, &row_key);
    let text = rendered(&state, cx);
    assert!(text.contains("[Image blocked: direct]"), "{text}");
    assert!(text.contains("[Image blocked: ref]"), "{text}");
    assert!(
        text.contains("[Embedded HTML blocked; original source retained]"),
        "{text}"
    );
    assert_eq!(requests.load(Ordering::SeqCst), 0);
    assert_eq!(cx.opened_url(), None);
    assert!(ui.read(cx, |app, _| app.file_preview.is_none()));
    ui.read(cx, |_, cx| {
        for url in urls {
            assert!(
                !gpui_kit::ImageSource::Resource(gpui_kit::Resource::Uri(url.into()))
                    .is_asset_cached(cx),
                "{url} must not reach the resource loader"
            );
            if !url.contains(':') {
                assert!(
                    !gpui_kit::ImageSource::Resource(gpui_kit::Resource::Embedded(url.into()))
                        .is_asset_cached(cx)
                );
            }
        }
    });
    assert_eq!(
        ui.read(cx, |app, _| match &app.session().view.items[row] {
            Item::Assistant { text, .. } => text.clone(),
            _ => panic!("assistant"),
        }),
        source
    );
}

#[gpui_kit::test]
fn retained_markdown_unsafe_clicked_links_are_rejected_and_safe_links_still_work(
    cx: &mut TestAppContext,
) {
    let ui = open(cx);
    let engine = running_turn(&ui, cx);
    for (n, url) in [
        "https://name:sentinel@invalid.example/",
        "https://invalid.example/?api%5Fkey=sentinel",
        "javascript:alert(1)",
        "data:text/html,unsafe",
        "vscode://file/tmp/no",
    ]
    .into_iter()
    .enumerate()
    {
        if n > 0 {
            engine.send(
                cx,
                AgentEvent::StepStarted {
                    turn_id: 1,
                    step: n as u32,
                },
            );
        }
        engine.send(cx, AgentEvent::TextDelta(format!("[Unsafe]({url})")));
        let row = ui.read(cx, |app, _| app.session().view.items.len() - 1);
        paint(&ui, cx);
        click_link(&ui, cx, row);
        assert_eq!(cx.opened_url(), None, "{url}");
        assert!(ui.read(cx, |app, _| app.file_preview.is_none()));
    }
    engine.send(
        cx,
        AgentEvent::StepStarted {
            turn_id: 1,
            step: 10,
        },
    );
    engine.send(
        cx,
        AgentEvent::TextDelta("[Safe](https://example.com/docs)".into()),
    );
    let row = ui.read(cx, |app, _| app.session().view.items.len() - 1);
    paint(&ui, cx);
    click_link(&ui, cx, row);
    assert_eq!(cx.opened_url().as_deref(), Some("https://example.com/docs"));
}

#[gpui_kit::test]
fn retained_markdown_offscreen_rows_are_lazy_and_tail_pause_jump_remains_native(
    cx: &mut TestAppContext,
) {
    let ui = open(cx);
    let engine = running_turn(&ui, cx);
    let owner = target(&ui, cx);
    for n in 0..80 {
        engine
            .events
            .try_send(AgentEvent::StepStarted {
                turn_id: 1,
                step: n + 1,
            })
            .unwrap();
        engine
            .events
            .try_send(AgentEvent::TextDelta(format!(
                "row {n}\n\n{}",
                "paragraph\n\n".repeat(5)
            )))
            .unwrap();
    }
    settle(cx);
    paint(&ui, cx);
    let stats = ui.read(cx, |app, cx| app.chat_content.borrow().stats(cx));
    assert!(
        stats.entries < 80,
        "offscreen rows must not eagerly allocate text: {stats:?}"
    );
    let first_key = key(&ui, cx, &owner, 1);
    let list = ui.read(cx, |app, _| app.session().list.clone());
    list.pause_following_tail();
    list.scroll_to_reveal_item(1);
    ui.app.update(cx, |_, cx| cx.notify());
    let first = text_state(&ui, cx, &first_key);
    assert!(rendered(&first, cx).contains("row 0"));
    engine.send(cx, AgentEvent::TextDelta(" tail update".into()));
    assert!(!ui.read(cx, |app, _| app.session().list.is_following_tail()));
    ui.click(
        cx,
        ElementId::Name(format!("transcript-latest-{}", owner.parent).into()),
    );
    paint(&ui, cx);
    assert!(ui.read(cx, |app, _| app.session().list.is_following_tail()));
    list.pause_following_tail();
    list.scroll_to_reveal_item(1);
    ui.app.update(cx, |_, cx| cx.notify());
    assert_eq!(
        text_state(&ui, cx, &first_key).entity_id(),
        first.entity_id()
    );
}

#[gpui_kit::test]
fn retained_markdown_cache_prunes_old_conversations_and_rehydrates_evicted_rows(
    cx: &mut TestAppContext,
) {
    let ui = open(cx);
    for n in 0..140 {
        if n > 0 {
            ui.press(cx, "cmd-n");
        }
        ui.app.update(cx, |app, cx| {
            let ix = app.active;
            for event in [
                AgentEvent::TurnStarted { turn_id: 1 },
                AgentEvent::TextDelta(format!("conversation {n}")),
                AgentEvent::TurnFinished {
                    turn_id: 1,
                    reason: TurnEndReason::Completed,
                },
            ] {
                app.apply_event(ix, event, cx);
            }
        });
        paint(&ui, cx);
        let stats = ui.read(cx, |app, cx| app.chat_content.borrow().stats(cx));
        assert!(stats.unpinned_entries <= MAX_UNPINNED_ENTRIES, "{stats:?}");
        assert!(
            stats.unpinned_source_bytes <= MAX_UNPINNED_SOURCE_BYTES,
            "{stats:?}"
        );
    }
    let first_target = ui.read(cx, |app, _| Conversation::parent(app.sessions[0].uid));
    let first_key = key(&ui, cx, &first_target, 0);
    assert!(ui.read(cx, |app, _| {
        app.chat_content.borrow().state(&first_key).is_none()
    }));
    ui.with(cx, |window, cx| {
        ui.app
            .update(cx, |app, cx| app.select_session(0, window, cx))
    });
    assert_eq!(
        rendered(&text_state(&ui, cx, &first_key), cx).trim(),
        "conversation 0"
    );
}

#[gpui_kit::test]
fn retained_markdown_byte_budget_counts_source_and_reports_selection_pins(cx: &mut TestAppContext) {
    let ui = open(cx);
    for n in 0..25 {
        if n > 0 {
            ui.press(cx, "cmd-n");
        }
        ui.app.update(cx, |app, cx| {
            let ix = app.active;
            for event in [
                AgentEvent::TurnStarted { turn_id: 1 },
                AgentEvent::TextDelta(format!("selected {n}\n\n<!-- {} -->", "x".repeat(400_000))),
                AgentEvent::TurnFinished {
                    turn_id: 1,
                    reason: TurnEndReason::Completed,
                },
            ] {
                app.apply_event(ix, event, cx);
            }
        });
        paint(&ui, cx);
    }
    let selected_target = target(&ui, cx);
    let selected_key = key(&ui, cx, &selected_target, 0);
    let state = text_state(&ui, cx, &selected_key);
    state.update(cx, |state, cx| state.select_all(cx));
    paint(&ui, cx);
    let stats = ui.read(cx, |app, cx| app.chat_content.borrow().stats(cx));
    assert!(
        stats.unpinned_source_bytes <= MAX_UNPINNED_SOURCE_BYTES,
        "{stats:?}"
    );
    assert!(
        stats.unpinned_entries < MAX_UNPINNED_ENTRIES,
        "byte pruning must trigger before count: {stats:?}"
    );
    assert!(stats.selection_pins >= 1, "{stats:?}");
    assert!(stats.pinned_source_bytes >= 400_000, "{stats:?}");
    assert!(ui.read(cx, |app, _| {
        app.chat_content.borrow().state(&selected_key).is_some()
    }));
}

#[gpui_kit::test]
fn retained_markdown_narrow_tiled_parent_and_child_use_real_pane_content(cx: &mut TestAppContext) {
    let (ui, sessions) = session_pane_fixture(cx, 2);
    drag_session_pane(
        &ui,
        cx,
        sessions[1].0,
        sessions[0].0,
        flint_app::docking::Edge::Right,
    );
    cx.simulate_window_resize(ui.window, size(px(900.), px(560.)));
    for (_, engine) in &sessions {
        engine.send(
            cx,
            AgentEvent::TextDelta("Narrow **界🙂**\n\n```rust\nlet long_name = 123;\n```".into()),
        );
    }
    paint(&ui, cx);
    for (uid, _) in &sessions {
        let conversation = Conversation::parent(*uid);
        let row = ui.read(cx, |app, _| {
            app.conversation_view(&conversation).unwrap().items.len() - 1
        });
        let row_key = key(&ui, cx, &conversation, row);
        let state = text_state(&ui, cx, &row_key);
        assert!(rendered(&state, cx).contains("Narrow 界🙂"));
        ui.with(cx, |window, cx| {
            window.render_frame(cx);
            let pane = window
                .find(ElementId::Name(format!("session-pane-{uid}").into()))
                .bounds();
            let content = window.find(row_key.element_id("chat-content")).bounds();
            assert!(pane.size.width < px(480.));
            assert!(content.left() >= pane.left());
            assert!(content.right() <= pane.right() + px(1.));
        });
    }
    let uid = ui.read(cx, |app, _| app.session().uid);
    let engine = &sessions
        .iter()
        .find(|(parent, _)| *parent == uid)
        .unwrap()
        .1;
    engine.send(
        cx,
        AgentEvent::ToolCallStarted {
            call_id: "delegate".into(),
            name: "spawn_agent".into(),
            kind: ToolKind::Other,
            summary: "Narrow child".into(),
            args: json!({"message":"Inspect narrow view"}),
        },
    );
    engine.send(
        cx,
        AgentEvent::SubagentStarted {
            call_id: "delegate".into(),
            session_id: "narrow-child".into(),
            model: "fixture".into(),
        },
    );
    child(engine, cx, AgentEvent::TurnStarted { turn_id: 1 });
    child(
        engine,
        cx,
        AgentEvent::TextDelta("Child **界🙂** in a narrow pane".into()),
    );
    ui.click(cx, "session-workspace-sidebar");
    ui.click(
        cx,
        ElementId::Name(format!("subagent-{uid}-narrow-child").into()),
    );
    assert_eq!(ui.read(cx, |app, _| app.session().uid), uid);
    assert_eq!(
        ui.read(cx, |app, _| app.session().selected_subagent.clone())
            .as_deref(),
        Some("narrow-child"),
    );
    let child_target = Conversation {
        parent: uid,
        child: Some("narrow-child".into()),
    };
    let child_key = key(&ui, cx, &child_target, 1);
    let child_state = text_state(&ui, cx, &child_key);
    assert!(rendered(&child_state, cx).contains("Child 界🙂"));
    ui.with(cx, |window, cx| {
        window.render_frame(cx);
        let pane = window
            .find(ElementId::Name(format!("session-pane-{uid}").into()))
            .bounds();
        let content = window.find(child_key.element_id("chat-content")).bounds();
        assert!(content.left() >= pane.left());
        assert!(content.right() <= pane.right() + px(1.));
    });
}

#[gpui_kit::test]
fn retained_markdown_image_source_guard_blocks_native_html_and_data_decoding(
    cx: &mut TestAppContext,
) {
    use gpui_kit::http_client::{FakeHttpClient, Response};
    use gpui_kit::{ParentElement as _, Styled as _};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    struct GuardedHtml;
    impl gpui_kit::Render for GuardedHtml {
        fn render(
            &mut self,
            _: &mut Window,
            _: &mut gpui_kit::Context<Self>,
        ) -> impl gpui_kit::IntoElement {
            gpui_kit::div().size_full().child(
                flint_app::reply_resources::chat_policy().apply(
                    gpui_kit::base::TextView::html(
                        "guarded-html",
                        "<img src='https://invalid.example/native.png'><img src='file:///tmp/no-read'><img src='data:image/png;base64,NOT_VALID'>",
                    ),
                ),
            )
        }
    }
    cx.update(flint_app::init);
    let requests = Arc::new(AtomicUsize::new(0));
    let count = requests.clone();
    cx.update(|cx| {
        cx.set_http_client(FakeHttpClient::create(move |_| {
            count.fetch_add(1, Ordering::SeqCst);
            async {
                Ok(Response::builder()
                    .status(404)
                    .body(Default::default())
                    .unwrap())
            }
        }))
    });
    let before = flint_app::reply_resources::chat_policy().image_override_count();
    let (window, _) = cx.update(|cx| {
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds {
                    origin: Point::default(),
                    size: size(px(400.), px(300.)),
                })),
                ..Default::default()
            },
            cx,
            |_, cx| cx.new(|_| GuardedHtml),
        )
        .unwrap()
    });
    for _ in 0..3 {
        cx.update_window(window, |_, window, cx| window.render_frame(cx))
            .unwrap();
        settle(cx);
    }
    assert!(flint_app::reply_resources::chat_policy().image_override_count() > before);
    assert_eq!(requests.load(Ordering::SeqCst), 0);
    cx.read(|cx| {
        for url in [
            "https://invalid.example/native.png",
            "file:///tmp/no-read",
            "data:image/png;base64,NOT_VALID",
        ] {
            assert!(
                !gpui_kit::ImageSource::Resource(gpui_kit::Resource::Uri(url.into()))
                    .is_asset_cached(cx)
            );
        }
    });
}

fn select_chat_content(ui: &Ui, cx: &mut TestAppContext, key: &RowKey) {
    ui.with(cx, |window, cx| {
        window.render_frame(cx);
        let bounds = window.find(key.element_id("chat-content")).bounds();
        window.drag(
            point(bounds.left() + px(1.), bounds.top() + px(12.)),
            point(bounds.left() + px(42.), bounds.top() + px(12.)),
            cx,
        );
    });
    paint(ui, cx);
    assert!(ui.read(cx, |app, cx| {
        app.chat_content
            .borrow()
            .state(key)
            .unwrap()
            .read(cx)
            .selected_source_range()
            .is_some()
    }));
}

struct SelectedChatOwner {
    target: Conversation,
    rows: Vec<(RowKey, gpui_kit::WeakEntity<TextViewState>)>,
    canonical: flint_app::view_model::SessionView,
    dir: std::path::PathBuf,
    records: Vec<u8>,
}

fn selected_chat_owner(ui: &Ui, cx: &mut TestAppContext, label: &str) -> SelectedChatOwner {
    let engine = running_turn(ui, cx);
    let owner = target(ui, cx);
    for id in ["first", "second"] {
        engine.send(
            cx,
            AgentEvent::ToolCallStarted {
                call_id: id.into(),
                name: "spawn_agent".into(),
                kind: ToolKind::Other,
                summary: "Archive fixture child".into(),
                args: json!({"message": "Inspect the archive fixture."}),
            },
        );
        engine.send(
            cx,
            AgentEvent::SubagentStarted {
                call_id: id.into(),
                session_id: id.into(),
                model: "fixture".into(),
            },
        );
        for event in [
            AgentEvent::TurnStarted { turn_id: 1 },
            AgentEvent::TextDelta(format!("{label} {id} child **界🙂**")),
            AgentEvent::TurnFinished {
                turn_id: 1,
                reason: TurnEndReason::Completed,
            },
        ] {
            engine.send(
                cx,
                AgentEvent::SubagentEvent {
                    call_id: id.into(),
                    event: Box::new(event),
                },
            );
        }
        engine.send(cx, tool_finished(id, "done", None));
    }
    engine.send(cx, AgentEvent::TextDelta(format!("{label} root **界🙂**")));
    finish(&engine, cx, 1);
    engine.send(
        cx,
        AgentEvent::SessionStopped {
            history_saved: true,
        },
    );
    drop(engine);
    settle(cx);
    assert!(ui.read(cx, |app, _| app.session().ops.is_none()));
    let row = ui.read(cx, |app, _| {
        app.session().view.turns[0].final_answer.unwrap()
    });
    let root_key = key(ui, cx, &owner, row);
    let root = text_state(ui, cx, &root_key);
    select_chat_content(ui, cx, &root_key);
    let mut rows = vec![(root_key, root.downgrade())];
    drop(root);
    for id in ["first", "second"] {
        ui.click(
            cx,
            ElementId::Name(format!("subagent-{}-{id}", owner.parent).into()),
        );
        let child = Conversation {
            parent: owner.parent,
            child: Some(id.into()),
        };
        let child_key = key(ui, cx, &child, 1);
        let state = text_state(ui, cx, &child_key);
        select_chat_content(ui, cx, &child_key);
        rows.push((child_key, state.downgrade()));
    }
    let (canonical, dir) = ui.app.update(cx, |app, _| {
        app.sessions[app.active].flush_records().unwrap();
        (
            app.session().view.clone(),
            app.session().dir.clone().unwrap(),
        )
    });
    write_native_counter_fixture(&dir, 1);
    let records = std::fs::read(dir.join("events.jsonl")).unwrap();
    SelectedChatOwner {
        target: owner,
        rows,
        canonical,
        dir,
        records,
    }
}

fn archive_owner(ui: &Ui, cx: &mut TestAppContext, owner: u64) {
    ui.with(cx, |window, cx| {
        ui.app.update(cx, |app, cx| {
            let ix = app.session_index(owner).unwrap();
            app.delete_session(ix, window, cx);
        });
    });
    paint(ui, cx);
}

fn show_chat_row(ui: &Ui, cx: &mut TestAppContext, row: &RowKey) {
    match &row.child {
        Some(id) => ui.click(
            cx,
            ElementId::Name(format!("subagent-{}-{id}", row.parent).into()),
        ),
        None if ui.read(cx, |app, _| app.session().selected_subagent.is_some()) => ui.click(
            cx,
            ElementId::Name(format!("subagent-back-{}", row.parent).into()),
        ),
        None => {}
    }
    assert_eq!(
        ui.read(cx, |app, _| Conversation {
            parent: app.session().uid,
            child: app.session().selected_subagent.clone(),
        }),
        row.target()
    );
}

#[gpui_kit::test]
fn retained_markdown_archive_retires_selected_root_children_and_undo_rehydrates(
    cx: &mut TestAppContext,
) {
    let options = test_options();
    let ui = open_with(cx, options.clone());
    let first = selected_chat_owner(&ui, cx, "First owner");
    ui.press(cx, "cmd-n");
    let second = selected_chat_owner(&ui, cx, "Second owner");
    ui.click(cx, ("session", 0usize));
    select_chat_content(&ui, cx, &first.rows[0].0);
    assert_eq!(
        ui.read(cx, |app, cx| app
            .chat_content
            .borrow()
            .stats(cx)
            .selection_pins),
        1
    );
    // Archive a selected root with two cached children. Its undo slot will
    // then be overwritten by an archive with a selected child.
    archive_owner(&ui, cx, first.target.parent);
    assert!(ui.read(cx, |app, _| {
        app.session_index(first.target.parent).is_none()
            && app.archived_session.as_ref().unwrap().0.view == first.canonical
    }));
    for (row, weak) in &first.rows {
        assert!(ui.read(cx, |app, _| app.chat_content.borrow().state(row).is_none()));
        assert!(
            weak.upgrade().is_none(),
            "archived selected entity must release"
        );
    }
    assert_eq!(
        ui.read(cx, |app, cx| app
            .chat_content
            .borrow()
            .stats(cx)
            .selection_pins),
        0
    );
    for (row, weak) in &second.rows {
        assert_eq!(text_state(&ui, cx, row).entity_id(), weak.entity_id());
    }
    assert_eq!(
        std::fs::read(home_archive(&options, &first.dir).join("events.jsonl")).unwrap(),
        first.records
    );
    for selected_row in [2, 0] {
        show_chat_row(&ui, cx, &second.rows[selected_row].0);
        select_chat_content(&ui, cx, &second.rows[selected_row].0);
        let retired_rows: Vec<_> = second
            .rows
            .iter()
            .map(|(row, _)| {
                let weak = ui.read(cx, |app, _| {
                    app.chat_content.borrow().state(row).unwrap().downgrade()
                });
                (row.clone(), weak)
            })
            .collect();
        assert_eq!(
            ui.read(cx, |app, cx| app
                .chat_content
                .borrow()
                .stats(cx)
                .selection_pins),
            1
        );
        archive_owner(&ui, cx, second.target.parent);
        assert!(ui.read(cx, |app, _| {
            app.session_index(second.target.parent).is_none()
                && app.archived_session.as_ref().unwrap().0.view == second.canonical
        }));
        assert_eq!(
            ui.read(cx, |app, cx| app.chat_content.borrow().stats(cx).entries),
            0,
            "no retired parent/child pin exceptions may remain"
        );
        for (_, weak) in first.rows.iter().chain(&retired_rows) {
            assert!(weak.upgrade().is_none());
        }
        assert_eq!(
            std::fs::read(home_archive(&options, &second.dir).join("events.jsonl")).unwrap(),
            second.records
        );
        ui.click(cx, "undo-archive");
        paint(&ui, cx);
        assert_eq!(
            ui.read(cx, |app, _| app.session().uid),
            second.target.parent
        );
        assert_eq!(
            ui.read(cx, |app, _| app.session().view.clone()),
            second.canonical
        );
        assert_eq!(
            std::fs::read(second.dir.join("events.jsonl")).unwrap(),
            second.records
        );
        for (row, old) in &retired_rows {
            show_chat_row(&ui, cx, row);
            let state = text_state(&ui, cx, row);
            assert_ne!(state.entity_id(), old.entity_id(), "undo must rehydrate");
            let expected = match &row.child {
                Some(id) => format!("Second owner {id} child 界🙂"),
                None => "Second owner root 界🙂".into(),
            };
            assert_eq!(rendered(&state, cx).trim(), expected);
            assert!(cx.read(|cx| state.read(cx).selected_source_range().is_none()));
        }
        assert_eq!(
            ui.read(cx, |app, cx| app
                .chat_content
                .borrow()
                .stats(cx)
                .selection_pins),
            0
        );
        // Select the restored root and both children again, then archive again.
        for (row, _) in &second.rows {
            show_chat_row(&ui, cx, row);
            select_chat_content(&ui, cx, row);
        }
    }
    assert_eq!(
        std::fs::read(home_archive(&options, &first.dir).join("events.jsonl")).unwrap(),
        first.records,
        "overwriting Undo must not alter the earlier archived history"
    );
}

fn assert_owner_content_retained(ui: &Ui, cx: &mut TestAppContext, owner: &SelectedChatOwner) {
    assert!(owner.dir.exists());
    assert!(ui.read(cx, |app, _| {
        app.session_index(owner.target.parent).is_some() && app.archived_session.is_none()
    }));
    for (row, old) in &owner.rows {
        let state = text_state(ui, cx, row);
        assert_eq!(
            state.entity_id(),
            old.entity_id(),
            "rejected archive must not retire"
        );
        let view = ui.read(cx, |app, _| {
            let view = app.conversation_view(&row.target()).unwrap();
            match &view.items[view.row_index(row.row).unwrap()] {
                Item::Assistant { text, .. } => text.replace("**", ""),
                _ => panic!("assistant"),
            }
        });
        assert_eq!(rendered(&state, cx).trim(), view);
    }
}

#[gpui_kit::test]
fn retained_markdown_rejected_archive_keeps_selection_streaming_and_history_content(
    cx: &mut TestAppContext,
) {
    let ui = open(cx);
    let owner = selected_chat_owner(&ui, cx, "Guarded owner");
    ui.app.update(cx, |app, cx| {
        app.apply_event(
            app.active,
            AgentEvent::SessionStopped {
                history_saved: false,
            },
            cx,
        );
    });
    archive_owner(&ui, cx, owner.target.parent);
    assert_owner_content_retained(&ui, cx, &owner);
    assert!(ui.read(cx, |app, cx| {
        app.store_error
            .as_deref()
            .unwrap()
            .contains("history was not saved")
            && app.chat_content.borrow().stats(cx).selection_pins == 1
    }));
    ui.app.update(cx, |app, cx| {
        app.apply_event(
            app.active,
            AgentEvent::SessionStopped {
                history_saved: true,
            },
            cx,
        );
    });
    let engine = ui.engine(cx);
    engine.send(cx, AgentEvent::TurnStarted { turn_id: 2 });
    engine.send(
        cx,
        AgentEvent::TextDelta("Streaming guarded **root**".into()),
    );
    ui.click(
        cx,
        ElementId::Name(format!("subagent-back-{}", owner.target.parent).into()),
    );
    let row = ui.read(cx, |app, _| app.session().view.items.len() - 1);
    let streaming_key = key(&ui, cx, &owner.target, row);
    let streaming_state = text_state(&ui, cx, &streaming_key);
    select_chat_content(&ui, cx, &streaming_key);
    archive_owner(&ui, cx, owner.target.parent);
    assert_eq!(
        ui.read(cx, |app, _| app.archive_confirm),
        Some(owner.target.parent)
    );
    assert!(engine.sent().is_empty());
    assert_owner_content_retained(&ui, cx, &owner);
    ui.click(cx, "archive-stop");
    assert!(matches!(engine.sent().as_slice(), [Op::Shutdown]));
    archive_owner(&ui, cx, owner.target.parent);
    assert_owner_content_retained(&ui, cx, &owner);
    assert_eq!(
        text_state(&ui, cx, &streaming_key).entity_id(),
        streaming_state.entity_id()
    );
    assert!(ui.read(cx, |app, cx| {
        app.store_error
            .as_deref()
            .unwrap()
            .contains("finish saving")
            && app.chat_content.borrow().stats(cx).streaming_pins >= 1
    }));
    finish(&engine, cx, 2);
    engine.send(
        cx,
        AgentEvent::SessionStopped {
            history_saved: false,
        },
    );
    drop(engine);
    settle(cx);
    archive_owner(&ui, cx, owner.target.parent);
    assert_owner_content_retained(&ui, cx, &owner);
    assert!(ui.read(cx, |app, _| {
        app.store_error
            .as_deref()
            .unwrap()
            .contains("history was not saved")
    }));
}

#[gpui_kit::test]
fn retained_markdown_failed_archive_storage_keeps_selected_root_and_children(
    cx: &mut TestAppContext,
) {
    let options = test_options();
    let ui = open_with(cx, options.clone());
    let owner = selected_chat_owner(&ui, cx, "Storage guarded owner");
    let meta = owner.dir.join("meta.json");
    let kept = owner.dir.join("kept-meta.json");
    std::fs::rename(&meta, &kept).unwrap();
    std::fs::create_dir(&meta).unwrap();
    archive_owner(&ui, cx, owner.target.parent);
    assert_owner_content_retained(&ui, cx, &owner);
    assert!(ui.read(cx, |app, cx| {
        app.store_error
            .as_deref()
            .unwrap()
            .contains("could not be saved")
            && app.chat_content.borrow().stats(cx).selection_pins == 1
    }));
    std::fs::remove_dir(&meta).unwrap();
    std::fs::rename(&kept, &meta).unwrap();
    let collision = home_archive(&options, &owner.dir);
    std::fs::create_dir_all(&collision).unwrap();
    std::fs::write(collision.join("sentinel"), "keep existing archive").unwrap();
    archive_owner(&ui, cx, owner.target.parent);
    assert_owner_content_retained(&ui, cx, &owner);
    assert!(ui.read(cx, |app, cx| {
        app.store_error
            .as_deref()
            .unwrap()
            .contains("Couldn't archive session")
            && app.chat_content.borrow().stats(cx).selection_pins == 1
    }));
    assert_eq!(
        std::fs::read_to_string(collision.join("sentinel")).unwrap(),
        "keep existing archive"
    );
    assert_eq!(
        std::fs::read(owner.dir.join("events.jsonl")).unwrap(),
        owner.records
    );
    assert_eq!(
        ui.read(cx, |app, _| app.session().view.clone()),
        owner.canonical
    );
}

#[gpui_kit::test]
fn retained_markdown_pending_review_archive_keeps_owner_entities(cx: &mut TestAppContext) {
    let ui = open(cx);
    let owner = selected_chat_owner(&ui, cx, "Review guarded owner");
    let mut task = flint_agent::TaskRecord::new(1, "Review the archive fixture.".into(), 0);
    task.assess(TurnEndReason::Completed, true);
    std::fs::write(
        owner.dir.join("history.json"),
        serde_json::to_vec(&json!({
            "version": 2, "turn_id": 1,
            "messages": [{"user": "Review the archive fixture."}], "task": task,
        }))
        .unwrap(),
    )
    .unwrap();
    ui.app.update(cx, |app, cx| {
        app.apply_event(
            app.active,
            AgentEvent::TaskAssessed {
                turn_id: 1,
                task: Box::new(task),
            },
            cx,
        );
    });
    ui.click(
        cx,
        ElementId::Name(format!("subagent-back-{}", owner.target.parent).into()),
    );
    select_chat_content(&ui, cx, &owner.rows[0].0);
    ui.click(cx, "task-details-current");
    ui.click(cx, "task-accept");
    ui.with(cx, |window, cx| {
        ui.app.update(cx, |app, cx| {
            // Archive races the real saved-history review before its background
            // writer is driven. The existing task-write guard stays authoritative.
            app.confirm_task_acceptance(cx);
            assert!(app.prompt_busy(app.active));
            app.delete_session(app.active, window, cx);
            assert!(app.session_index(owner.target.parent).is_some());
            assert!(app.archived_session.is_none());
            assert!(
                app.store_error
                    .as_deref()
                    .unwrap()
                    .contains("finish saving")
            );
            for (row, old) in &owner.rows {
                assert_eq!(
                    app.chat_content.borrow().state(row).unwrap().entity_id(),
                    old.entity_id()
                );
            }
        });
    });
    settle(cx);
    assert_owner_content_retained(&ui, cx, &owner);
}
