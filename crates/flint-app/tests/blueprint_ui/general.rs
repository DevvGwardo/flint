use super::*;

#[gpui_kit::test]
fn starts_without_a_project_and_sends_an_ordinary_question(cx: &mut TestAppContext) {
    let mut options = test_options();
    options.workspace = None;
    let home = options.home.clone().unwrap();
    let ui = open_with(cx, options);
    assert!(ui.read(cx, |app, _| app.session().general));
    assert_eq!(
        ui.read(cx, |app, _| app.session().workspace.clone()),
        flint_app::general::workspace(&home)
    );
    assert!(ui.read(cx, |app, _| app.session().workspace.is_dir()));
    assert!(!ui.read(cx, |app, _| app.session().workspace.join(".git").exists()));
    assert_eq!(
        ui.read(cx, |app, _| app.session().workspace_label()),
        "General agent"
    );
    assert_eq!(ui.read(cx, |app, _| app.branch()), None);
    let engine = ui.engine(cx);
    ui.input(cx, "Help me plan my day");
    ui.press(cx, "enter");
    assert!(
        matches!(engine.sent().as_slice(), [Op::UserMessage(text)] if text == "Help me plan my day")
    );
    let dir = ui.read(cx, |app, _| app.session().dir.clone().unwrap());
    ui.app
        .update(cx, |app, _| app.sessions[app.active].flush_records())
        .unwrap();
    let meta: flint_app::store::Meta =
        serde_json::from_slice(&std::fs::read(dir.join("meta.json")).unwrap()).unwrap();
    assert!(meta.general);
}

#[gpui_kit::test]
fn switching_between_general_and_project_preserves_running_conversations(cx: &mut TestAppContext) {
    let ui = open(cx);
    let project = ui.read(cx, |app, _| app.session().workspace.clone());
    let engine = ui.engine(cx);
    ui.input(cx, "Work on the project");
    ui.press(cx, "enter");
    engine.sent();
    engine.send(cx, AgentEvent::TurnStarted { turn_id: 1 });
    let original = ui.read(cx, |app, _| app.session().uid);
    ui.app.update(cx, |app, cx| app.use_general_agent(cx));
    assert!(ui.read(cx, |app, _| app.session().general));
    assert!(ui.read(cx, |app, _| {
        app.sessions
            .iter()
            .any(|s| s.uid == original && s.view.running)
    }));
    assert_eq!(
        ui.read(cx, |app, _| app
            .sessions
            .iter()
            .find(|s| s.uid == original)
            .unwrap()
            .workspace
            .clone()),
        project
    );
    let new_project = temp_dir("attached-project");
    ui.app.update(cx, |app, cx| {
        app.set_project_folder(new_project.clone(), cx)
    });
    assert!(!ui.read(cx, |app, _| app.session().general));
    assert_eq!(
        ui.read(cx, |app, _| app.session().workspace.clone()),
        new_project
    );
}

#[gpui_kit::test]
fn general_mode_restores_without_changing_legacy_project_sessions(cx: &mut TestAppContext) {
    let mut options = test_options();
    options.workspace = None;
    let home = options.home.clone().unwrap();
    let ui = open_with(cx, options.clone());
    let engine = ui.engine(cx);
    ui.input(cx, "Write a short plan");
    ui.press(cx, "enter");
    engine.sent();
    let dir = ui.read(cx, |app, _| app.session().dir.clone().unwrap());
    ui.app
        .update(cx, |app, _| app.sessions[app.active].flush_records())
        .unwrap();
    let restored = open_with(cx, options);
    assert!(restored.read(cx, |app, _| {
        app.sessions
            .iter()
            .any(|s| s.dir.as_ref() == Some(&dir) && s.general)
    }));
    assert!(restored.read(cx, |app, _| app.project_items().iter().all(
        |item| !matches!(item, flint_app::project_menu::ProjectItem::Recent(path)
            if path == &flint_app::general::workspace(&home))
    )));
    let legacy = serde_json::json!({
        "id": "legacy", "title": null, "workspace": "/tmp/legacy-project",
        "created_at": 1, "updated_at": 2
    });
    let meta: flint_app::store::Meta = serde_json::from_value(legacy).unwrap();
    assert!(!meta.general);
}

#[gpui_kit::test]
fn general_welcome_and_menu_fit_a_narrow_window(cx: &mut TestAppContext) {
    let mut options = test_options();
    options.workspace = None;
    options.window_size = Some((560., 480.));
    let ui = open_with(cx, options);
    ui.with(cx, |window, _| {
        for id in ["welcome-folder", "send", "composer-toolbar"] {
            let bounds = window.find(ElementId::Name(id.into())).bounds();
            assert!(bounds.left() >= px(0.) && bounds.right() <= px(560.));
            assert!(bounds.top() >= px(0.) && bounds.bottom() <= px(480.));
        }
    });
    ui.click(cx, "attach");
    assert!(ui.read(cx, |app, _| {
        app.project_items()
            .contains(&flint_app::project_menu::ProjectItem::General)
    }));
}
