use super::*;
use pretty_assertions::assert_eq;

fn spawn_args(message: &str) -> Value {
    json!({"label": "Research", "message": message})
}

fn result(events: &[AgentEvent], id: &str) -> (Value, bool) {
    events
        .iter()
        .find_map(|event| match event {
            AgentEvent::ToolCallFinished {
                call_id,
                output,
                success,
                ..
            } if call_id == id => Some((
                serde_json::from_str(output).unwrap_or_else(|_| json!(output)),
                *success,
            )),
            _ => None,
        })
        .expect("tool result")
}

fn multi_spawn(count: usize) -> String {
    let calls: Vec<Value> = (0..count).map(|i| json!({
        "index": i, "id": format!("delegate-{i}"), "type": "function",
        "function": {"name": "spawn_agent", "arguments": spawn_args("Research the project.").to_string()}
    })).collect();
    sse(&[json!({"choices": [{"delta": {"tool_calls": calls}, "finish_reason": "tool_calls"}]})])
}

#[tokio::test]
async fn corrupt_subagent_history_reports_its_restore_error_in_the_child_card() {
    let (_guard, ws) = temp_workspace();
    let sessions = tempfile::tempdir().expect("tempdir");
    let child_dir = sessions.path().join("subagents/agent-1");
    std::fs::create_dir_all(&child_dir).expect("directory");
    std::fs::write(
        child_dir.join("subagent.json"),
        r#"{"model":"child-model"}"#,
    )
    .expect("metadata");
    std::fs::write(child_dir.join("history.json"), "damaged history").expect("history");
    let (url, _) = mock_server(vec![
        sse_tool_call(
            "resume",
            "spawn_agent",
            json!({
                "label": "Follow-up", "message": "Report.", "session_id": "agent-1"
            }),
        ),
        sse_text("Child."),
        sse_text("Parent."),
    ])
    .await;
    let mut config = test_config(url, ws);
    config.session_dir = Some(sessions.path().to_path_buf());
    let events = run_turns(config, &["Research."]).await;
    assert!(result(&events, "resume").1);
    assert!(child_dir.join("history.json.corrupt").exists());
    assert!(events.iter().any(|event| matches!(event,
        AgentEvent::SubagentEvent { event, .. } if matches!(event.as_ref(),
            AgentEvent::Error(error) if error.starts_with("Couldn't restore the saved conversation")
        )
    )));
}

#[tokio::test]
async fn subagent_models_are_paginated_without_limiting_override_validation() {
    let (_guard, ws) = temp_workspace();
    let models: Vec<_> = (0..205)
        .map(|i| json!({"id": format!("model-{i:03}")}))
        .collect();
    let (url, requests) = mock_server_with_models(
        vec![
            sse_tool_call("first", "list_models", json!({})),
            sse_tool_call("second", "list_models", json!({"offset": 100})),
            sse_tool_call("last", "list_models", json!({"offset": 200})),
            sse_tool_call(
                "delegate",
                "spawn_agent",
                json!({
                    "label": "Research", "message": "Report.", "model": "model-204"
                }),
            ),
            sse_text("Child."),
            sse_text("Parent."),
        ],
        json!({"data": models}),
    )
    .await;
    let events = run_one_turn(url, ws, "Research.").await;
    assert_eq!(
        result(&events, "first").0["models"]
            .as_array()
            .expect("models")
            .len(),
        100
    );
    assert_eq!(result(&events, "first").0["next_offset"], 100);
    assert_eq!(result(&events, "second").0["next_offset"], 200);
    assert_eq!(
        result(&events, "last").0["models"]
            .as_array()
            .expect("models")
            .len(),
        5
    );
    assert!(result(&events, "last").0["next_offset"].is_null());
    assert!(result(&events, "delegate").1);
    assert_eq!(requests.lock().expect("lock")[4]["model"], "model-204");
}

#[tokio::test]
async fn blank_session_ids_create_children_and_bad_followups_do_not_create_more() {
    let (_guard, ws) = temp_workspace();
    let (url, requests) = mock_server(vec![
        sse_tool_call("blank", "spawn_agent", json!({
            "label": "Research", "message": "Report.", "session_id": "   "
        })),
        sse_text("Child."),
        sse_tool_call("change", "spawn_agent", json!({
            "label": "Follow-up", "message": "Report.", "session_id": "agent-1", "model": "override-model"
        })),
        sse_tool_call("unknown", "spawn_agent", json!({
            "label": "Follow-up", "message": "Report.", "session_id": "../../outside"
        })),
        sse_text("Finished."),
    ]).await;
    let events = run_one_turn(url, ws, "Research.").await;
    assert_eq!(result(&events, "blank").0["session_id"], "agent-1");
    assert!(!result(&events, "change").1);
    assert!(
        result(&events, "change")
            .0
            .as_str()
            .expect("error")
            .contains("cannot be changed")
    );
    assert!(!result(&events, "unknown").1);
    assert_eq!(requests.lock().expect("lock").len(), 5);
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, AgentEvent::SubagentStarted { .. }))
            .count(),
        1
    );
}

#[tokio::test]
async fn child_cannot_spawn_grandchildren_even_if_it_requests_the_tool() {
    let (_guard, ws) = temp_workspace();
    let (url, requests) = mock_server(vec![
        sse_tool_call("delegate", "spawn_agent", spawn_args("Report.")),
        sse_tool_call("nested", "spawn_agent", spawn_args("Report.")),
        sse_text("Child finished without delegation."),
        sse_text("Parent finished."),
    ])
    .await;
    let events = run_one_turn(url, ws, "Research.").await;
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, AgentEvent::SubagentStarted { .. }))
            .count(),
        1
    );
    assert_eq!(requests.lock().expect("lock").len(), 4);
    assert!(events.iter().any(|event| matches!(event,
        AgentEvent::SubagentEvent { event, .. } if matches!(event.as_ref(),
            AgentEvent::ToolCallFinished { success: false, output, .. } if output.contains("unavailable")
        )
    )));
}

#[tokio::test]
async fn subagent_session_limit_still_allows_existing_children_to_resume() {
    let (_guard, ws) = temp_workspace();
    let config = test_config("http://unused.invalid/v1".into(), ws);
    let mut children = Subagents::new(
        config.clone(),
        crate::tools::ToolContext::new(config.workspace.clone(), false),
        None,
    );
    let approvals = Arc::new(crate::approvals::Approvals::default());
    let args = spawn_args("Report.").as_object().expect("args").clone();
    for _ in 0..32 {
        let (child, _) = children
            .prepare(&args, None, approvals.clone(), None)
            .expect("child");
        children.put(child);
    }
    let error = children
        .prepare(&args, None, approvals.clone(), None)
        .err()
        .expect("limit");
    assert!(error.contains("limit"));
    let resume = json!({"label": "Follow-up", "message": "Report.", "session_id": "agent-1"});
    let (child, _) = children
        .prepare(
            resume.as_object().expect("args"),
            None,
            approvals.clone(),
            None,
        )
        .expect("resume");
    assert_eq!(child.id, "agent-1");
    let error = children
        .prepare(resume.as_object().expect("args"), None, approvals, None)
        .err()
        .expect("duplicate resume");
    assert!(error.contains("already running"));
    children.put(child);
}

#[tokio::test]
async fn invalid_subagent_inputs_do_not_allocate_sessions() {
    let (_guard, ws) = temp_workspace();
    let config = test_config("http://unused.invalid/v1".into(), ws);
    let mut children = Subagents::new(
        config.clone(),
        crate::tools::ToolContext::new(config.workspace.clone(), false),
        None,
    );
    let approvals = Arc::new(crate::approvals::Approvals::default());
    for args in [
        json!({"label": "Research"}),
        json!({"label": "", "message": "Report."}),
        json!({"label": "Research", "message": 42}),
        json!({"label": "Research", "message": "Report.", "session_id": 42}),
    ] {
        assert!(
            children
                .prepare(
                    args.as_object().expect("args"),
                    None,
                    approvals.clone(),
                    None
                )
                .is_err()
        );
    }
    let args = spawn_args("Report.");
    let (child, _) = children
        .prepare(args.as_object().expect("args"), None, approvals, None)
        .expect("child");
    assert_eq!(child.id, "agent-1");
}

#[tokio::test]
async fn subagent_is_isolated_and_resumes_after_restart_with_its_model_and_effort() {
    let (_guard, ws) = temp_workspace();
    let sessions = tempfile::tempdir().expect("tempdir");
    std::fs::write(ws.join("note.txt"), "private child tool output").expect("write");
    let (url, requests) = mock_server(vec![
        sse_tool_call(
            "delegate",
            "spawn_agent",
            spawn_args("Read note.txt and report the result."),
        ),
        sse_tool_call("read", "read_file", json!({"path": "note.txt"})),
        sse_text("Child summary."),
        sse_text("Parent summary."),
        sse_tool_call(
            "resume",
            "spawn_agent",
            json!({
                "label": "Follow-up", "message": "What did you find?", "session_id": "agent-1"
            }),
        ),
        sse_text("Follow-up summary."),
        sse_text("Parent follow-up."),
    ])
    .await;
    let mut config = test_config(url, ws);
    config.session_dir = Some(sessions.path().to_path_buf());
    config.subagent_model = Some("child-model".into());
    config.reasoning_effort = Some(ReasoningEffort::High);
    let first = run_turns(config.clone(), &["Parent-only conversation."]).await;
    assert_eq!(
        result(&first, "delegate"),
        (
            json!({"session_id": "agent-1", "output": "Child summary."}),
            true,
        )
    );
    let usage = first
        .iter()
        .rev()
        .find_map(|event| match event {
            AgentEvent::Usage(usage) => Some(*usage),
            _ => None,
        })
        .expect("usage");
    assert_eq!(usage.input_tokens, 200);
    assert_eq!(usage.output_tokens, 20);

    config.subagent_model = Some("override-model".into());
    config.reasoning_effort = Some(ReasoningEffort::Low);
    let second = run_turns(config, &["Ask the child again."]).await;
    assert_eq!(
        result(&second, "resume"),
        (
            json!({"session_id": "agent-1", "output": "Follow-up summary."}),
            true,
        )
    );
    let requests = requests.lock().expect("lock");
    assert_eq!(requests.len(), 7);
    assert_eq!(requests[1]["model"], "child-model");
    assert_eq!(requests[5]["model"], "child-model");
    assert_eq!(requests[5]["reasoning_effort"], "high");
    let child_messages = requests[1]["messages"].to_string();
    assert!(!child_messages.contains("Parent-only conversation"));
    let tool_names: Vec<_> = requests[1]["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .filter_map(|tool| tool["function"]["name"].as_str())
        .collect();
    assert!(!tool_names.contains(&"spawn_agent"));
    let resumed = requests[5]["messages"].to_string();
    assert!(resumed.contains("Child summary."));
    assert!(resumed.contains("What did you find?"));
    let parent_history = requests[3]["messages"].to_string();
    assert!(!parent_history.contains("private child tool output"));
    assert!(!parent_history.contains("plan read"));
    // Provider tool ids are untouched; only UI/approval ids are namespaced.
    assert_eq!(requests[2]["messages"][2]["tool_calls"][0]["id"], "read");
    assert_eq!(requests[2]["messages"][3]["tool_call_id"], "read");
}

#[tokio::test]
async fn subagent_model_override_wins_and_unavailable_models_do_not_fall_back() {
    let (_guard, ws) = temp_workspace();
    let (url, requests) = mock_server(vec![
        sse_tool_call("models", "list_models", json!({})),
        sse_tool_call(
            "delegate",
            "spawn_agent",
            json!({
                "label": "Research", "message": "Report.", "model": "override-model"
            }),
        ),
        sse_text("Child answer."),
        sse_tool_call(
            "missing",
            "spawn_agent",
            json!({
                "label": "Research", "message": "Report.", "model": "not-available"
            }),
        ),
        sse_text("The requested model is unavailable."),
    ])
    .await;
    let mut config = test_config(url, ws);
    config.subagent_model = Some("child-model".into());
    let events = run_turns(config, &["Research the project."]).await;
    assert!(
        result(&events, "models").0["models"]
            .as_array()
            .expect("models")
            .contains(&json!("override-model"))
    );
    assert!(result(&events, "delegate").1);
    let (error, success) = result(&events, "missing");
    assert!(!success);
    assert!(error.as_str().expect("error").contains("unavailable"));
    let requests = requests.lock().expect("lock");
    assert_eq!(requests.len(), 5);
    assert_eq!(requests[2]["model"], "override-model");
}

#[tokio::test]
async fn subagents_inherit_the_parent_model_when_no_default_is_set() {
    let (_guard, ws) = temp_workspace();
    let (url, requests) = mock_server(vec![
        sse_tool_call("delegate", "spawn_agent", spawn_args("Report.")),
        sse_text("Child."),
        sse_text("Parent."),
    ])
    .await;
    let events = run_one_turn(url, ws, "Research.").await;
    assert!(result(&events, "delegate").1);
    assert_eq!(requests.lock().expect("lock")[1]["model"], "test-model");
}

#[tokio::test]
async fn independent_subagents_actually_run_in_parallel() {
    let (_guard, ws) = temp_workspace();
    let command = |own: &str, other: &str| {
        format!(
            "printf ready > {own}; for i in 1 2 3 4 5 6 7 8 9 10; do test -f {other} && break; sleep 0.05; done; test -f {other}"
        )
    };
    let (url, _) = mock_server(vec![
        multi_spawn(2),
        sse_tool_call(
            "same-id",
            "run_command",
            json!({"command": command("left", "right")}),
        ),
        sse_tool_call(
            "same-id",
            "run_command",
            json!({"command": command("right", "left")}),
        ),
        sse_text("Child finished."),
        sse_text("Child finished."),
        sse_text("Both finished."),
    ])
    .await;
    let events = run_one_turn(url, ws, "Research in parallel.").await;
    let results: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            AgentEvent::SubagentEvent { event, .. } => match event.as_ref() {
                AgentEvent::ToolCallFinished {
                    call_id,
                    success,
                    exit_code,
                    ..
                } => Some((call_id.clone(), *success, *exit_code)),
                _ => None,
            },
            _ => None,
        })
        .collect();
    assert_eq!(results.len(), 2);
    assert!(
        results
            .iter()
            .all(|(_, success, code)| *success && *code == Some(0))
    );
    assert_ne!(results[0].0, results[1].0);
    assert!(result(&events, "delegate-0").1);
    assert!(result(&events, "delegate-1").1);
}

#[tokio::test]
async fn subagent_parallelism_is_bounded_to_four() {
    let (_guard, ws) = temp_workspace();
    let mut script = vec![multi_spawn(6)];
    script.extend((0..6).map(|_| sse_text("Child finished.")));
    script.push(sse_text("All finished."));
    let (url, _) = mock_server(script).await;
    let events = run_one_turn(url, ws, "Research in parallel.").await;
    let (mut active, mut peak) = (0usize, 0usize);
    for event in events {
        if let AgentEvent::SubagentEvent { event, .. } = event {
            match *event {
                AgentEvent::TurnStarted { .. } => {
                    active += 1;
                    peak = peak.max(active);
                }
                AgentEvent::TurnFinished { .. } => active -= 1,
                _ => {}
            }
        }
    }
    assert_eq!(active, 0);
    assert_eq!(peak, 4);
}

#[tokio::test]
async fn subagent_edits_and_verification_count_as_parent_work() {
    let (_guard, ws) = temp_workspace();
    let (url, _) = mock_server(vec![
        sse_tool_call(
            "delegate",
            "spawn_agent",
            spawn_args("Create a.txt containing hi and verify it."),
        ),
        sse_tool_call(
            "edit",
            "write_file",
            json!({"path": "a.txt", "content": "hi\n"}),
        ),
        sse_tool_call(
            "verify",
            "run_command",
            json!({"command": "test \"$(cat a.txt)\" = hi"}),
        ),
        sse_text("Created and verified."),
        sse_text("Completed."),
    ])
    .await;
    let events = run_one_turn(url, ws.clone(), "Create a.txt containing hi.").await;
    assert_eq!(
        std::fs::read_to_string(ws.join("a.txt")).expect("file"),
        "hi\n"
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, AgentEvent::HarnessNudge { .. }))
    );
    assert!(result(&events, "delegate").1);
}

#[tokio::test]
async fn subagent_approval_denial_prevents_writes() {
    let (_guard, ws) = temp_workspace();
    let (url, _) = mock_server(vec![
        sse_tool_call("delegate", "spawn_agent", spawn_args("Write a.txt.")),
        sse_tool_call(
            "edit",
            "write_file",
            json!({"path": "a.txt", "content": "hi"}),
        ),
        sse_text("The user declined."),
        sse_text("No changes made."),
    ])
    .await;
    let mut config = test_config(url, ws.clone());
    config.approval = ApprovalMode::AskForChanges;
    let handle = crate::spawn_session(config);
    handle
        .ops
        .send(Op::UserMessage("Research.".into()))
        .await
        .expect("send");
    let mut events = Vec::new();
    loop {
        let event = tokio::time::timeout(Duration::from_secs(5), handle.events.recv())
            .await
            .expect("event")
            .expect("open");
        if let AgentEvent::ApprovalRequested { call_id, .. } = &event {
            assert_eq!(call_id, "agent-1:edit");
            handle
                .ops
                .send(Op::Approval {
                    call_id: call_id.clone(),
                    decision: ApprovalDecision::Deny,
                })
                .await
                .expect("deny");
        }
        let done = matches!(event, AgentEvent::TurnFinished { .. });
        events.push(event);
        if done {
            break;
        }
    }
    assert!(!ws.join("a.txt").exists());
    assert!(events.iter().any(|event| matches!(event,
        AgentEvent::SubagentEvent { event, .. } if matches!(event.as_ref(), AgentEvent::ToolCallFinished { success: false, .. })
    )));
    let _ = handle.ops.send(Op::Shutdown).await;
}

#[tokio::test]
async fn interrupt_cancels_child_approval_and_preserves_a_valid_resumable_history() {
    let (_guard, ws) = temp_workspace();
    let sessions = tempfile::tempdir().expect("tempdir");
    let (url, requests) = mock_server(vec![
        sse_tool_call("delegate", "spawn_agent", spawn_args("Write a.txt.")),
        sse_tool_call(
            "edit",
            "write_file",
            json!({"path": "a.txt", "content": "hi"}),
        ),
    ])
    .await;
    let mut config = test_config(url, ws.clone());
    config.approval = ApprovalMode::AskForChanges;
    config.session_dir = Some(sessions.path().to_path_buf());
    let handle = crate::spawn_session(config);
    handle
        .ops
        .send(Op::UserMessage("Research.".into()))
        .await
        .expect("send");
    let mut events = Vec::new();
    loop {
        let event = tokio::time::timeout(Duration::from_secs(5), handle.events.recv())
            .await
            .expect("event")
            .expect("open");
        if matches!(event, AgentEvent::ApprovalRequested { .. }) {
            handle.ops.send(Op::Interrupt).await.expect("interrupt");
        }
        let done = matches!(event, AgentEvent::TurnFinished { .. });
        events.push(event);
        if done {
            break;
        }
    }
    assert!(!ws.join("a.txt").exists());
    assert_eq!(requests.lock().expect("lock").len(), 2);
    assert_eq!(
        events.last(),
        Some(&AgentEvent::TurnFinished {
            turn_id: 1,
            reason: TurnEndReason::Interrupted
        })
    );
    assert_eq!(
        result(&events, "delegate"),
        (
            json!({
                "session_id": "agent-1", "error": "Subagent interrupted."
            }),
            false
        )
    );
    let _ = handle.ops.send(Op::Shutdown).await;
    while let Ok(Ok(_)) = tokio::time::timeout(Duration::from_secs(5), handle.events.recv()).await {
    }
    let child = persist::load(&sessions.path().join("subagents/agent-1"))
        .expect("load")
        .expect("history");
    assert!(
        matches!(child.messages.last(), Some(Message::Tool { call_id, .. }) if call_id == "edit")
    );
}

#[tokio::test]
async fn failed_child_returns_its_id_instead_of_an_earlier_answer() {
    let (_guard, ws) = temp_workspace();
    let (url, _) = mock_server(vec![
        sse_tool_call("delegate", "spawn_agent", spawn_args("Report.")),
        "HTTP/1.1 400 Bad Request\r\ncontent-length: 0\r\nconnection: close\r\n\r\n".to_string(),
        sse_text("Child failed."),
    ])
    .await;
    let events = run_one_turn(url, ws, "Research.").await;
    let (output, success) = result(&events, "delegate");
    assert!(!success);
    assert_eq!(output["session_id"], "agent-1");
    assert!(output["error"].as_str().expect("error").contains("400"));
    assert!(output.get("output").is_none());
}
