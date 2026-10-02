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
            let reply = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{sse}",
                sse.len()
            );
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

async fn run_one_turn(url: String, workspace: PathBuf, prompt: &str) -> Vec<AgentEvent> {
    let handle = crate::spawn_session(AgentConfig {
        base_url: url,
        model: "test-model".to_string(),
        api_key: "test-key".to_string(),
        workspace,
        approval: ApprovalMode::Auto,
        jev: None,
    });
    handle
        .ops
        .send(Op::UserMessage(prompt.to_string()))
        .await
        .expect("send");
    let mut events = Vec::new();
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
    let _ = handle.ops.send(Op::Shutdown).await;
    events
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
