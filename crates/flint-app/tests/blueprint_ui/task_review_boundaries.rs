use super::*;
use flint_agent::{RequirementSpec, TaskEdit};

fn queued_verification_fixture(cx: &mut TestAppContext, enabled: bool) -> (Ui, Engine) {
    let ui = open(cx);
    ui.app.update(cx, |app, _| {
        app.settings.queue_after_verification = enabled;
    });
    let engine = running_turn(&ui, cx);
    ui.input(cx, "Next verified task");
    ui.press(cx, "enter");
    assert!(engine.sent().is_empty());
    (ui, engine)
}

fn finish_queue_fixture(
    engine: &Engine,
    cx: &mut TestAppContext,
    task: TaskRecord,
    reason: TurnEndReason,
) {
    engine.send(
        cx,
        AgentEvent::TaskAssessed {
            turn_id: 1,
            task: Box::new(task),
        },
    );
    engine.send(cx, AgentEvent::TurnFinished { turn_id: 1, reason });
}

fn enable_queue_policy(ui: &Ui, cx: &mut TestAppContext) {
    ui.press(cx, "secondary-,");
    ui.scroll_settings(cx);
    ui.click(cx, "settings-queue-verification");
    ui.click(cx, "settings-save");
}

#[gpui_kit::test]
fn queue_verification_cancel_and_failed_settings_save_cannot_release_review_holds(
    cx: &mut TestAppContext,
) {
    let (ui, engine) = queued_verification_fixture(cx, false);
    finish_queue_fixture(
        &engine,
        cx,
        task_fixture(TaskOutcome::NeedsReview),
        TurnEndReason::Completed,
    );
    ui.press(cx, "secondary-,");
    ui.scroll_settings(cx);
    ui.click(cx, "settings-queue-verification");
    ui.click(cx, "settings-cancel");
    assert!(engine.sent().is_empty());
    ui.press(cx, "secondary-,");
    ui.scroll_settings(cx);
    ui.click(cx, "settings-queue-verification");
    let home = ui.read(cx, |app, _| app.home.clone());
    std::fs::create_dir(home.join("config.toml.tmp")).unwrap();
    ui.click(cx, "settings-save");
    assert!(engine.sent().is_empty());
    ui.read(cx, |app, _| {
        assert!(!app.settings.queue_after_verification);
        assert!(app.settings_form.as_ref().unwrap().queue_after_verification);
        assert!(app.session().prompt_queue.paused);
        assert!(app.session().prompt_queue.review_hold.is_some());
        assert_eq!(app.session().prompt_queue.items.len(), 1);
    });
}

#[gpui_kit::test]
fn queue_verification_preserves_one_shot_override_without_accepting_or_releasing_the_next_hold(
    cx: &mut TestAppContext,
) {
    let (ui, engine) = queued_verification_fixture(cx, true);
    ui.input(cx, "Still needs review afterward");
    ui.press(cx, "enter");
    finish_queue_fixture(
        &engine,
        cx,
        task_fixture(TaskOutcome::Partial),
        TurnEndReason::Completed,
    );
    ui.click(cx, "queue-pause");
    ui.click(cx, "queue-resume-confirm");
    assert!(
        matches!(engine.sent().as_slice(), [Op::UserMessage(text)] if text == "Next verified task")
    );
    assert!(ui.read(cx, |app, _| {
        app.session().prompt_queue.review_override_turn.is_none()
    }));
    engine.send(cx, AgentEvent::TurnStarted { turn_id: 2 });
    engine.send(
        cx,
        AgentEvent::TaskAssessed {
            turn_id: 2,
            task: Box::new(task_fixture(TaskOutcome::Partial)),
        },
    );
    engine.send(
        cx,
        AgentEvent::TurnFinished {
            turn_id: 2,
            reason: TurnEndReason::Completed,
        },
    );
    assert!(engine.sent().is_empty());
    ui.read(cx, |app, _| {
        assert!(app.session().prompt_queue.paused);
        assert_eq!(app.session().prompt_queue.items.len(), 1);
        assert_eq!(app.session().view.outcome(), TaskOutcome::Partial);
        assert!(!app.session().view.task.as_ref().unwrap().is_accepted());
    });
}

#[gpui_kit::test]
fn queue_verification_opt_in_advances_without_accepting_the_result(cx: &mut TestAppContext) {
    let (ui, engine) = queued_verification_fixture(cx, true);
    let task = task_fixture(TaskOutcome::NeedsReview);
    assert!(task.verified_for_queue());
    finish_queue_fixture(&engine, cx, task, TurnEndReason::Completed);
    assert!(
        matches!(engine.sent().as_slice(), [Op::UserMessage(text)] if text == "Next verified task")
    );
    ui.read(cx, |app, _| {
        assert_eq!(app.session().view.outcome(), TaskOutcome::NeedsReview);
        assert!(!app.session().view.task.as_ref().unwrap().is_accepted());
        assert!(app.session().prompt_queue.items.is_empty());
        assert!(app.session().prompt_queue.review_hold.is_none());
    });
}

#[gpui_kit::test]
fn queue_verification_default_holds_until_saved_opt_in_releases_only_the_review_pause(
    cx: &mut TestAppContext,
) {
    let (ui, engine) = queued_verification_fixture(cx, false);
    let permissions = ui.read(cx, |app, _| (app.approval, app.settings.sandbox));
    finish_queue_fixture(
        &engine,
        cx,
        task_fixture(TaskOutcome::NeedsReview),
        TurnEndReason::Completed,
    );
    assert!(engine.sent().is_empty());
    ui.read(cx, |app, _| {
        assert!(app.session().prompt_queue.paused);
        assert!(app.session().prompt_queue.resume_after_review);
        assert!(app.session().prompt_queue.review_hold.is_some());
    });
    enable_queue_policy(&ui, cx);
    assert!(
        matches!(engine.sent().as_slice(), [Op::UserMessage(text)] if text == "Next verified task")
    );
    ui.read(cx, |app, _| {
        assert_eq!((app.approval, app.settings.sandbox), permissions);
        assert_eq!(app.session().view.outcome(), TaskOutcome::NeedsReview);
        assert!(!app.session().view.task.as_ref().unwrap().is_accepted());
        assert!(
            flint_app::settings::Settings::load(&app.home, &KeySources::none())
                .queue_after_verification
        );
    });
}

#[gpui_kit::test]
fn queue_verification_saved_opt_in_preserves_manual_pause(cx: &mut TestAppContext) {
    let (ui, engine) = queued_verification_fixture(cx, false);
    ui.click(cx, "queue-pause");
    finish_queue_fixture(
        &engine,
        cx,
        task_fixture(TaskOutcome::NeedsReview),
        TurnEndReason::Completed,
    );
    enable_queue_policy(&ui, cx);
    assert!(engine.sent().is_empty());
    ui.read(cx, |app, _| {
        assert!(app.session().prompt_queue.paused);
        assert!(!app.session().prompt_queue.resume_after_review);
        assert_eq!(app.session().prompt_queue.items.len(), 1);
    });
    ui.click(cx, "queue-pause");
    assert!(matches!(engine.sent().as_slice(), [Op::UserMessage(_)]));
    assert!(ui.read(cx, |app, _| app.queue_resume_confirm.is_none()));
}

#[gpui_kit::test]
fn queue_verification_does_not_blanket_release_needs_review_or_other_unresolved_outcomes(
    cx: &mut TestAppContext,
) {
    for outcome in [
        TaskOutcome::NeedsReview,
        TaskOutcome::Partial,
        TaskOutcome::Blocked,
        TaskOutcome::Unknown,
    ] {
        let (ui, engine) = queued_verification_fixture(cx, true);
        let mut task = task_fixture(outcome);
        if outcome == TaskOutcome::NeedsReview {
            // Evidence can support review while lacking a verification check.
            task.observed_edits = 0;
            task.checks[0].verification = false;
            task.assess(TurnEndReason::Completed, true);
            assert_eq!(
                task.assessment.as_ref().unwrap().outcome,
                TaskOutcome::NeedsReview
            );
        }
        assert!(!task.verified_for_queue());
        finish_queue_fixture(&engine, cx, task, TurnEndReason::Completed);
        assert!(engine.sent().is_empty(), "{outcome:?}");
        ui.read(cx, |app, _| {
            assert!(app.session().prompt_queue.paused);
            assert_eq!(app.session().prompt_queue.items.len(), 1);
        });
        ui.click(cx, "queue-pause");
        assert!(ui.read(cx, |app, _| app.queue_resume_confirm.is_some()));
    }
}

#[gpui_kit::test]
fn queue_verification_never_bypasses_failed_interrupted_or_budget_limited_execution(
    cx: &mut TestAppContext,
) {
    for reason in [
        TurnEndReason::Interrupted,
        TurnEndReason::Failed("Fixture provider failure".into()),
        TurnEndReason::StepLimit,
        TurnEndReason::ContinuationLimit,
    ] {
        let (ui, engine) = queued_verification_fixture(cx, true);
        // Even a late successful assessment cannot override execution failure.
        finish_queue_fixture(&engine, cx, task_fixture(TaskOutcome::NeedsReview), reason);
        assert!(engine.sent().is_empty());
        engine.send(
            cx,
            AgentEvent::TaskAssessed {
                turn_id: 1,
                task: Box::new(task_fixture(TaskOutcome::Accepted)),
            },
        );
        assert!(engine.sent().is_empty());
        assert!(ui.read(cx, |app, _| app.session().prompt_queue.paused));
    }
}

#[gpui_kit::test]
fn queue_verification_never_controls_acp_or_pending_permissions(cx: &mut TestAppContext) {
    for kind in ["acp", "approval"] {
        let (ui, engine) = queued_verification_fixture(cx, true);
        if kind == "acp" {
            ui.app.update(cx, |app, _| {
                app.sessions[app.active].agent = flint_agent::AgentKind::Droid
            });
            finish_queue_fixture(
                &engine,
                cx,
                task_fixture(TaskOutcome::NeedsReview),
                TurnEndReason::Completed,
            );
            assert!(ui.read(cx, |app, _| app.session().prompt_queue.paused));
        } else {
            approval_requested(&engine, cx, "verified-permission");
            engine.send(
                cx,
                AgentEvent::TaskAssessed {
                    turn_id: 1,
                    task: Box::new(task_fixture(TaskOutcome::NeedsReview)),
                },
            );
            assert!(ui.read(cx, |app, _| app.session().view.running));
        }
        assert!(engine.sent().is_empty(), "{kind}");
        if kind == "approval" {
            assert!(has(&ui, cx, "approval-card"));
        }
    }
}

#[gpui_kit::test]
fn queue_verification_policy_cannot_release_a_storage_failure(cx: &mut TestAppContext) {
    let (ui, engine) = queued_verification_fixture(cx, false);
    finish_queue_fixture(
        &engine,
        cx,
        task_fixture(TaskOutcome::NeedsReview),
        TurnEndReason::Completed,
    );
    let path = ui.read(cx, |app, _| {
        app.session()
            .dir
            .as_ref()
            .unwrap()
            .join("prompt-queue.json")
    });
    std::fs::rename(&path, path.with_file_name("kept-prompt-queue.json")).unwrap();
    std::fs::create_dir(&path).unwrap();
    enable_queue_policy(&ui, cx);
    assert!(engine.sent().is_empty());
    ui.read(cx, |app, _| {
        assert!(app.session().prompt_queue.paused);
        assert_eq!(app.session().prompt_queue.items.len(), 1);
        assert!(
            app.session()
                .prompt_queue
                .error
                .as_ref()
                .unwrap()
                .contains("Couldn't save")
        );
    });
}

#[gpui_kit::test]
fn queue_verification_keeps_restored_and_stopped_queues_paused(cx: &mut TestAppContext) {
    let (ui, engine) = queued_verification_fixture(cx, false);
    finish_queue_fixture(
        &engine,
        cx,
        task_fixture(TaskOutcome::NeedsReview),
        TurnEndReason::Completed,
    );
    let options = ui.read(cx, |app, _| Options {
        home: Some(app.home.clone()),
        workspace: Some(app.session().workspace.clone()),
        ..test_options()
    });
    let dir = ui.read(cx, |app, _| app.session().dir.clone().unwrap());
    let restored = open_with(cx, options);
    restored.with(cx, |window, cx| {
        restored.app.update(cx, |app, cx| {
            let ix = app
                .sessions
                .iter()
                .position(|session| session.dir.as_ref() == Some(&dir))
                .unwrap();
            app.select_session(ix, window, cx);
        })
    });
    enable_queue_policy(&restored, cx);
    restored.read(cx, |app, _| {
        assert!(app.session().prompt_queue.paused);
        assert!(!app.session().prompt_queue.resume_after_review);
        assert_eq!(app.session().prompt_queue.items.len(), 1);
        assert!(app.session().ops.is_none());
    });
    engine.send(
        cx,
        AgentEvent::SessionStopped {
            history_saved: true,
        },
    );
    enable_queue_policy(&ui, cx);
    assert!(engine.sent().is_empty());
    ui.read(cx, |app, _| {
        assert!(app.session().prompt_queue.paused);
        assert!(!app.session().prompt_queue.resume_after_review);
    });
}

#[gpui_kit::test]
fn native_counter_cannot_be_reused_when_history_is_missing_damaged_or_taskless(
    cx: &mut TestAppContext,
) {
    for kind in ["missing", "damaged", "taskless-version-one"] {
        let (ui, dir) = stopped_native_fixture(cx);
        let path = dir.join("history.json");
        match kind {
            "missing" => std::fs::remove_file(&path).unwrap(),
            "damaged" => std::fs::write(&path, b"{partial native history").unwrap(),
            _ => std::fs::write(
                &path,
                serde_json::to_vec(&serde_json::json!({
                    "version":1,"turn_id":0,"messages":[]
                }))
                .unwrap(),
            )
            .unwrap(),
        }
        let kept = std::fs::read(&path).ok();
        let options = ui.read(cx, |app, _| Options {
            home: Some(app.home.clone()),
            workspace: Some(app.session().workspace.clone()),
            key_path: Some("fixture-only-no-provider-request".into()),
            ..test_options()
        });
        let restored = open_with(cx, options);
        let ix = restored.read(cx, |app, _| {
            app.sessions
                .iter()
                .position(|session| session.dir.as_ref() == Some(&dir))
                .unwrap()
        });
        restored.with(cx,|window,cx| restored.app.update(cx,|app,cx| {
            app.select_session(ix,window,cx);
            app.composer.update(cx,|input,cx| input.set_value("Keep the saved conversation.",window,cx));
            app.submit(window,cx);
            assert!(app.session().ops.is_none(), "{kind}");
            assert!(!app.session().prompt_pending, "{kind}");
            assert_eq!(app.session().view.turn_id,1, "{kind}");
            assert!(app.session().view.items.iter().any(|item|
                matches!(item,Item::Error(message) if message.contains("Saved native history does not match this conversation"))
            ), "{kind}: the history guard must run before provider setup");
        }));
        assert_eq!(std::fs::read(&path).ok(), kept, "{kind}");
    }
}

#[cfg(unix)]
#[gpui_kit::test]
fn unchanged_review_hold_does_not_rewrite_the_saved_prompt_queue(cx: &mut TestAppContext) {
    use std::os::unix::fs::MetadataExt as _;
    let (ui, engine) = finished_fixture(cx, TaskOutcome::Partial);
    let path = ui.read(cx, |app, _| {
        app.session()
            .dir
            .as_ref()
            .unwrap()
            .join("prompt-queue.json")
    });
    // Retain the old inode, so an atomic replacement cannot reuse its number.
    let previous = std::fs::File::open(&path).unwrap();
    let inode = previous.metadata().unwrap().ino();
    engine.send(
        cx,
        AgentEvent::TaskAssessed {
            turn_id: 1,
            task: Box::new(task_fixture(TaskOutcome::Partial)),
        },
    );
    assert_eq!(std::fs::metadata(path).unwrap().ino(), inode);
}

#[gpui_kit::test]
fn failed_review_hold_save_is_retried_even_when_the_outcome_is_unchanged(cx: &mut TestAppContext) {
    let (ui, engine) = finished_fixture(cx, TaskOutcome::NeedsReview);
    let path = ui.read(cx, |app, _| {
        app.session()
            .dir
            .as_ref()
            .unwrap()
            .join("prompt-queue.json")
    });
    let kept = path.with_file_name("prompt-queue-kept.json");
    std::fs::rename(&path, &kept).unwrap();
    std::fs::create_dir(&path).unwrap();
    engine.send(
        cx,
        AgentEvent::TaskAssessed {
            turn_id: 1,
            task: Box::new(task_fixture(TaskOutcome::Partial)),
        },
    );
    assert!(ui.read(cx, |app, _| {
        app.session()
            .prompt_queue
            .error
            .as_deref()
            .unwrap()
            .contains("Couldn't save the prompt queue")
    }));
    std::fs::remove_dir(&path).unwrap();
    engine.send(
        cx,
        AgentEvent::TaskAssessed {
            turn_id: 1,
            task: Box::new(task_fixture(TaskOutcome::Partial)),
        },
    );
    assert!(ui.read(cx, |app, _| app.session().prompt_queue.error.is_none()));
    assert!(
        flint_app::prompt_queue::Queue::load(path.parent().unwrap())
            .unwrap()
            .review_hold
            .as_deref()
            .unwrap()
            .starts_with("Partial.")
    );
    assert!(kept.exists());
}

#[gpui_kit::test]
fn stopped_review_keeps_writer_ownership_after_undo_error_and_dismissal(cx: &mut TestAppContext) {
    let (ui, dir) = stopped_native_fixture(cx);
    show_task(&ui, cx);
    ui.click(cx, "task-accept");
    ui.with(cx, |window, cx| {
        ui.app.update(cx, |app, cx| {
            // Do not drive the background executor until all racing handlers ran.
            app.confirm_task_acceptance(cx);
            assert!(app.prompt_busy(app.active));
            app.undo_last_turn(cx);
            assert!(
                app.prompt_busy(app.active),
                "an unrelated idle error released the saved-history writer"
            );
            app.close_task_details(window, cx);
            app.composer.update(cx, |input, cx| {
                input.set_value("A competing prompt.", window, cx)
            });
            app.submit(window, cx);
            assert!(app.session().ops.is_none());
            assert!(!app.session().prompt_pending);
            let queued = app.session().prompt_queue.items.front().unwrap().clone();
            assert_eq!(queued.text, "A competing prompt.");
            app.remove_queued_prompt(queued.id, cx);
            assert!(app.prompt_busy(app.active));
            let count = app.sessions.len();
            app.delete_session(app.active, window, cx);
            assert_eq!(app.sessions.len(), count);
            assert!(
                app.store_error
                    .as_deref()
                    .unwrap()
                    .contains("finish saving")
            );
        })
    });
    assert!(dir.exists());
    settle(cx);
    assert!(!ui.read(cx, |app, _| app.prompt_busy(app.active)));
    assert_eq!(
        ui.read(cx, |app, _| app.session().view.outcome()),
        TaskOutcome::Accepted
    );
    assert!(
        flint_agent::saved_task(&dir)
            .unwrap()
            .unwrap()
            .1
            .is_accepted()
    );
}

#[gpui_kit::test]
fn retry_of_revised_readonly_task_preserves_the_continuation_operation(cx: &mut TestAppContext) {
    let (ui, engine) = finished_fixture(cx, TaskOutcome::NeedsReview);
    let mut task = task_fixture(TaskOutcome::NeedsReview);
    task.edit(TaskEdit {
        task_id: 1,
        expected_revision: 1,
        objective: "Inspect the methods without changing files.".into(),
        mode: TaskMode::ReadOnly,
        requirements: vec![RequirementSpec::Review(
            "Explain the missing methods.".into(),
        )],
        restrictions: vec!["Do not change files.".into()],
        clear_reported_blockers: false,
    })
    .unwrap();
    task.assess(TurnEndReason::Completed, true);
    engine.send(
        cx,
        AgentEvent::TaskAssessed {
            turn_id: 1,
            task: Box::new(task.clone()),
        },
    );
    show_task(&ui, cx);
    ui.click(cx, "task-continue");
    assert!(matches!(
        engine.sent().as_slice(),
        [Op::ContinueTask {
            task_id: 1,
            expected_revision: 2
        }]
    ));
    engine.send(cx, AgentEvent::TurnStarted { turn_id: 2 });
    engine.send(
        cx,
        AgentEvent::TaskUpdated {
            turn_id: 2,
            task: Box::new(task),
        },
    );
    engine.send(
        cx,
        AgentEvent::Error("The continuation's provider request failed.".into()),
    );
    engine.send(
        cx,
        AgentEvent::TurnFinished {
            turn_id: 2,
            reason: TurnEndReason::Failed("Provider request failed.".into()),
        },
    );
    ui.app.update(cx, |app, cx| app.retry(cx));
    assert!(matches!(
        engine.sent().as_slice(),
        [Op::ContinueTask {
            task_id: 1,
            expected_revision: 2
        }]
    ));
    ui.read(cx, |app, _| {
        let task = app.session().view.task.as_ref().unwrap();
        assert_eq!(task.contract.mode, TaskMode::ReadOnly);
        assert_eq!(task.contract.revision, 2);
    });
}

#[gpui_kit::test]
fn retry_of_image_only_prompt_does_not_promote_attachment_labels_to_user_scope(
    cx: &mut TestAppContext,
) {
    let ui = open(cx);
    let engine = ui.engine(cx);
    let image = temp_dir("retry-authority").join("read-only-required-file.png");
    std::fs::write(&image, b"\x89PNG\r\n\x1a\npixels").unwrap();
    ui.app.update(cx, |app, cx| {
        app.add_image_attachment(image);
        cx.notify();
    });
    ui.press(cx, "enter");
    let original = engine.sent();
    let [Op::UserMessageWithImages { text, images }] = original.as_slice() else {
        panic!("image-only prompt");
    };
    assert!(text.is_empty());
    let expected = images.clone();
    engine.send(cx, AgentEvent::Error("Could not send the image.".into()));
    ui.app.update(cx, |app, cx| app.retry(cx));
    let retried = engine.sent();
    let [Op::UserMessageWithImages { text, images }] = retried.as_slice() else {
        panic!("image-only retry");
    };
    assert!(
        text.is_empty(),
        "image filename became user-authorized task text"
    );
    assert_eq!(images, &expected);
}

#[gpui_kit::test]
fn failed_continuation_setup_retains_revised_scope_retry_instead_of_the_original_prompt(
    cx: &mut TestAppContext,
) {
    let (ui, dir) = stopped_native_fixture(cx);
    ui.app.update(cx, |app, _| {
        app.key_path = None;
        assert!(app.settings.resolve_key(None, &app.key_sources).is_none());
    });
    show_task(&ui, cx);
    ui.click(cx, "task-edit");
    ui.app.update(cx, |app, cx| {
        app.task_panel
            .as_mut()
            .unwrap()
            .draft
            .as_mut()
            .unwrap()
            .mode = TaskMode::ReadOnly;
        cx.notify();
    });
    ui.click(cx, "task-edit-save");
    settle(cx);
    assert_eq!(
        flint_agent::saved_task(&dir)
            .unwrap()
            .unwrap()
            .1
            .contract
            .revision,
        2
    );
    ui.click(cx, "task-continue");
    ui.read(cx, |app, _| {
        assert!(
            app.session().ops.is_none(),
            "credential-free fixture must not start inference"
        );
        assert!(matches!(
            &app.session().last_message,
            Some(flint_app::session::RetryRequest::Continue {
                task_id: 1,
                revision: 2
            })
        ));
    });
    ui.app.update(cx, |app, cx| app.retry(cx));
    let task = flint_agent::saved_task(&dir).unwrap().unwrap().1;
    assert_eq!(task.contract.revision, 2);
    assert_eq!(task.contract.mode, TaskMode::ReadOnly);
    ui.read(cx, |app, _| {
        assert!(matches!(
            &app.session().last_message,
            Some(flint_app::session::RetryRequest::Continue {
                task_id: 1,
                revision: 2
            })
        ))
    });
}

#[gpui_kit::test]
fn pending_native_restore_keeps_the_ui_responsive_and_refuses_competing_writers(
    cx: &mut TestAppContext,
) {
    let (ui, dir) = stopped_native_fixture(cx);
    let options = ui.read(cx, |app, _| Options {
        home: Some(app.home.clone()),
        workspace: Some(app.session().workspace.clone()),
        ..test_options()
    });
    let restored = open_unsettled(cx, options);
    restored.with(cx, |window, cx| {
        restored.app.update(cx, |app, cx| {
            let ix = app
                .sessions
                .iter()
                .position(|session| session.dir.as_ref() == Some(&dir))
                .unwrap();
            app.select_session(ix, window, cx);
            assert!(app.prompt_busy(ix));
            app.apply_event(
                ix,
                AgentEvent::Error("Unrelated startup information.".into()),
                cx,
            );
            assert!(app.prompt_busy(ix));
            app.open_current_task(window, cx);
            app.request_task_acceptance(cx);
            assert!(app.task_panel.as_ref().unwrap().confirm_accept.is_none());
            app.edit_task_scope(window, cx);
            assert!(app.task_panel.as_ref().unwrap().draft.is_none());
            app.close_task_details(window, cx);
            app.composer.update(cx, |input, cx| {
                input.set_value("Main-thread input still works.", window, cx)
            });
            app.submit(window, cx);
            assert!(app.session().ops.is_none());
            let count = app.sessions.len();
            app.delete_session(ix, window, cx);
            assert_eq!(app.sessions.len(), count);
        })
    });
    settle(cx);
    assert!(!restored.read(cx, |app, _| app.prompt_busy(app.active)));
    assert!(restored.composer_text(cx).is_empty());
    assert_eq!(
        restored.read(cx, |app, _| app
            .session()
            .prompt_queue
            .items
            .front()
            .unwrap()
            .text
            .clone()),
        "Main-thread input still works."
    );
    assert!(dir.exists());
}

#[gpui_kit::test]
fn delayed_native_restore_cannot_replace_a_changed_turn(cx: &mut TestAppContext) {
    let (ui, dir) = stopped_native_fixture(cx);
    let options = ui.read(cx, |app, _| Options {
        home: Some(app.home.clone()),
        workspace: Some(app.session().workspace.clone()),
        ..test_options()
    });
    let restored = open_unsettled(cx, options);
    let mut current = TaskRecord::new(42, "A different current task.".into(), 0);
    current.assess(TurnEndReason::Interrupted, false);
    restored.with(cx, |window, cx| {
        restored.app.update(cx, |app, cx| {
            let ix = app
                .sessions
                .iter()
                .position(|session| session.dir.as_ref() == Some(&dir))
                .unwrap();
            app.select_session(ix, window, cx);
            for event in [
                AgentEvent::TurnStarted { turn_id: 2 },
                AgentEvent::TaskUpdated {
                    turn_id: 2,
                    task: Box::new(current),
                },
                AgentEvent::TurnFinished {
                    turn_id: 2,
                    reason: TurnEndReason::Interrupted,
                },
            ] {
                app.apply_event(ix, event, cx);
            }
        })
    });
    settle(cx);
    restored.read(cx, |app, _| {
        assert_eq!(app.session().view.turn_id, 2);
        assert_eq!(app.session().view.task.as_ref().unwrap().contract.id, 42);
        assert!(!app.prompt_busy(app.active));
    });
}

#[gpui_kit::test]
fn hidden_composer_menus_cannot_run_undo_from_the_task_scope_editor(cx: &mut TestAppContext) {
    let (ui, engine) = finished_fixture(cx, TaskOutcome::NeedsReview);
    ui.press(cx, "cmd-l");
    ui.input(cx, "/undo");
    assert!(ui.read(cx, |app, _| app.slash.is_some()));
    show_task(&ui, cx);
    ui.click(cx, "task-edit");
    ui.press(cx, "enter");
    assert!(
        engine.sent().is_empty(),
        "the hidden slash command ran instead of inserting a line break"
    );
    assert!(ui.read(cx, |app, _| app.slash.is_none() && app.mention.is_none()));
    assert!(ui.read(cx, |app, _| {
        app.task_panel.as_ref().unwrap().draft.is_some()
    }));
}

#[gpui_kit::test]
fn failed_native_scope_save_retains_the_draft_until_a_matching_acknowledgment(
    cx: &mut TestAppContext,
) {
    let (ui, engine) = finished_fixture(cx, TaskOutcome::NeedsReview);
    show_task(&ui, cx);
    ui.click(cx, "task-edit");
    ui.press(cx, "cmd-a");
    ui.input(cx, "Keep this objective after a failed save.");
    ui.click(cx, "task-edit-save");
    let sent = engine.sent();
    let [Op::EditTask(edit)] = sent.as_slice() else {
        panic!("{sent:?}")
    };
    engine.send(
        cx,
        AgentEvent::Error("Couldn't save engine history.".into()),
    );
    assert!(has(&ui, cx, "task-sheet-error"));
    ui.read(cx, |app, cx| {
        assert_eq!(
            app.task_panel
                .as_ref()
                .unwrap()
                .draft
                .as_ref()
                .unwrap()
                .objective
                .read(cx)
                .value()
                .as_ref(),
            edit.objective
        );
    });
}

#[gpui_kit::test]
fn failed_stopped_scope_save_keeps_edits_for_a_retry(cx: &mut TestAppContext) {
    let (ui, dir) = stopped_native_fixture(cx);
    // Publication now uses unique temporaries. Block the actual destination
    // instead of assuming a fixed temporary filename.
    let history = dir.join("history.json");
    let kept = dir.join("history.fixture-backup");
    std::fs::rename(&history, &kept).unwrap();
    std::fs::create_dir(&history).unwrap();
    show_task(&ui, cx);
    ui.click(cx, "task-edit");
    ui.press(cx, "cmd-a");
    ui.input(cx, "Retain this stopped scope draft.");
    ui.click(cx, "task-edit-save");
    settle(cx);
    assert!(has(&ui, cx, "task-sheet-error"));
    ui.read(cx, |app, cx| {
        assert_eq!(
            app.task_panel
                .as_ref()
                .unwrap()
                .draft
                .as_ref()
                .unwrap()
                .objective
                .read(cx)
                .value()
                .as_ref(),
            "Retain this stopped scope draft."
        );
    });
    std::fs::remove_dir(&history).unwrap();
    std::fs::rename(&kept, &history).unwrap();
    ui.click(cx, "task-edit-save");
    settle(cx);
    assert_eq!(
        flint_agent::saved_task(&dir)
            .unwrap()
            .unwrap()
            .1
            .contract
            .objective,
        "Retain this stopped scope draft."
    );
    assert!(ui.read(cx, |app, _| {
        app.task_panel.as_ref().unwrap().draft.is_none()
    }));
}

#[gpui_kit::test]
fn failed_continuation_setup_exposes_the_settings_recovery_instead_of_hiding_it(
    cx: &mut TestAppContext,
) {
    let (ui, _) = stopped_native_fixture(cx);
    ui.app.update(cx, |app, _| app.key_path = None);
    show_task(&ui, cx);
    ui.click(cx, "task-continue");
    assert!(
        !has(&ui, cx, "task-sheet") || has(&ui, cx, "task-sheet-error"),
        "the modal hides the continuation setup error"
    );
    ui.press(cx, "escape");
    let error = ui.read(cx, |app, _| {
        app.session()
            .view
            .items
            .iter()
            .rposition(|item| matches!(item, Item::Error(message) if message.contains("key")))
            .expect("credential setup error")
    });
    assert!(ui.with(cx, |window, _| {
        window.try_find(("error-settings", error)).is_some()
    }));
}

#[gpui_kit::test]
fn a_verified_history_save_recovers_controls_and_survives_reopening(cx: &mut TestAppContext) {
    let (ui, engine) = finished_fixture(cx, TaskOutcome::NeedsReview);
    engine.send(
        cx,
        AgentEvent::SessionStopped {
            history_saved: false,
        },
    );
    show_task(&ui, cx);
    ui.click(cx, "task-edit");
    assert!(ui.read(cx, |app, _| {
        app.task_panel.as_ref().unwrap().draft.is_none()
    }));
    engine.send(
        cx,
        AgentEvent::SessionStopped {
            history_saved: true,
        },
    );
    ui.click(cx, "task-edit");
    assert!(ui.read(cx, |app, _| {
        app.task_panel.as_ref().unwrap().draft.is_some()
    }));
    let (home, workspace, dir) = ui.read(cx, |app, _| {
        (
            app.home.clone(),
            app.session().workspace.clone(),
            app.session().dir.clone().unwrap(),
        )
    });
    let task = ui.read(cx, |app, _| app.session().view.task.clone().unwrap());
    std::fs::write(
        dir.join("history.json"),
        serde_json::to_vec(&json!({"version":2,"turn_id":1,"messages":[],"task":task})).unwrap(),
    )
    .unwrap();
    let restored = open_with(
        cx,
        Options {
            home: Some(home),
            workspace: Some(workspace),
            ..test_options()
        },
    );
    restored.with(cx, |window, cx| {
        restored.app.update(cx, |app, cx| {
            let ix = app
                .sessions
                .iter()
                .position(|session| session.dir.as_ref() == Some(&dir))
                .unwrap();
            app.select_session(ix, window, cx);
            app.open_current_task(window, cx);
            app.edit_task_scope(window, cx);
            assert!(app.task_panel.as_ref().unwrap().draft.is_some());
        });
    });
}

#[gpui_kit::test]
fn a_replayed_turn_ahead_of_native_history_cannot_reuse_the_engine_counter(
    cx: &mut TestAppContext,
) {
    let (ui, dir) = stopped_native_fixture(cx);
    let mut later = TaskRecord::new(2, "Implement the later request.".into(), 9);
    later.assess(TurnEndReason::Interrupted, false);
    ui.app.update(cx, |app, cx| {
        let ix = app.active;
        for event in [
            AgentEvent::TurnStarted { turn_id: 2 },
            AgentEvent::TaskAssessed {
                turn_id: 2,
                task: Box::new(later),
            },
            AgentEvent::TurnFinished {
                turn_id: 2,
                reason: TurnEndReason::Interrupted,
            },
        ] {
            app.apply_event(ix, event, cx);
        }
    });
    let options = ui.read(cx, |app, _| Options {
        home: Some(app.home.clone()),
        workspace: Some(app.session().workspace.clone()),
        ..test_options()
    });
    let restored = open_with(cx, options);
    restored.with(cx, |window, cx| {
        restored.app.update(cx, |app, cx| {
            let ix = app
                .sessions
                .iter()
                .position(|session| session.dir.as_ref() == Some(&dir))
                .unwrap();
            app.select_session(ix, window, cx);
            app.open_current_task(window, cx);
            app.edit_task_scope(window, cx);
            assert!(
                app.task_panel.as_ref().unwrap().draft.is_none(),
                "lagging native history must not release current-turn controls"
            );
        });
    });
    assert!(restored.read(cx, |app,_| app.session().view.items.iter().any(|item|
        matches!(item, Item::Error(message) if message.contains("history") && message.contains("behind"))
    )));
    let kept = std::fs::read(dir.join("history.json")).unwrap();
    restored.with(cx, |window, cx| {
        restored.app.update(cx, |app, cx| {
            app.close_task_details(window, cx);
            app.composer.update(cx, |input, cx| {
                input.set_value("Do not reuse the saved counter.", window, cx)
            });
            app.submit(window, cx);
            assert!(app.session().ops.is_none());
            assert!(!app.session().prompt_pending);
            assert_eq!(app.session().view.turn_id, 2);
        })
    });
    assert_eq!(std::fs::read(dir.join("history.json")).unwrap(), kept);
}
