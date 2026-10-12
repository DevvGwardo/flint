use super::*;
use crate::worktree::{Checkout, Request, prepare};
use pretty_assertions::assert_eq;

#[test]
fn screenshot_capability_question_is_read_only() {
    for prompt in [
        "dont make any changes, are you able to work in a worktree",
        "Do not make any changes. Can you work in a worktree?",
        "Don't make changes, can you use a worktree?",
        "Inspect the saved checkout without changes.",
    ] {
        assert_eq!(
            crate::task::TaskMode::infer(prompt),
            crate::task::TaskMode::ReadOnly
        );
    }
}

async fn fixture() -> (tempfile::TempDir, PathBuf, Checkout) {
    let temp = tempfile::tempdir().unwrap();
    let repo = crate::worktree::tests::repository(temp.path());
    let checkout = prepare(
        &Request {
            branch: "agent-task".into(),
            base: "HEAD".into(),
            repository: None,
        },
        &repo,
        &temp.path().join("home"),
    )
    .await
    .unwrap();
    (temp, repo, checkout)
}

async fn approved_turn(
    config: AgentConfig,
    prompt: &str,
    decision: ApprovalDecision,
) -> Vec<AgentEvent> {
    let handle = crate::spawn_session(config);
    handle
        .ops
        .send(Op::UserMessage(prompt.into()))
        .await
        .unwrap();
    let mut events = Vec::new();
    loop {
        let event = tokio::time::timeout(Duration::from_secs(20), handle.events.recv())
            .await
            .unwrap()
            .unwrap();
        if let AgentEvent::ApprovalRequested { call_id, .. } = &event {
            handle
                .ops
                .send(Op::Approval {
                    call_id: call_id.clone(),
                    decision,
                })
                .await
                .unwrap();
        }
        let done = matches!(event, AgentEvent::TurnFinished { .. });
        events.push(event);
        if done {
            break;
        }
    }
    handle.ops.send(Op::Shutdown).await.unwrap();
    while let Ok(event) = handle.events.recv().await {
        events.push(event);
    }
    events
}

fn outcome(events: &[AgentEvent], id: &str) -> (String, bool) {
    events
        .iter()
        .find_map(|event| match event {
            AgentEvent::ToolCallFinished {
                call_id,
                output,
                success,
                ..
            } if call_id == id => Some((output.clone(), *success)),
            _ => None,
        })
        .unwrap()
}

#[tokio::test]
async fn worktree_child_uses_real_file_and_command_workspace_and_resumes_after_restart() {
    let (temp, repo, checkout) = fixture().await;
    let parent = temp.path().join("general");
    std::fs::create_dir(&parent).unwrap();
    let (url, requests) = mock_server(vec![
        sse_tool_call(
            "delegate",
            "spawn_agent",
            json!({
                "label":"Worktree task", "message":"Create generated.txt and verify it exists.",
                "worktree":{"repository":repo, "branch":"agent-task"}
            }),
        ),
        sse_tool_call(
            "write",
            "write_file",
            json!({"path":"generated.txt", "content":"child output\n"}),
        ),
        sse_tool_call(
            "check",
            "run_command",
            json!({"command":"test -f generated.txt"}),
        ),
        sse_text("Created and checked generated.txt."),
        sse_text("The result is in the isolated checkout."),
    ])
    .await;
    let mut config = test_config(url, parent.clone());
    config.general = true;
    config.sandbox = true;
    config.session_dir = Some(temp.path().join("saved"));
    let events = approved_turn(
        config.clone(),
        "Create generated.txt in the repository's agent-task worktree.",
        ApprovalDecision::Approve,
    )
    .await;
    assert!(events.iter().any(|event| matches!(event, AgentEvent::ApprovalRequested { call_id, .. } if call_id == "delegate")));
    assert!(events.iter().any(|event| matches!(event, AgentEvent::SubagentWorkspace { workspace, .. } if workspace == &checkout.workspace)));
    assert_eq!(
        std::fs::read_to_string(checkout.workspace.join("generated.txt")).unwrap(),
        "child output\n"
    );
    assert!(!repo.join("generated.txt").exists());
    assert!(!parent.join("generated.txt").exists());
    let (output, success) = outcome(&events, "delegate");
    assert!(success, "{output}");
    assert_eq!(
        serde_json::from_str::<Value>(&output).unwrap()["workspace"],
        json!(checkout.workspace)
    );
    {
        let seen = requests.lock().unwrap();
        assert!(
            seen[1]["messages"][0]["content"]
                .as_str()
                .unwrap()
                .contains(checkout.workspace.to_str().unwrap())
        );
    }
    let meta: Value = serde_json::from_slice(
        &std::fs::read(temp.path().join("saved/subagents/agent-1/subagent.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(meta["worktree"]["workspace"], json!(checkout.workspace));
    assert!(events.iter().any(
        |event| matches!(event, AgentEvent::TaskAssessed { task, .. }
        if task.files.is_empty() && task.checks.is_empty() && !task.children.is_empty())
    ));

    let (url, _) = mock_server(vec![
        sse_tool_call("resume", "spawn_agent", json!({"label":"Inspect", "message":"Read generated.txt without changes.", "session_id":"agent-1"})),
        sse_tool_call("read", "read_file", json!({"path":"generated.txt"})),
        sse_text("The file contains child output."),
        sse_text("The saved checkout was inspected without changes."),
    ]).await;
    config.base_url = url;
    let events = approved_turn(
        config,
        "Inspect the saved child's generated.txt without changes.",
        ApprovalDecision::Approve,
    )
    .await;
    let (output, success) = outcome(&events, "resume");
    assert!(success, "{output}");
    assert!(events.iter().any(|event| matches!(event,
        AgentEvent::SubagentEvent { event, .. } if matches!(event.as_ref(),
            AgentEvent::ToolCallFinished { call_id, output, success:true, .. } if call_id.ends_with(":read") && output.contains("child output")
        )
    )));
}

#[tokio::test]
async fn denied_worktree_scope_does_not_create_a_checkout_even_in_auto_mode() {
    let temp = tempfile::tempdir().unwrap();
    let repo = crate::worktree::tests::repository(temp.path());
    let before = crate::worktree::tests::run(&repo, &["worktree", "list", "--porcelain"]);
    let (url, _) = mock_server(vec![
        sse_tool_call(
            "delegate",
            "spawn_agent",
            json!({"label":"Task","message":"Create a file.","worktree":{"branch":"not-approved"}}),
        ),
        sse_text("Worktree access was declined."),
    ])
    .await;
    let events = approved_turn(
        test_config(url, repo.clone()),
        "Create a file in an isolated checkout.",
        ApprovalDecision::Deny,
    )
    .await;
    assert!(!outcome(&events, "delegate").1);
    assert_eq!(
        crate::worktree::tests::run(&repo, &["worktree", "list", "--porcelain"]),
        before
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, AgentEvent::SubagentStarted { .. }))
    );
}

#[tokio::test]
async fn read_only_request_blocks_worktree_preparation_before_approval() {
    let temp = tempfile::tempdir().unwrap();
    let repo = crate::worktree::tests::repository(temp.path());
    let (url, _) = mock_server(vec![
        sse_tool_call(
            "delegate",
            "spawn_agent",
            json!({"label":"Inspect","message":"Inspect only.","worktree":{"branch":"forbidden"}}),
        ),
        sse_text("I can use Git worktrees for editing tasks. No changes were made."),
    ])
    .await;
    let events = run_one_turn(
        url,
        repo,
        "Do not make any changes. Can you work in a worktree?",
    )
    .await;
    assert!(outcome(&events, "delegate").0.contains("read-only"));
    assert!(!events.iter().any(|event| matches!(
        event,
        AgentEvent::ApprovalRequested { .. } | AgentEvent::SubagentStarted { .. }
    )));
}

#[tokio::test]
async fn two_children_cannot_run_in_the_same_worktree_and_resume_cannot_change_it() {
    let (_temp, repo, _) = fixture().await;
    let config = test_config("http://unused.invalid/v1".into(), repo);
    let mut children = Subagents::new(
        config.clone(),
        tools::ToolContext::new(config.workspace.clone(), false),
        None,
    );
    let approvals = Arc::new(Approvals::default());
    let args = json!({"label":"Task", "message":"Inspect.", "worktree":{"branch":"agent-task"}});
    let (child, _) = children
        .prepare(args.as_object().unwrap(), None, approvals.clone(), None)
        .await
        .unwrap();
    assert!(
        children
            .prepare(args.as_object().unwrap(), None, approvals.clone(), None)
            .await
            .err()
            .unwrap()
            .contains("already using")
    );
    let id = child.id.clone();
    children.put(child);
    let changed = json!({"label":"Task", "message":"Inspect.", "session_id":id, "worktree":{"branch":"different"}});
    assert!(
        children
            .prepare(changed.as_object().unwrap(), None, approvals, None)
            .await
            .err()
            .unwrap()
            .contains("cannot be changed")
    );
}
