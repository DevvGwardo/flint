use super::*;

#[gpui_kit::test]
fn large_failure_details_keep_sidebar_status_and_retained_text(cx: &mut TestAppContext) {
    for window_size in [(1440., 900.), (1000., 560.)] {
        let ui = open_with(
            cx,
            Options {
                window_size: Some(window_size),
                ..test_options()
            },
        );
        let error = format!("  fixture failure 界🙂  \n{}", "detail".repeat(128 * 1024));
        ui.app.update(cx, |app, cx| {
            app.sessions[0].view.idle_error = Some(error.clone());
            cx.notify();
        });
        ui.read(cx, |app, _| {
            assert_eq!(
                app.session().status(),
                flint_app::session::Status::Failed { seen: true }
            );
            assert_eq!(
                app.session().status_line(app.now()).0,
                "Failed: fixture failure 界🙂"
            );
            assert_eq!(app.session().failure().as_deref(), Some(error.as_str()));
        });
        ui.with(cx, |window, cx| {
            window.render_frame(cx);
            let panel = window.find("sidebar").bounds();
            let subtitle = window.find(("sidebar-subtitle", 0usize)).bounds();
            assert!(subtitle.size.width > px(20.));
            assert!(subtitle.left() >= panel.left() && subtitle.right() <= panel.right());
        });
    }
}

#[gpui_kit::test]
fn long_failure_first_line_is_bounded_in_the_sidebar_and_kept_in_the_model(
    cx: &mut TestAppContext,
) {
    for window_size in [(1440., 900.), (1000., 560.)] {
        for turn_failure in [false, true] {
            let ui = open_with(
                cx,
                Options {
                    window_size: Some(window_size),
                    ..test_options()
                },
            );
            let error = format!("  {}  \r\nORIGINAL_ERROR_BODY", "界🙂".repeat(512));
            ui.app.update(cx, |app, cx| {
                if turn_failure {
                    app.sessions[0].view.last_reason = Some(TurnEndReason::Failed(error.clone()));
                } else {
                    app.sessions[0].view.idle_error = Some(error.clone());
                }
                cx.notify();
            });
            ui.read(cx, |app, _| {
                assert_eq!(
                    app.session().status(),
                    flint_app::session::Status::Failed { seen: true }
                );
                let (line, tone) = app.session().status_line(app.now());
                assert_eq!(tone, flint_app::session::Tone::Danger);
                assert_eq!(line, format!("Failed: {} …", "界🙂".repeat(120)));
                assert!(line.chars().count() <= flint_app::ui::MAX_HEADER_PREVIEW_CHARS + 10);
                assert_eq!(app.session().failure().as_deref(), Some(error.as_str()));
            });
            ui.with(cx, |window, cx| {
                window.render_frame(cx);
                let panel = window.find("sidebar").bounds();
                let subtitle = window.find(("sidebar-subtitle", 0usize)).bounds();
                assert!(subtitle.size.width > px(20.));
                assert!(subtitle.left() >= panel.left() && subtitle.right() <= panel.right());
            });
        }
    }
}

#[gpui_kit::test]
fn large_approval_summary_has_a_bounded_sidebar_without_shortening_request_details(
    cx: &mut TestAppContext,
) {
    for window_size in [(1440., 900.), (1000., 560.)] {
        let ui = open_with(
            cx,
            Options {
                window_size: Some(window_size),
                ..test_options()
            },
        );
        let engine = running_turn(&ui, cx);
        let summary = format!("{}\nORIGINAL_BODY", "界🙂".repeat(512));
        let command = "printf 'ORIGINAL_ARGUMENT'";
        engine.send(
            cx,
            AgentEvent::ToolCallStarted {
                call_id: "approval-preview".into(),
                name: "run_command".into(),
                kind: ToolKind::Command,
                summary: summary.clone(),
                args: json!({"command": command}),
            },
        );
        engine.send(
            cx,
            AgentEvent::ApprovalRequested {
                call_id: "approval-preview".into(),
                kind: ToolKind::Command,
                summary: summary.clone(),
            },
        );
        engine.send(
            cx,
            AgentEvent::ApprovalRequested {
                call_id: "second".into(),
                kind: ToolKind::Command,
                summary: "second".into(),
            },
        );
        ui.read(cx, |app, _| {
            assert_eq!(
                app.session().status(),
                flint_app::session::Status::NeedsApproval
            );
            let (line, tone) = app.session().status_line(app.now());
            assert_eq!(tone, flint_app::session::Tone::Warning);
            assert!(line.chars().count() <= flint_app::ui::MAX_HEADER_PREVIEW_CHARS + 64);
            assert!(line.ends_with(" … (+1 more)"));
            assert!(app.session().view.items.iter().any(|item| matches!(
                item, Item::Approval { summary: stored, decision: None, .. } if stored == &summary
            )));
            let preview = app.session().view.approval_preview(
                "approval-preview",
                &app.session().workspace,
                true,
            );
            assert!(
                preview
                    .fields
                    .iter()
                    .any(|(name, value)| *name == "Command" && value == command)
            );
        });
        ui.with(cx, |window, cx| {
            window.render_frame(cx);
            let panel = window.find("sidebar").bounds();
            let subtitle = window.find(("sidebar-subtitle", 0usize)).bounds();
            assert!(subtitle.size.width > px(20.));
            assert!(subtitle.left() >= panel.left() && subtitle.right() <= panel.right());
        });
    }
}
