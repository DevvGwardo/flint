use super::*;

static SCROLL_KEY_LOOKUPS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn scrolling_key_source(name: &str) -> Option<String> {
    if name == "HOOTER_SCROLL_PROBE" {
        SCROLL_KEY_LOOKUPS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    None
}

fn scroll_fixture(cx: &mut TestAppContext) -> Ui {
    let mut options = test_options();
    options.window_size = Some((1000., 700.));
    options.key_path = None;
    options.key_sources = Some(KeySources {
        env: scrolling_key_source,
        legacy_key_file: None,
    });
    let ui = open_with(cx, options);
    ui.app.update(cx, |app, cx| {
        app.settings.api_key_env = "HOOTER_SCROLL_PROBE".into();
        let workspace = app.session().workspace.clone();
        for uid in 101..149 {
            let mut session = flint_app::session::Session::new(uid, workspace.clone());
            session.view.push_user(format!("Conversation {uid}"));
            for turn_id in 1..=200 {
                session.view.fold(
                    AgentEvent::TurnStarted { turn_id },
                    std::time::Duration::ZERO,
                );
                session.view.fold(
                    AgentEvent::TurnFinished {
                        turn_id,
                        reason: TurnEndReason::Completed,
                    },
                    std::time::Duration::ZERO,
                );
            }
            app.sessions.push(session);
        }
        cx.notify();
    });
    settle(cx);
    ui
}

#[gpui_kit::test]
#[ignore = "manual homepage render/scroll performance probe"]
fn homepage_scroll_performance_probe(cx: &mut TestAppContext) {
    let ui = scroll_fixture(cx);
    SCROLL_KEY_LOOKUPS.store(0, std::sync::atomic::Ordering::Relaxed);
    let mut milliseconds = Vec::new();
    for frame in 0..120 {
        let started = std::time::Instant::now();
        ui.with(cx, |window, cx| {
            window.scroll(
                "welcome-content",
                ScrollDelta::Pixels(point(px(0.), px(if frame % 40 < 20 { -18. } else { 18. }))),
                cx,
            );
            window.render_frame(cx);
        });
        milliseconds.push(started.elapsed().as_secs_f64() * 1000.);
    }
    milliseconds.sort_by(f64::total_cmp);
    let result = json!({
        "profile": if cfg!(debug_assertions) { "debug" } else { "release" },
        "fixture_sessions":49, "fixture_finished_turns":9600, "scroll_frames":120,
        "render_p50_ms":milliseconds[60], "render_p95_ms":milliseconds[114],
        "credential_lookups":SCROLL_KEY_LOOKUPS.load(std::sync::atomic::Ordering::Relaxed),
        "measurement":"headless GPUI scroll plus render CPU time, not display frame intervals",
    });
    println!("homepage_scroll_probe={result}");
    if let Some(path) = std::env::var_os("FLINT_HOME_SCROLL_RESULT") {
        std::fs::write(path, serde_json::to_vec_pretty(&result).unwrap()).unwrap();
    }
}

#[gpui_kit::test]
fn homepage_scrolling_reuses_credential_status_and_updates_changed_key_configuration(
    cx: &mut TestAppContext,
) {
    let ui = scroll_fixture(cx);
    SCROLL_KEY_LOOKUPS.store(0, std::sync::atomic::Ordering::Relaxed);
    let started = std::time::Instant::now();
    for frame in 0..12 {
        ui.with(cx, |window, cx| {
            window.scroll(
                "welcome-content",
                ScrollDelta::Pixels(point(px(0.), px(if frame < 6 { -18. } else { 18. }))),
                cx,
            );
            window.render_frame(cx);
        });
    }
    let lookups = SCROLL_KEY_LOOKUPS.load(std::sync::atomic::Ordering::Relaxed);
    let allowed = 1 + started.elapsed().as_secs() as usize / 2;
    assert!(
        lookups <= allowed,
        "{lookups} credential lookups across 12 scroll frames; expected at most {allowed}"
    );
    assert!(has(&ui, cx, "welcome-credential-settings"));
    let key = temp_dir("scroll-credential").join("api.key");
    std::fs::write(&key, "fixture-key").unwrap();
    ui.app.update(cx, |app, cx| {
        app.key_path = Some(key);
        cx.notify();
    });
    settle(cx);
    assert!(!has(&ui, cx, "welcome-credential-settings"));
}

#[gpui_kit::test]
fn hooter_sets_the_native_app_and_window_identity(cx: &mut TestAppContext) {
    let ui = open(cx);
    assert_eq!(
        cx.app_identity(),
        Some(("com.hooter.desktop".into(), "Hooter".into(),))
    );
    assert_eq!(
        gpui_kit::VisualTestContext::from_window(ui.window, cx)
            .window_title()
            .as_deref(),
        Some("Hooter")
    );
    assert!(has(&ui, cx, "hooter-brand"));
}

#[gpui_kit::test]
fn fullscreen_welcome_uses_a_wide_workbench_and_two_even_columns(cx: &mut TestAppContext) {
    for general in [false, true] {
        let mut options = test_options();
        options.window_size = Some((1920., 1080.));
        if general {
            options.workspace = None;
        }
        let ui = open_with(cx, options);
        settle(cx);
        ui.with(cx, |window, cx| {
            window.render_frame(cx);
            let workbench = window.find("welcome-workbench").bounds();
            let header = window.find("header").bounds();
            let toolbar = window.find("composer-toolbar").bounds();
            assert_eq!(
                workbench.size.width,
                px(flint_app::transcript::WELCOME_WIDTH)
            );
            assert!(toolbar.size.width > px(900.), "{toolbar:?}");
            assert!(workbench.top() >= header.bottom(), "{workbench:?}");
            assert!(workbench.bottom() <= px(1080.), "{workbench:?}");
            let working = window.find("welcome-working").bounds();
            let recent = window.find("welcome-recent").bounds();
            let activity = window.find("welcome-activity").bounds();
            let workspace = window.find("welcome-workspace-overview").bounds();
            assert_eq!(working.top(), recent.top());
            assert_eq!(activity.top(), workspace.top());
            assert!(working.right() < recent.left());
            assert!(working.bottom() < activity.top());
            assert_eq!(working.size.width, recent.size.width);
            let cards: Vec<_> = (0..4usize)
                .map(|ix| window.find(("suggestion", ix)).bounds())
                .collect();
            assert_eq!(cards[0].top(), cards[1].top());
            assert_eq!(cards[2].top(), cards[3].top());
            assert!(cards[0].right() < cards[1].left());
            assert!(cards[0].bottom() < cards[2].top());
            assert_eq!(cards[0].size.width, cards[1].size.width);
            for card in cards {
                assert!(card.left() >= workbench.left() && card.right() <= workbench.right());
            }
        });
    }
}

#[gpui_kit::test]
fn native_fullscreen_reclaims_the_window_controls_strip(cx: &mut TestAppContext) {
    let ui = open(cx);
    ui.with(cx, |window, cx| {
        let before = window.find("workspace-titlebar").bounds();
        assert_eq!(before.size.height, px(36.));
        window.toggle_fullscreen();
        window.refresh();
        window.render_frame(cx);
        let after = window.find("workspace-titlebar").bounds();
        assert_eq!(after.size.height, px(8.));
        assert!(
            window.find("welcome-logo").bounds().top() >= window.find("header").bounds().bottom()
        );
        window.toggle_fullscreen();
        window.refresh();
        window.render_frame(cx);
        assert_eq!(
            window.find("workspace-titlebar").bounds().size.height,
            px(36.)
        );
    });
}

#[gpui_kit::test]
fn compact_welcome_keeps_all_four_suggestions_inside_the_minimum_window(cx: &mut TestAppContext) {
    for general in [false, true] {
        let mut options = test_options();
        options.window_size = Some((900., 560.));
        if general {
            options.workspace = None;
        }
        let ui = open_with(cx, options);
        settle(cx);
        ui.with(cx, |window, cx| {
            window.render_frame(cx);
            assert!(window.try_find("welcome-coast").is_some());
            assert!(window.try_find("welcome-dashboard").is_none());
            for ix in 0..4usize {
                let bounds = window.find(("suggestion", ix)).bounds();
                assert!(
                    bounds.top() >= px(0.) && bounds.bottom() <= px(560.),
                    "{bounds:?}"
                );
            }
        });
    }
}

#[gpui_kit::test]
fn suggestion_cards_fill_the_draft_without_sending_it(cx: &mut TestAppContext) {
    for (general, prompts) in [
        (
            false,
            [
                "Fix the failing tests",
                "Explain this codebase",
                "Review my uncommitted changes",
                "Add input validation",
            ],
        ),
        (
            true,
            [
                "Help me plan my day",
                "Explain a topic simply",
                "Help me write something",
                "Think through a decision with me",
            ],
        ),
    ] {
        let mut options = test_options();
        if general {
            options.workspace = None;
        }
        let ui = open_with(cx, options);
        let engine = ui.engine(cx);
        for (ix, prompt) in prompts.into_iter().enumerate() {
            ui.with(cx, |window, cx| {
                window.scroll(
                    "welcome-content",
                    ScrollDelta::Pixels(point(px(0.), px(-1500.))),
                    cx,
                );
                window.render_frame(cx);
            });
            ui.click(cx, ("suggestion", ix));
            assert_eq!(ui.composer_text(cx), prompt);
            assert!(engine.sent().is_empty());
            assert!(ui.with(cx, |window, cx| {
                ui.app.read(cx).composer.focus_handle(cx).is_focused(window)
            }));
            ui.with(cx, |window, cx| {
                window.render_frame(cx);
                let viewport = window.find("welcome-content").bounds();
                let toolbar = window.find("composer-toolbar").bounds();
                assert!(toolbar.top() >= viewport.top() && toolbar.bottom() <= viewport.bottom());
            });
        }
    }
}

#[gpui_kit::test]
fn fullscreen_welcome_becomes_compact_when_the_terminal_takes_its_height(cx: &mut TestAppContext) {
    let ui = open(cx);
    let engine = ui.engine(cx);
    engine.send(
        cx,
        AgentEvent::TerminalStarted {
            terminal_id: "welcome-terminal".into(),
            call_id: None,
            label: "Welcome terminal".into(),
            cwd: None,
        },
    );
    ui.app.update(cx, |app, cx| {
        app.terminal.open = true;
        cx.notify();
    });
    settle(cx);
    assert!(!has(&ui, cx, "welcome-logo"));
    assert!(!has(&ui, cx, "welcome-dashboard"));
    let chat = panel_bounds(&ui, cx, flint_app::docking::Panel::Chat);
    ui.with(cx, |window, cx| {
        window.render_frame(cx);
        let toolbar = window.find("composer-toolbar").bounds();
        assert!(
            toolbar.bottom() <= chat.bottom(),
            "{toolbar:?} outside {chat:?}"
        );
    });
}

#[gpui_kit::test]
fn coastal_welcome_has_a_real_empty_dashboard_and_fourteen_day_chart(cx: &mut TestAppContext) {
    let ui = open(cx);
    settle(cx);
    assert!(ui.read(cx, |app, _| app.sessions.len() == 1
        && app.session().view.items.is_empty()));
    ui.with(cx, |window, cx| {
        window.render_frame(cx);
        for id in [
            "welcome-coast",
            "welcome-dashboard",
            "welcome-working",
            "welcome-recent",
            "welcome-activity",
            "welcome-workspace-overview",
        ] {
            assert!(window.try_find(id).is_some(), "{id} missing");
        }
        let chart = window.find("welcome-activity-chart").bounds();
        for day in 0..14usize {
            let bar = window.find(("welcome-activity-day", day)).bounds();
            assert_eq!(bar.size.height, px(2.));
            assert!(bar.size.width > px(0.));
            assert!(bar.left() >= chart.left() && bar.right() <= chart.right());
            assert!(bar.bottom() <= chart.bottom());
        }
    });
}

#[gpui_kit::test]
fn welcome_dashboard_stacks_in_a_narrow_panel_and_scrolls_to_its_actions(cx: &mut TestAppContext) {
    let mut options = test_options();
    options.window_size = Some((900., 800.));
    let ui = open_with(cx, options);
    ui.press(cx, "secondary-j");
    settle(cx);
    ui.with(cx, |window, cx| {
        window.render_frame(cx);
        let working = window.find("welcome-working").bounds();
        let recent = window.find("welcome-recent").bounds();
        let activity = window.find("welcome-activity").bounds();
        let workspace = window.find("welcome-workspace-overview").bounds();
        assert_eq!(working.left(), recent.left());
        assert_eq!(working.size.width, recent.size.width);
        assert!(working.bottom() < recent.top());
        assert!(recent.bottom() < activity.top());
        assert!(activity.bottom() < workspace.top());
        window.scroll(
            "welcome-content",
            ScrollDelta::Pixels(point(px(0.), px(-1500.))),
            cx,
        );
        window.render_frame(cx);
        let viewport = window.find("welcome-content").bounds();
        let terminal = window.find("welcome-terminals").bounds();
        assert!(terminal.top() >= viewport.top() && terminal.bottom() <= viewport.bottom());
    });
}

#[gpui_kit::test]
fn welcome_dashboard_terminal_action_opens_the_real_dock(cx: &mut TestAppContext) {
    let mut options = test_options();
    options.window_size = Some((1920., 1080.));
    options.terminal_command = Some(flint_platform::interactive_shell());
    let ui = open_with(cx, options);
    settle(cx);
    assert!(!ui.read(cx, |app, _| app.terminal.open));
    ui.click(cx, "welcome-terminals");
    assert!(ui.read(cx, |app, _| app.terminal.open
        && app.terminal.tabs.len() == 1));
    assert!(has(&ui, cx, "terminal-panel"));
}

#[gpui_kit::test]
fn welcome_dashboard_recent_rows_are_bounded_ordered_and_open_the_actual_session(
    cx: &mut TestAppContext,
) {
    let mut options = test_options();
    options.window_size = Some((1920., 1080.));
    let ui = open_with(cx, options);
    let now = std::time::SystemTime::now();
    ui.app.update(cx, |app, cx| {
        let workspace = app.session().workspace.clone();
        for (uid, age) in [(101, 30), (102, 10), (103, 20)] {
            let mut session = flint_app::session::Session::new(uid, workspace.clone());
            let change = session.view.push_user(format!("Conversation {uid}"));
            session.apply(change);
            session.touched = now - std::time::Duration::from_secs(age);
            app.sessions.push(session);
        }
        cx.notify();
    });
    settle(cx);
    assert!(!has(&ui, cx, "welcome-recent-101"));
    ui.with(cx, |window, cx| {
        window.render_frame(cx);
        assert!(
            window.find("welcome-recent-102").bounds().top()
                < window.find("welcome-recent-103").bounds().top()
        );
        assert_eq!(
            window
                .find(("welcome-activity-day", 13usize))
                .bounds()
                .size
                .height,
            px(32.)
        );
    });
    ui.click(cx, "welcome-recent-103");
    assert_eq!(ui.read(cx, |app, _| app.session().uid), 103);
    assert!(!has(&ui, cx, "welcome-dashboard"));
}

#[gpui_kit::test]
fn welcome_dashboard_filters_working_and_attention_without_sending_a_prompt(
    cx: &mut TestAppContext,
) {
    let mut options = test_options();
    options.window_size = Some((1920., 1080.));
    let ui = open_with(cx, options);
    let engine = ui.engine(cx);
    ui.app.update(cx, |app, cx| {
        let workspace = app.session().workspace.clone();
        for uid in 101..104 {
            let mut session = flint_app::session::Session::new(uid, workspace.clone());
            let change = session.view.push_user(format!("Working {uid}"));
            session.apply(change);
            session.view.running = true;
            app.sessions.push(session);
        }
        let mut waiting = flint_app::session::Session::new(104, workspace);
        let change = waiting.view.push_user("Needs approval".into());
        waiting.apply(change);
        let change = waiting.view.fold(
            AgentEvent::ApprovalRequested {
                call_id: "fixture".into(),
                kind: ToolKind::Read,
                summary: "Inspect a local fixture".into(),
            },
            std::time::Duration::ZERO,
        );
        waiting.apply(change);
        app.sessions.push(waiting);
        cx.notify();
    });
    settle(cx);
    assert!(has(&ui, cx, "welcome-working-101"));
    assert!(has(&ui, cx, "welcome-working-102"));
    assert!(!has(&ui, cx, "welcome-working-103"));
    ui.click(cx, "welcome-running-agents");
    assert!(ui.read(cx, |app, _| app.filter
        == flint_app::app::SessionFilter::Only(
            flint_app::session::Bucket::Working
        )));
    assert_eq!(ui.read(cx, |app, cx| app.visible_sessions(cx).len()), 3);
    ui.click(cx, "welcome-needs-you");
    assert!(ui.read(cx, |app, _| app.filter
        == flint_app::app::SessionFilter::Only(
            flint_app::session::Bucket::NeedsYou
        )));
    assert_eq!(ui.read(cx, |app, cx| app.visible_sessions(cx)), vec![4]);
    assert!(engine.sent().is_empty());
    assert_eq!(ui.composer_text(cx), "");
}

#[gpui_kit::test]
fn missing_credentials_have_a_direct_settings_action(cx: &mut TestAppContext) {
    let mut options = test_options();
    options.key_path = None;
    let ui = open_with(cx, options);
    ui.click(cx, "welcome-credential-settings");
    assert!(ui.read(cx, |app, _| app.settings_form.is_some()));
}

#[gpui_kit::test]
fn overflowing_welcome_keeps_the_heading_reachable_at_the_initial_scroll_position(
    cx: &mut TestAppContext,
) {
    for general in [false, true] {
        for (width, height) in [(1000., 700.), (900., 620.), (1100., 800.)] {
            let mut options = test_options();
            options.window_size = Some((width, height));
            if general {
                options.workspace = None;
            }
            let ui = open_with(cx, options);
            settle(cx);
            ui.with(cx, |window, cx| {
                window.render_frame(cx);
                let viewport = window.find("welcome-content").bounds();
                let heading = window.find("welcome-heading").bounds();
                assert!(
                    heading.top() >= viewport.top(),
                    "{width}x{height}, general={general}: {heading:?} above {viewport:?}"
                );
                assert!(heading.bottom() <= viewport.bottom());
            });
        }
    }
}
