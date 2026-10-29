use super::{
    BTreeMap, CATALOG_ID, Component, FlintApp, PathBuf, SURFACE_ID, SurfaceSnapshot, TextValue,
};
use flint_agent::{AgentEvent, TaskRecord, ToolKind};
use gpui_kit::{TestAppContext, WindowHandle};

fn fixture(cx: &mut TestAppContext) -> (tempfile::TempDir, WindowHandle<FlintApp>) {
    cx.update(crate::init);
    let root = tempfile::tempdir().unwrap();
    let options = crate::app::Options {
        home: Some(root.path().join("home")),
        workspace: Some(root.path().to_owned()),
        skip_permission_choice: true,
        ..Default::default()
    };
    let window = cx.add_window(|window, cx| FlintApp::new(options, window, cx));
    (root, window)
}

#[gpui_kit::test]
fn genui_staging_preserves_selected_draft_attachments_and_never_submits(cx: &mut TestAppContext) {
    let (_root, window) = fixture(cx);
    window
        .update(cx, |app, window, cx| {
            app.sessions[app.active].view.task =
                Some(TaskRecord::new(1, "Implement the requested work".into(), 0));
            let (ops, received) = async_channel::unbounded();
            app.sessions[app.active].ops = Some(ops);
            app.attachments = vec!["existing.txt".into()];
            app.image_attachments = vec![PathBuf::from("/fixture/existing.png")];
            app.composer.update(cx, |input, cx| {
                input.set_value("Existing draft, not disposable.", window, cx);
                input.select_all(window, cx);
            });
            app.stage_genui_followup(window, cx);
            let text = app.composer.read(cx).value().to_string();
            assert!(text.contains("Existing draft, not disposable."));
            assert!(text.contains("Do not make changes."));
            assert_eq!(app.attachments, vec!["existing.txt".to_string()]);
            assert_eq!(
                app.image_attachments,
                vec![PathBuf::from("/fixture/existing.png")]
            );
            assert!(received.try_recv().is_err(), "Staging must never submit.");
        })
        .unwrap();
}

#[gpui_kit::test]
fn genui_staging_is_held_without_an_active_parent_task(cx: &mut TestAppContext) {
    let (_root, window) = fixture(cx);
    window
        .update(cx, |app, window, cx| {
            app.composer
                .update(cx, |input, cx| input.set_value("Keep me", window, cx));
            app.stage_genui_followup(window, cx);
            assert_eq!(app.composer.read(cx).value().to_string(), "Keep me");
            app.sessions[app.active].view.task =
                Some(TaskRecord::new(1, "Implement the change".into(), 0));
            app.sessions[app.active].selected_subagent = Some("child".into());
            app.stage_genui_followup(window, cx);
            assert_eq!(app.composer.read(cx).value().to_string(), "Keep me");
            app.sessions[app.active].selected_subagent = None;
            app.sessions[app.active].prompt_pending = true;
            app.stage_genui_followup(window, cx);
            assert_eq!(app.composer.read(cx).value().to_string(), "Keep me");
        })
        .unwrap();
}

#[gpui_kit::test]
fn genui_generated_content_is_revoked_after_native_task_or_work_changes(cx: &mut TestAppContext) {
    let (_root, window) = fixture(cx);
    window
        .update(cx, |app, _, _| {
            app.sessions[app.active].view.task =
                Some(TaskRecord::new(1, "Implement the change".into(), 0));
            let uid = app.session().uid;
            let surface = SurfaceSnapshot {
                surface_id: SURFACE_ID.into(),
                catalog_id: CATALOG_ID.into(),
                data: BTreeMap::new(),
                components: vec![Component::PlainText {
                    id: "root".into(),
                    text: TextValue::Literal("Prepared by the model".into()),
                    style: None,
                }],
            };
            app.capture_genui_event(
                uid,
                &AgentEvent::ToolCallStarted {
                    call_id: "preview".into(),
                    name: flint_agent::tools::RENDER_UI.into(),
                    kind: ToolKind::Read,
                    args: serde_json::json!({"surface":surface}),
                    summary: "Prepare view".into(),
                },
            );
            app.capture_genui_event(
                uid,
                &AgentEvent::ToolCallFinished {
                    call_id: "preview".into(),
                    output: serde_json::json!({"surface":surface}).to_string(),
                    exit_code: None,
                    success: true,
                    diff: None,
                    duration_ms: 0,
                },
            );
            assert!(app.genui_snapshot().surface == surface);
            let generated_key = app.genui_snapshot_key();
            assert!(app.changed_genui_snapshot(Some(&generated_key)).is_none());
            app.capture_genui_event(
                uid,
                &AgentEvent::ToolCallStarted {
                    call_id: "replacement".into(),
                    name: flint_agent::tools::RENDER_UI.into(),
                    kind: ToolKind::Read,
                    args: serde_json::json!({"surface":surface}),
                    summary: "Replace view".into(),
                },
            );
            app.capture_genui_event(
                uid,
                &AgentEvent::ToolCallFinished {
                    call_id: "replacement".into(),
                    output: serde_json::json!({"surface":surface}).to_string(),
                    exit_code: None,
                    success: true,
                    diff: None,
                    duration_ms: 0,
                },
            );
            assert!(app.changed_genui_snapshot(Some(&generated_key)).is_some());
            app.sessions[app.active]
                .view
                .task
                .as_mut()
                .unwrap()
                .work_revision += 1;
            assert!(app.genui_snapshot().surface != surface);
            app.sessions[app.active]
                .view
                .task
                .as_mut()
                .unwrap()
                .work_revision -= 1;
            app.sessions[app.active]
                .view
                .task
                .as_mut()
                .unwrap()
                .contract
                .revision += 1;
            assert!(app.genui_snapshot().surface != surface);
            app.sessions[app.active].view.task =
                Some(TaskRecord::new(2, "A different task".into(), 0));
            assert!(app.genui_snapshot().surface != surface);
        })
        .unwrap();
}

#[gpui_kit::test]
fn genui_runtime_remains_sandbox_authorized_with_custom_application_home(cx: &mut TestAppContext) {
    let (_root, window) = fixture(cx);
    window
        .update(cx, |app, _, _| {
            let runtime = super::runtime_directory().unwrap();
            let user_home = flint_platform::home_dir().unwrap();
            assert_eq!(runtime, user_home.join(".flint/genui-runtime"));
            assert_ne!(runtime, app.home.join("genui-runtime"));
        })
        .unwrap();
}

#[gpui_kit::test]
fn genui_cache_skips_unchanged_content_but_tracks_projection_and_visibility(
    cx: &mut TestAppContext,
) {
    let (_root, window) = fixture(cx);
    window
        .update(cx, |app, _, _| {
            let mut task = TaskRecord::new(1, "Implement the change".into(), 1);
            task.assess(flint_agent::TurnEndReason::Completed, true);
            app.sessions[app.active].view.fold(
                AgentEvent::TurnStarted { turn_id: 1 },
                std::time::Duration::ZERO,
            );
            app.sessions[app.active].view.fold(
                AgentEvent::TaskUpdated {
                    turn_id: 1,
                    task: Box::new(task.clone()),
                },
                std::time::Duration::ZERO,
            );
            let (key, original) = app.changed_genui_snapshot(None).unwrap();
            assert!(app.changed_genui_snapshot(Some(&key)).is_none());
            task.assessment
                .as_mut()
                .unwrap()
                .issues
                .push("New assessment detail without a work revision change.".into());
            app.sessions[app.active].view.fold(
                AgentEvent::TaskAssessed {
                    turn_id: 1,
                    task: Box::new(task),
                },
                std::time::Duration::ZERO,
            );
            let (new_key, changed) = app.changed_genui_snapshot(Some(&key)).unwrap();
            assert!(key.task == new_key.task);
            assert!(original.surface != changed.surface);
            assert!(app.changed_genui_snapshot(Some(&new_key)).is_none());
            app.sessions[app.active].view.running = false;
            app.sessions[app.active].prompt_pending = true;
            let (hidden, _) = app.changed_genui_snapshot(Some(&new_key)).unwrap();
            assert!(hidden.task.is_none());
            app.sessions[app.active].prompt_pending = false;
            app.sessions[app.active].selected_subagent = Some("child".into());
            assert!(app.changed_genui_snapshot(Some(&hidden)).is_none());
            app.sessions[app.active].selected_subagent = None;
            assert!(app.changed_genui_snapshot(Some(&hidden)).is_some());
            app.sessions[app.active].view.task_projection_revision = u64::MAX;
            let (exhausted, _) = app.changed_genui_snapshot(None).unwrap();
            assert!(app.changed_genui_snapshot(Some(&exhausted)).is_some());
        })
        .unwrap();
}
