use super::*;

#[gpui_kit::test]
fn unicode_filename_match_is_first_and_keyboard_attaches_the_exact_path(cx: &mut TestAppContext) {
    for window_size in [(1440., 900.), (900., 560.)] {
        let workspace = tempfile::tempdir().unwrap();
        for (path, content) in [
            ("目录/x.rs", "// chosen Unicode filename fixture\n"),
            ("x/note.rs", "// directory-only match fixture\n"),
        ] {
            let file = workspace.path().join(path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, content).unwrap();
        }
        let ui = open_with(
            cx,
            Options {
                workspace: Some(workspace.path().into()),
                window_size: Some(window_size),
                ..test_options()
            },
        );
        let engine = ui.engine(cx);
        ui.input(cx, "explain @X");
        ui.read(cx, |app, _| {
            let menu = app.mention.as_ref().unwrap();
            assert_eq!(menu.results, ["目录/x.rs", "x/note.rs"]);
            assert_eq!(menu.selected, 0);
        });
        ui.with(cx, |window, cx| {
            window.render_frame(cx);
            let panel = window.find("mention-menu").bounds();
            let row = window.find(("mention-item", 0usize)).bounds();
            assert!(row.top() >= panel.top() && row.bottom() <= panel.bottom());
        });
        ui.press(cx, "enter");
        assert_eq!(ui.composer_text(cx), "explain @目录/x.rs ");
        assert_eq!(ui.read(cx, |app, _| app.attachments.clone()), ["目录/x.rs"]);
        ui.press(cx, "enter");
        let sent = engine.sent();
        assert!(matches!(sent.as_slice(), [Op::UserMessage(message)]
            if message.contains("[Attached file 目录/x.rs (1 lines)]")
                && message.contains("// chosen Unicode filename fixture")
                && !message.contains("// directory-only match fixture")));
    }
}
