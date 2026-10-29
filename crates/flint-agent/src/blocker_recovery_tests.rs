//! Scripted inference, real disposable inspection tools, no credentials.
use super::*;
use pretty_assertions::assert_eq;

const REQUEST: &str = "what did i do recently in the past 3 days? which repos were created";
const BLOCKER: &str = "The workspace does not contain Git metadata or timestamped artifacts \
    sufficient to identify which repositories were created or what work was done.";

fn report(blockers: &[&str], resolved: &[&str]) -> String {
    sse_tool_call(
        "report",
        "report_task",
        json!({
            "summary":"The scratch folder is not the project inventory. Activity records \
                       identify recent work; repository creation dates remain unconfirmed.",
            "deliverables":[], "blockers":blockers, "resolved_blockers":resolved,
        }),
    )
}

fn recovery_nudges(events: &[AgentEvent]) -> usize {
    events
        .iter()
        .filter(|event| {
            matches!(
                event,
                AgentEvent::HarnessNudge {
                    reason: NudgeReason::BlockerRecovery,
                    ..
                }
            )
        })
        .count()
}

async fn recovered_turn(judge: Option<JevConfig>) -> (Vec<AgentEvent>, Arc<StdMutex<Vec<Value>>>) {
    let (_root, ws) = temp_workspace();
    std::fs::write(
        ws.join("activity.md"),
        "Recent work: project-alpha, inspected yesterday.",
    )
    .unwrap();
    let (url, requests) = mock_server(vec![
        sse_tool_call("list", "list_dir", json!({"path":"."})),
        report(&[BLOCKER], &[]),
        sse_text("I could not confirm repository activity from the scratch folder."),
        sse_tool_call("activity", "read_file", json!({"path":"activity.md"})),
        report(&[], &[BLOCKER]),
        sse_text("Activity records show work on project-alpha. Its creation date is unconfirmed."),
    ])
    .await;
    let mut config = test_config(url, ws);
    config.general = true;
    config.jev = judge;
    (run_one_turn_config(config, REQUEST).await, requests)
}

#[tokio::test]
async fn scratch_folder_blocker_recovers_with_jev_off_without_tests_or_edits() {
    let (events, requests) = recovered_turn(None).await;
    let task = assessed(&events);
    assert_eq!(requests.lock().unwrap().len(), 6);
    assert_eq!(recovery_nudges(&events), 1);
    assert!(task.blockers.is_empty());
    assert_eq!(
        task.assessment.as_ref().unwrap().outcome,
        TaskOutcome::Answered
    );
    assert_eq!(task.contract.original_request, REQUEST);
    assert_eq!(task.last_inspection.as_ref().unwrap().tool, "read_file");
    assert_eq!(task.observed_edits, 0);
    assert!(!task.has_current_verification() && !task.is_accepted());
    assert!(!events.iter().any(|event| matches!(
        event,
        AgentEvent::ToolCallStarted {
            kind: ToolKind::Edit | ToolKind::Command,
            ..
        } | AgentEvent::JevVerdict { .. }
    )));
}

#[tokio::test]
async fn jev_assesses_blocker_recovery_and_receives_bounded_evidence_and_tools() {
    let (base_url, judgments) = mock_judge(0.9).await;
    let (events, _) = recovered_turn(Some(JevConfig {
        api_key: "test-key".into(),
        base_url,
        model: "jev-test".into(),
    }))
    .await;
    assert!(assessed(&events).blockers.is_empty());
    assert_eq!(recovery_nudges(&events), 1);
    assert!(events.iter().any(|event| matches!(
        event,
        AgentEvent::JevVerdict {
            question: JevQuestion::BlockerRecovery,
            confirmed: true,
            ..
        }
    )));
    let judgments = judgments.lock().unwrap();
    assert_eq!(judgments.len(), 1);
    let state = &judgments[0]["state"];
    assert!(judgments[0]["questions"]["blocker_recovery"].is_object());
    assert_eq!(state["task_contract"]["mode"], "conversation");
    assert_eq!(state["reported_blockers"][0]["text"], BLOCKER);
    assert_eq!(state["permissions_unchanged"], true);
    assert!(
        state["available_tools"]
            .as_array()
            .unwrap()
            .contains(&json!("read_file"))
    );
    assert!(state["recent_tool_calls"].as_array().unwrap().len() <= 8);
}

#[tokio::test]
async fn jev_can_decline_recovery_but_cannot_clear_the_blocker() {
    let (_root, ws) = temp_workspace();
    let (base_url, judgments) = mock_judge(0.1).await;
    let (url, requests) = mock_server(vec![
        report(&[BLOCKER], &[]),
        sse_text("Please provide the relevant project location."),
    ])
    .await;
    let mut config = test_config(url, ws);
    config.jev = Some(JevConfig {
        api_key: "test-key".into(),
        base_url,
        model: "test".into(),
    });
    let events = run_one_turn_config(config, REQUEST).await;
    assert_eq!(requests.lock().unwrap().len(), 2);
    assert_eq!(judgments.lock().unwrap().len(), 1);
    assert_eq!(recovery_nudges(&events), 0);
    assert_eq!(assessed(&events).blockers.len(), 1);
    assert_eq!(
        assessed(&events).assessment.as_ref().unwrap().outcome,
        TaskOutcome::Blocked
    );
    assert!(events.iter().any(|event| matches!(
        event,
        AgentEvent::JevVerdict {
            question: JevQuestion::BlockerRecovery,
            confirmed: false,
            ..
        }
    )));
}

#[tokio::test]
async fn unavailable_jev_uses_the_same_bounded_blocker_fallback() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);
    let (events, requests) = recovered_turn(Some(JevConfig {
        api_key: "test-key".into(),
        base_url,
        model: "test".into(),
    }))
    .await;
    assert_eq!(requests.lock().unwrap().len(), 6);
    assert_eq!(recovery_nudges(&events), 1);
    assert!(assessed(&events).blockers.is_empty());
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, AgentEvent::JevVerdict { .. }))
    );
}

#[tokio::test]
async fn repeated_or_omitted_blocker_claims_get_only_one_recovery_pass() {
    for repeat in [false, true] {
        let (_root, ws) = temp_workspace();
        let (url, requests) = mock_server(vec![
            report(&[BLOCKER], &[]),
            sse_text("The scratch folder has no Git metadata."),
            report(if repeat { &[BLOCKER] } else { &[] }, &[]),
            sse_text("I still cannot establish the requested activity."),
        ])
        .await;
        let events = run_one_turn(url, ws, REQUEST).await;
        assert_eq!(requests.lock().unwrap().len(), 4);
        assert_eq!(recovery_nudges(&events), 1);
        assert_eq!(assessed(&events).blockers.len(), 1);
        assert_eq!(
            assessed(&events).assessment.as_ref().unwrap().outcome,
            TaskOutcome::Blocked
        );
        assert!(!assessed(&events).is_accepted());
    }
}

#[tokio::test]
async fn final_text_blocker_claim_also_gets_a_bounded_recovery_assessment() {
    let (_root, ws) = temp_workspace();
    let (url, requests) = mock_server(vec![
        sse_text(&format!("Blocked: {BLOCKER}")),
        sse_text(&format!("Blocked: {BLOCKER}")),
    ])
    .await;
    let events = run_one_turn(url, ws, REQUEST).await;
    assert_eq!(requests.lock().unwrap().len(), 2);
    assert_eq!(recovery_nudges(&events), 1);
    assert_eq!(assessed(&events).blockers.len(), 1);
}

#[tokio::test]
async fn observed_missing_input_is_not_overridden_even_by_a_pro_recovery_judge() {
    let (_root, ws) = temp_workspace();
    let (base_url, judgments) = mock_judge(1.0).await;
    let (url, requests) = mock_server(vec![
        sse_tool_call("missing", "read_file", json!({"path":"missing.md"})),
        report(&["The required missing.md input is unavailable."], &[]),
        sse_text("Please provide missing.md."),
    ])
    .await;
    let mut config = test_config(url, ws);
    config.jev = Some(JevConfig {
        api_key: "test-key".into(),
        base_url,
        model: "test".into(),
    });
    let events = run_one_turn_config(config, "Analyze missing.md. Do not change files.").await;
    assert_eq!(requests.lock().unwrap().len(), 3);
    assert_eq!(recovery_nudges(&events), 0);
    assert!(judgments.lock().unwrap().is_empty());
    assert_eq!(
        assessed(&events).assessment.as_ref().unwrap().outcome,
        TaskOutcome::Blocked
    );
}

#[tokio::test]
async fn read_only_denial_and_explicit_required_check_remain_blocked() {
    let (_root, ws) = temp_workspace();
    let (url, requests) = mock_server(vec![
        sse_tool_call("denied", "run_command", json!({"command":"cargo test"})),
        report(
            &["The explicitly required cargo test cannot run in read-only scope."],
            &[],
        ),
        sse_text("The required check needs a scope revision."),
    ])
    .await;
    let events = run_one_turn(url, ws, "Analyze only.\nRequired check: cargo test").await;
    assert_eq!(requests.lock().unwrap().len(), 3);
    assert_eq!(recovery_nudges(&events), 0);
    assert_eq!(assessed(&events).scope_denials.len(), 1);
    assert_eq!(
        assessed(&events).assessment.as_ref().unwrap().outcome,
        TaskOutcome::Blocked
    );
    assert!(!assessed(&events).has_current_verification());
}

#[cfg(unix)]
#[tokio::test]
async fn saved_failure_shape_recovers_without_inventing_code_verification() {
    let (_root, ws) = temp_workspace();
    let projects = tempfile::tempdir().unwrap();
    std::fs::write(
        projects.path().join("activity.md"),
        "Recent work: project-alpha.",
    )
    .unwrap();
    let command = format!("ls '{}'", projects.path().display());
    let (url, requests) = mock_server(vec![
        sse_tool_call(
            "find",
            "run_command",
            json!({
                "command":"find . -maxdepth 3 -type f -newermt '3 days ago' -print | sort"
            }),
        ),
        report(&[BLOCKER], &[]),
        sse_text("No activity was found in the scratch folder."),
        sse_tool_call("discover", "run_command", json!({"command":command})),
        report(&[], &[BLOCKER]),
        sse_text(
            "The project activity source is available outside the scratch folder; \
                  repository creation dates still need confirmation.",
        ),
    ])
    .await;
    let mut config = test_config(url, ws);
    config.general = true;
    config.sandbox = true;
    let events = run_one_turn_config(config, REQUEST).await;
    let task = assessed(&events);
    assert_eq!(requests.lock().unwrap().len(), 6);
    assert_eq!(recovery_nudges(&events), 1);
    assert!(task.blockers.is_empty());
    assert!(
        task.unknown_effects,
        "conservative shell evidence remains uncertain"
    );
    assert_eq!(
        task.assessment.as_ref().unwrap().outcome,
        TaskOutcome::NeedsReview
    );
    assert!(!task.has_current_verification() && !task.verified_for_queue());
    assert!(!events.iter().any(|event| matches!(
        event,
        AgentEvent::HarnessNudge {
            reason: NudgeReason::Verify,
            ..
        }
    )));
}
