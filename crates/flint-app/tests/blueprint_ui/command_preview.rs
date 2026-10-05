use super::*;

#[gpui_kit::test]
fn command_preview_bounds_live_headers_without_changing_the_literal_command(
    cx: &mut TestAppContext,
) {
    let ui = open_with(
        cx,
        Options {
            window_size: Some((900., 560.)),
            ..test_options()
        },
    );
    let engine = running_turn(&ui, cx);
    let command = format!("printf '%s' \"{} UNIQUE_END\"", "界🙂".repeat(512));
    engine.send(
        cx,
        AgentEvent::ToolCallStarted {
            call_id: "oversized".into(),
            name: "run_command".into(),
            kind: ToolKind::Command,
            args: json!({"command": command}),
            summary: command.clone(),
        },
    );
    let row = ui.read(cx, |app, _| app.session().view.items.len() - 1);
    ui.with(cx, |window, cx| {
        window.render_frame(cx);
        let header = window.find(("tool-header", row)).bounds();
        let summary = window.find(("tool-summary", row)).bounds();
        assert!(summary.bottom() <= header.bottom());
        assert!(summary.right() <= header.right());
        let tray = window.find("running-command-oversized").bounds();
        let preview = window.find("running-command-summary-oversized").bounds();
        assert!(preview.bottom() <= tray.bottom());
        assert!(preview.right() <= tray.right());
    });
    ui.read(cx, |app, _| {
        let Item::Tool(call) = &app.session().view.items[row] else {
            panic!("expected tool");
        };
        assert_eq!(call.summary, command);
        assert_eq!(call.args["command"].as_str(), Some(command.as_str()));
        assert!(
            app.session().activity().chars().count()
                <= flint_app::ui::MAX_HEADER_PREVIEW_CHARS + 10
        );
        let preview = flint_app::ui::one_line(&call.summary);
        assert!(preview.ends_with(" …"));
        assert!(!preview.contains("UNIQUE_END"));
    });
    ui.click(cx, ("tool", row));
    ui.with(cx, |window, cx| {
        window.render_frame(cx);
        assert!(window.try_find(("tool-command", row)).is_some());
    });
    ui.read(cx, |app, _| {
        let Item::Tool(call) = &app.session().view.items[row] else {
            panic!("expected tool");
        };
        assert!(call.expanded);
        assert_eq!(call.args["command"].as_str(), Some(command.as_str()));
    });
}

#[gpui_kit::test]
fn command_preview_bounds_mirrored_terminal_labels_and_keeps_exit_status(cx: &mut TestAppContext) {
    let ui = open(cx);
    let engine = running_turn(&ui, cx);
    engine.send(
        cx,
        AgentEvent::TerminalStarted {
            terminal_id: "large-label".into(),
            call_id: None,
            label: format!("agent: {}", "界".repeat(10_000)),
            cwd: None,
        },
    );
    let label = ui.read(cx, |app, cx| {
        app.terminal.active_view().unwrap().read(cx).label()
    });
    assert_eq!(
        label.chars().count(),
        flint_app::ui::MAX_HEADER_PREVIEW_CHARS + 2
    );
    assert!(label.starts_with("agent: "));
    assert!(label.ends_with(" …"));
    engine.send(
        cx,
        AgentEvent::TerminalExited {
            terminal_id: "large-label".into(),
            exit_code: Some(7),
        },
    );
    let label = ui.read(cx, |app, cx| {
        app.terminal.active_view().unwrap().read(cx).label()
    });
    assert!(label.ends_with(" … (exit 7)"));
}
