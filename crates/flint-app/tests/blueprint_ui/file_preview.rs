use super::*;
use flint_app::file_preview::PreviewContent;

fn answer_link(ui: &Ui, cx: &mut TestAppContext, text: &str) -> usize {
    let engine = ui.engine(cx);
    ui.input(cx, "show me the file");
    ui.press(cx, "enter");
    engine.send(cx, AgentEvent::TurnStarted { turn_id: 1 });
    engine.send(cx, AgentEvent::TextDelta(text.into()));
    engine.send(
        cx,
        AgentEvent::TurnFinished {
            turn_id: 1,
            reason: TurnEndReason::Completed,
        },
    );
    ui.read(cx, |app, _| {
        app.session().view.turns[0].final_answer.unwrap()
    })
}

fn click_link(ui: &Ui, cx: &mut TestAppContext, id: impl Into<ElementId>) {
    let id = id.into();
    ui.with(cx, |window, cx| {
        window.click_at(id, point(px(8.), px(12.)), cx);
    });
    settle(cx);
}

#[gpui_kit::test]
fn file_preview_markdown_links_open_in_the_right_dock(cx: &mut TestAppContext) {
    let ui = open(cx);
    let workspace = ui.read(cx, |app, _| app.session().workspace.clone());
    std::fs::create_dir(workspace.join("docs")).unwrap();
    std::fs::write(workspace.join("docs/guide.md"), "[Source](../main.rs)").unwrap();
    std::fs::write(workspace.join("main.rs"), "fn main() {}").unwrap();
    let ix = answer_link(&ui, cx, "[Guide](docs/guide.md)");
    click_link(&ui, cx, ("answer", ix));
    assert!(has(&ui, cx, "file-preview-content"));
    assert_eq!(
        ui.read(cx, |app, _| app.file_preview.as_ref().unwrap().path.clone()),
        workspace.join("docs/guide.md")
    );
    let chat = panel_bounds(&ui, cx, flint_app::docking::Panel::Chat);
    let preview = panel_bounds(&ui, cx, flint_app::docking::Panel::Changes);
    assert!(preview.left() >= chat.right() - px(2.));
    assert_eq!(cx.opened_url(), None);

    // Links inside a document resolve against that document's directory.
    click_link(&ui, cx, "file-preview-content");
    assert_eq!(
        ui.read(cx, |app, _| app.file_preview.as_ref().unwrap().path.clone()),
        workspace.join("main.rs")
    );
    assert!(ui.read(cx, |app, _| matches!(
        &app.file_preview.as_ref().unwrap().content,
        PreviewContent::Text(text) if text.contains("fn main() {}")
    )));
    ui.click(cx, "file-preview-close");
    assert!(ui.read(cx, |app, _| app.file_preview.is_none() && !app.changes_open));
}

#[gpui_kit::test]
fn file_preview_web_links_still_open_in_the_browser(cx: &mut TestAppContext) {
    let ui = open(cx);
    let ix = answer_link(&ui, cx, "[Docs](https://example.com/docs)");
    click_link(&ui, cx, ("answer", ix));
    assert_eq!(cx.opened_url().as_deref(), Some("https://example.com/docs"));
    assert!(ui.read(cx, |app, _| app.file_preview.is_none()));
}

#[gpui_kit::test]
fn file_preview_missing_files_show_an_in_app_error(cx: &mut TestAppContext) {
    let ui = open(cx);
    let ix = answer_link(&ui, cx, "[Missing](missing.md)");
    click_link(&ui, cx, ("answer", ix));
    assert!(has(&ui, cx, "file-preview-error"));
    assert_eq!(cx.opened_url(), None);
    ui.click(cx, "file-preview-changes");
    assert!(ui.read(cx, |app, _| app.file_preview.is_none() && app.changes_open));
}

#[gpui_kit::test]
fn file_preview_changes_action_previews_the_file_without_launching_an_editor(
    cx: &mut TestAppContext,
) {
    let ui = open(cx);
    let engine = ui.engine(cx);
    let workspace = ui.read(cx, |app, _| app.session().workspace.clone());
    std::fs::create_dir(workspace.join("src")).unwrap();
    std::fs::write(workspace.join("src/a.rs"), "fn a() -> u8 { 1 }").unwrap();
    finished_turn(&ui, cx, &engine);
    let end = ui.read(cx, |app, _| app.session().view.turns[0].end.unwrap());
    ui.click(cx, ("review-button", end));
    ui.click(cx, "open-file-preview");
    settle(cx);
    assert!(has(&ui, cx, "file-preview-content"));
    ui.click(cx, "file-preview-changes");
    assert!(has(&ui, cx, "open-file-preview"));
    assert!(ui.read(cx, |app, _| app.file_preview.is_none()
        && app.selected_change == Some(0)));
}

#[gpui_kit::test]
fn file_preview_latest_selection_wins_and_session_switch_clears_it(cx: &mut TestAppContext) {
    let ui = open(cx);
    let workspace = ui.read(cx, |app, _| app.session().workspace.clone());
    std::fs::write(workspace.join("first.md"), "# First").unwrap();
    std::fs::write(workspace.join("second.md"), "# Second").unwrap();
    ui.app.update(cx, |app, cx| {
        app.open_file_preview(workspace.join("first.md"), cx);
        app.open_file_preview(workspace.join("second.md"), cx);
    });
    settle(cx);
    assert!(ui.read(cx, |app, _| matches!(
        &app.file_preview.as_ref().unwrap().content,
        PreviewContent::Text(text) if text.as_ref() == "# Second"
    )));
    // Force a new session rather than reusing an untouched one.
    let _engine = ui.engine(cx);
    ui.input(cx, "first session");
    ui.press(cx, "enter");
    ui.press(cx, "cmd-n");
    settle(cx);
    assert!(ui.read(cx, |app, _| app.file_preview.is_none()));
}

#[gpui_kit::test]
fn file_preview_streaming_links_are_handled_before_the_turn_finishes(cx: &mut TestAppContext) {
    let ui = open(cx);
    let workspace = ui.read(cx, |app, _| app.session().workspace.clone());
    std::fs::write(workspace.join("guide.md"), "# Live guide").unwrap();
    let engine = ui.engine(cx);
    ui.input(cx, "show the guide");
    ui.press(cx, "enter");
    engine.send(cx, AgentEvent::TurnStarted { turn_id: 1 });
    engine.send(cx, AgentEvent::TextDelta("[Guide](guide.md)".into()));
    let ix = ui.read(cx, |app, _| app.session().view.items.len() - 1);
    click_link(&ui, cx, ("assistant-text", ix));
    assert!(has(&ui, cx, "file-preview-content"));
    assert!(ui.read(cx, |app, _| app.session().view.running));
    assert_eq!(cx.opened_url(), None);
}

#[gpui_kit::test]
fn file_preview_terminal_command_click_opens_file_urls_in_app(cx: &mut TestAppContext) {
    use gpui_kit::{InputEvent as _, MouseButton, MouseDownEvent, MouseUpEvent};
    let ui = open(cx);
    let workspace = ui.read(cx, |app, _| app.session().workspace.clone());
    let path = workspace.join("guide.md");
    std::fs::write(&path, "# Terminal guide").unwrap();
    let url = reqwest::Url::from_file_path(&path).unwrap().to_string();
    let engine = ui.engine(cx);
    engine.send(
        cx,
        AgentEvent::TerminalStarted {
            terminal_id: "preview".into(),
            call_id: None,
            label: "Files".into(),
            cwd: None,
        },
    );
    engine.send(
        cx,
        AgentEvent::TerminalOutput {
            terminal_id: "preview".into(),
            data: url,
            replace: true,
        },
    );
    // Showing a mirror does not start a shell or source a login profile.
    ui.press(cx, "ctrl-`");
    ui.with(cx, |window, cx| {
        window.render_frame(cx);
        let position = window.find("terminal-panel").bounds().origin + point(px(14.), px(48.));
        let modifiers = gpui_kit::Modifiers {
            platform: true,
            ..Default::default()
        };
        window.dispatch_event(
            MouseDownEvent {
                position,
                button: MouseButton::Left,
                modifiers,
                click_count: 1,
                first_mouse: false,
            }
            .to_platform_input(),
            cx,
        );
        window.dispatch_event(
            MouseUpEvent {
                position,
                button: MouseButton::Left,
                modifiers,
                click_count: 1,
            }
            .to_platform_input(),
            cx,
        );
    });
    settle(cx);
    assert!(has(&ui, cx, "file-preview-content"));
    assert_eq!(
        ui.read(cx, |app, _| app.file_preview.as_ref().unwrap().path.clone()),
        path
    );
    assert_eq!(cx.opened_url(), None);
}

#[gpui_kit::test]
fn terminal_command_click_uses_unicode_cell_columns_for_file_links(cx: &mut TestAppContext) {
    use gpui_kit::{InputEvent as _, MouseButton, MouseDownEvent, MouseUpEvent};
    for dimensions in [(1440., 900.), (900., 560.)] {
        for (prefix, start) in [("界 ", 3), ("e\u{301} ", 2)] {
            let workspace = tempfile::Builder::new()
                .prefix("flint-link-")
                .tempdir_in("/tmp")
                .unwrap();
            let path = workspace.path().join("g.md");
            std::fs::write(&path, "# Unicode terminal link").unwrap();
            let url = reqwest::Url::from_file_path(&path).unwrap().to_string();
            let ui = open_with(
                cx,
                Options {
                    workspace: Some(workspace.path().to_path_buf()),
                    window_size: Some(dimensions),
                    ..test_options()
                },
            );
            let engine = ui.engine(cx);
            engine.send(
                cx,
                AgentEvent::TerminalStarted {
                    terminal_id: "unicode-link".into(),
                    call_id: None,
                    label: "Files".into(),
                    cwd: None,
                },
            );
            engine.send(
                cx,
                AgentEvent::TerminalOutput {
                    terminal_id: "unicode-link".into(),
                    data: format!("{prefix}\x1b[31m{url}\x1b[0m suffix"),
                    replace: true,
                },
            );
            ui.press(cx, "ctrl-`");
            let click_column = |ui: &Ui, cx: &mut TestAppContext, col: usize| {
                ui.with(cx, |window, cx| {
                    window.render_frame(cx);
                    let metrics = flint_app::term_paint::Metrics::measure(window);
                    let position = window.find("terminal-panel").bounds().origin
                        + point(px(10.) + metrics.cell.width * (col as f32 + 0.5), px(48.));
                    let modifiers = gpui_kit::Modifiers {
                        platform: true,
                        ..Default::default()
                    };
                    window.dispatch_event(
                        MouseDownEvent {
                            position,
                            button: MouseButton::Left,
                            modifiers,
                            click_count: 1,
                            first_mouse: false,
                        }
                        .to_platform_input(),
                        cx,
                    );
                    window.dispatch_event(
                        MouseUpEvent {
                            position,
                            button: MouseButton::Left,
                            modifiers,
                            click_count: 1,
                        }
                        .to_platform_input(),
                        cx,
                    );
                });
                settle(cx);
            };
            let snapshot = ui.read(cx, |app, cx| {
                app.terminal
                    .active_view()
                    .unwrap()
                    .read(cx)
                    .terminal
                    .snapshot()
            });
            assert!(
                start + url.len() < snapshot.cols,
                "url must fit the viewport"
            );
            assert_eq!(snapshot.text_lines()[0], format!("{prefix}{url} suffix"));
            click_column(&ui, cx, start - 1);
            assert!(!has(&ui, cx, "file-preview-content"), "prefix={prefix:?}");
            click_column(&ui, cx, start + url.len());
            assert!(
                !has(&ui, cx, "file-preview-content"),
                "suffix after {prefix:?}"
            );
            let hit = if prefix.starts_with('界') {
                start + url.len() - 1
            } else {
                start
            };
            click_column(&ui, cx, hit);
            assert!(has(&ui, cx, "file-preview-content"), "prefix={prefix:?}");
            assert_eq!(
                ui.read(cx, |app, _| app.file_preview.as_ref().unwrap().path.clone()),
                path
            );
            assert_eq!(cx.opened_url(), None);
        }
    }
}
