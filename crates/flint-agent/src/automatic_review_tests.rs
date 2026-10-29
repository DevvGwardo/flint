use super::*;
use pretty_assertions::assert_eq;

fn ready_task(workspace: &std::path::Path) -> TaskRecord {
    std::fs::write(workspace.join("result.txt"), "result").unwrap();
    let mut task = TaskRecord::new(1, "Create and verify result.txt.".into(), 1);
    task.observed_edits = 1;
    task.note_file("result.txt", true);
    task.refresh_files(workspace);
    task.checks.push(CommandEvidence {
        id: 1,
        call_id: "checked".into(),
        command: exists_check("result.txt"),
        contract_revision: 1,
        work_revision: 1,
        started_revision: 1,
        success: true,
        exit_code: Some(0),
        detail: "passed".into(),
        verification: true,
    });
    task.assess(TurnEndReason::Completed, true);
    assert!(task.can_automatically_review());
    task
}

fn clean_review(task: &TaskRecord) -> AutomaticReview {
    AutomaticReview {
        contract_revision: task.contract.revision,
        work_revision: task.work_revision,
        model: "fixture".into(),
        verdict: AutomaticReviewVerdict::Clean,
        summary: "Inspected the requested result and covered every requirement.".into(),
        findings: Vec::new(),
        reviewed_requirements: task
            .contract
            .requirements
            .iter()
            .map(|requirement| requirement.id)
            .collect(),
        inspected_files: task.files.clone(),
    }
}

fn verdict(verdict: &str, findings: Vec<&str>) -> String {
    sse_tool_call(
        "review-verdict",
        "finish_review",
        json!({
            "verdict": verdict,
            "summary": "Independent fixture review.",
            "findings": findings,
            "reviewed_requirements": [1],
        }),
    )
}

async fn finished(handle: &crate::SessionHandle) -> Vec<AgentEvent> {
    let mut events = Vec::new();
    loop {
        let event = match tokio::time::timeout(Duration::from_secs(15), handle.events.recv()).await
        {
            Ok(event) => event.expect("session still running"),
            // The events so far show exactly where the turn stalled.
            Err(_) => panic!("review turn stalled after events: {events:#?}"),
        };
        // Printed only when the test fails: CI captures per-test output, so
        // the next failure shows exactly how far the turn progressed.
        eprintln!("auto-review event: {event:?}");
        let done = matches!(event, AgentEvent::TurnFinished { .. });
        events.push(event);
        if done {
            return events;
        }
    }
}

async fn stop(handle: &crate::SessionHandle) {
    handle.ops.send(Op::Shutdown).await.unwrap();
    loop {
        let event = tokio::time::timeout(Duration::from_secs(15), handle.events.recv())
            .await
            .unwrap()
            .unwrap();
        if matches!(
            event,
            AgentEvent::SessionStopped {
                history_saved: true
            }
        ) {
            break;
        }
    }
}

async fn request_review(handle: &crate::SessionHandle, task: &TaskRecord) {
    handle
        .ops
        .send(Op::AutomaticallyReviewTask {
            task_id: task.contract.id,
            expected_revision: task.contract.revision,
            expected_work_revision: task.work_revision,
        })
        .await
        .unwrap();
}

#[test]
fn automatic_review_clears_human_review_state_without_claiming_user_acceptance() {
    let (_root, ws) = temp_workspace();
    let mut task = ready_task(&ws);
    task.record_automatic_review(clean_review(&task)).unwrap();
    assert_eq!(
        task.assessment.as_ref().unwrap().outcome,
        TaskOutcome::Verified
    );
    assert!(task.is_automatically_verified());
    assert!(!task.is_accepted());
    assert!(
        task.assessment
            .as_ref()
            .unwrap()
            .requirements
            .iter()
            .all(|requirement| requirement.state == EvidenceState::Passed)
    );
    assert!(TaskOutcome::Verified.allows_queue());
    task.validate().unwrap();
    let restored: TaskRecord = serde_json::from_slice(&serde_json::to_vec(&task).unwrap()).unwrap();
    restored.validate().unwrap();
    assert!(restored.is_automatically_verified() && !restored.is_accepted());
}

#[test]
fn automatic_review_requires_source_inspection_and_every_protected_requirement() {
    let (_root, ws) = temp_workspace();
    let mut task = ready_task(&ws);
    for missing in ["inspection", "requirements", "findings"] {
        let mut review = clean_review(&task);
        match missing {
            "inspection" => review.inspected_files.clear(),
            "requirements" => review.reviewed_requirements.clear(),
            "findings" => review.findings.push("Unresolved issue".into()),
            _ => unreachable!(),
        }
        assert!(task.record_automatic_review(review).is_err(), "{missing}");
    }
    task.assessment.as_mut().unwrap().outcome = TaskOutcome::Verified;
    assert!(
        task.validate().is_err(),
        "cached status cannot invent review evidence"
    );
}

#[test]
fn automatic_review_keeps_old_git_inspection_historical_but_rejects_stale_verification() {
    let (_root, ws) = temp_workspace();
    let mut task = ready_task(&ws);
    task.checks.push(CommandEvidence {
        id: 2,
        call_id: "old-inspection".into(),
        command: "git status --short".into(),
        contract_revision: 1,
        work_revision: 0,
        started_revision: 0,
        success: true,
        exit_code: Some(0),
        detail: "earlier inspection".into(),
        verification: false,
    });
    task.assess(TurnEndReason::Completed, true);
    assert!(task.can_automatically_review());
    task.record_automatic_review(clean_review(&task)).unwrap();
    assert!(task.is_automatically_verified());
    task.set_work_revision(2);
    task.assess(TurnEndReason::Completed, true);
    assert!(!task.is_automatically_verified());
    assert!(!task.can_automatically_review());
    assert_eq!(task.assessment.unwrap().outcome, TaskOutcome::Partial);
}

#[test]
fn automatic_review_fingerprints_dependencies_and_revokes_verified_on_external_drift() {
    let (_root, ws) = temp_workspace();
    let mut task = ready_task(&ws);
    std::fs::write(ws.join("dependency.txt"), "before").unwrap();
    let mut review = clean_review(&task);
    review.inspected_files.push(FileEvidence {
        path: "dependency.txt".into(),
        state: crate::task::file_state(&ws, "dependency.txt"),
        reported: false,
    });
    task.record_automatic_review(review).unwrap();
    std::fs::write(ws.join("dependency.txt"), "after").unwrap();
    assert!(task.refresh_files(&ws));
    task.set_work_revision(2);
    task.assess(TurnEndReason::Completed, true);
    assert!(!task.is_automatically_verified());
}

#[tokio::test]
async fn automatic_review_native_loop_persists_a_separate_clean_verdict() {
    let (_root, ws) = temp_workspace();
    let dir = tempfile::tempdir().unwrap();
    let check = exists_check("result.txt");
    let (url, _) = mock_server(vec![
        sse_tool_call(
            "write",
            "write_file",
            json!({"path":"result.txt","content":"result"}),
        ),
        sse_tool_call("check", "run_command", json!({"command":check})),
        sse_text("Result created and checked."),
        sse_tool_call("inspect", "read_file", json!({"path":"result.txt"})),
        verdict("clean", vec![]),
    ])
    .await;
    let mut config = test_config(url, ws);
    config.session_dir = Some(dir.path().into());
    let handle = crate::spawn_session(config);
    handle
        .ops
        .send(Op::UserMessage("Create and verify result.txt.".into()))
        .await
        .unwrap();
    let first = finished(&handle).await;
    let task = assessed(&first);
    assert_eq!(
        task.assessment.as_ref().unwrap().outcome,
        TaskOutcome::NeedsReview
    );
    request_review(&handle, task).await;
    let reviewed = finished(&handle).await;
    let task = assessed(&reviewed);
    assert_eq!(
        task.assessment.as_ref().unwrap().outcome,
        TaskOutcome::Verified
    );
    assert!(!task.is_accepted());
    assert!(reviewed.iter().any(
        |event| matches!(event, AgentEvent::ToolCallStarted { name, .. } if name == "read_file")
    ));
    assert!(!reviewed.iter().any(|event| matches!(
        event,
        AgentEvent::ToolCallStarted {
            kind: ToolKind::Edit | ToolKind::Command,
            ..
        }
    )));
    stop(&handle).await;
    let saved = crate::saved_task(dir.path()).unwrap().unwrap().1;
    assert_eq!(
        saved.assessment.as_ref().unwrap().outcome,
        TaskOutcome::Verified
    );
    assert!(saved.is_automatically_verified() && !saved.is_accepted());
}

#[tokio::test]
async fn automatic_review_findings_survive_fixes_until_a_new_independent_review() {
    let (_root, ws) = temp_workspace();
    let check = exists_check("result.txt");
    let (url, _) = mock_server(vec![
        sse_tool_call(
            "write",
            "write_file",
            json!({"path":"result.txt","content":"wrong"}),
        ),
        sse_tool_call("check", "run_command", json!({"command":check})),
        sse_text("Result created."),
        sse_tool_call("inspect", "read_file", json!({"path":"result.txt"})),
        verdict(
            "needs_fixes",
            vec!["result.txt contains the wrong requested value."],
        ),
        sse_tool_call(
            "repair",
            "edit_file",
            json!({"path":"result.txt","old_string":"wrong","new_string":"result"}),
        ),
        sse_tool_call("check-fixed", "run_command", json!({"command":check})),
        sse_text("Repaired and checked the result."),
        sse_tool_call("inspect-fixed", "read_file", json!({"path":"result.txt"})),
        verdict("clean", vec![]),
    ])
    .await;
    let handle = crate::spawn_session(test_config(url, ws.clone()));
    handle
        .ops
        .send(Op::UserMessage("Create and verify result.txt.".into()))
        .await
        .unwrap();
    let first = finished(&handle).await;
    request_review(&handle, assessed(&first)).await;
    let review = finished(&handle).await;
    let task = assessed(&review);
    assert_eq!(
        task.assessment.as_ref().unwrap().outcome,
        TaskOutcome::Partial
    );
    assert!(task.context().contains("wrong requested value"));
    handle
        .ops
        .send(Op::ContinueTask {
            task_id: 1,
            expected_revision: 1,
        })
        .await
        .unwrap();
    let fixed = finished(&handle).await;
    let task = assessed(&fixed);
    assert!(task.can_automatically_review(), "{task:#?}");
    assert!(!task.is_automatically_verified());
    assert!(!task.automatic_review.as_ref().unwrap().findings.is_empty());
    request_review(&handle, task).await;
    let clean = finished(&handle).await;
    assert_eq!(
        assessed(&clean).assessment.as_ref().unwrap().outcome,
        TaskOutcome::Verified
    );
    assert_eq!(
        std::fs::read_to_string(ws.join("result.txt")).unwrap(),
        "result"
    );
    stop(&handle).await;
}

#[tokio::test]
async fn automatic_reviewer_cannot_write_execute_or_pass_without_inspection() {
    for call in [
        sse_tool_call(
            "write",
            "write_file",
            json!({"path":"result.txt","content":"tampered"}),
        ),
        sse_tool_call(
            "command",
            "run_command",
            json!({"command":"touch tampered.txt"}),
        ),
        verdict("clean", vec![]),
    ] {
        let (_root, ws) = temp_workspace();
        let task = ready_task(&ws);
        let (url, _) = mock_server(vec![call]).await;
        let result = crate::automatic_review::run(
            &test_config(url, ws.clone()),
            &task,
            2,
            &CancellationToken::new(),
            &|_| {},
        )
        .await;
        assert!(result.is_err());
        assert_eq!(
            std::fs::read_to_string(ws.join("result.txt")).unwrap(),
            "result"
        );
        assert!(!ws.join("tampered.txt").exists());
    }
}

#[test]
fn automatic_review_rejects_duplicate_unknown_and_uninspected_evidence() {
    let (_root, ws) = temp_workspace();
    let mut task = ready_task(&ws);
    for invalid in ["duplicate-path", "duplicate-id", "unknown-id"] {
        let mut review = clean_review(&task);
        match invalid {
            "duplicate-path" => review
                .inspected_files
                .push(review.inspected_files[0].clone()),
            "duplicate-id" => review.reviewed_requirements.push(1),
            "unknown-id" => review.reviewed_requirements.push(999),
            _ => unreachable!(),
        }
        assert!(task.record_automatic_review(review).is_err(), "{invalid}");
    }
    let review = clean_review(&task);
    std::fs::write(ws.join("another-deliverable.txt"), "also required").unwrap();
    task.note_file("another-deliverable.txt", true);
    task.refresh_files(&ws);
    task.assess(TurnEndReason::Completed, true);
    assert!(task.record_automatic_review(review).is_err());
    assert!(!task.is_automatically_verified());
}

#[test]
fn automatic_review_cannot_certify_unknown_effects() {
    let (_root, ws) = temp_workspace();
    let mut task = ready_task(&ws);
    task.unknown_effects = true;
    assert!(!task.can_automatically_review());
    assert!(task.record_automatic_review(clean_review(&task)).is_err());
}

#[tokio::test]
async fn automatic_review_reports_genuine_missing_evidence_as_blocked() {
    let (_root, ws) = temp_workspace();
    let mut task = ready_task(&ws);
    let (url, _) = mock_server(vec![verdict(
        "blocked",
        vec!["Cannot establish semantic correctness with the available source."],
    )])
    .await;
    let review = crate::automatic_review::run(
        &test_config(url, ws),
        &task,
        2,
        &CancellationToken::new(),
        &|_| {},
    )
    .await
    .unwrap();
    task.record_automatic_review(review).unwrap();
    assert_eq!(
        task.assessment.as_ref().unwrap().outcome,
        TaskOutcome::Blocked
    );
    assert!(!task.is_automatically_verified() && !task.is_accepted());
    assert!(!task.can_automatically_review());
}

#[tokio::test]
async fn automatic_review_never_turns_unstructured_output_into_completion() {
    let (_root, ws) = temp_workspace();
    let task = ready_task(&ws);
    let (url, _) = mock_server(vec![sse_text("Looks good. Approved!")]).await;
    let result = crate::automatic_review::run(
        &test_config(url, ws),
        &task,
        2,
        &CancellationToken::new(),
        &|_| {},
    )
    .await;
    assert!(result.is_err());
    assert!(!task.is_automatically_verified());
}

#[tokio::test]
async fn automatic_review_respects_cancellation_before_and_during_inspection() {
    for cancel_before in [true, false] {
        let (_root, ws) = temp_workspace();
        let task = ready_task(&ws);
        let (url, _) = mock_server(vec![
            sse_tool_call("inspect", "read_file", json!({"path":"result.txt"})),
            verdict("clean", vec![]),
        ])
        .await;
        let cancel = CancellationToken::new();
        if cancel_before {
            cancel.cancel();
        }
        let result =
            crate::automatic_review::run(&test_config(url, ws), &task, 2, &cancel, &|event| {
                if matches!(event, AgentEvent::ToolCallFinished { .. }) {
                    cancel.cancel();
                }
            })
            .await;
        assert!(matches!(result, Err(ProviderError::Cancelled)));
        assert!(!task.is_automatically_verified());
    }
}

#[tokio::test]
async fn automatic_review_detects_file_changes_during_inspection() {
    let (_root, ws) = temp_workspace();
    let task = ready_task(&ws);
    let (url, _) = mock_server(vec![
        sse_tool_call("inspect", "read_file", json!({"path":"result.txt"})),
        verdict("clean", vec![]),
    ])
    .await;
    let result = crate::automatic_review::run(
        &test_config(url, ws.clone()),
        &task,
        2,
        &CancellationToken::new(),
        &|event| {
            if matches!(event, AgentEvent::ToolCallFinished { .. }) {
                std::fs::write(ws.join("result.txt"), "changed during inspection").unwrap();
            }
        },
    )
    .await;
    assert!(result.is_err());
    assert!(!task.is_automatically_verified());
}

#[tokio::test]
async fn automatic_review_forbids_credential_paths_and_bounds_model_steps() {
    let (_root, ws) = temp_workspace();
    let task = ready_task(&ws);
    let (url, _) = mock_server(vec![sse_tool_call(
        "credential",
        "read_file",
        json!({"path":".env"}),
    )])
    .await;
    assert!(
        crate::automatic_review::run(
            &test_config(url, ws.clone()),
            &task,
            2,
            &CancellationToken::new(),
            &|_| {},
        )
        .await
        .is_err()
    );
    let calls = (0..16)
        .map(|index| {
            sse_tool_call(
                &format!("inspect-{index}"),
                "read_file",
                json!({"path":"result.txt"}),
            )
        })
        .collect();
    let (url, _) = mock_server(calls).await;
    let result = crate::automatic_review::run(
        &test_config(url, ws),
        &task,
        2,
        &CancellationToken::new(),
        &|_| {},
    )
    .await;
    assert!(matches!(result, Err(ProviderError::Failed(error)) if error.contains("step budget")));
    assert!(!task.is_automatically_verified());
}

#[tokio::test]
async fn automatic_review_searches_only_individual_noncredential_files() {
    for (tool, path) in [
        ("grep", "."),
        ("grep", "credentials"),
        ("grep", "secret.key"),
        ("read_file", "credentials/token"),
        ("read_file", ".Env"),
    ] {
        let (_root, ws) = temp_workspace();
        let task = ready_task(&ws);
        std::fs::create_dir(ws.join("credentials")).unwrap();
        for path in ["credentials/token", "secret.key", ".Env"] {
            std::fs::write(ws.join(path), "fixture-confidential-never-forwarded").unwrap();
        }
        let (url, requests) = mock_server(vec![sse_tool_call(
            "credential-search",
            tool,
            json!({"path":path,"pattern":".","glob":"*.key"}),
        )])
        .await;
        let events = std::sync::Mutex::new(Vec::new());
        let result = crate::automatic_review::run(
            &test_config(url, ws),
            &task,
            2,
            &CancellationToken::new(),
            &|event| events.lock().unwrap().push(event),
        )
        .await;
        assert!(result.is_err(), "{tool}: {path}");
        assert_eq!(requests.lock().unwrap().len(), 1);
        assert!(
            !format!("{:?}", events.lock().unwrap())
                .contains("fixture-confidential-never-forwarded")
        );
    }
}

#[tokio::test]
async fn automatic_review_bounds_model_response_before_inspection() {
    let (_root, ws) = temp_workspace();
    let task = ready_task(&ws);
    let (url, requests) = mock_server(vec![sse_tool_call(
        "oversize",
        "read_file",
        json!({"path":"x".repeat(128 * 1024)}),
    )])
    .await;
    let started = std::sync::atomic::AtomicBool::new(false);
    let result = crate::automatic_review::run(
        &test_config(url, ws),
        &task,
        2,
        &CancellationToken::new(),
        &|event| {
            if matches!(event, AgentEvent::ToolCallStarted { .. }) {
                started.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        },
    )
    .await;
    assert!(
        matches!(result, Err(ProviderError::Failed(error)) if error.contains("context budget"))
    );
    assert!(!started.load(std::sync::atomic::Ordering::Relaxed));
    let requests = requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    let grep = requests[0]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["function"]["name"] == "grep")
        .unwrap();
    assert_eq!(
        grep["function"]["parameters"]["required"],
        json!(["pattern", "path"])
    );
    assert!(
        grep["function"]["description"]
            .as_str()
            .unwrap()
            .contains("Directory searches are forbidden")
    );
}

#[tokio::test]
async fn automatic_review_lists_directories_and_searches_safe_files() {
    let (_root, ws) = temp_workspace();
    let task = ready_task(&ws);
    let (url, requests) = mock_server(vec![
        sse_tool_call("list", "list_dir", json!({"path":"."})),
        sse_tool_call(
            "search",
            "grep",
            json!({"path":"result.txt","pattern":"result"}),
        ),
        sse_tool_call("read", "read_file", json!({"path":"result.txt"})),
        verdict("clean", vec![]),
    ])
    .await;
    let review = crate::automatic_review::run(
        &test_config(url, ws),
        &task,
        2,
        &CancellationToken::new(),
        &|_| {},
    )
    .await
    .unwrap();
    assert_eq!(review.inspected_files.len(), 1);
    assert_eq!(requests.lock().unwrap().len(), 4);
}

#[tokio::test]
async fn automatic_review_attributes_equivalent_paths_to_persisted_evidence() {
    let (_root, ws) = temp_workspace();
    let base = ready_task(&ws);
    let paths = [
        "result.txt".to_owned(),
        "./result.txt".to_owned(),
        ws.join("result.txt").to_string_lossy().into_owned(),
    ];
    for tracked in &paths {
        for read in &paths {
            let mut task = base.clone();
            task.files[0].path.clone_from(tracked);
            let (url, _) = mock_server(vec![
                sse_tool_call("read", "read_file", json!({"path":read})),
                verdict("clean", vec![]),
            ])
            .await;
            let review = crate::automatic_review::run(
                &test_config(url, ws.clone()),
                &task,
                2,
                &CancellationToken::new(),
                &|_| {},
            )
            .await
            .unwrap();
            assert_eq!(&review.inspected_files[0].path, tracked);
            task.record_automatic_review(review).unwrap();
            assert!(task.is_automatically_verified());
        }
    }
}

#[cfg(unix)]
#[tokio::test]
async fn automatic_review_covers_safe_aliases_without_duplicate_receipts() {
    let (_root, ws) = temp_workspace();
    let mut task = ready_task(&ws);
    std::os::unix::fs::symlink("result.txt", ws.join("alias.txt")).unwrap();
    let mut alias = task.files[0].clone();
    alias.path = "alias.txt".into();
    task.files.push(alias);
    let (url, _) = mock_server(vec![
        sse_tool_call("read", "read_file", json!({"path":"./alias.txt"})),
        sse_tool_call("reread", "read_file", json!({"path":"result.txt"})),
        verdict("clean", vec![]),
    ])
    .await;
    let review = crate::automatic_review::run(
        &test_config(url, ws),
        &task,
        2,
        &CancellationToken::new(),
        &|_| {},
    )
    .await
    .unwrap();
    assert_eq!(review.inspected_files.len(), 2);
    task.record_automatic_review(review).unwrap();
    assert!(task.is_automatically_verified());
}

#[cfg(unix)]
#[tokio::test]
async fn automatic_review_rejects_credential_links_and_outside_aliases() {
    for alias in ["symlink", "hardlink", "outside"] {
        for tool in ["read_file", "grep"] {
            let (_root, ws) = temp_workspace();
            let task = ready_task(&ws);
            let outside = tempfile::tempdir().unwrap();
            std::fs::write(ws.join(".env"), "fixture-confidential-never-forwarded").unwrap();
            std::fs::write(
                outside.path().join("private.txt"),
                "fixture-confidential-never-forwarded",
            )
            .unwrap();
            match alias {
                "symlink" => std::os::unix::fs::symlink(".env", ws.join("alias.txt")).unwrap(),
                "hardlink" => std::fs::hard_link(ws.join(".env"), ws.join("alias.txt")).unwrap(),
                _ => std::os::unix::fs::symlink(
                    outside.path().join("private.txt"),
                    ws.join("alias.txt"),
                )
                .unwrap(),
            }
            let (url, requests) = mock_server(vec![sse_tool_call(
                "alias",
                tool,
                json!({"path":"alias.txt","pattern":"."}),
            )])
            .await;
            let result = crate::automatic_review::run(
                &test_config(url, ws),
                &task,
                2,
                &CancellationToken::new(),
                &|_| {},
            )
            .await;
            assert!(result.is_err(), "{alias}: {tool}");
            assert_eq!(requests.lock().unwrap().len(), 1);
        }
    }
}

#[cfg(unix)]
#[tokio::test]
async fn automatic_review_withholds_output_when_source_becomes_protected() {
    let (_root, ws) = temp_workspace();
    let task = ready_task(&ws);
    std::fs::write(ws.join(".env"), "fixture-confidential-never-forwarded").unwrap();
    let (url, requests) = mock_server(vec![sse_tool_call(
        "read",
        "read_file",
        json!({"path":"result.txt"}),
    )])
    .await;
    let events = std::sync::Mutex::new(Vec::new());
    let result = crate::automatic_review::run(
        &test_config(url, ws.clone()),
        &task,
        2,
        &CancellationToken::new(),
        &|event| {
            if matches!(&event, AgentEvent::ToolCallStarted { .. }) {
                std::fs::remove_file(ws.join("result.txt")).unwrap();
                std::os::unix::fs::symlink(".env", ws.join("result.txt")).unwrap();
            }
            events.lock().unwrap().push(event);
        },
    )
    .await;
    assert!(result.is_err());
    assert_eq!(requests.lock().unwrap().len(), 1);
    let events = events.lock().unwrap();
    assert!(!format!("{events:?}").contains("fixture-confidential-never-forwarded"));
    assert!(
        events
            .iter()
            .any(|event| matches!(event, AgentEvent::ToolCallFinished { success: false, .. }))
    );
}

#[tokio::test]
async fn automatic_review_rejects_oversized_sources_before_starting_tools() {
    for tool in ["read_file", "grep"] {
        let (_root, ws) = temp_workspace();
        let task = ready_task(&ws);
        // A sparse fixture proves the metadata budget without allocating or
        // reading a large payload, and exercises the real fingerprint path.
        std::fs::File::create(ws.join("oversized.txt"))
            .unwrap()
            .set_len(8 * 1024 * 1024 + 1)
            .unwrap();
        let (url, requests) = mock_server(vec![sse_tool_call(
            "oversized-source",
            tool,
            json!({"path":"oversized.txt","pattern":"."}),
        )])
        .await;
        let started = std::sync::atomic::AtomicBool::new(false);
        let result = crate::automatic_review::run(
            &test_config(url, ws),
            &task,
            2,
            &CancellationToken::new(),
            &|event| {
                if matches!(event, AgentEvent::ToolCallStarted { .. }) {
                    started.store(true, std::sync::atomic::Ordering::Relaxed);
                }
            },
        )
        .await;
        assert!(
            matches!(result, Err(ProviderError::Failed(error)) if error.contains("file budget"))
        );
        assert!(!started.load(std::sync::atomic::Ordering::Relaxed));
        assert_eq!(requests.lock().unwrap().len(), 1);
        assert!(!task.is_automatically_verified());
    }
}
