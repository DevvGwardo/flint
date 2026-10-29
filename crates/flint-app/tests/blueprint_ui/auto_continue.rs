use super::*;
use flint_agent::AgentKind;

fn fixture(cx: &mut TestAppContext, enabled: bool) -> (Ui, Engine) {
    let ui = open(cx);
    ui.app
        .update(cx, |app, _| app.settings.auto_continue_fixes = enabled);
    let engine = running_turn(&ui, cx);
    (ui, engine)
}

#[gpui_kit::test]
fn stop_pending_start_sends_interrupt_before_turn_started(cx: &mut TestAppContext) {
    let ui = open(cx);
    let engine = ui.engine(cx);
    ui.input(cx, "Create and verify result.txt.");
    ui.press(cx, "enter");
    assert!(matches!(engine.sent().as_slice(), [Op::UserMessage(_)]));
    assert!(ui.read(cx, |app, _| app.session().prompt_pending
        && !app.session().view.running));
    ui.app.update(cx, |app, cx| app.interrupt(cx));
    assert!(matches!(engine.sent().as_slice(), [Op::Interrupt]));
    assert!(ui.read(cx, |app, _| {
        app.session().auto_continue.paused_reason.is_some()
    }));
}

fn finish(engine: &Engine, cx: &mut TestAppContext, turn_id: u64, task: TaskRecord) {
    engine.send(
        cx,
        AgentEvent::TaskAssessed {
            turn_id,
            task: Box::new(task),
        },
    );
    engine.send(
        cx,
        AgentEvent::TurnFinished {
            turn_id,
            reason: TurnEndReason::Completed,
        },
    );
}

fn assert_continued(engine: &Engine) {
    assert!(matches!(
        engine.sent().as_slice(),
        [Op::ContinueTask {
            task_id: 1,
            expected_revision: 1
        }]
    ));
}

fn assert_review_requested(engine: &Engine) {
    assert!(matches!(
        engine.sent().as_slice(),
        [Op::AutomaticallyReviewTask {
            task_id: 1,
            expected_revision: 1,
            expected_work_revision: 7
        }]
    ));
}

fn scroll_automatic_settings(ui: &Ui, cx: &mut TestAppContext) {
    ui.with(cx, |window, cx| {
        window.scroll(
            "settings-content",
            ScrollDelta::Pixels(point(px(0.), px(-2_000.))),
            cx,
        );
    });
    cx.run_until_parked();
}

#[gpui_kit::test]
fn auto_continue_default_waits_and_opt_in_recovers_only_the_protected_task(
    cx: &mut TestAppContext,
) {
    for enabled in [false, true] {
        for outcome in [TaskOutcome::Partial, TaskOutcome::Blocked] {
            let (ui, engine) = fixture(cx, enabled);
            let permissions = ui.read(cx, |app, _| (app.approval, app.settings.sandbox));
            let task = task_fixture(outcome);
            let contract = task.contract.clone();
            let blockers = task.blockers.clone();
            finish(&engine, cx, 1, task);
            if enabled {
                assert_continued(&engine);
            } else {
                assert!(engine.sent().is_empty());
            }
            ui.read(cx, |app, _| {
                assert_eq!((app.approval, app.settings.sandbox), permissions);
                let task = app.session().view.task.as_ref().unwrap();
                assert_eq!(task.contract, contract);
                assert_eq!(task.blockers, blockers);
                assert!(!task.is_accepted());
                assert_eq!(app.session().prompt_pending, enabled);
                assert_eq!(app.session().auto_continue.attempts, u32::from(enabled));
            });
        }
    }
}

#[gpui_kit::test]
fn auto_continue_never_retries_verified_accepted_or_external_work(cx: &mut TestAppContext) {
    for outcome in [
        TaskOutcome::Verified,
        TaskOutcome::Accepted,
        TaskOutcome::Unknown,
    ] {
        let (_, engine) = fixture(cx, true);
        finish(&engine, cx, 1, task_fixture(outcome));
        assert!(engine.sent().is_empty(), "{outcome:?}");
    }
    for mode in [
        TaskMode::ReadOnly,
        TaskMode::Conversation,
        TaskMode::Unspecified,
    ] {
        let (_, engine) = fixture(cx, true);
        let mut task = task_fixture(TaskOutcome::Partial);
        task.contract.mode = mode;
        task.assess(TurnEndReason::Completed, true);
        finish(&engine, cx, 1, task);
        assert!(engine.sent().is_empty(), "{mode:?}");
    }
    let (ui, engine) = fixture(cx, true);
    ui.app.update(cx, |app, _| {
        app.sessions[app.active].agent = AgentKind::Droid
    });
    finish(&engine, cx, 1, task_fixture(TaskOutcome::Partial));
    assert!(engine.sent().is_empty());
    assert!(!has(&ui, cx, "auto-continue-toggle"));
}

#[gpui_kit::test]
fn auto_continue_reviews_ready_work_only_when_enabled(cx: &mut TestAppContext) {
    for enabled in [false, true] {
        let (ui, engine) = fixture(cx, enabled);
        finish(&engine, cx, 1, task_fixture(TaskOutcome::NeedsReview));
        if enabled {
            assert_review_requested(&engine);
        } else {
            assert!(engine.sent().is_empty());
        }
        ui.read(cx, |app, _| {
            assert_eq!(app.session().auto_continue.attempts, 0);
            assert_eq!(app.session().prompt_pending, enabled);
            assert!(!app.session().view.task.as_ref().unwrap().is_accepted());
        });
    }
}

#[gpui_kit::test]
fn auto_continue_stops_without_progress_and_records_why(cx: &mut TestAppContext) {
    let (ui, engine) = fixture(cx, true);
    finish(&engine, cx, 1, task_fixture(TaskOutcome::Partial));
    assert_continued(&engine);
    engine.send(cx, AgentEvent::TurnStarted { turn_id: 2 });
    finish(&engine, cx, 2, task_fixture(TaskOutcome::Partial));
    assert!(engine.sent().is_empty());
    ui.read(cx, |app, _| {
        assert_eq!(app.session().auto_continue.attempts, 1);
        assert!(
            app.session()
                .auto_continue
                .paused_reason
                .unwrap()
                .contains("No recovery progress")
        );
        assert!(!app.session().prompt_pending);
    });
}

#[gpui_kit::test]
fn auto_continue_is_bounded_even_when_each_attempt_observes_more_work(cx: &mut TestAppContext) {
    let (ui, engine) = fixture(cx, true);
    for turn_id in 1..=4 {
        if turn_id > 1 {
            engine.send(cx, AgentEvent::TurnStarted { turn_id });
        }
        let mut task = task_fixture(TaskOutcome::Partial);
        task.work_revision += turn_id;
        task.assess(TurnEndReason::Completed, true);
        finish(&engine, cx, turn_id, task);
        if turn_id <= 3 {
            assert_continued(&engine);
        } else {
            assert!(engine.sent().is_empty());
        }
    }
    ui.read(cx, |app, _| {
        assert_eq!(app.session().auto_continue.attempts, 3);
        assert!(
            app.session()
                .auto_continue
                .paused_reason
                .unwrap()
                .contains("three")
        );
    });
}

#[gpui_kit::test]
fn auto_continue_reviews_after_repair_without_claiming_personal_acceptance(
    cx: &mut TestAppContext,
) {
    let (ui, engine) = fixture(cx, true);
    finish(&engine, cx, 1, task_fixture(TaskOutcome::Partial));
    assert_continued(&engine);
    engine.send(cx, AgentEvent::TurnStarted { turn_id: 2 });
    finish(&engine, cx, 2, task_fixture(TaskOutcome::NeedsReview));
    assert_review_requested(&engine);
    engine.send(cx, AgentEvent::TurnStarted { turn_id: 3 });
    finish(&engine, cx, 3, task_fixture(TaskOutcome::Verified));
    assert!(engine.sent().is_empty());
    assert!(ui.read(cx, |app, _| {
        app.session().view.outcome() == TaskOutcome::Verified
            && !app.session().view.task.as_ref().unwrap().is_accepted()
    }));
}

#[gpui_kit::test]
fn auto_continue_preserves_pending_queue_until_current_work_is_verified(cx: &mut TestAppContext) {
    let (ui, engine) = fixture(cx, true);
    ui.app
        .update(cx, |app, _| app.settings.queue_after_verification = true);
    ui.input(cx, "Next queued task");
    ui.press(cx, "enter");
    finish(&engine, cx, 1, task_fixture(TaskOutcome::Partial));
    assert_continued(&engine);
    ui.read(cx, |app, _| {
        assert_eq!(app.session().prompt_queue.items.len(), 1);
        assert!(app.session().prompt_queue.paused);
    });
    engine.send(cx, AgentEvent::TurnStarted { turn_id: 2 });
    finish(&engine, cx, 2, task_fixture(TaskOutcome::NeedsReview));
    assert_review_requested(&engine);
    assert!(ui.read(cx, |app, _| {
        app.session().prompt_queue.items.len() == 1 && app.session().prompt_queue.paused
    }));
    engine.send(cx, AgentEvent::TurnStarted { turn_id: 3 });
    finish(&engine, cx, 3, task_fixture(TaskOutcome::Verified));
    assert!(
        matches!(engine.sent().as_slice(), [Op::UserMessage(text)] if text == "Next queued task")
    );
    assert!(ui.read(cx, |app, _| {
        !app.session().view.task.as_ref().unwrap().is_accepted()
    }));
}

#[gpui_kit::test]
fn auto_continue_respects_stop_errors_pending_approvals_and_storage_failures(
    cx: &mut TestAppContext,
) {
    for hold in [
        "stop",
        "error",
        "approval",
        "denied",
        "unreadable",
        "stopping",
        "queue-save",
    ] {
        let (ui, engine) = fixture(cx, true);
        match hold {
            "stop" => {
                ui.app.update(cx, |app, cx| app.interrupt(cx));
                assert!(matches!(engine.sent().as_slice(), [Op::Interrupt]));
            }
            "error" => engine.send(cx, AgentEvent::Error("Provider failed".into())),
            "approval" => approval_requested(&engine, cx, "auto-fix-permission"),
            "denied" => {
                approval_requested(&engine, cx, "denied-fix");
                ui.app.update(cx, |app, cx| {
                    app.answer_approval("denied-fix".into(), ApprovalDecision::Deny, cx);
                });
                assert!(matches!(
                    engine.sent().as_slice(),
                    [Op::Approval {
                        decision: ApprovalDecision::Deny,
                        ..
                    }]
                ));
            }
            "unreadable" => ui.app.update(cx, |app, _| {
                app.sessions[app.active].prompt_queue.unreadable = true
            }),
            "stopping" => ui
                .app
                .update(cx, |app, _| app.sessions[app.active].stopping = true),
            "queue-save" => {
                let path = ui.read(cx, |app, _| {
                    app.session()
                        .dir
                        .as_ref()
                        .unwrap()
                        .join("prompt-queue.json")
                });
                std::fs::create_dir(path).unwrap();
            }
            _ => unreachable!(),
        }
        finish(&engine, cx, 1, task_fixture(TaskOutcome::Partial));
        assert!(engine.sent().is_empty(), "{hold}");
    }
}

#[gpui_kit::test]
fn auto_continue_does_not_override_failure_or_existing_harness_limits(cx: &mut TestAppContext) {
    for reason in [
        TurnEndReason::Interrupted,
        TurnEndReason::Failed("Provider failure".into()),
        TurnEndReason::StepLimit,
        TurnEndReason::ContinuationLimit,
    ] {
        let (_, engine) = fixture(cx, true);
        engine.send(
            cx,
            AgentEvent::TaskAssessed {
                turn_id: 1,
                task: Box::new(task_fixture(TaskOutcome::Partial)),
            },
        );
        engine.send(cx, AgentEvent::TurnFinished { turn_id: 1, reason });
        assert!(engine.sent().is_empty());
    }
    for hold in [
        "user-review",
        "scope-denial",
        "harness-limit",
        "overflow",
        "stale-assessment",
    ] {
        let (_, engine) = fixture(cx, true);
        let mut task = task_fixture(TaskOutcome::Blocked);
        match hold {
            "user-review" => task.add_blocker("A user decision is required.", "user review"),
            "scope-denial" => task.scope_denials.push(flint_agent::ScopeDenial {
                contract_revision: 1,
                tool: "write_file".into(),
                target: "protected.txt".into(),
            }),
            "harness-limit" => {
                task.continuation_stop = Some(flint_agent::ContinuationStop {
                    cause: flint_agent::ContinuationStopCause::NoProgress,
                    followups: 1,
                    condition: "No progress".into(),
                })
            }
            "overflow" => task.evidence_overflow = true,
            "stale-assessment" => task.contract.revision += 1,
            _ => unreachable!(),
        }
        finish(&engine, cx, 1, task);
        assert!(engine.sent().is_empty(), "{hold}");
    }
}

#[gpui_kit::test]
fn auto_continue_toggle_persists_without_starting_idle_or_restored_work(cx: &mut TestAppContext) {
    let (ui, engine) = fixture(cx, false);
    finish(&engine, cx, 1, task_fixture(TaskOutcome::Partial));
    ui.click(cx, "auto-continue-toggle");
    assert!(engine.sent().is_empty());
    let home = ui.read(cx, |app, _| app.home.clone());
    assert!(flint_app::settings::Settings::load(&home, &KeySources::none()).auto_continue_fixes);
    let mut options = test_options();
    options.home = Some(home);
    let restarted = open_with(cx, options);
    assert!(restarted.read(cx, |app, _| {
        app.settings.auto_continue_fixes
            && !app.session().prompt_pending
            && app.session().auto_continue.attempts == 0
    }));
    ui.click(cx, "auto-continue-toggle");
    assert!(!ui.read(cx, |app, _| app.settings.auto_continue_fixes));
}

#[gpui_kit::test]
fn auto_continue_settings_cancel_and_failed_save_do_not_enable_recovery(cx: &mut TestAppContext) {
    let (ui, engine) = fixture(cx, false);
    ui.press(cx, "secondary-,");
    scroll_automatic_settings(&ui, cx);
    ui.click(cx, "settings-auto-continue");
    ui.click(cx, "settings-cancel");
    assert!(!ui.read(cx, |app, _| app.settings.auto_continue_fixes));
    ui.press(cx, "secondary-,");
    scroll_automatic_settings(&ui, cx);
    ui.click(cx, "settings-auto-continue");
    let home = ui.read(cx, |app, _| app.home.clone());
    std::fs::create_dir(home.join("config.toml.tmp")).unwrap();
    ui.click(cx, "settings-save");
    assert!(!ui.read(cx, |app, _| app.settings.auto_continue_fixes));
    finish(&engine, cx, 1, task_fixture(TaskOutcome::Partial));
    assert!(engine.sent().is_empty());
}

#[gpui_kit::test]
fn auto_continue_duplicate_finish_and_late_assessment_do_not_send_extra_turns(
    cx: &mut TestAppContext,
) {
    let (ui, engine) = fixture(cx, true);
    finish(&engine, cx, 1, task_fixture(TaskOutcome::Partial));
    assert_continued(&engine);
    finish(&engine, cx, 1, task_fixture(TaskOutcome::Partial));
    assert!(engine.sent().is_empty());
    assert_eq!(
        ui.read(cx, |app, _| app.session().auto_continue.attempts),
        1
    );
}

#[gpui_kit::test]
fn auto_continue_saved_settings_enable_future_turns_without_changing_permissions(
    cx: &mut TestAppContext,
) {
    let (ui, engine) = fixture(cx, false);
    let permissions = ui.read(cx, |app, _| (app.approval, app.settings.sandbox));
    ui.press(cx, "secondary-,");
    scroll_automatic_settings(&ui, cx);
    ui.click(cx, "settings-auto-continue");
    ui.click(cx, "settings-save");
    ui.read(cx, |app, _| {
        assert!(app.settings.auto_continue_fixes);
        assert_eq!((app.approval, app.settings.sandbox), permissions);
        assert!(
            flint_app::settings::Settings::load(&app.home, &KeySources::none()).auto_continue_fixes
        );
    });
    assert!(engine.sent().is_empty());
    finish(&engine, cx, 1, task_fixture(TaskOutcome::Partial));
    assert_continued(&engine);
}

#[gpui_kit::test]
fn auto_continue_turning_off_stops_future_followups_without_interrupting_current_work(
    cx: &mut TestAppContext,
) {
    let (ui, engine) = fixture(cx, true);
    finish(&engine, cx, 1, task_fixture(TaskOutcome::Partial));
    assert_continued(&engine);
    engine.send(cx, AgentEvent::TurnStarted { turn_id: 2 });
    ui.click(cx, "auto-continue-toggle");
    assert!(engine.sent().is_empty());
    let mut task = task_fixture(TaskOutcome::Partial);
    task.work_revision += 1;
    task.assess(TurnEndReason::Completed, true);
    finish(&engine, cx, 2, task);
    assert!(engine.sent().is_empty());
    ui.click(cx, "auto-continue-toggle");
    assert!(engine.sent().is_empty());
    assert!(ui.read(cx, |app, _| {
        app.session().auto_continue.paused_reason.is_some()
    }));
}

#[gpui_kit::test]
fn auto_continue_manual_task_continuation_resets_the_paused_recovery_budget(
    cx: &mut TestAppContext,
) {
    let (ui, engine) = fixture(cx, true);
    finish(&engine, cx, 1, task_fixture(TaskOutcome::Partial));
    assert_continued(&engine);
    engine.send(cx, AgentEvent::TurnStarted { turn_id: 2 });
    finish(&engine, cx, 2, task_fixture(TaskOutcome::Partial));
    assert!(engine.sent().is_empty());
    show_task(&ui, cx);
    ui.click(cx, "task-continue");
    assert_continued(&engine);
    assert!(ui.read(cx, |app, _| {
        app.session().auto_continue.attempts == 0
            && app.session().auto_continue.paused_reason.is_none()
    }));
    engine.send(cx, AgentEvent::TurnStarted { turn_id: 3 });
    finish(&engine, cx, 3, task_fixture(TaskOutcome::Partial));
    assert_continued(&engine);
}
