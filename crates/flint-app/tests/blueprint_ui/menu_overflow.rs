use super::*;

#[gpui_kit::test]
fn long_picker_query_and_filename_fit_without_changing_the_attachment(cx: &mut TestAppContext) {
    for window_size in [(1440., 900.), (900., 560.), (600., 560.)] {
        let workspace = tempfile::tempdir().unwrap();
        let name = format!("{}.rs", "long-name-".repeat(22));
        let path = format!("目录/{name}");
        let file = workspace.path().join(&path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, "// full-path fixture\n").unwrap();
        let ui = open_with(
            cx,
            Options {
                workspace: Some(workspace.path().into()),
                window_size: Some(window_size),
                ..test_options()
            },
        );
        let engine = ui.engine(cx);
        ui.input(cx, &format!("explain @{}", "long-name-".repeat(12)));
        ui.read(cx, |app, _| {
            assert_eq!(
                app.mention.as_ref().unwrap().results.as_slice(),
                std::slice::from_ref(&path)
            );
        });
        ui.with(cx, |window, cx| {
            window.render_frame(cx);
            let panel = window.find("mention-menu").bounds();
            let title = window.find("menu-title").bounds();
            let hint = window.find("menu-hint").bounds();
            let row = window.find(("mention-item", 0usize)).bounds();
            let name = window.find(("mention-name", 0usize)).bounds();
            eprintln!("picker_panel={panel:?} hint={hint:?} filename={name:?} row={row:?}");
            assert!(
                title.size.height <= px(22.) && title.right() <= hint.left(),
                "title {title:?} overlaps/wraps past hint {hint:?}"
            );
            assert!(hint.right() <= panel.right(), "{hint:?} exceeds {panel:?}");
            assert!(
                name.size.height <= px(26.)
                    && name.right() <= row.right()
                    && name.bottom() <= row.bottom(),
                "filename {name:?} overflows fixed row {row:?}"
            );
        });
        ui.press(cx, "enter");
        assert_eq!(ui.composer_text(cx), format!("explain @{path} "));
        ui.read(cx, |app, _| {
            assert_eq!(app.attachments.as_slice(), std::slice::from_ref(&path));
        });
        ui.press(cx, "enter");
        let sent = engine.sent();
        assert!(matches!(sent.as_slice(), [Op::UserMessage(message)]
            if message.contains(&format!("[Attached file {path} (1 lines)]"))
                && message.contains("// full-path fixture")));
    }
}
