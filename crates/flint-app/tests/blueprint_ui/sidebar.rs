use super::*;

#[gpui_kit::test]
fn sidebar_dock_resizing_keeps_controls_readable(cx: &mut TestAppContext) {
    let ui = open(cx);
    cx.simulate_window_resize(ui.window, size(px(1000.), px(560.)));
    settle(cx);
    ui.with(cx, |window, cx| {
        window.render_frame(cx);
        window.simulate_next_frame(cx);
        window.render_frame(cx);
        let sidebar = window.find("sidebar").bounds();
        assert!(
            sidebar.size.width >= px(264.),
            "sidebar controls need a readable minimum width: {sidebar:?}"
        );
    });
    ui.with(cx, |window, cx| {
        let bounds = window
            .find(ElementId::Name("dock-panel-sidebar".into()))
            .bounds();
        let from = point(bounds.right(), bounds.center().y);
        window.drag(from, from - point(px(200.), px(0.)), cx);
    });
    settle(cx);
    ui.with(cx, |window, cx| {
        window.render_frame(cx);
        let sidebar = window.find("sidebar").bounds();
        let settings = window.find("settings").bounds();
        let grouping = window.find("session-grouping").bounds();
        assert!(sidebar.size.width >= px(264.), "{sidebar:?}");
        assert!(settings.size.width >= px(140.), "{settings:?}");
        assert!(settings.right() <= grouping.left());
        assert!(grouping.right() <= sidebar.right());
    });
}

#[gpui_kit::test]
fn sidebar_filter_positions_survive_multi_digit_counts(cx: &mut TestAppContext) {
    let ui = open(cx);
    let bounds = |cx: &mut TestAppContext| {
        ui.with(cx, |window, cx| {
            window.render_frame(cx);
            (0..4usize)
                .map(|ix| window.find(("session-filter", ix)).bounds())
                .collect::<Vec<_>>()
        })
    };
    let before = bounds(cx);
    seed_sidebar_sessions(&ui, cx, 120);
    ui.app.update(cx, |app, cx| {
        for session in app.sessions.iter_mut().skip(1).take(60) {
            session.view.running = true;
        }
        for session in app.sessions.iter_mut().skip(61) {
            session.unread = true;
        }
        cx.notify();
    });
    assert_eq!(bounds(cx), before, "counts must not move filter targets");
}

#[gpui_kit::test]
fn sidebar_long_branch_preserves_readable_metadata(cx: &mut TestAppContext) {
    let tmp = tempfile::tempdir().unwrap();
    let git = tmp.path().join(".git");
    std::fs::create_dir(&git).unwrap();
    std::fs::write(
        git.join("HEAD"),
        format!(
            "ref: refs/heads/{}\n",
            "very-long-feature-branch-".repeat(20)
        ),
    )
    .unwrap();
    for width in [900., 1000.] {
        let ui = open_with(
            cx,
            Options {
                workspace: Some(tmp.path().to_path_buf()),
                window_size: Some((width, 560.)),
                ..test_options()
            },
        );
        ui.app.update(cx, |app, cx| {
            app.sessions[0].view.session_usage.input_tokens = 123_456;
            cx.notify();
        });
        if width < 1000. {
            ui.click(cx, "sessions-control");
        }
        ui.with(cx, |window, cx| {
            window.render_frame(cx);
            let details = window.find(("sidebar-details", 0usize)).bounds();
            let branch = window.find(("sidebar-branch", 0usize)).bounds();
            let usage = window.find(("sidebar-usage", 0usize)).bounds();
            assert!(branch.size.width >= px(24.), "{branch:?}");
            assert!(branch.right() <= usage.left(), "{branch:?} / {usage:?}");
            assert!(usage.right() <= details.right(), "{usage:?} / {details:?}");
        });
    }
}

#[gpui_kit::test]
fn sidebar_timestamp_does_not_take_space_from_the_title(cx: &mut TestAppContext) {
    let ui = open(cx);
    ui.with(cx, |window, cx| {
        window.render_frame(cx);
        let title = window.find(("sidebar-title", 0usize)).bounds();
        let when = window.find(("sidebar-when", 0usize)).bounds();
        let actions = window.find(("session-actions", 0usize)).bounds();
        assert!(when.top() >= title.bottom(), "{title:?} / {when:?}");
        assert!(title.right() > when.left());
        assert!(title.right() <= actions.left());
    });
}

#[gpui_kit::test]
fn sidebar_rename_reveals_a_background_session_beyond_the_first_page(cx: &mut TestAppContext) {
    let ui = open(cx);
    seed_sidebar_sessions(&ui, cx, flint_app::app::SESSION_PAGE + 15);
    assert!(!ui.with(cx, |window, cx| {
        window.render_frame(cx);
        window.try_find(("session", 1usize)).is_some()
    }));
    ui.with(cx, |window, cx| {
        ui.app.update(cx, |app, cx| app.start_rename(1, window, cx));
    });
    ui.with(cx, |window, cx| {
        window.render_frame(cx);
        window.simulate_next_frame(cx);
        window.render_frame(cx);
        let editor = window.find(("rename-input", 1usize)).bounds();
        let list = window.find("session-list").bounds();
        assert!(editor.top() >= list.top(), "{editor:?} / {list:?}");
        assert!(editor.bottom() <= list.bottom(), "{editor:?} / {list:?}");
        let input = &ui.app.read(cx).renaming.as_ref().unwrap().1;
        assert!(input.focus_handle(cx).is_focused(window));
    });
    ui.press(cx, "cmd-a");
    ui.input(cx, "Renamed older session");
    ui.press(cx, "enter");
    assert_eq!(
        ui.read(cx, |app, _| app.sessions[1].title()),
        "Renamed older session"
    );
    assert_eq!(ui.read(cx, |app, _| app.active), 0);
}

#[gpui_kit::test]
fn sidebar_project_heading_does_not_depend_on_the_newest_worktree(cx: &mut TestAppContext) {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    let linked = tmp.path().join("repo-feature");
    let admin = repo.join(".git/worktrees/repo-feature");
    std::fs::create_dir_all(&admin).unwrap();
    std::fs::create_dir_all(linked.join("src")).unwrap();
    std::fs::create_dir_all(repo.join("src")).unwrap();
    std::fs::write(admin.join("commondir"), "../..\n").unwrap();
    std::fs::write(
        linked.join(".git"),
        format!("gitdir: {}\n", admin.display()),
    )
    .unwrap();
    let ui = open(cx);
    seed_sidebar_sessions(&ui, cx, 3);
    ui.app.update(cx, |app, cx| {
        app.sessions[0].workspace = repo.join("src");
        app.sessions[1].workspace = repo.join("src");
        app.sessions[2].workspace = linked.join("src");
        app.sessions[2].touched = std::time::SystemTime::now() + std::time::Duration::from_secs(10);
        cx.notify();
    });
    let groups = ui.read(cx, |app, cx| app.session_groups(cx).groups);
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].label, "repo/src");
    assert_eq!(
        groups[0].tooltip.as_deref(),
        Some(repo.join("src").to_str().unwrap())
    );
    ui.app.update(cx, |app, cx| {
        app.sessions[1].touched = std::time::SystemTime::now() + std::time::Duration::from_secs(20);
        cx.notify();
    });
    assert_eq!(
        ui.read(cx, |app, cx| app.session_groups(cx).groups[0].label.clone()),
        groups[0].label
    );
}
