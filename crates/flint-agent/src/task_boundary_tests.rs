use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn an_executed_failed_mutation_still_invalidates_checks_and_holds_the_queue() {
    let (_root, ws) = temp_workspace();
    std::fs::write(ws.join("result.txt"), "original").unwrap();
    let check = exists_check("result.txt");
    let mutation = if cfg!(windows) {
        r#"powershell.exe -NoProfile -NonInteractive -Command "Set-Content result.txt changed; exit 23""#
    } else {
        "printf changed > result.txt; exit 23"
    };
    let (url, _) = mock_server(vec![
        sse_tool_call("required-check", "run_command", json!({"command":check})),
        sse_tool_call(
            "failed-mutation",
            "run_command",
            json!({"command":mutation}),
        ),
        sse_text("The command failed after changing the file."),
        sse_text("Current verification is still missing."),
    ])
    .await;
    let events = run_one_turn(
        url,
        ws.clone(),
        &format!("Check result.txt.\nRequired file: result.txt\nRequired check: {check}"),
    )
    .await;
    let task = assessed(&events);
    let required = task
        .checks
        .iter()
        .find(|evidence| evidence.call_id == "required-check")
        .unwrap();
    assert_eq!(
        required.state(task.contract.revision, task.work_revision),
        EvidenceState::Stale
    );
    assert!(task.unknown_effects);
    assert!(!task.has_current_verification());
    assert!(!task.verified_for_queue());
    assert!(!task.is_accepted());
    assert_eq!(
        task.assessment.as_ref().unwrap().outcome,
        TaskOutcome::Partial
    );
    assert_eq!(
        std::fs::read_to_string(ws.join("result.txt"))
            .unwrap()
            .trim(),
        "changed"
    );
    assert!(events.iter().any(|event| matches!(
        event, AgentEvent::ToolCallFinished { call_id, exit_code: Some(23), success: false, .. }
            if call_id == "failed-mutation"
    )));
}

#[tokio::test]
async fn denied_mutations_do_not_make_current_required_checks_stale() {
    for (name, args) in [
        (
            "write_file",
            json!({"path":"result.txt","content":"unapproved"}),
        ),
        (
            "run_command",
            json!({"command":"printf unapproved > result.txt"}),
        ),
    ] {
        let (_root, ws) = temp_workspace();
        std::fs::write(ws.join("result.txt"), "original").unwrap();
        let check = exists_check("result.txt");
        let (url, _) = mock_server(vec![
            sse_tool_call("required-check", "run_command", json!({"command":check})),
            sse_tool_call("denied-mutation", name, args),
            sse_text("The requested check passed; the unapproved change was not applied."),
        ])
        .await;
        let mut config = test_config(url, ws.clone());
        config.approval = ApprovalMode::AskForChanges;
        let handle = crate::spawn_session(config);
        handle
            .ops
            .send(Op::UserMessage(format!(
                "Check result.txt.\nRequired file: result.txt\nRequired check: {check}"
            )))
            .await
            .unwrap();
        let mut events = Vec::new();
        loop {
            let event = tokio::time::timeout(Duration::from_secs(5), handle.events.recv())
                .await
                .unwrap()
                .unwrap();
            if let AgentEvent::ApprovalRequested { call_id, .. } = &event {
                handle
                    .ops
                    .send(Op::Approval {
                        call_id: call_id.clone(),
                        decision: if call_id == "denied-mutation" {
                            ApprovalDecision::Deny
                        } else {
                            ApprovalDecision::Approve
                        },
                    })
                    .await
                    .unwrap();
            }
            let finished = matches!(event, AgentEvent::TurnFinished { .. });
            events.push(event);
            if finished {
                break;
            }
        }
        handle.ops.send(Op::Shutdown).await.unwrap();
        while let Ok(Ok(event)) =
            tokio::time::timeout(Duration::from_secs(5), handle.events.recv()).await
        {
            events.push(event);
        }
        let task = assessed(&events);
        let required = task
            .checks
            .iter()
            .find(|evidence| evidence.call_id == "required-check")
            .unwrap();
        assert_eq!(
            required.state(task.contract.revision, task.work_revision),
            EvidenceState::Passed,
            "{name}: a host-denied call did not execute"
        );
        assert_eq!(
            task.assessment.as_ref().unwrap().outcome,
            TaskOutcome::NeedsReview
        );
        assert!(!task.unknown_effects);
        assert!(!task.is_accepted());
        assert_eq!(
            std::fs::read_to_string(ws.join("result.txt")).unwrap(),
            "original"
        );
        assert!(events.iter().any(|event| matches!(
            event, AgentEvent::ToolCallFinished { call_id, success: false, .. }
                if call_id == "denied-mutation"
        )));
    }
}

#[tokio::test]
async fn scope_edit_during_final_inference_is_answered_before_the_turn_stops() {
    scope_edit_during_final_inference_result(false).await;
}

#[tokio::test]
async fn scope_edit_then_provider_failure_does_not_reuse_the_old_scope_final() {
    scope_edit_during_final_inference_result(true).await;
}

async fn scope_edit_during_final_inference_result(fail_revised_request: bool) {
    let (_root, ws) = temp_workspace();
    let (inbox_tx, inbox_rx) = async_channel::unbounded();
    let (url, requests) = mock_server_with_response_hook(
        vec![
            sse_text("Blocked: old parser unavailable."),
            if fail_revised_request {
                "HTTP/1.1 400 Bad Request\r\ncontent-length: 0\r\nconnection: close\r\n\r\n".into()
            } else {
                sse_text("The revised tokenizer inspection is ready for review.")
            },
        ],
        json!({"data":[]}),
        move |index| {
            if index == 0 {
                inbox_tx
                    .try_send(Input::EditTask(TaskEdit {
                        task_id: 1,
                        expected_revision: 1,
                        objective: "Inspect the tokenizer without changing files.".into(),
                        mode: TaskMode::ReadOnly,
                        requirements: vec![RequirementSpec::Review(
                            "Explain tokenizer risks.".into(),
                        )],
                        restrictions: vec!["Do not change files.".into()],
                        clear_reported_blockers: false,
                    }))
                    .unwrap();
            }
        },
    )
    .await;
    let (events_tx, events_rx) = async_channel::unbounded();
    let mut session = Session::new(
        test_config(url, ws.clone()),
        events_tx,
        Arc::new(Approvals::default()),
        Arc::new(StdMutex::new(None)),
        tools::ToolContext::new(ws, false),
        None,
        false,
    );
    session.inbox = Some(inbox_rx);
    let reason = session
        .run_turn(
            "Inspect the old parser without changing files.".into(),
            &CancellationToken::new(),
        )
        .await;
    if fail_revised_request {
        assert!(matches!(reason, TurnEndReason::Failed(_)));
    } else {
        assert_eq!(reason, TurnEndReason::Completed);
    }
    let task = session.task.as_ref().unwrap();
    assert!(
        task.blockers.is_empty(),
        "the previous scope's final became a current blocker"
    );
    assert_eq!(task.contract.revision, 2);
    assert_eq!(
        task.assessment.as_ref().unwrap().outcome,
        if fail_revised_request {
            TaskOutcome::Partial
        } else {
            TaskOutcome::NeedsReview
        }
    );
    assert!(!task.is_accepted());
    let requests = requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert!(
        requests[1]["messages"]
            .to_string()
            .contains("Explain tokenizer risks.")
    );
    let mut events = Vec::new();
    while let Ok(event) = events_rx.try_recv() {
        events.push(event);
    }
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, AgentEvent::TurnStarted { .. }))
            .count(),
        1
    );
}

#[tokio::test]
async fn file_drift_during_final_inference_gets_current_verification_recovery() {
    let (_root, ws) = temp_workspace();
    let path = ws.join("result.txt");
    let check = exists_check("result.txt");
    let (url, _) = mock_server_with_response_hook(
        vec![
            sse_tool_call(
                "write",
                "write_file",
                json!({"path":"result.txt","content":"first"}),
            ),
            sse_tool_call(
                "check-before-drift",
                "run_command",
                json!({"command":check}),
            ),
            sse_text("Verified; done."),
            sse_tool_call("check-after-drift", "run_command", json!({"command":check})),
            sse_text("Rechecked the changed file. Review the result."),
        ],
        json!({"data":[]}),
        move |index| {
            if index == 2 {
                std::fs::write(&path, "external update").unwrap();
            }
        },
    )
    .await;
    let events = run_one_turn(url, ws, "Create result.txt and verify it.").await;
    assert!(events.iter().any(|event| matches!(
        event,
        AgentEvent::HarnessNudge { reason: NudgeReason::Verify, message }
            if message.contains("became stale")
    )));
    let task = assessed(&events);
    assert!(task.has_current_verification());
    assert_eq!(task.checks[0].call_id, "check-after-drift");
    assert_eq!(
        task.assessment.as_ref().unwrap().outcome,
        TaskOutcome::NeedsReview
    );
    assert!(!task.is_accepted());
}

#[tokio::test]
async fn resumed_native_plan_gets_a_finish_nudge_and_can_recover() {
    let (_root, ws) = temp_workspace();
    let saved = tempfile::tempdir().unwrap();
    let mut task = TaskRecord::new(1, "Inspect the parser without changing files.".into(), 0);
    task.plan = tools::parse_plan(
        json!({"plan":[{"step":"Explain parser risks","status":"pending"}]})
            .as_object()
            .unwrap(),
    )
    .unwrap();
    persist::write_atomic(
        saved.path(),
        &persist::snapshot_with_task(
            1,
            &[Message::User(task.contract.original_request.clone())],
            Some(&task),
        ),
    )
    .unwrap();
    let (url, requests) = mock_server(vec![
        sse_text("The inspection is ready."),
        sse_tool_call(
            "close-plan",
            "update_plan",
            json!({"plan":[{"step":"Explain parser risks","status":"completed"}]}),
        ),
        sse_text("Parser risks explained. Review the findings."),
    ])
    .await;
    let mut config = test_config(url, ws);
    config.session_dir = Some(saved.path().into());
    let events = run_turns(config, &["Continue"]).await;
    assert!(events.iter().any(|event| matches!(
        event,
        AgentEvent::HarnessNudge { reason: NudgeReason::Watchdog, message }
            if message.contains("Explain parser risks")
    )));
    let task = assessed(&events);
    assert!(!task.plan.iter().any(|step| step.status.is_open()));
    assert_eq!(
        task.assessment.as_ref().unwrap().outcome,
        TaskOutcome::NeedsReview
    );
    assert_eq!(requests.lock().unwrap().len(), 3);
    assert!(!task.is_accepted());
}

#[tokio::test]
async fn an_unassessed_scope_emits_its_initial_projection_again_for_the_new_turn() {
    let (_root, ws) = temp_workspace();
    let saved = tempfile::tempdir().unwrap();
    let task = TaskRecord::new(1, "Research the result without changing files.".into(), 0);
    persist::write_atomic(
        saved.path(),
        &persist::snapshot_with_task(
            1,
            &[Message::User(task.contract.original_request.clone())],
            Some(&task),
        ),
    )
    .unwrap();
    let (url, _) = mock_server(vec![sse_text(
        "The requested inspection is ready for review.",
    )])
    .await;
    let mut config = test_config(url, ws);
    config.session_dir = Some(saved.path().into());
    let handle = crate::spawn_session(config);
    handle
        .ops
        .send(Op::EditTask(TaskEdit {
            task_id: 1,
            expected_revision: 1,
            objective: "Research the result without changing files.".into(),
            mode: TaskMode::ReadOnly,
            requirements: vec![RequirementSpec::Review("Explain the result.".into())],
            restrictions: Vec::new(),
            clear_reported_blockers: false,
        }))
        .await
        .unwrap();
    let mut events = Vec::new();
    loop {
        let event = tokio::time::timeout(Duration::from_secs(5), handle.events.recv())
            .await
            .unwrap()
            .unwrap();
        let edited = matches!(&event,AgentEvent::TaskUpdated { turn_id:1,task } if task.contract.revision==2);
        events.push(event);
        if edited {
            break;
        }
    }
    handle
        .ops
        .send(Op::ContinueTask {
            task_id: 1,
            expected_revision: 2,
        })
        .await
        .unwrap();
    collect_turn(&handle, &mut events).await;
    handle.ops.send(Op::Shutdown).await.unwrap();
    while let Ok(Ok(_)) = tokio::time::timeout(Duration::from_secs(5), handle.events.recv()).await {
    }
    assert!(
        events
            .iter()
            .any(|event| matches!(event, AgentEvent::TaskUpdated { turn_id: 2, .. })),
        "an identical scope from the previous turn suppressed the new turn's initial projection"
    );
}

#[tokio::test]
async fn image_only_native_turn_keeps_the_empty_request_and_reloadable_history() {
    let (_root, ws) = temp_workspace();
    let saved = tempfile::tempdir().unwrap();
    let (url, _) = mock_server(vec![sse_text("The image needs your review.")]).await;
    let mut config = test_config(url, ws);
    config.session_dir = Some(saved.path().into());
    let handle = crate::spawn_session(config);
    handle
        .ops
        .send(Op::UserMessageWithImages {
            text: String::new(),
            images: vec![ImageAttachment {
                name: "read-only.png".into(),
                mime_type: "image/png".into(),
                data: "aGVsbG8=".into(),
            }],
        })
        .await
        .unwrap();
    let mut events = Vec::new();
    collect_turn(&handle, &mut events).await;
    let task = assessed(&events);
    assert!(task.contract.original_request.is_empty());
    assert_eq!(task.contract.mode, TaskMode::Unspecified);
    task.validate().unwrap();
    handle.ops.send(Op::Shutdown).await.unwrap();
    while let Ok(Ok(_)) = tokio::time::timeout(Duration::from_secs(5), handle.events.recv()).await {
    }
    assert!(
        crate::saved_task(saved.path())
            .unwrap()
            .unwrap()
            .1
            .contract
            .original_request
            .is_empty()
    );
    assert!(!saved.path().join("history.json.corrupt").exists());
}

#[tokio::test]
async fn malformed_command_arguments_do_not_poison_recoverable_native_history() {
    for arguments in ["not JSON", "{}", "{\"command\":\"   \"}"] {
        let (_root, ws) = temp_workspace();
        let saved = tempfile::tempdir().unwrap();
        let bad = sse(&[
            json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"bad","type":"function","function":{"name":"run_command","arguments":arguments}}]}}]}),
            json!({"choices":[{"delta":{},"finish_reason":"tool_calls"}]}),
        ]);
        let (url, _) = mock_server(vec![
            bad,
            sse_tool_call("good", "run_command", json!({"command":"ls"})),
            sse_text("Finished inspection."),
        ])
        .await;
        let mut config = test_config(url, ws);
        config.session_dir = Some(saved.path().into());
        let handle = crate::spawn_session(config);
        handle
            .ops
            .send(Op::UserMessage(
                "Research only. Do not change files.".into(),
            ))
            .await
            .unwrap();
        let mut events = Vec::new();
        collect_turn(&handle, &mut events).await;
        let task = assessed(&events);
        task.validate().unwrap();
        assert!(
            task.checks
                .iter()
                .all(|check| !check.command.trim().is_empty())
        );
        assert!(events.iter().any(|event|matches!(event,AgentEvent::ToolCallFinished { call_id,success:false,.. } if call_id=="bad")));
        handle.ops.send(Op::Shutdown).await.unwrap();
        while let Ok(Ok(_)) =
            tokio::time::timeout(Duration::from_secs(5), handle.events.recv()).await
        {}
        crate::saved_task(saved.path())
            .unwrap()
            .unwrap()
            .1
            .validate()
            .unwrap();
        assert!(!saved.path().join("history.json.corrupt").exists());
    }
}

#[tokio::test]
async fn readonly_native_file_compile_is_refused_before_execution() {
    for command in ["file -C -m rules", "file --compile -m rules"] {
        let (_root, ws) = temp_workspace();
        std::fs::write(ws.join("rules"), "0 string HELLO sample\n").unwrap();
        let (url, _) = mock_server(vec![
            sse_tool_call("compile", "run_command", json!({"command":command})),
            sse_text("Blocked: the protected task is read-only."),
        ])
        .await;
        let events = run_one_turn(url, ws.clone(), "Research only. Do not change files.").await;
        assert!(events.iter().any(
            |event| matches!(event,AgentEvent::ToolCallFinished { call_id,success:false,output,.. }
            if call_id=="compile" && output.contains("read-only"))
        ));
        assert!(!ws.join("rules.mgc").exists());
    }
}

#[tokio::test]
async fn unchanged_read_steps_do_not_emit_duplicate_task_projections() {
    let (_root, ws) = temp_workspace();
    std::fs::write(ws.join("input"), "unchanged").unwrap();
    let (url, _) = mock_server(vec![
        sse_tool_call("read-one", "read_file", json!({"path":"input"})),
        sse_tool_call("read-two", "read_file", json!({"path":"input"})),
        sse_text("Finished reading."),
    ])
    .await;
    let events = run_one_turn(url, ws, "Research only. Do not change files.").await;
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, AgentEvent::TaskUpdated { .. }))
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, AgentEvent::TaskAssessed { .. }))
            .count(),
        1
    );
}

#[tokio::test]
async fn mutations_with_test_named_operands_still_invalidate_earlier_native_checks() {
    let (_root, ws) = temp_workspace();
    let (url, _) = mock_server(vec![
        sse_tool_call(
            "write",
            "write_file",
            json!({"path":"result.txt","content":"result"}),
        ),
        sse_tool_call(
            "check",
            "run_command",
            json!({"command":"test -f result.txt"}),
        ),
        sse_tool_call(
            "mutate",
            "run_command",
            json!({"command":"touch validation_test_artefact"}),
        ),
        sse_text("Done."),
        sse_text("Partial: the earlier check is stale."),
    ])
    .await;
    let events = run_one_turn(
        url,
        ws.clone(),
        "Produce result.txt.\nRequired file: result.txt\nRequired check: test -f result.txt",
    )
    .await;
    let task = assessed(&events);
    assert!(ws.join("validation_test_artefact").exists());
    assert_eq!(
        task.checks
            .iter()
            .find(|check| check.call_id == "check")
            .unwrap()
            .state(task.contract.revision, task.work_revision),
        crate::EvidenceState::Stale
    );
    assert_eq!(
        task.assessment.as_ref().unwrap().outcome,
        TaskOutcome::Partial
    );
}

#[tokio::test]
async fn a_gated_fingerprint_scan_yields_while_sibling_work_invalidates_checks() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("result"), "before").unwrap();
    let mut task = TaskRecord::new(1, "Produce a result.\nRequired file: result".into(), 4);
    task.refresh_files(root.path());
    task.observe_command(
        "check",
        "cargo test",
        CommandTiming {
            started: 4,
            finished: 4,
            order: 1,
        },
        &tools::ToolOutcome::ok("passed"),
        true,
    );
    let clock = Arc::new(task::WorkClock::default());
    clock.restore(4, 1);
    let (started, entered) = tokio::sync::oneshot::channel();
    let (release, gate) = std::sync::mpsc::channel();
    let workspace = root.path().to_path_buf();
    let scan = tokio::spawn(scan_task_files(task, workspace, move |task, workspace| {
        started.send(()).unwrap();
        gate.recv_timeout(Duration::from_secs(3)).unwrap();
        task.refresh_files(workspace)
    }));
    tokio::time::timeout(Duration::from_secs(1), entered)
        .await
        .unwrap()
        .unwrap();
    let sibling_clock = clock.clone();
    let sibling = tokio::spawn(async move { sibling_clock.advance() });
    let revision = tokio::time::timeout(Duration::from_secs(1), sibling)
        .await
        .unwrap()
        .unwrap();
    std::fs::write(root.path().join("result"), "sibling changed this").unwrap();
    release.send(()).unwrap();
    let (mut task, changed) = scan.await.unwrap().unwrap();
    assert!(changed);
    task.set_work_revision(revision);
    assert_eq!(
        task.checks[0].state(1, revision),
        crate::EvidenceState::Stale
    );
}

#[tokio::test]
async fn readonly_shell_quotes_cannot_hide_git_output_options() {
    for command in [
        "git diff '--output=forbidden.patch'",
        "git diff \"--output=forbidden.patch\"",
        "git diff --'output'=forbidden.patch",
    ] {
        let (_root, ws) = temp_workspace();
        assert!(
            std::process::Command::new("git")
                .args(["init", "--quiet"])
                .current_dir(&ws)
                .status()
                .unwrap()
                .success()
        );
        std::fs::write(ws.join("input"), "before\n").unwrap();
        assert!(
            std::process::Command::new("git")
                .args(["add", "input"])
                .current_dir(&ws)
                .status()
                .unwrap()
                .success()
        );
        std::fs::write(ws.join("input"), "after\n").unwrap();
        let (url, _) = mock_server(vec![
            sse_tool_call("output", "run_command", json!({"command":command})),
            sse_text("The task remains read-only."),
        ])
        .await;
        let events = run_one_turn(url, ws.clone(), "Research only. Do not change files.").await;
        assert!(
            !ws.join("forbidden.patch").exists(),
            "shell decoding bypassed read-only policy: {command}"
        );
        assert!(events.iter().any(|event| matches!(
            event,
            AgentEvent::ToolCallFinished { call_id, success: false, output, .. }
                if call_id == "output" && output.contains("read-only")
        )));
    }
}

#[tokio::test]
async fn imperative_do_request_cannot_finish_as_an_answer_without_work() {
    let (_root, ws) = temp_workspace();
    let (url, _) = mock_server(vec![sse_text("Done."), sse_text("Done.")]).await;
    let events = run_one_turn(url, ws, "Do the migration and run the tests.").await;
    let task = assessed(&events);
    assert_eq!(task.contract.mode, TaskMode::Work);
    assert_ne!(
        task.assessment.as_ref().unwrap().outcome,
        TaskOutcome::Answered
    );
    assert!(!task.assessment.as_ref().unwrap().outcome.allows_queue());
}

#[tokio::test]
async fn steering_work_into_a_question_preserves_new_required_deliverables() {
    let (_root, ws) = temp_workspace();
    let (url, _) = mock_server(vec![sse_text("The parser reads tokens.")]).await;
    let handle = crate::spawn_session(test_config(url, ws));
    handle
        .ops
        .send(Op::UserMessage("How does the parser work?".into()))
        .await
        .unwrap();
    handle
        .ops
        .send(Op::SteerMessage {
            id: 41,
            text: "Implement the parser fix.\nRequired file: result.txt\nRequired check: test -s result.txt"
                .into(),
            images: Vec::new(),
        })
        .await
        .unwrap();
    let mut events = Vec::new();
    collect_turn(&handle, &mut events).await;
    let task = assessed(&events);
    assert!(
        events
            .iter()
            .any(|event| matches!(event, AgentEvent::SteeringAccepted { id: 41 }))
    );
    assert_eq!(task.contract.mode, TaskMode::Work);
    assert!(
        task.contract
            .requirements
            .iter()
            .any(|requirement| requirement.spec == RequirementSpec::File("result.txt".into()))
    );
    assert!(task.contract.requirements.iter().any(
        |requirement| requirement.spec == RequirementSpec::Command("test -s result.txt".into())
    ));
    assert_eq!(
        task.assessment.as_ref().unwrap().outcome,
        TaskOutcome::Partial
    );
    handle.ops.send(Op::Shutdown).await.unwrap();
    while let Ok(Ok(_)) = tokio::time::timeout(Duration::from_secs(5), handle.events.recv()).await {
    }
}

#[tokio::test]
async fn a_new_failed_turn_does_not_inherit_the_previous_final_answer_blocker() {
    let (_root, ws) = temp_workspace();
    let saved = tempfile::tempdir().unwrap();
    let (url, _) = mock_server(vec![sse_text("Blocked: input binary unavailable.")]).await;
    let mut config = test_config(url, ws.clone());
    config.session_dir = Some(saved.path().into());
    let first = run_turns(config, &["Recover the binary."]).await;
    assert!(!assessed(&first).blockers.is_empty());
    let mut config = test_config("http://127.0.0.1:1/v1".into(), ws);
    config.session_dir = Some(saved.path().into());
    let second = run_turns(config, &["Implement a different parser."]).await;
    let task = assessed(&second);
    assert_eq!(task.contract.id, 2);
    assert!(matches!(
        task.assessment.as_ref().unwrap().reason,
        TurnEndReason::Failed(_)
    ));
    assert!(
        task.blockers.is_empty(),
        "a final answer from turn one became a turn-two blocker"
    );
    assert_eq!(
        task.assessment.as_ref().unwrap().outcome,
        TaskOutcome::Partial
    );
}

#[tokio::test]
async fn an_exact_required_make_check_is_not_stale_from_its_own_execution() {
    let (_root, ws) = temp_workspace();
    std::fs::write(
        ws.join("Makefile"),
        ".PHONY: test\ntest:\n\t@printf 'checked\\n'\n",
    )
    .unwrap();
    let (url, _) = mock_server(vec![
        sse_tool_call("check", "run_command", json!({"command":"make test"})),
        sse_text("The requested check passed. Review the result."),
    ])
    .await;
    let events = run_one_turn(url, ws, "Run the checks.\nRequired check: make test").await;
    let task = assessed(&events);
    let check = task
        .checks
        .iter()
        .find(|check| check.call_id == "check")
        .unwrap();
    assert_eq!(check.exit_code, Some(0));
    assert_eq!(
        check.state(task.contract.revision, task.work_revision),
        EvidenceState::Passed
    );
    assert_eq!(
        task.assessment.as_ref().unwrap().outcome,
        TaskOutcome::NeedsReview
    );
}
