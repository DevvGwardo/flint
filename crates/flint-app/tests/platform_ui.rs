//! Platform-neutral headless regressions, also run by Windows CI.

use flint_agent::{AgentEvent, ApprovalDecision, Op, SessionHandle, ToolKind};
use flint_app::app::{FlintApp, Options};
use flint_app::settings::KeySources;
use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    AnyWindowHandle, App, AppContext as _, Bounds, Entity, Point, TestAppContext, Window,
    WindowBounds, WindowOptions, px, size,
};

struct Fixture {
    window: AnyWindowHandle,
    app: Entity<FlintApp>,
    _home: tempfile::TempDir,
    _workspace: tempfile::TempDir,
}

impl Fixture {
    fn open(cx: &mut TestAppContext) -> Self {
        let home = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let options = Options {
            home: Some(home.path().to_path_buf()),
            workspace: Some(workspace.path().to_path_buf()),
            key_sources: Some(KeySources::none()),
            engine_poll: true,
            skip_permission_choice: true,
            ..Options::default()
        };
        cx.update(flint_app::init);
        let (window, app) = cx.update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Point::default(),
                        size: size(px(1440.), px(900.)),
                    })),
                    ..Default::default()
                },
                cx,
                |window, cx| cx.new(|cx| FlintApp::new(options, window, cx)),
            )
            .unwrap()
        });
        cx.run_until_parked();
        Self {
            window,
            app,
            _home: home,
            _workspace: workspace,
        }
    }

    fn with<R>(
        &self,
        cx: &mut TestAppContext,
        action: impl FnOnce(&mut Window, &mut App) -> R,
    ) -> R {
        cx.update_window(self.window, |_, window, cx| action(window, cx))
            .unwrap()
    }

    fn press(&self, cx: &mut TestAppContext, key: &str) {
        self.with(cx, |window, cx| window.press(key, cx));
        cx.run_until_parked();
    }
}

fn settle(cx: &mut TestAppContext) {
    for _ in 0..2 {
        cx.run_until_parked();
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(20));
    }
    cx.run_until_parked();
}

#[gpui_kit::test]
fn coastal_home_preserves_the_composer_setup_action_and_compact_layout(cx: &mut TestAppContext) {
    let fixture = Fixture::open(cx);
    settle(cx);
    fixture.with(cx, |window, cx| {
        window.render_frame(cx);
        for id in [
            "welcome-coast",
            "welcome-dashboard",
            "welcome-heading",
            "composer-toolbar",
            "welcome-working",
            "welcome-recent",
            "welcome-workspace-overview",
        ] {
            assert!(window.try_find(id).is_some(), "{id} missing");
        }
        window.click("welcome-credential-settings", cx);
    });
    assert!(cx.read(|cx| fixture.app.read(cx).settings_form.is_some()));
    fixture.press(cx, "escape");
    cx.simulate_window_resize(fixture.window, size(px(900.), px(560.)));
    settle(cx);
    fixture.with(cx, |window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("welcome-coast").is_some());
        assert!(window.try_find("welcome-dashboard").is_none());
        for ix in 0..4usize {
            assert!(window.find(("suggestion", ix)).bounds().bottom() <= px(560.));
        }
    });
}

#[gpui_kit::test]
fn jev_composer_toggle_remains_reachable_and_persists_in_compact_windows(cx: &mut TestAppContext) {
    let fixture = Fixture::open(cx);
    cx.simulate_window_resize(fixture.window, size(px(900.), px(560.)));
    settle(cx);
    fixture.with(cx, |window, cx| {
        window.render_frame(cx);
        window.scroll(
            "composer-options",
            gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(-160.))),
            cx,
        );
    });
    settle(cx);
    fixture.with(cx, |window, cx| {
        window.render_frame(cx);
        let options = window.find("composer-options").bounds();
        let toggle = window.find("jev-toggle").bounds();
        assert!(toggle.top() >= options.top() && toggle.bottom() <= options.bottom());
        assert!(toggle.left() >= options.left() && toggle.right() <= options.right());
        window.click("jev-toggle", cx);
    });
    assert!(!cx.read(|cx| fixture.app.read(cx).settings.jev_judge));
    assert!(
        !flint_app::settings::Settings::load(fixture._home.path(), &KeySources::none()).jev_judge
    );
    fixture.press(cx, "secondary-,");
    assert!(!cx.read(|cx| {
        fixture
            .app
            .read(cx)
            .settings_form
            .as_ref()
            .unwrap()
            .jev_judge
    }));
    fixture.press(cx, "escape");
}

#[gpui_kit::test]
fn native_primary_shortcuts_open_sessions_settings_and_changes(cx: &mut TestAppContext) {
    let fixture = Fixture::open(cx);
    assert_eq!(cx.read(|cx| fixture.app.read(cx).sessions.len()), 1);
    // An attached engine makes the original session non-reusable, without
    // invoking any provider.
    let (ops, _received) = async_channel::unbounded();
    let (_events, stream) = async_channel::unbounded();
    fixture.with(cx, |_, cx| {
        fixture.app.update(cx, |app, cx| {
            app.attach_engine(
                app.active,
                SessionHandle {
                    ops,
                    events: stream,
                },
                cx,
            );
        });
    });
    fixture.press(cx, "secondary-n");
    assert_eq!(cx.read(|cx| fixture.app.read(cx).sessions.len()), 2);
    fixture.press(cx, "secondary-j");
    assert!(cx.read(|cx| fixture.app.read(cx).changes_open));
    fixture.press(cx, "secondary-j");
    assert!(!cx.read(|cx| fixture.app.read(cx).changes_open));
    fixture.press(cx, "secondary-,");
    assert!(cx.read(|cx| fixture.app.read(cx).settings_form.is_some()));
    fixture.press(cx, "escape");
    assert!(cx.read(|cx| fixture.app.read(cx).settings_form.is_none()));
    fixture.with(cx, |window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("workspace-titlebar").is_some());
        assert!(window.find("workspace-titlebar").bounds().size.height > px(0.));
    });
}

#[gpui_kit::test]
fn native_primary_enter_answers_approval_instead_of_submitting_text(cx: &mut TestAppContext) {
    let fixture = Fixture::open(cx);
    let (ops, received) = async_channel::unbounded();
    let (events, stream) = async_channel::unbounded();
    fixture.with(cx, |_, cx| {
        fixture.app.update(cx, |app, cx| {
            app.attach_engine(
                app.active,
                SessionHandle {
                    ops,
                    events: stream,
                },
                cx,
            )
        });
    });
    events
        .try_send(AgentEvent::ToolCallStarted {
            call_id: "platform-approval".into(),
            name: "run_command".into(),
            kind: ToolKind::Command,
            summary: "echo fixture".into(),
            args: serde_json::json!({"command":"echo fixture"}),
        })
        .unwrap();
    events
        .try_send(AgentEvent::ApprovalRequested {
            call_id: "platform-approval".into(),
            kind: ToolKind::Command,
            summary: "echo fixture".into(),
        })
        .unwrap();
    settle(cx);
    assert!(cx.read(|cx| fixture.app.read(cx).session().view.pending_approvals > 0));
    fixture.press(cx, "secondary-enter");
    let sent: Vec<_> = std::iter::from_fn(|| received.try_recv().ok()).collect();
    assert!(sent.iter().any(|op| matches!(op, Op::Approval { call_id, decision: ApprovalDecision::Approve } if call_id == "platform-approval")));
    assert!(
        !sent
            .iter()
            .any(|op| matches!(op, Op::UserMessage(_) | Op::UserMessageWithImages { .. }))
    );
}

#[gpui_kit::test]
fn terminal_focus_preserves_control_app_shortcuts_and_selection_copy(cx: &mut TestAppContext) {
    let fixture = Fixture::open(cx);
    // Exercise the Windows Ctrl binding on macOS too, against the same terminal
    // key-down path, rather than hiding the regression behind cfg(windows).
    cx.update(|cx| {
        cx.bind_keys([gpui_kit::KeyBinding::new(
            "ctrl-n",
            flint_app::app::NewSession,
            None,
        )])
    });
    let (ops, _received) = async_channel::unbounded();
    let (_events, stream) = async_channel::unbounded();
    let terminal = fixture.with(cx, |window, cx| {
        fixture.app.update(cx, |app, cx| {
            app.attach_engine(
                app.active,
                SessionHandle {
                    ops,
                    events: stream,
                },
                cx,
            );
            let view = cx.new(|cx| {
                flint_app::term_view::TermView::new(
                    flint_term::Terminal::detached(flint_term::Size {
                        cols: 80,
                        rows: 16,
                        cell_width: 8,
                        cell_height: 18,
                    }),
                    app.session().workspace.clone(),
                    false,
                    false,
                    cx,
                )
            });
            view.update(cx, |view, cx| {
                view.session_uid = Some(app.session().uid);
                view.feed("clipboard", false, cx);
            });
            app.add_terminal_tab(view.clone(), window, cx);
            view
        })
    });
    fixture.with(cx, |window, cx| window.render_frame(cx));
    terminal.update(cx, |view, cx| {
        view.terminal.start_selection(0, 0, false, 1);
        view.terminal.update_selection(0, 8, true);
        cx.notify();
    });
    assert_eq!(
        terminal.read_with(cx, |view, _| view.terminal.selection_text()),
        Some("clipboard".into())
    );
    fixture.press(
        cx,
        if cfg!(target_os = "macos") {
            "cmd-c"
        } else {
            "ctrl-shift-c"
        },
    );
    assert_eq!(
        cx.update(|cx| cx.read_from_clipboard().unwrap().text().unwrap()),
        "clipboard"
    );
    fixture.press(cx, "ctrl-n");
    assert_eq!(cx.read(|cx| fixture.app.read(cx).sessions.len()), 2);
    assert!(terminal.read_with(cx, |view, _| view.terminal.selection_text().is_some()));
}
