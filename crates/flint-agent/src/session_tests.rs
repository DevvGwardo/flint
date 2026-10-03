use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::time::Duration;

use pretty_assertions::assert_eq;
use serde_json::json;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;

use super::*;

/// A one-request-per-connection HTTP server that answers each POST with the
/// next scripted SSE body and records the request bodies.
async fn mock_server(responses: Vec<String>) -> (String, Arc<StdMutex<Vec<Value>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let url = format!("http://{}/v1", listener.local_addr().expect("addr"));
    let requests = Arc::new(StdMutex::new(Vec::new()));
    let seen = Arc::clone(&requests);
    tokio::spawn(async move {
        let mut responses = responses.into_iter();
        while let Ok((mut socket, _)) = listener.accept().await {
            let mut buf = Vec::new();
            let mut chunk = [0u8; 4096];
            let body = loop {
                let n = socket.read(&mut chunk).await.unwrap_or(0);
                if n == 0 {
                    break None;
                }
                buf.extend_from_slice(&chunk[..n]);
                let text = String::from_utf8_lossy(&buf).to_string();
                let Some(split) = text.find("\r\n\r\n") else {
                    continue;
                };
                let length = text[..split]
                    .lines()
                    .find_map(|l| {
                        l.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                    })
                    .unwrap_or(0);
                if buf.len() >= split + 4 + length {
                    break Some(buf[split + 4..split + 4 + length].to_vec());
                }
            };
            let Some(body) = body else { continue };
            seen.lock()
                .expect("lock")
                .push(serde_json::from_slice(&body).unwrap_or(Value::Null));
            let sse = responses
                .next()
                .unwrap_or_else(|| sse_text("(script exhausted)"));
            // A scripted response starting with "HTTP/" is sent as-is.
            let reply = if sse.starts_with("HTTP/") {
                sse
            } else {
                format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{sse}",
                    sse.len()
                )
            };
            let _ = socket.write_all(reply.as_bytes()).await;
            let _ = socket.shutdown().await;
        }
    });
    (url, requests)
}

fn sse(chunks: &[Value]) -> String {
    let mut out: String = chunks.iter().map(|c| format!("data: {c}\n\n")).collect();
    out.push_str("data: [DONE]\n\n");
    out
}

fn sse_tool_call(id: &str, name: &str, args: Value) -> String {
    sse(&[
        json!({"choices": [{"delta": {"reasoning_content": format!("plan {id}")}}]}),
        json!({"choices": [{"delta": {"tool_calls": [{"index": 0, "id": id, "type": "function",
            "function": {"name": name, "arguments": args.to_string()}}]}}]}),
        json!({"choices": [{"delta": {}, "finish_reason": "tool_calls"}]}),
        json!({"choices": [], "usage": {"prompt_tokens": 100, "completion_tokens": 10,
            "prompt_tokens_details": {"cached_tokens": 80}}}),
    ])
}

fn sse_text(text: &str) -> String {
    sse(&[
        json!({"choices": [{"delta": {"content": text}}]}),
        json!({"choices": [{"delta": {}, "finish_reason": "stop"}]}),
    ])
}

fn test_config(url: String, workspace: PathBuf) -> AgentConfig {
    AgentConfig {
        base_url: url,
        model: "test-model".to_string(),
        api_key: "test-key".to_string(),
        workspace,
        approval: ApprovalMode::Auto,
        jev: None,
        session_dir: None,
        // Explicit, so the mock server only sees chat requests (0 would fetch `/models`).
        context_budget_tokens: 100_000,
        reasoning_effort: None,
    }
}

async fn run_one_turn(url: String, workspace: PathBuf, prompt: &str) -> Vec<AgentEvent> {
    let events = run_turns(test_config(url, workspace), &[prompt]).await;
    // Only the turn itself, as these tests were written.
    let end = events
        .iter()
        .position(|e| matches!(e, AgentEvent::TurnFinished { .. }))
        .map_or(events.len(), |i| i + 1);
    events[..end].to_vec()
}

/// Runs each prompt as a turn in one session, then shuts it down cleanly.
async fn run_turns(config: AgentConfig, prompts: &[&str]) -> Vec<AgentEvent> {
    let handle = crate::spawn_session(config);
    let mut events = Vec::new();
    for prompt in prompts {
        handle
            .ops
            .send(Op::UserMessage((*prompt).to_string()))
            .await
            .expect("send");
        collect_turn(&handle, &mut events).await;
    }
    let _ = handle.ops.send(Op::Shutdown).await;
    // Wait for the session to end (and flush its history).
    while let Ok(Ok(event)) =
        tokio::time::timeout(Duration::from_secs(5), handle.events.recv()).await
    {
        events.push(event);
    }
    events
}

async fn collect_turn(handle: &crate::SessionHandle, events: &mut Vec<AgentEvent>) {
    loop {
        let event = tokio::time::timeout(Duration::from_secs(20), handle.events.recv())
            .await
            .expect("event in time")
            .expect("open stream");
        let done = matches!(event, AgentEvent::TurnFinished { .. });
        events.push(event);
        if done {
            break;
        }
    }
}

/// Compact names for asserting event order.
fn outline(events: &[AgentEvent]) -> Vec<String> {
    events
        .iter()
        .filter_map(|event| match event {
            AgentEvent::TurnStarted { .. } => Some("turn".to_string()),
            AgentEvent::StepStarted { step, .. } => Some(format!("step {step}")),
            AgentEvent::ToolCallStarted { name, summary, .. } => {
                Some(format!("call {name} {summary}"))
            }
            AgentEvent::ToolCallFinished { success, .. } => Some(format!("done ok={success}")),
            AgentEvent::HarnessNudge { reason, .. } => Some(format!("nudge {reason:?}")),
            AgentEvent::TurnFinished { reason, .. } => Some(format!("finished {reason:?}")),
            AgentEvent::Error(message) => Some(format!("error {message}")),
            AgentEvent::ContextCompacted { .. } => Some("compacted".to_string()),
            AgentEvent::SessionOptions(_) => None,
            AgentEvent::TerminalStarted { .. }
            | AgentEvent::TerminalOutput { .. }
            | AgentEvent::TerminalExited { .. } => None,
            AgentEvent::ReasoningDelta(_)
            | AgentEvent::TextDelta(_)
            | AgentEvent::ToolOutputDelta { .. }
            | AgentEvent::ApprovalRequested { .. }
            | AgentEvent::ToolRepaired { .. }
            | AgentEvent::Usage(_) => None,
        })
        .collect()
}

fn temp_workspace() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().canonicalize().expect("canonical");
    (dir, path)
}

#[tokio::test]
async fn full_loop_writes_runs_and_finishes() {
    let (_guard, ws) = temp_workspace();
    let (url, requests) = mock_server(vec![
        sse_tool_call(
            "c1",
            "write_file",
            json!({"path": "a.txt", "content": "hi\n"}),
        ),
        sse_tool_call("c2", "run_command", json!({"command": "cat a.txt"})),
        sse_text("Created a.txt and checked it."),
    ])
    .await;
    let events = run_one_turn(url, ws.clone(), "create a.txt containing hi").await;
    assert_eq!(
        outline(&events),
        vec![
            "turn",
            "step 0",
            "call write_file a.txt",
            "done ok=true",
            "step 1",
            "call run_command cat a.txt",
            "done ok=true",
            "step 2",
            "finished Completed",
        ]
    );
    assert_eq!(
        std::fs::read_to_string(ws.join("a.txt")).expect("file"),
        "hi\n"
    );
    let usage = events.iter().rev().find_map(|e| match e {
        AgentEvent::Usage(u) => Some(*u),
        _ => None,
    });
    assert_eq!(
        usage,
        Some(Usage {
            input_tokens: 200,
            cached_input_tokens: 160,
            output_tokens: 20,
            reasoning_tokens: 0
        })
    );

    let requests = requests.lock().expect("lock").clone();
    assert_eq!(requests.len(), 3);
    assert_eq!(
        requests[0]["stream_options"],
        json!({"include_usage": true})
    );
    // The tool-call assistant message of this turn carries its reasoning back.
    let replayed = &requests[2]["messages"][2];
    assert_eq!(replayed["role"], "assistant");
    assert_eq!(replayed["reasoning_content"], "plan c1");
    assert_eq!(
        requests[2]["messages"][3],
        json!({"role": "tool", "tool_call_id": "c1", "content": "Created a.txt (+1 -0)"})
    );
}

#[tokio::test]
async fn a_long_turn_runs_past_sixty_model_calls() {
    // 60 model calls used to end a turn mid-task ("Hit the step limit").
    // The cap is a cost backstop far past any real task, so a turn that
    // keeps working keeps going.
    let (_guard, ws) = temp_workspace();
    let lines: String = (1..=100).map(|n| format!("line {n}\n")).collect();
    std::fs::write(ws.join("src.txt"), lines).expect("write");
    let steps = 61;
    let mut responses: Vec<String> = (0..steps)
        .map(|n| {
            sse_tool_call(
                &format!("c{n}"),
                "read_file",
                json!({"path": "src.txt", "offset": n + 1, "limit": 1}),
            )
        })
        .collect();
    responses.push(sse_text("It holds 100 numbered lines."));
    let (url, requests) = mock_server(responses).await;
    let events = run_one_turn(url, ws, "what does src.txt contain?").await;

    assert_eq!(
        events.last(),
        Some(&AgentEvent::TurnFinished {
            turn_id: 1,
            reason: TurnEndReason::Completed
        })
    );
    assert_eq!(requests.lock().expect("lock").len(), steps as usize + 1);
}

#[tokio::test]
async fn verify_nudge_fires_when_no_command_follows_an_edit() {
    let (_guard, ws) = temp_workspace();
    let (url, requests) = mock_server(vec![
        sse_tool_call(
            "c1",
            "write_file",
            json!({"path": "a.py", "content": "x = 1\n"}),
        ),
        sse_text("Done."),
        sse_text("No tests exist; nothing to run."),
    ])
    .await;
    let events = run_one_turn(url, ws, "fix the value in a.py").await;
    assert_eq!(
        outline(&events),
        vec![
            "turn",
            "step 0",
            "call write_file a.py",
            "done ok=true",
            "step 1",
            "nudge Verify",
            "step 2",
            "finished Completed",
        ]
    );
    let requests = requests.lock().expect("lock").clone();
    let last = requests[2]["messages"]
        .as_array()
        .expect("messages")
        .last()
        .cloned();
    assert_eq!(
        last,
        Some(json!({"role": "user", "content": crate::harness::guard::VERIFY_NUDGE}))
    );
}

#[tokio::test]
async fn repairs_names_and_arguments() {
    let (_guard, ws) = temp_workspace();
    let (url, _requests) = mock_server(vec![
        sse(&[
            json!({"choices": [{"delta": {"tool_calls": [{"index": 0, "id": "c1",
                "function": {"name": "ListDir", "arguments": "{\"path\":\".\",\"depth\":null,"}}]}}]}),
            json!({"choices": [{"delta": {}, "finish_reason": "tool_calls"}]}),
        ]),
        sse_text("Listed."),
    ])
    .await;
    let events = run_one_turn(url, ws, "what is in this folder?").await;
    let repairs: Vec<&AgentEvent> = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::ToolRepaired { .. }))
        .collect();
    assert_eq!(
        repairs,
        vec![
            &AgentEvent::ToolRepaired {
                tool: "list_dir".into(),
                detail: "renamed from ListDir".into()
            },
            &AgentEvent::ToolRepaired {
                tool: "list_dir".into(),
                detail: "repaired arguments".into()
            },
        ]
    );
}

#[tokio::test]
async fn saved_session_continues_after_restart() {
    let (_guard, ws) = temp_workspace();
    let sessions = tempfile::tempdir().expect("tempdir");
    let (url, requests) =
        mock_server(vec![sse_text("First answer."), sse_text("Second answer.")]).await;
    let config = AgentConfig {
        session_dir: Some(sessions.path().to_path_buf()),
        ..test_config(url, ws)
    };

    run_turns(config.clone(), &["what is 2+2?"]).await;
    let events = run_turns(config, &["and 3+3?"]).await;
    assert_eq!(
        events.first(),
        Some(&AgentEvent::TurnStarted { turn_id: 2 })
    );

    let requests = requests.lock().expect("lock").clone();
    let roles: Vec<(String, String)> = requests[1]["messages"]
        .as_array()
        .expect("messages")
        .iter()
        .skip(1)
        .map(|m| {
            (
                m["role"].as_str().unwrap_or_default().to_string(),
                m["content"].as_str().unwrap_or_default().to_string(),
            )
        })
        .collect();
    assert_eq!(
        roles,
        vec![
            ("user".to_string(), "what is 2+2?".to_string()),
            ("assistant".to_string(), "First answer.".to_string()),
            ("user".to_string(), "and 3+3?".to_string()),
        ]
    );
}

#[tokio::test]
async fn corrupt_history_starts_fresh_with_an_error() {
    let (_guard, ws) = temp_workspace();
    let sessions = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        sessions.path().join("history.json"),
        "{\"version\":1,\"turn_id\":3,\"messa",
    )
    .expect("write");
    let (url, requests) = mock_server(vec![sse_text("Hi.")]).await;
    let config = AgentConfig {
        session_dir: Some(sessions.path().to_path_buf()),
        ..test_config(url, ws)
    };
    let events = run_turns(config, &["hello"]).await;
    assert!(
        matches!(&events[0], AgentEvent::Error(m) if m.starts_with("Couldn't restore the saved conversation"))
    );
    assert_eq!(events[1], AgentEvent::TurnStarted { turn_id: 1 });
    assert_eq!(
        requests.lock().expect("lock")[0]["messages"]
            .as_array()
            .map(Vec::len),
        Some(2)
    );
}

#[tokio::test]
async fn reasoning_effort_is_sent_and_dropped_once_rejected() {
    let (_guard, ws) = temp_workspace();
    let body = "{\"error\":{\"message\":\"unknown reasoning_effort\"}}";
    let rejected = format!(
        "HTTP/1.1 400 Bad Request\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    );
    let (url, requests) = mock_server(vec![rejected, sse_text("One."), sse_text("Two.")]).await;
    let config = AgentConfig {
        reasoning_effort: Some(ReasoningEffort::High),
        ..test_config(url, ws)
    };
    let events = run_turns(config, &["a", "b"]).await;
    let errors: Vec<&AgentEvent> = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::Error(_)))
        .collect();
    assert_eq!(
        errors,
        vec![&AgentEvent::Error(
            "This model endpoint doesn't accept reasoning_effort; continuing without it."
                .to_string()
        )]
    );
    let finished: Vec<String> = outline(&events)
        .into_iter()
        .filter(|e| e.starts_with("finished"))
        .collect();
    assert_eq!(finished, vec!["finished Completed", "finished Completed"]);
    let efforts: Vec<Value> = requests
        .lock()
        .expect("lock")
        .iter()
        .map(|r| r["reasoning_effort"].clone())
        .collect();
    assert_eq!(efforts, vec![json!("high"), Value::Null, Value::Null]);
}

#[tokio::test]
async fn effort_can_change_mid_session() {
    let (_guard, ws) = temp_workspace();
    let (url, requests) = mock_server(vec![sse_text("One."), sse_text("Two.")]).await;
    let handle = crate::spawn_session(test_config(url, ws));
    let mut events = Vec::new();
    handle
        .ops
        .send(Op::UserMessage("a".into()))
        .await
        .expect("send");
    collect_turn(&handle, &mut events).await;
    handle
        .ops
        .send(Op::SetReasoningEffort(Some(ReasoningEffort::Low)))
        .await
        .expect("send");
    handle
        .ops
        .send(Op::UserMessage("b".into()))
        .await
        .expect("send");
    collect_turn(&handle, &mut events).await;
    let _ = handle.ops.send(Op::Shutdown).await;
    let efforts: Vec<Value> = requests
        .lock()
        .expect("lock")
        .iter()
        .map(|r| r["reasoning_effort"].clone())
        .collect();
    assert_eq!(efforts, vec![Value::Null, json!("low")]);
}

#[tokio::test]
async fn small_budget_compacts_and_reports_it() {
    let (_guard, ws) = temp_workspace();
    let big = "line of output\n".repeat(400);
    let mut script = Vec::new();
    for i in 0..6 {
        std::fs::write(ws.join(format!("f{i}.txt")), &big).expect("write");
        script.push(sse_tool_call(
            &format!("c{i}"),
            "read_file",
            json!({"path": format!("f{i}.txt")}),
        ));
        script.push(sse_text("Read it."));
    }
    let (url, requests) = mock_server(script).await;
    let config = AgentConfig {
        context_budget_tokens: 6_000,
        ..test_config(url, ws)
    };
    let prompts = [
        "read f0", "read f1", "read f2", "read f3", "read f4", "read f5",
    ];
    let events = run_turns(config, &prompts).await;
    let compactions: Vec<(u64, u64)> = events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::ContextCompacted {
                before_tokens,
                after_tokens,
            } => Some((*before_tokens, *after_tokens)),
            _ => None,
        })
        .collect();
    assert!(!compactions.is_empty());
    for (before, after) in &compactions {
        assert!(*before >= 4_800 && after < before, "{before} -> {after}");
    }
    // The last request still has every user message and stubs for old output.
    let last = requests
        .lock()
        .expect("lock")
        .last()
        .cloned()
        .expect("request");
    let text = last["messages"].to_string();
    for prompt in prompts {
        assert!(text.contains(prompt), "{prompt} missing");
    }
    assert!(text.contains("[output trimmed:"));
}

#[test]
fn every_event_round_trips_through_serde() {
    let events = vec![
        AgentEvent::TurnStarted { turn_id: 1 },
        AgentEvent::StepStarted {
            turn_id: 1,
            step: 0,
        },
        AgentEvent::ReasoningDelta("r".into()),
        AgentEvent::TextDelta("t".into()),
        AgentEvent::ToolCallStarted {
            call_id: "c".into(),
            name: "run_command".into(),
            kind: ToolKind::Command,
            args: json!({"command": "ls", "n": [1, 2]}),
            summary: "ls".into(),
        },
        AgentEvent::ToolOutputDelta {
            call_id: "c".into(),
            chunk: "out".into(),
        },
        AgentEvent::ToolCallFinished {
            call_id: "c".into(),
            output: "o".into(),
            exit_code: Some(-1),
            success: false,
            diff: Some(crate::protocol::FileDiff {
                path: "a".into(),
                unified: "@@".into(),
                added: 1,
                removed: 2,
                created: true,
            }),
            duration_ms: 5,
        },
        AgentEvent::ApprovalRequested {
            call_id: "c".into(),
            kind: ToolKind::Edit,
            summary: "a.rs".into(),
        },
        AgentEvent::HarnessNudge {
            reason: NudgeReason::LeakedCall,
            message: "m".into(),
        },
        AgentEvent::ToolRepaired {
            tool: "t".into(),
            detail: "d".into(),
        },
        AgentEvent::Usage(Usage {
            input_tokens: 1,
            cached_input_tokens: 2,
            output_tokens: 3,
            reasoning_tokens: 4,
        }),
        AgentEvent::ContextCompacted {
            before_tokens: 9,
            after_tokens: 4,
        },
        AgentEvent::TurnFinished {
            turn_id: 1,
            reason: TurnEndReason::Failed("x".into()),
        },
        AgentEvent::TurnFinished {
            turn_id: 2,
            reason: TurnEndReason::StepLimit,
        },
        AgentEvent::Error("e".into()),
    ];
    for event in events {
        let line = serde_json::to_string(&event).expect("serialize");
        assert!(!line.contains('\n'));
        assert_eq!(
            serde_json::from_str::<AgentEvent>(&line).expect("deserialize"),
            event
        );
    }
}
