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

// Rows are two lines now (docs/PLAN-SIDEBAR-SIMPLIFY.md P3): the branch shares
// the second line with the status and the clock, and the token total moved into
// the title's tooltip, so `sidebar-details` / `sidebar-usage` are gone.
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
            let panel = window.find("sidebar").bounds();
            let row = window.find(("session", 0usize)).bounds();
            let branch = window.find(("sidebar-branch", 0usize)).bounds();
            let when = window.find(("sidebar-when", 0usize)).bounds();
            // The branch truncates inside the panel instead of pushing the
            // status and the clock out of it.
            assert!(branch.size.width >= px(24.), "{branch:?}");
            assert!(branch.right() <= when.left(), "{branch:?} / {when:?}");
            assert!(when.right() <= panel.right(), "{when:?} / {panel:?}");
            assert!(row.size.height <= px(60.), "two-line rows: {row:?}");
            assert!(window.try_find(("sidebar-details", 0usize)).is_none());
            assert!(window.try_find(("sidebar-usage", 0usize)).is_none());
        });
        // The token total is still reachable, from the title's tooltip.
        assert!(ui.read(cx, |app, _| app.sessions[0].total_tokens()) > 0);
    }
}

#[gpui_kit::test]
fn sidebar_timestamp_does_not_take_space_from_the_title(cx: &mut TestAppContext) {
    let ui = open(cx);
    ui.app.update(cx, |app, cx| {
        // Fresh sessions intentionally omit timestamps; test an actual label.
        app.sessions[0].touched =
            std::time::SystemTime::now() - std::time::Duration::from_secs(120);
        cx.notify();
    });
    ui.with(cx, |window, cx| {
        window.render_frame(cx);
        let title = window.find(("sidebar-title", 0usize)).bounds();
        let when = window.find(("sidebar-when", 0usize)).bounds();
        let actions = window.find(("session-actions", 0usize)).bounds();
        assert!(when.size.width > px(0.));
        assert!(when.top() >= title.bottom(), "{title:?} / {when:?}");
        assert!(title.right() > when.left());
        assert!(title.right() <= actions.left());
    });
}

#[gpui_kit::test]
fn sidebar_fresh_empty_timestamp_does_not_reserve_title_space(cx: &mut TestAppContext) {
    let ui = open(cx);
    ui.app.update(cx, |app, cx| {
        app.sessions[0].touched = std::time::SystemTime::now();
        cx.notify();
    });
    ui.with(cx, |window, cx| {
        window.render_frame(cx);
        let title = window.find(("sidebar-title", 0usize)).bounds();
        let when = window.find(("sidebar-when", 0usize)).bounds();
        let actions = window.find(("session-actions", 0usize)).bounds();
        assert_eq!(when.size.width, px(0.));
        assert!(title.right() >= when.left());
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

/// `has`, for an id that is not a plain name.
fn has_id(ui: &Ui, cx: &mut TestAppContext, id: impl Into<ElementId>) -> bool {
    let id = id.into();
    ui.with(cx, move |window, cx| {
        window.render_frame(cx);
        window.try_find(id).is_some()
    })
}

/// Drags the sidebar's dock edge left as far as the minimum width allows.
fn shrink_sidebar_to_minimum(ui: &Ui, cx: &mut TestAppContext) {
    ui.with(cx, |window, cx| {
        let bounds = window
            .find(ElementId::Name("dock-panel-sidebar".into()))
            .bounds();
        let from = point(bounds.right(), bounds.center().y);
        window.drag(from, from - point(px(200.), px(0.)), cx);
    });
    settle(cx);
}

#[gpui_kit::test]
fn sidebar_primary_actions_fit_one_row(cx: &mut TestAppContext) {
    let ui = open(cx);
    cx.simulate_window_resize(ui.window, size(px(1000.), px(560.)));
    settle(cx);
    shrink_sidebar_to_minimum(&ui, cx);
    ui.with(cx, |window, cx| {
        window.render_frame(cx);
        let panel = window.find("sidebar").bounds();
        let new_agent = window.find("new-agent").bounds();
        let open_folder = window.find("open-folder").bounds();
        let terminal = window.find("terminal").bounds();
        assert!(new_agent.size.width > px(60.), "{new_agent:?}");
        assert!(new_agent.right() <= open_folder.left(), "{new_agent:?}");
        assert!(open_folder.right() <= terminal.left(), "{open_folder:?}");
        assert!(
            terminal.right() <= panel.right(),
            "{terminal:?} / {panel:?}"
        );
        // The primary action is taller than the icon buttons; one row means
        // their centres line up.
        assert_eq!(new_agent.center().y, open_folder.center().y);
        assert_eq!(open_folder.center().y, terminal.center().y);
    });
}

#[gpui_kit::test]
fn sidebar_chrome_leaves_most_of_the_panel_to_sessions(cx: &mut TestAppContext) {
    let ui = open_with(
        cx,
        Options {
            window_size: Some((1440., 900.)),
            ..test_options()
        },
    );
    seed_sidebar_sessions(&ui, cx, 20);
    ui.with(cx, |window, cx| {
        window.render_frame(cx);
        let panel = window.find("sidebar").bounds();
        let list = window.find("session-list").bounds();
        assert!(
            list.size.height.as_f32() >= panel.size.height.as_f32() * 0.6,
            "list {list:?} / panel {panel:?}"
        );
        // Header, primary actions, search and filters: the fixed chrome above.
        let chrome = (list.top() - panel.top()).as_f32();
        assert!(chrome <= 202., "chrome above the list: {chrome}");
    });
    let short = open_with(
        cx,
        Options {
            window_size: Some((1000., 560.)),
            ..test_options()
        },
    );
    seed_sidebar_sessions(&short, cx, 20);
    short.with(cx, |window, cx| {
        window.render_frame(cx);
        let rows = (0..20usize)
            .filter(|&ix| window.try_find(("session", ix)).is_some())
            .count();
        assert!(rows >= 3, "rows visible at 1000×560: {rows}");
    });
}

#[gpui_kit::test]
fn sidebar_filters_stay_in_one_row_at_minimum_width(cx: &mut TestAppContext) {
    let ui = open(cx);
    cx.simulate_window_resize(ui.window, size(px(1000.), px(560.)));
    settle(cx);
    shrink_sidebar_to_minimum(&ui, cx);
    ui.with(cx, |window, cx| {
        window.render_frame(cx);
        let panel = window.find("sidebar").bounds();
        let chips: Vec<_> = (0..4usize)
            .map(|ix| window.find(("session-filter", ix)).bounds())
            .collect();
        assert!(
            chips
                .windows(2)
                .all(|pair| pair[0].right() <= pair[1].left())
        );
        assert_eq!(chips[0].top(), chips[3].top(), "one row: {chips:?}");
        assert!(chips[3].right() <= panel.right(), "{:?}", chips[3]);
    });
}

#[gpui_kit::test]
fn sidebar_branch_and_status_share_one_line_without_clipping_the_clock(cx: &mut TestAppContext) {
    let tmp = tempfile::tempdir().unwrap();
    let git = tmp.path().join(".git");
    std::fs::create_dir(&git).unwrap();
    std::fs::write(git.join("HEAD"), "ref: refs/heads/feature/sidebar\n").unwrap();
    let ui = open_with(
        cx,
        Options {
            workspace: Some(tmp.path().to_path_buf()),
            window_size: Some((1000., 560.)),
            ..test_options()
        },
    );
    ui.app.update(cx, |app, cx| {
        app.sessions[0].touched =
            std::time::SystemTime::now() - std::time::Duration::from_secs(120);
        cx.notify();
    });
    ui.with(cx, |window, cx| {
        window.render_frame(cx);
        let panel = window.find("sidebar").bounds();
        let title = window.find(("sidebar-title", 0usize)).bounds();
        let branch = window.find(("sidebar-branch", 0usize)).bounds();
        let status = window.find(("sidebar-subtitle", 0usize)).bounds();
        let when = window.find(("sidebar-when", 0usize)).bounds();
        // The clock keeps its slot; the branch truncates before it can push it.
        assert!(when.size.width > px(0.), "{when:?}");
        assert!(branch.top() >= title.bottom(), "{branch:?} / {title:?}");
        assert_eq!(
            branch.top(),
            status.top(),
            "one line: {branch:?} {status:?}"
        );
        assert!(branch.right() <= status.left(), "{branch:?} / {status:?}");
        assert!(status.right() <= when.left(), "{status:?} / {when:?}");
        assert!(when.right() <= panel.right(), "{when:?} / {panel:?}");
    });
}

#[gpui_kit::test]
fn sidebar_row_actions_appear_on_hover_and_focus(cx: &mut TestAppContext) {
    // The snapshot API exposes bounds, not paint opacity, so this fixture pins
    // what is observable: the slot never moves, and it stays clickable while
    // the glyph is hidden.
    let ui = open(cx);
    seed_sidebar_sessions(&ui, cx, 3);
    let actions = |ui: &Ui, cx: &mut TestAppContext| {
        ui.with(cx, |window, cx| {
            window.render_frame(cx);
            window.find(("session-actions", 1usize)).bounds()
        })
    };
    let before = actions(&ui, cx);
    ui.with(cx, |window, cx| {
        window.hover(("session", 1usize), cx);
        window.render_frame(cx);
    });
    assert_eq!(actions(&ui, cx), before, "hovering must not move the slot");
    ui.click(cx, ("session-actions", 1usize));
    assert_eq!(ui.read(cx, |app, _| app.session_menu), Some(1));
    assert_eq!(ui.read(cx, |app, _| app.active), 0);
}

#[gpui_kit::test]
fn sidebar_groups_collapse_and_remember_it(cx: &mut TestAppContext) {
    let options = test_options();
    let ui = open_with(cx, options.clone());
    seed_sidebar_sessions(&ui, cx, 4);
    assert!(ui.with(cx, |window, cx| {
        window.render_frame(cx);
        window.try_find(("session", 1usize)).is_some()
    }));
    ui.click(cx, ("group-header", 0usize));
    assert!(
        !ui.with(cx, |window, cx| {
            window.render_frame(cx);
            window.try_find(("session", 1usize)).is_some()
        }),
        "a collapsed heading hides its rows"
    );
    assert_eq!(ui.read(cx, |app, _| app.collapsed_groups.len()), 1);
    // The state survives a relaunch.
    let again = open_with(cx, options);
    assert_eq!(again.read(cx, |app, _| app.collapsed_groups.len()), 1);
}

#[gpui_kit::test]
fn sidebar_archived_disclosure_remembers_its_state(cx: &mut TestAppContext) {
    let options = test_options();
    for ix in 0..3 {
        let dir = options
            .home
            .as_ref()
            .unwrap()
            .join("archive")
            .join(format!("saved-{ix:02}"));
        flint_app::store::write_meta(
            &dir,
            &flint_app::store::Meta {
                id: format!("saved-{ix:02}"),
                title: Some(format!("Archived {ix}")),
                workspace: options.workspace.clone().unwrap(),
                subagent_workspaces: Vec::new(),
                created_at: ix,
                updated_at: ix,
                agent: flint_agent::AgentKind::Flint,
                general: false,
            },
        )
        .unwrap();
    }
    let ui = open_with(cx, options.clone());
    assert!(has(&ui, cx, "archived-sessions"));
    assert!(!has(&ui, cx, "archive-list"), "collapsed by default");
    ui.click(cx, "archived-sessions");
    assert!(has(&ui, cx, "archive-list"));
    let again = open_with(cx, options);
    assert!(
        has(&again, cx, "archive-list"),
        "the disclosure keeps its state across launches"
    );
}

#[gpui_kit::test]
fn sidebar_grouping_menu_offers_all_three_orders(cx: &mut TestAppContext) {
    use flint_app::app::SessionGrouping;
    let ui = open(cx);
    assert!(!has(&ui, cx, "grouping-menu"));
    ui.click(cx, "session-grouping");
    for ix in 0..3usize {
        assert!(has_id(&ui, cx, ("grouping-option", ix)), "option {ix}");
    }
    ui.click(cx, ("grouping-option", 2usize));
    assert_eq!(ui.read(cx, |app, _| app.grouping), SessionGrouping::Agent);
    assert!(!has(&ui, cx, "grouping-menu"), "choosing closes the menu");
    // Escape closes it too, and ⌘⇧G still cycles.
    ui.click(cx, "session-grouping");
    ui.press(cx, "escape");
    assert!(!has(&ui, cx, "grouping-menu"));
    ui.press(cx, "cmd-shift-g");
    assert_eq!(ui.read(cx, |app, _| app.grouping), SessionGrouping::Project);
}
