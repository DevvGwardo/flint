use super::*;
#[path = "auto_continue.rs"]
mod auto_continue;
#[path = "task_review_boundaries.rs"]
mod review_boundaries;
use flint_agent::{
    AutomaticReview, AutomaticReviewVerdict, CommandEvidence, FileEvidence, FileState,
    ReviewDecision, TaskMode, TaskOutcome, TaskRecord, TaskReview, TaskSource,
};

pub(super) fn task_fixture(outcome: TaskOutcome) -> TaskRecord {
    if outcome == TaskOutcome::Unknown {
        return TaskRecord::unverified(
            1,
            "Recover actual method bodies, not just metadata.".into(),
            TurnEndReason::Completed,
            TaskSource::External,
        );
    }
    let mut task = TaskRecord::new(
        1,
        "Decompile the input.\nRequired file: method-bodies.c\n\
        Required check: python3 validate_metadata.py\n\
        Acceptance: Recover actual methods, not only class metadata."
            .into(),
        7,
    );
    task.observed_edits = 1;
    task.files = vec![
        FileEvidence {
            path: "metadata.json".into(),
            state: FileState::Present {
                bytes: 412,
                fingerprint: 11,
            },
            reported: true,
        },
        FileEvidence {
            path: "method-bodies.c".into(),
            state: if outcome == TaskOutcome::Partial {
                FileState::Missing
            } else {
                FileState::Present {
                    bytes: 912,
                    fingerprint: 12,
                }
            },
            reported: false,
        },
    ];
    task.checks.push(CommandEvidence {
        id: 1,
        call_id: "metadata-check".into(),
        command: "python3 validate_metadata.py".into(),
        contract_revision: 1,
        work_revision: 7,
        started_revision: 7,
        success: true,
        exit_code: Some(0),
        detail: "Metadata schema validated. No method-content check.".into(),
        verification: true,
    });
    if outcome == TaskOutcome::Blocked {
        task.add_blocker("The input binary is unavailable.", "agent report");
    }
    if outcome == TaskOutcome::Verified {
        task.automatic_review = Some(AutomaticReview {
            contract_revision: task.contract.revision,
            work_revision: task.work_revision,
            model: "fixture".into(),
            verdict: AutomaticReviewVerdict::Clean,
            summary: "Inspected the method bodies against all protected requirements.".into(),
            findings: Vec::new(),
            reviewed_requirements: task
                .contract
                .requirements
                .iter()
                .map(|requirement| requirement.id)
                .collect(),
            inspected_files: task.files.clone(),
        });
    }
    task.assess(TurnEndReason::Completed, true);
    if outcome == TaskOutcome::Accepted {
        task.review(&TaskReview {
            turn_id: 1,
            task_id: 1,
            contract_revision: 1,
            work_revision: 7,
            decision: ReviewDecision::Accept,
        })
        .unwrap();
    }
    assert_eq!(task.assessment.as_ref().unwrap().outcome, outcome);
    task
}

#[gpui_kit::test]
fn completed_turn_labels_keep_execution_separate_from_task_acceptance(cx: &mut TestAppContext) {
    for outcome in [
        TaskOutcome::Partial,
        TaskOutcome::Blocked,
        TaskOutcome::Unknown,
        TaskOutcome::NeedsReview,
        TaskOutcome::Verified,
        TaskOutcome::Accepted,
    ] {
        let (ui, _) = finished_fixture(cx, outcome);
        let end = ui.read(cx, |app, _| app.session().view.turns[0].end.unwrap());
        ui.with(cx, |window, cx| {
            window.render_frame(cx);
            assert_eq!(
                window.find(("turn-ended", end)).label(),
                Some(if outcome.allows_queue() {
                    "Done"
                } else {
                    "Turn ended"
                }),
                "{outcome:?}"
            );
            assert!(window.try_find(("task-outcome", end)).is_some());
        });
        show_task(&ui, cx);
        assert!(has(&ui, cx, "task-checks"));
        assert!(has(&ui, cx, "task-deliverables"));
    }
}

#[gpui_kit::test]
fn continuation_stop_labels_distinguish_no_progress_budget_and_legacy_logs(
    cx: &mut TestAppContext,
) {
    for cause in [
        Some(flint_agent::ContinuationStopCause::NoProgress),
        Some(flint_agent::ContinuationStopCause::BudgetExhausted),
        None,
    ] {
        let ui = open(cx);
        let engine = ui.engine(cx);
        ui.input(cx, "Create and verify a result.");
        ui.press(cx, "enter");
        engine.sent();
        engine.send(cx, AgentEvent::TurnStarted { turn_id: 1 });
        engine.send(
            cx,
            AgentEvent::TextDelta("Verification remains unresolved.".into()),
        );
        let mut task = TaskRecord::new(1, "Create and verify a result.".into(), 0);
        task.observed_edits = 1;
        task.continuation_stop = cause.map(|cause| flint_agent::ContinuationStop {
            cause,
            followups: if cause == flint_agent::ContinuationStopCause::NoProgress {
                1
            } else {
                4
            },
            condition: "Earlier checks became stale after dependency setup.".into(),
        });
        task.assess(TurnEndReason::ContinuationLimit, true);
        engine.send(
            cx,
            AgentEvent::TaskAssessed {
                turn_id: 1,
                task: Box::new(task),
            },
        );
        engine.send(
            cx,
            AgentEvent::TurnFinished {
                turn_id: 1,
                reason: TurnEndReason::ContinuationLimit,
            },
        );
        let end = ui.read(cx, |app, _| app.session().view.turns[0].end.unwrap());
        ui.with(cx, |window, cx| {
            window.render_frame(cx);
            assert_eq!(
                window.find(("turn-ended", end)).label(),
                Some(match cause {
                    Some(flint_agent::ContinuationStopCause::NoProgress) =>
                        "Stopped without recovery progress",
                    Some(flint_agent::ContinuationStopCause::BudgetExhausted) =>
                        "Continuation budget exhausted",
                    None => "Automatic follow-ups stopped",
                })
            );
        });
        show_task(&ui, cx);
        assert!(ui.read(cx, |app, _| {
            !app.task_for_panel().unwrap().verified_for_queue()
        }));
    }
}

fn finished_fixture(cx: &mut TestAppContext, outcome: TaskOutcome) -> (Ui, Engine) {
    let ui = open(cx);
    let engine = ui.engine(cx);
    ui.input(cx, "Recover the requested method bodies.");
    ui.press(cx, "enter");
    assert!(matches!(engine.sent().as_slice(), [Op::UserMessage(_)]));
    engine.send(cx, AgentEvent::TurnStarted { turn_id: 1 });
    engine.send(
        cx,
        AgentEvent::TextDelta("Metadata was extracted. Review the remaining requirements.".into()),
    );
    engine.send(
        cx,
        AgentEvent::TaskAssessed {
            turn_id: 1,
            task: Box::new(task_fixture(outcome)),
        },
    );
    engine.send(
        cx,
        AgentEvent::TurnFinished {
            turn_id: 1,
            reason: TurnEndReason::Completed,
        },
    );
    (ui, engine)
}

#[gpui_kit::test]
fn readonly_review_shows_analysis_ready_and_unapproved_next_steps(cx: &mut TestAppContext) {
    let (ui, engine) = finished_fixture(cx, TaskOutcome::NeedsReview);
    let mut task = TaskRecord::new(1, "Analyze the gaps.".into(), 0);
    task.report(&serde_json::json!({
        "summary":"Accessibility can be improved.", "deliverables":[], "blockers":[],
        "proposed_actions":["Add reduced-motion support and run tests."]
    }))
    .unwrap();
    task.assess(TurnEndReason::Completed, true);
    engine.send(
        cx,
        AgentEvent::TaskAssessed {
            turn_id: 1,
            task: Box::new(task),
        },
    );
    let end = ui.read(cx, |app, _| app.session().view.turns[0].end.unwrap());
    ui.with(cx, |window, cx| {
        window.render_frame(cx);
        assert_eq!(
            window.find(("task-outcome", end)).label(),
            Some("Analysis ready")
        );
        assert_eq!(window.find(("turn-ended", end)).label(), Some("Turn ended"));
    });
    show_task(&ui, cx);
    assert!(has(&ui, cx, "task-proposed-actions"));
    assert!(!has(&ui, cx, "task-blockers"));
    assert!(!ui.read(cx, |app, _| app.task_for_panel().unwrap().is_accepted()));
}
fn show_task(ui: &Ui, cx: &mut TestAppContext) {
    ui.click(cx, "task-details-current");
    assert!(ui.read(cx, |app, _| app.task_panel.is_some()));
    assert!(has(ui, cx, "task-sheet"));
}

fn queue_prompt(ui: &Ui, cx: &mut TestAppContext) {
    ui.app.update(cx, |app, cx| {
        app.sessions[app.active]
            .prompt_queue
            .enqueue(flint_app::prompt_queue::Prompt::new(
                "Next task".into(),
                "Next task".into(),
                Vec::new(),
            ))
            .unwrap();
        app.sessions[app.active].prompt_queue.paused = true;
        cx.notify();
    });
}

#[gpui_kit::test]
fn task_details_expose_missing_deliverables_without_conflating_metadata_checks(
    cx: &mut TestAppContext,
) {
    let (ui, engine) = finished_fixture(cx, TaskOutcome::Partial);
    show_task(&ui, cx);
    assert!(has(&ui, cx, "task-goal"));
    assert!(has(&ui, cx, "task-checks"));
    assert!(has(&ui, cx, "task-deliverables"));
    ui.read(cx, |app, _| {
        let task = app.task_for_panel().unwrap();
        assert_eq!(
            task.assessment.as_ref().unwrap().outcome,
            TaskOutcome::Partial
        );
        assert_eq!(
            task.assessment.as_ref().unwrap().requirements[1].state,
            flint_agent::EvidenceState::Missing
        );
        assert_eq!(
            task.checks[0].state(1, 7),
            flint_agent::EvidenceState::Passed
        );
        assert!(!task.is_accepted());
    });
    assert!(engine.sent().is_empty());
}

#[gpui_kit::test]
fn task_acceptance_requires_confirmation_and_waits_for_native_acknowledgement(
    cx: &mut TestAppContext,
) {
    let (ui, engine) = finished_fixture(cx, TaskOutcome::NeedsReview);
    show_task(&ui, cx);
    ui.click(cx, "task-accept");
    assert!(has(&ui, cx, "task-accept-confirm"));
    assert!(engine.sent().is_empty());
    ui.click(cx, "task-accept-cancel");
    assert!(engine.sent().is_empty());
    ui.click(cx, "task-accept");
    ui.click(cx, "task-accept-confirm");
    let sent = engine.sent();
    let [Op::ReviewTask(review)] = sent.as_slice() else {
        panic!("review operation: {sent:?}")
    };
    assert_eq!(review.turn_id, 1);
    assert_eq!(review.decision, ReviewDecision::Accept);
    assert_eq!(
        ui.read(cx, |app, _| app.session().view.outcome()),
        TaskOutcome::NeedsReview
    );
    assert!(ui.read(cx, |app, _| {
        app.task_panel.as_ref().unwrap().pending_review.is_some()
    }));
    let mut accepted = task_fixture(TaskOutcome::NeedsReview);
    accepted.review(review).unwrap();
    engine.send(
        cx,
        AgentEvent::TaskAssessed {
            turn_id: 1,
            task: Box::new(accepted),
        },
    );
    assert_eq!(
        ui.read(cx, |app, _| app.session().view.outcome()),
        TaskOutcome::Accepted
    );
    assert!(ui.read(cx, |app, _| {
        app.task_panel.as_ref().unwrap().pending_review.is_none()
    }));
}

#[gpui_kit::test]
fn task_scope_editor_keeps_original_goal_and_sends_an_explicit_guarded_revision(
    cx: &mut TestAppContext,
) {
    let (ui, engine) = finished_fixture(cx, TaskOutcome::Blocked);
    show_task(&ui, cx);
    ui.click(cx, "task-edit");
    ui.press(cx, "cmd-a");
    ui.input(cx, "Recover metadata only, by user decision.");
    assert_eq!(
        ui.read(cx, |app, cx| app
            .task_panel
            .as_ref()
            .unwrap()
            .draft
            .as_ref()
            .unwrap()
            .objective
            .read(cx)
            .value()
            .to_string()),
        "Recover metadata only, by user decision."
    );
    ui.click(cx, ("task-mode", 2usize));
    ui.click(cx, "task-clear-blockers");
    ui.click(cx, "task-edit-save");
    let sent = engine.sent();
    let [Op::EditTask(edit)] = sent.as_slice() else {
        panic!("scope operation: {sent:?}")
    };
    assert_eq!(edit.mode, TaskMode::ReadOnly);
    assert_eq!(edit.expected_revision, 1);
    assert!(edit.clear_reported_blockers);
    assert_eq!(
        ui.read(cx, |app, _| app
            .session()
            .view
            .task
            .as_ref()
            .unwrap()
            .contract
            .revision),
        1
    );
    let mut task = task_fixture(TaskOutcome::Blocked);
    task.edit(edit.clone()).unwrap();
    task.assess(TurnEndReason::Completed, true);
    engine.send(
        cx,
        AgentEvent::TaskUpdated {
            turn_id: 1,
            task: Box::new(task),
        },
    );
    ui.read(cx, |app, _| {
        let task = app.task_for_panel().unwrap();
        assert_eq!(task.contract.revision, 2);
        assert!(task.contract.original_request.contains("actual methods"));
        assert_eq!(task.contract.objective, edit.objective);
        assert!(task.blockers.is_empty());
    });
}

#[gpui_kit::test]
fn task_scope_editor_rejects_unknown_requirement_prefixes_without_dropping_them(
    cx: &mut TestAppContext,
) {
    let (ui, engine) = finished_fixture(cx, TaskOutcome::NeedsReview);
    show_task(&ui, cx);
    ui.click(cx, "task-edit");
    ui.with(cx, |window, cx| {
        ui.app.update(cx, |app, cx| {
            let input = app
                .task_panel
                .as_ref()
                .unwrap()
                .draft
                .as_ref()
                .unwrap()
                .requirements
                .clone();
            input.update(cx, |input, cx| {
                input.set_value("drop: important requirement", window, cx)
            });
        })
    });
    ui.click(cx, "task-edit-save");
    assert!(has(&ui, cx, "task-sheet-error"));
    assert!(ui.read(cx, |app, _| {
        app.task_panel.as_ref().unwrap().draft.is_some()
    }));
    assert!(engine.sent().is_empty());
}

#[gpui_kit::test]
fn task_continue_uses_the_native_contract_instead_of_replacing_it_with_a_short_prompt(
    cx: &mut TestAppContext,
) {
    let (ui, engine) = finished_fixture(cx, TaskOutcome::Partial);
    show_task(&ui, cx);
    ui.click(cx, "task-continue");
    assert!(matches!(
        engine.sent().as_slice(),
        [Op::ContinueTask {
            task_id: 1,
            expected_revision: 1
        }]
    ));
    assert!(!has(&ui, cx, "task-sheet"));
    assert!(ui.read(cx, |app, _| app.session().prompt_pending));
    assert!(ui.read(cx, |app, _| {
        app.session()
            .view
            .task
            .as_ref()
            .unwrap()
            .contract
            .original_request
            .contains("actual methods")
    }));
}

#[gpui_kit::test]
fn task_sheet_contains_tab_focus_and_shortcuts_but_preserves_text_editing(cx: &mut TestAppContext) {
    let (ui, engine) = finished_fixture(cx, TaskOutcome::NeedsReview);
    show_task(&ui, cx);
    let sessions = ui.read(cx, |app, _| app.sessions.len());
    for key in ["cmd-n", "cmd-shift-p", "cmd-j", "cmd-.", "cmd-,", "ctrl-`"] {
        ui.press(cx, key);
        assert_eq!(
            ui.read(cx, |app, _| app.sessions.len()),
            sessions,
            "shortcut {key}"
        );
    }
    assert_eq!(ui.read(cx, |app, _| app.sessions.len()), sessions);
    assert!(engine.sent().is_empty());
    assert!(has(&ui, cx, "task-sheet"));
    for key in ["tab", "shift-tab"] {
        for _ in 0..8 {
            ui.press(cx, key);
            assert!(ui.with(cx, |window, cx| {
                ui.app
                    .read(cx)
                    .task_panel
                    .as_ref()
                    .unwrap()
                    .focus
                    .contains_focused(window, cx)
            }));
        }
    }
    ui.click(cx, "task-edit");
    ui.press(cx, "cmd-a");
    ui.input(cx, "A deliberate goal edit.");
    ui.press(cx, "escape");
    assert!(has(&ui, cx, "task-sheet"));
    assert!(ui.read(cx, |app, _| {
        app.task_panel.as_ref().unwrap().draft.is_none()
    }));
    ui.press(cx, "escape");
    assert!(!has(&ui, cx, "task-sheet"));
}

#[gpui_kit::test]
fn task_sheet_scrolls_long_evidence_and_keeps_actions_inside_narrow_windows(
    cx: &mut TestAppContext,
) {
    for (width, height) in [(1440., 900.), (900., 560.), (480., 480.)] {
        let (ui, _) = finished_fixture(cx, TaskOutcome::Partial);
        cx.simulate_window_resize(ui.window, size(px(width), px(height)));
        show_task(&ui, cx);
        ui.with(cx, |window, cx| {
            window.render_frame(cx);
            let sheet = window.find("task-sheet").bounds();
            let content = window.find("task-sheet-content").bounds();
            let footer = window.find("task-sheet-footer").bounds();
            assert!(sheet.left() >= px(0.) && sheet.right() <= px(width));
            assert!(sheet.top() >= px(0.) && sheet.bottom() <= px(height));
            assert!(
                content.size.height > px(40.) && content.bottom() <= footer.top(),
                "{width}x{height}: content {content:?}, footer {footer:?}"
            );
            for id in ["task-continue", "task-edit", "task-accept"] {
                let button = window.find(id).bounds();
                assert!(
                    button.left() >= sheet.left() && button.right() <= sheet.right(),
                    "{id} {button:?}"
                );
                assert!(
                    button.top() >= sheet.top() && button.bottom() <= sheet.bottom(),
                    "{id} {button:?}"
                );
            }
            window.scroll(
                "task-sheet-content",
                ScrollDelta::Pixels(point(px(0.), px(-900.))),
                cx,
            );
        });
        assert!(ui.read(
            cx,
            |app, _| app.task_panel.as_ref().unwrap().scroll.offset().y < px(0.)
        ));
    }
}

#[gpui_kit::test]
fn unverified_and_partial_turns_pause_queued_work_even_after_being_read(cx: &mut TestAppContext) {
    for outcome in [
        TaskOutcome::Unknown,
        TaskOutcome::Partial,
        TaskOutcome::Blocked,
    ] {
        let (ui, engine) = finished_fixture(cx, outcome);
        queue_prompt(&ui, cx);
        ui.app
            .update(cx, |app, _| app.sessions[app.active].unread = false);
        ui.click(cx, "queue-pause");
        assert!(engine.sent().is_empty());
        assert!(ui.read(cx, |app, _| app.queue_resume_confirm.is_some()));
        assert!(!ui.read(cx, |app, _| app.session().view.outcome().allows_queue()));
        assert_eq!(
            ui.read(cx, |app, _| app.session().status().bucket()),
            flint_app::session::Bucket::NeedsYou
        );
    }
}

#[gpui_kit::test]
fn queue_override_has_a_cancelled_confirmation_and_never_accepts_the_old_task(
    cx: &mut TestAppContext,
) {
    let (ui, engine) = finished_fixture(cx, TaskOutcome::Partial);
    queue_prompt(&ui, cx);
    ui.click(cx, "queue-pause");
    assert!(has(&ui, cx, "queue-resume-confirmation"));
    ui.click(cx, "queue-resume-cancel");
    assert!(engine.sent().is_empty());
    assert_eq!(
        ui.read(cx, |app, _| app.session().prompt_queue.items.len()),
        1
    );
    ui.click(cx, "queue-pause");
    ui.click(cx, "queue-resume-confirm");
    assert!(matches!(engine.sent().as_slice(),[Op::UserMessage(text)] if text=="Next task"));
    assert!(!ui.read(cx, |app, _| {
        app.session().view.task.as_ref().unwrap().is_accepted()
    }));
    assert!(ui.read(cx, |app, _| app.session().prompt_queue.items.is_empty()));
}

#[gpui_kit::test]
fn changed_scope_invalidates_both_acceptance_and_queue_override_confirmations(
    cx: &mut TestAppContext,
) {
    let (ui, engine) = finished_fixture(cx, TaskOutcome::Partial);
    queue_prompt(&ui, cx);
    ui.click(cx, "queue-pause");
    let old_confirmation = ui.read(cx, |app, _| {
        app.queue_resume_confirm.as_ref().unwrap().key.clone()
    });
    show_task(&ui, cx);
    ui.click(cx, "task-accept");
    let mut task = task_fixture(TaskOutcome::Partial);
    task.set_work_revision(8);
    task.assess(TurnEndReason::Completed, true);
    engine.send(
        cx,
        AgentEvent::TaskUpdated {
            turn_id: 1,
            task: Box::new(task),
        },
    );
    assert!(!has(&ui, cx, "task-accept-confirm"));
    ui.app.update(cx, |app, cx| {
        app.queue_resume_confirm = Some(flint_app::task_ui::QueueConfirmation {
            key: old_confirmation,
            prompt_id: None,
        });
        app.confirm_queue_override(cx);
    });
    assert!(engine.sent().is_empty());
    assert_eq!(
        ui.read(cx, |app, _| app.session().prompt_queue.items.len()),
        1
    );
}

#[gpui_kit::test]
fn held_queue_scrolls_rows_beneath_visible_review_and_confirmation_controls(
    cx: &mut TestAppContext,
) {
    for (width, height) in [(1440., 900.), (900., 560.), (700., 480.)] {
        let (ui, engine) = finished_fixture(cx, TaskOutcome::Partial);
        cx.simulate_window_resize(ui.window, size(px(width), px(height)));
        ui.app.update(cx, |app, cx| {
            app.sidebar_visible = false;
            let ix = app.active;
            let queue = &mut app.sessions[ix].prompt_queue;
            for number in 0..16 {
                queue
                    .enqueue(flint_app::prompt_queue::Prompt::new(
                        format!("Next task {number}"),
                        format!("Next task {number}"),
                        Vec::new(),
                    ))
                    .unwrap();
            }
            queue.paused = true;
            queue.review_hold = Some("Partial. Review the task before running queued work.".into());
            app.queue_popover = true;
            app.request_queue_override(None, cx);
            cx.notify();
        });
        settle(cx);
        ui.with(cx, |window, cx| {
            for _ in 0..3 {
                window.render_frame(cx);
            }
            let queue = window.find("prompt-queue").bounds();
            let rows = window.find("queue-rows").bounds();
            let confirmation = window.find("queue-resume-confirmation").bounds();
            assert!(
                rows.top() >= confirmation.bottom(),
                "{rows:?}, {confirmation:?}"
            );
            assert!(rows.bottom() <= queue.bottom(), "{rows:?}, {queue:?}");
            assert!(
                rows.size.height > px(25.),
                "no usable row viewport at {width}x{height}: {rows:?}"
            );
            for id in [
                "queue-review-task",
                "queue-resume-confirm",
                "queue-resume-cancel",
            ] {
                let button = window.find(id).bounds();
                assert!(
                    button.left() >= queue.left() && button.right() <= queue.right(),
                    "{id}: {button:?}, {queue:?}"
                );
                assert!(
                    button.top() >= queue.top() && button.bottom() <= queue.bottom(),
                    "{id}: {button:?}, {queue:?}"
                );
                assert!(button.top() >= px(0.) && button.bottom() <= px(height));
            }
            window.scroll(
                "queue-rows",
                ScrollDelta::Pixels(point(px(0.), px(-900.))),
                cx,
            );
        });
        assert!(ui.read(cx, |app, _| app.session().queue_scroll.offset().y < px(0.)));
        assert!(engine.sent().is_empty());
    }
}

#[gpui_kit::test]
fn feedback_is_not_task_acceptance(cx: &mut TestAppContext) {
    let (ui, engine) = finished_fixture(cx, TaskOutcome::NeedsReview);
    let end = ui.read(cx, |app, _| app.session().view.turns[0].end.unwrap());
    ui.click(cx, ("up", end));
    assert_eq!(
        ui.read(cx, |app, _| app.session().view.turns[0].feedback),
        Some(true)
    );
    assert_eq!(
        ui.read(cx, |app, _| app.session().view.outcome()),
        TaskOutcome::NeedsReview
    );
    assert!(engine.sent().is_empty());
}

#[gpui_kit::test]
fn external_acceptance_is_a_local_user_review_and_replays_without_native_controls(
    cx: &mut TestAppContext,
) {
    let (ui, engine) = finished_fixture(cx, TaskOutcome::Unknown);
    ui.app.update(cx, |app, _| {
        app.sessions[app.active].agent = flint_agent::AgentKind::Droid
    });
    show_task(&ui, cx);
    assert!(!has(&ui, cx, "task-edit"));
    ui.click(cx, "task-accept");
    ui.click(cx, "task-accept-confirm");
    assert!(engine.sent().is_empty());
    assert_eq!(
        ui.read(cx, |app, _| app.session().view.outcome()),
        TaskOutcome::Accepted
    );
    let (home, workspace, dir) = ui.read(cx, |app, _| {
        (
            app.home.clone(),
            app.session().workspace.clone(),
            app.session().dir.clone().unwrap(),
        )
    });
    let restored = open_with(
        cx,
        Options {
            home: Some(home),
            workspace: Some(workspace),
            ..test_options()
        },
    );
    assert_eq!(
        restored.read(cx, |app, _| app
            .sessions
            .iter()
            .find(|session| session.dir.as_ref() == Some(&dir))
            .unwrap()
            .view
            .outcome()),
        TaskOutcome::Accepted
    );
}

#[gpui_kit::test]
fn task_review_remains_a_session_guard_after_the_sheet_is_dismissed(cx: &mut TestAppContext) {
    let (ui, engine) = finished_fixture(cx, TaskOutcome::NeedsReview);
    show_task(&ui, cx);
    ui.click(cx, "task-accept");
    ui.click(cx, "task-accept-confirm");
    assert!(matches!(engine.sent().as_slice(), [Op::ReviewTask(_)]));
    ui.press(cx, "escape");
    ui.press(cx, "cmd-l");
    ui.input(cx, "Keep this prompt while saving.");
    ui.press(cx, "enter");
    assert!(engine.sent().is_empty());
    assert_eq!(
        ui.read(cx, |app, _| app.session().prompt_queue.items.len()),
        1
    );
    ui.with(cx, |window, cx| {
        ui.app
            .update(cx, |app, cx| app.delete_session(app.active, window, cx))
    });
    assert_eq!(ui.read(cx, |app, _| app.sessions.len()), 1);
    // A delayed event from a different turn cannot release the writer guard.
    engine.send(
        cx,
        AgentEvent::TaskAssessed {
            turn_id: 99,
            task: Box::new(task_fixture(TaskOutcome::Accepted)),
        },
    );
    show_task(&ui, cx);
    ui.click(cx, "task-continue");
    assert!(engine.sent().is_empty());
    engine.send(
        cx,
        AgentEvent::Error("The task review could not be saved.".into()),
    );
    assert!(has(&ui, cx, "task-sheet-error"));
    assert!(engine.sent().is_empty());
}

#[gpui_kit::test]
fn closed_native_ops_cannot_start_a_saved_task_write_while_its_event_pump_is_live(
    cx: &mut TestAppContext,
) {
    let (ui, engine) = finished_fixture(cx, TaskOutcome::NeedsReview);
    engine.ops.close();
    show_task(&ui, cx);
    ui.click(cx, "task-accept");
    assert!(!has(&ui, cx, "task-accept-confirm"));
    ui.click(cx, "task-edit");
    assert!(ui.read(cx, |app, _| {
        app.task_panel.as_ref().unwrap().draft.is_none()
    }));
    assert_eq!(
        ui.read(cx, |app, _| app.session().view.outcome()),
        TaskOutcome::NeedsReview
    );
}

fn stopped_native_fixture(cx: &mut TestAppContext) -> (Ui, std::path::PathBuf) {
    let (ui, engine) = finished_fixture(cx, TaskOutcome::NeedsReview);
    let (dir, workspace) = ui.read(cx, |app, _| {
        (
            app.session().dir.clone().unwrap(),
            app.session().workspace.clone(),
        )
    });
    std::fs::write(workspace.join("metadata.json"), "{}").unwrap();
    std::fs::write(workspace.join("method-bodies.c"), "actual methods").unwrap();
    let mut task = task_fixture(TaskOutcome::NeedsReview);
    task.refresh_files(&workspace);
    task.assess(TurnEndReason::Completed, true);
    engine.send(
        cx,
        AgentEvent::TaskAssessed {
            turn_id: 1,
            task: Box::new(task.clone()),
        },
    );
    std::fs::write(
        dir.join("history.json"),
        serde_json::to_vec(&json!({
            "version":2,"turn_id":1,"messages":[{"user":"Recover the actual methods."}],"task":task,
        }))
        .unwrap(),
    )
    .unwrap();
    engine.send(
        cx,
        AgentEvent::SessionStopped {
            history_saved: true,
        },
    );
    engine.events.close();
    settle(cx);
    assert!(ui.read(cx, |app, _| app.session().ops.is_none()));
    (ui, dir)
}

#[gpui_kit::test]
fn stopped_native_acceptance_reconciles_durable_history_after_a_lost_ui_event(
    cx: &mut TestAppContext,
) {
    let (ui, dir) = stopped_native_fixture(cx);
    let review = ui.read(cx, |app, _| {
        let task = app.session().view.task.as_ref().unwrap();
        TaskReview {
            turn_id: 1,
            task_id: 1,
            contract_revision: 1,
            work_revision: task.work_revision,
            decision: ReviewDecision::Accept,
        }
    });
    let workspace = ui.read(cx, |app, _| app.session().workspace.clone());
    flint_agent::review_saved_task(&dir, &workspace, &review).unwrap();
    // Deliberately omit the task-assessed UI event, as if the app closed.
    let options = ui.read(cx, |app, _| Options {
        home: Some(app.home.clone()),
        workspace: Some(workspace),
        ..test_options()
    });
    let restored = open_with(cx, options);
    assert_eq!(
        restored.read(cx, |app, _| app
            .sessions
            .iter()
            .find(|session| session.dir.as_ref() == Some(&dir))
            .unwrap()
            .view
            .outcome()),
        TaskOutcome::Accepted
    );
}

#[gpui_kit::test]
fn stopped_native_review_and_scope_controls_use_saved_history_without_starting_inference(
    cx: &mut TestAppContext,
) {
    let (ui, dir) = stopped_native_fixture(cx);
    show_task(&ui, cx);
    ui.click(cx, "task-accept");
    ui.click(cx, "task-accept-confirm");
    settle(cx);
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
    assert!(ui.read(cx, |app, _| app.session().ops.is_none()));
    ui.click(cx, "task-edit");
    ui.press(cx, "cmd-a");
    ui.input(cx, "A user-approved narrowed goal.");
    ui.click(cx, "task-edit-save");
    settle(cx);
    let (_, saved) = flint_agent::saved_task(&dir).unwrap().unwrap();
    assert_eq!(saved.contract.revision, 2);
    assert_eq!(saved.contract.objective, "A user-approved narrowed goal.");
    assert!(!saved.is_accepted());
    assert!(ui.read(cx, |app, _| app.session().ops.is_none()));
}

#[gpui_kit::test]
fn native_history_restores_a_missing_later_ui_turn_without_inventing_answer_text(
    cx: &mut TestAppContext,
) {
    let (ui, dir) = stopped_native_fixture(cx);
    let mut task = task_fixture(TaskOutcome::NeedsReview);
    task.contract.id = 2;
    task.review(&TaskReview {
        turn_id: 2,
        task_id: 2,
        contract_revision: 1,
        work_revision: task.work_revision,
        decision: ReviewDecision::Accept,
    })
    .unwrap();
    std::fs::write(
        dir.join("history.json"),
        serde_json::to_vec(&json!({
            "version":2,"turn_id":2,"messages":[{"user":"Second request"}],"task":task,
        }))
        .unwrap(),
    )
    .unwrap();
    let options = ui.read(cx, |app, _| Options {
        home: Some(app.home.clone()),
        workspace: Some(app.session().workspace.clone()),
        ..test_options()
    });
    let restored = open_with(cx, options);
    restored.read(cx, |app, _| {
        let session = app
            .sessions
            .iter()
            .find(|session| session.dir.as_ref() == Some(&dir))
            .unwrap();
        assert_eq!(session.view.turn_id, 2);
        assert_eq!(session.view.outcome(), TaskOutcome::Accepted);
        assert!(
            session
                .view
                .turns
                .iter()
                .find(|turn| turn.turn_id == 2)
                .unwrap()
                .task
                .as_ref()
                .unwrap()
                .is_accepted()
        );
        assert!(
            session
                .view
                .items
                .iter()
                .any(|item| matches!(item,Item::Error(text) if text.contains("no answer text")))
        );
        assert!(!session.view.items.iter().any(
            |item| matches!(item,Item::Assistant { text,.. } if text.contains("Second request"))
        ));
    });
}

#[gpui_kit::test]
fn damaged_native_task_history_keeps_bytes_and_reports_why_controls_are_unavailable(
    cx: &mut TestAppContext,
) {
    let (ui, dir) = stopped_native_fixture(cx);
    let bytes = b"{partial native history";
    std::fs::write(dir.join("history.json"), bytes).unwrap();
    let options = ui.read(cx, |app, _| Options {
        home: Some(app.home.clone()),
        workspace: Some(app.session().workspace.clone()),
        ..test_options()
    });
    let restored = open_with(cx, options);
    restored.read(cx,|app,_| {
        let session=app.sessions.iter().find(|session|session.dir.as_ref()==Some(&dir)).unwrap();
        assert!(session.view.items.iter().any(|item|matches!(item,Item::Error(text) if text.contains("Couldn't read saved native task history"))));
    });
    let ix = restored.read(cx, |app, _| {
        app.sessions
            .iter()
            .position(|session| session.dir.as_ref() == Some(&dir))
            .unwrap()
    });
    restored.click(cx, ("session", ix));
    show_task(&restored, cx);
    restored.click(cx, "task-accept");
    assert!(!has(&restored, cx, "task-accept-confirm"));
    restored.click(cx, "task-edit");
    assert!(restored.read(cx, |app, _| {
        app.task_panel.as_ref().unwrap().draft.is_none()
    }));
    assert_eq!(std::fs::read(dir.join("history.json")).unwrap(), bytes);
    assert!(!dir.join("history.json.corrupt").exists());
}
