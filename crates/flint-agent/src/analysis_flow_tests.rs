//! Reviews after editing must use the current scope, not the old workflow.
use super::*;
use pretty_assertions::assert_eq;

const REQUEST: &str = "analyze this for improvements and gaps to fix";
const POLICY_BLOCKER: &str = "Task is now in read-only mode: edit_file is rejected \
    ('The protected task is read-only'). I cannot update ANALYSIS.md.";

#[tokio::test]
async fn restored_editing_history_returns_review_without_optional_write_blockers() {
    let (_root, ws) = temp_workspace();
    let saved = tempfile::tempdir().unwrap();
    std::fs::write(ws.join("source.ts"), "export const motion = true;\n").unwrap();
    std::fs::write(ws.join("ANALYSIS.md"), "Earlier report.\n").unwrap();
    let mut previous = TaskRecord::new(1, "Implement the feature and verify it.".into(), 10);
    previous.observed_edits = 1;
    previous.assess(TurnEndReason::Completed, true);
    persist::write_atomic(
        saved.path(),
        &persist::snapshot_with_task(
            1,
            &[
                Message::User(previous.contract.original_request.clone()),
                Message::Assistant {
                    content: "Updated source.ts and ANALYSIS.md. Tests passed earlier.".into(),
                    reasoning: String::new(),
                    tool_calls: Vec::new(),
                },
            ],
            Some(&previous),
        ),
    )
    .unwrap();
    let (url, requests) = mock_server(vec![
        sse_tool_call("read", "read_file", json!({"path":"source.ts"})),
        sse_tool_call(
            "edit",
            "edit_file",
            json!({
                "path":"ANALYSIS.md","old_string":"Earlier report.","new_string":"Changed report."
            }),
        ),
        sse_tool_call("test", "run_command", json!({"command":"bun test"})),
        sse_tool_call(
            "bad-report",
            "report_task",
            json!({
                "summary":"I found a motion accessibility gap.",
                "blockers":[POLICY_BLOCKER],"deliverables":[]
            }),
        ),
        sse_tool_call(
            "review-report",
            "report_task",
            json!({
                "summary":"The motion flag is unconditional. Add reduced-motion support.",
                "blockers":[],"deliverables":[],
                "proposed_actions":["Add reduced-motion support, update the report and run tests."]
            }),
        ),
        sse_text(
            "The motion flag is unconditional. I have not changed files or rerun tests. \
                  Would you like me to add reduced-motion support?",
        ),
    ])
    .await;
    let mut config = test_config(url, ws.clone());
    config.session_dir = Some(saved.path().into());
    let events = run_one_turn_config(config, REQUEST).await;
    let task = assessed(&events);
    assert_eq!(task.contract.id, 2);
    assert_eq!(task.contract.mode, TaskMode::ReadOnly);
    assert_eq!(task.observed_edits, 0);
    assert!(!task.unknown_effects);
    assert!(task.blockers.is_empty());
    assert_eq!(task.scope_denials.len(), 2);
    assert_eq!(task.proposed_actions.len(), 1);
    assert!(!task.checks.iter().any(|check| check.verification));
    assert!(!task.has_current_verification());
    assert!(!task.verified_for_queue());
    assert!(!task.is_accepted());
    assert_eq!(
        task.assessment.as_ref().unwrap().outcome,
        TaskOutcome::NeedsReview
    );
    assert_eq!(
        std::fs::read_to_string(ws.join("source.ts")).unwrap(),
        "export const motion = true;\n"
    );
    assert_eq!(
        std::fs::read_to_string(ws.join("ANALYSIS.md")).unwrap(),
        "Earlier report.\n"
    );
    assert!(events.iter().any(|event| matches!(event,
        AgentEvent::ToolCallFinished {call_id, success:false, output, ..}
        if call_id=="bad-report" && output.contains("Return the analysis in chat"))));
    let nudges: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            AgentEvent::HarnessNudge { reason, .. } => Some(reason),
            _ => None,
        })
        .collect();
    assert!(nudges.len() <= 1);
    assert!(
        nudges.iter().all(|reason| **reason == NudgeReason::Stuck),
        "rejected optional actions must not trigger verification or finish loops"
    );
    for request in requests.lock().unwrap().iter() {
        let names: Vec<_> = request["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|tool| tool["function"]["name"].as_str())
            .collect();
        assert!(!names.contains(&"edit_file") && !names.contains(&"write_file"));
        let context = request["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|message| message["content"].as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(context.contains("current contract overrides earlier conversation history"));
        assert!(context.contains("Tests passed earlier."));
    }
    let (_, restored) = crate::saved_task(saved.path()).unwrap().unwrap();
    assert_eq!(restored.proposed_actions, task.proposed_actions);
    assert_eq!(restored.scope_denials, task.scope_denials);
    restored.validate().unwrap();
}

#[tokio::test]
async fn a_final_optional_policy_blocker_gets_one_bounded_review_correction() {
    for recover in [false, true] {
        let (_root, ws) = temp_workspace();
        let blocked = format!("Blocked: {POLICY_BLOCKER}");
        let last = if recover {
            "Findings are ready in chat. No files changed. Would you like me to write a report?"
        } else {
            blocked.as_str()
        };
        let (url, requests) = mock_server(vec![
            sse_tool_call(
                "write",
                "write_file",
                json!({"path":"ANALYSIS.md","content":"unrequested"}),
            ),
            sse_text(&blocked),
            sse_text(last),
        ])
        .await;
        let events = run_one_turn(url, ws.clone(), REQUEST).await;
        let task = assessed(&events);
        assert_eq!(requests.lock().unwrap().len(), 3);
        assert!(task.blockers.is_empty());
        assert_eq!(task.observed_edits, 0);
        assert!(!ws.join("ANALYSIS.md").exists());
        assert_eq!(
            task.assessment.as_ref().unwrap().outcome,
            if recover {
                TaskOutcome::NeedsReview
            } else {
                TaskOutcome::Partial
            }
        );
        assert_eq!(
            task.assessment.as_ref().unwrap().reason,
            if recover {
                TurnEndReason::Completed
            } else {
                TurnEndReason::ContinuationLimit
            }
        );
        assert!(!task.is_accepted() && !task.verified_for_queue());
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, AgentEvent::HarnessNudge { .. }))
                .count(),
            1
        );
    }
}

#[tokio::test]
async fn explaining_a_denied_optional_write_is_not_itself_a_finish_blocker() {
    let (_root, ws) = temp_workspace();
    let (url, requests) = mock_server(vec![
        sse_tool_call(
            "write",
            "write_file",
            json!({"path":"ANALYSIS.md","content":"unrequested"}),
        ),
        sse_text(
            "This review is read-only; edit_file was unavailable for ANALYSIS.md, \
                  so I am returning the findings here. The motion flag needs review.",
        ),
    ])
    .await;
    let events = run_one_turn(url, ws, REQUEST).await;
    assert_eq!(requests.lock().unwrap().len(), 2);
    assert_eq!(
        assessed(&events).assessment.as_ref().unwrap().outcome,
        TaskOutcome::NeedsReview
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, AgentEvent::HarnessNudge { .. }))
    );
}

#[tokio::test]
async fn genuinely_missing_review_input_remains_blocked() {
    let (_root, ws) = temp_workspace();
    let (url, _) = mock_server(vec![
        sse_tool_call("read", "read_file", json!({"path":"missing-source.ts"})),
        sse_tool_call(
            "blocked",
            "report_task",
            json!({
                "summary":"The requested source file is missing.",
                "blockers":["missing-source.ts is unavailable; provide the file to review."],
                "deliverables":[],
                "proposed_actions":["Provide missing-source.ts."]
            }),
        ),
        sse_text("Blocked: the source file needed for the review is missing."),
    ])
    .await;
    let events = run_one_turn(url, ws, "Analyze missing-source.ts. Do not change files.").await;
    let task = assessed(&events);
    assert_eq!(
        task.assessment.as_ref().unwrap().outcome,
        TaskOutcome::Blocked
    );
    assert!(!task.blockers.is_empty());
    assert!(!task.is_accepted());
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, AgentEvent::HarnessNudge { .. }))
    );
}

#[tokio::test]
async fn a_required_denied_check_is_not_reported_as_executed_or_as_optional() {
    let (_root, ws) = temp_workspace();
    let blocker = "The protected task is read-only; bun test cannot run.";
    let (url, _) = mock_server(vec![
        sse_tool_call("test", "run_command", json!({"command":"bun test"})),
        sse_tool_call(
            "blocked",
            "report_task",
            json!({
                "summary":"The explicitly required check cannot run in this scope.",
                "blockers":[blocker],"deliverables":[]
            }),
        ),
        sse_text("Blocked: the required check needs a scope revision."),
    ])
    .await;
    let events = run_one_turn(url, ws, "Analyze only.\nRequired check: bun test").await;
    let task = assessed(&events);
    let check = &task.checks[0];
    assert!(!check.success && !check.verification);
    assert_eq!(check.exit_code, None);
    assert_eq!(
        task.assessment.as_ref().unwrap().outcome,
        TaskOutcome::Blocked
    );
    assert!(
        task.assessment
            .as_ref()
            .unwrap()
            .requirements
            .iter()
            .any(|requirement| requirement.state == EvidenceState::Failed)
    );
    assert!(!task.has_current_verification());
    assert!(
        task.blockers
            .iter()
            .any(|candidate| candidate.text == blocker)
    );
}

#[tokio::test]
async fn explicit_new_work_after_review_restores_edit_tools_and_requires_checks() {
    let (_root, ws) = temp_workspace();
    let command = exists_check(&ws, "result.txt");
    let (url, requests) = mock_server(vec![
        sse_text("I found a gap. Would you like me to fix it?"),
        sse_tool_call(
            "write",
            "write_file",
            json!({"path":"result.txt","content":"fixed"}),
        ),
        sse_tool_call("check", "run_command", json!({"command":command})),
        sse_text("Implemented the requested fix and ran the check."),
    ])
    .await;
    let events = run_turns(
        test_config(url, ws),
        &[REQUEST, "Please fix the gap and verify the result."],
    )
    .await;
    let task = assessed(&events);
    assert_eq!(task.contract.mode, TaskMode::Work);
    assert_eq!(task.observed_edits, 1);
    assert!(task.has_current_verification());
    assert!(task.verified_for_queue());
    assert!(!task.is_accepted());
    let seen = requests.lock().unwrap();
    assert!(
        !seen[0]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool["function"]["name"] == "write_file")
    );
    assert!(
        seen[1]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool["function"]["name"] == "write_file")
    );
}
