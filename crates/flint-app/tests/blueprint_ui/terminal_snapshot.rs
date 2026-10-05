use super::*;

#[gpui_kit::test]
fn detached_terminal_keeps_styled_unicode_snapshots_across_real_view_paints(
    cx: &mut TestAppContext,
) {
    for dimensions in [(1440., 900.), (900., 560.)] {
        let ui = open_with(
            cx,
            Options {
                window_size: Some(dimensions),
                ..test_options()
            },
        );
        let terminal = ui.app.update(cx, |app, cx| {
            let uid = app.session().uid;
            let terminal = cx.new(|cx| {
                flint_app::term_view::TermView::new(
                    flint_term::Terminal::detached(flint_term::Size {
                        cols: 80,
                        rows: 16,
                        cell_width: 8,
                        cell_height: 18,
                    }),
                    app.session().workspace.clone(),
                    true,
                    false,
                    cx,
                )
            });
            terminal.update(cx, |view, cx| {
                view.session_uid = Some(uid);
                view.feed(
                    "\x1b[1;31me\u{301}界🙂\x1b[0m end\r\n\x1b[4munderlined\x1b[0m",
                    false,
                    cx,
                );
            });
            app.terminal.tabs.push(terminal.clone());
            app.terminal.active = app.terminal.tabs.len() - 1;
            app.terminal.open = true;
            cx.notify();
            terminal
        });
        settle(cx);
        ui.with(cx, |window, cx| {
            window.render_frame(cx);
            let bounds = window.find("terminal-panel").bounds();
            assert!(
                bounds.size.width > px(0.) && bounds.size.height > px(0.),
                "{bounds:?}"
            );
            assert!(
                bounds.right() <= px(dimensions.0) && bounds.bottom() <= px(dimensions.1),
                "{bounds:?}"
            );
        });
        terminal.update(cx, |view, cx| {
            view.terminal.start_selection(0, 0, false, 1);
            view.terminal.update_selection(0, 5, true);
            cx.notify();
        });
        ui.with(cx, |window, cx| window.render_frame(cx));
        let snapshot = terminal.read_with(cx, |view, _| view.terminal.snapshot());
        assert_ne!((snapshot.cols, snapshot.rows), (80, 16));
        assert_eq!(snapshot.text_lines()[0], "e\u{301}界🙂 end");
        assert_eq!(snapshot.text_lines()[1], "underlined");
        assert_eq!(
            (
                snapshot.lines[0][0].text.as_str(),
                snapshot.lines[0][0].width
            ),
            ("e\u{301}界🙂", 5)
        );
        assert!(snapshot.lines[0][0].bold && snapshot.lines[1][0].underline);
        assert_ne!(snapshot.lines[0][0].fg, snapshot.lines[0][1].fg);
        assert!(!snapshot.selection.is_empty());
        ui.with(cx, |window, cx| window.render_frame(cx));
        assert_eq!(
            terminal.read_with(cx, |view, _| view.terminal.snapshot()),
            snapshot
        );
    }
}
