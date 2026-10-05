use super::*;

#[gpui_kit::test]
fn collapsed_output_details_fit_and_expansion_keeps_the_original_output(cx: &mut TestAppContext) {
    for window_size in [(1440., 900.), (900., 560.)] {
        for kind in [ToolKind::Command, ToolKind::Search, ToolKind::Edit] {
            let ui = open_with(
                cx,
                Options {
                    window_size: Some(window_size),
                    ..test_options()
                },
            );
            let engine = running_turn(&ui, cx);
            engine.send(
                cx,
                AgentEvent::ToolCallStarted {
                    call_id: "detail".into(),
                    name: "fixture_tool".into(),
                    kind,
                    summary: "fixture detail".into(),
                    args: json!({}),
                },
            );
            let output = format!("{} UNIQUE_TAIL", "界🙂".repeat(512));
            engine.send(
                cx,
                AgentEvent::ToolCallFinished {
                    call_id: "detail".into(),
                    output: output.clone(),
                    exit_code: Some(1),
                    success: kind != ToolKind::Edit,
                    diff: None,
                    duration_ms: 1,
                },
            );
            let row = ui.read(cx, |app, _| app.session().view.items.len() - 1);
            ui.with(cx, |window, cx| {
                window.render_frame(cx);
                let detail = window.find(("tool-detail", row)).bounds();
                let header = window.find(("tool-header", row)).bounds();
                assert!(detail.right() <= header.right() && detail.bottom() > header.bottom());
            });
            ui.click(cx, ("tool", row));
            ui.read(cx, |app, _| {
                let Item::Tool(call) = &app.session().view.items[row] else {
                    panic!("expected tool");
                };
                assert!(call.expanded);
                assert_eq!(call.output, output);
                assert_eq!(
                    call.result.as_ref().unwrap().success,
                    kind != ToolKind::Edit
                );
            });
            ui.with(cx, |window, cx| {
                window.render_frame(cx);
                assert!(window.try_find(("tool-detail", row)).is_none());
            });
        }
    }
}
