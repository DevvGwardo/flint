use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::time::Duration;

use pretty_assertions::assert_eq;
use serde_json::json;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;

use std::path::PathBuf;

use crate::JevQuestion;

use super::*;

#[path = "archive_shutdown_tests.rs"]
mod archive_shutdown_tests;
#[path = "queue_steering_tests.rs"]
mod queue_steering_tests;
#[path = "subagent_tests.rs"]
mod subagent_tests;
#[path = "task_engine_tests.rs"]
mod task_engine_tests;
#[path = "worktree_session_tests.rs"]
mod worktree_session_tests;

/// A one-request-per-connection HTTP server that answers each POST with the
/// next scripted SSE body and records the request bodies.
async fn mock_server(responses: Vec<String>) -> (String, Arc<StdMutex<Vec<Value>>>) {
    mock_server_with_models(
        responses,
        json!({"data": [
            {"id": "test-model", "context_length": 128000},
            {"id": "child-model", "context_length": 200000},
            {"id": "override-model", "context_length": 32000}
        ]}),
    )
    .await
}

async fn mock_server_with_models(
    responses: Vec<String>,
    listing: Value,
) -> (String, Arc<StdMutex<Vec<Value>>>) {
    mock_server_with_response_hook(responses, listing, |_| {}).await
}

/// Run deterministic host-side changes while inference is in flight.
async fn mock_server_with_response_hook(
    responses: Vec<String>,
    listing: Value,
    before_response: impl Fn(usize) + Send + 'static,
) -> (String, Arc<StdMutex<Vec<Value>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let url = format!("http://{}/v1", listener.local_addr().expect("addr"));
    let requests = Arc::new(StdMutex::new(Vec::new()));
    let seen = Arc::clone(&requests);
    let listing = listing.to_string();
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
                if text.starts_with("GET /v1/models ") {
                    let reply = format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{listing}",
                        listing.len()
                    );
                    let _ = socket.write_all(reply.as_bytes()).await;
                    let _ = socket.shutdown().await;
                    break None;
                }
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
            let index = seen.lock().expect("lock").len() - 1;
            before_response(index);
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

/// A one-request-per-connection stand-in for the Typesafe System One endpoint.
/// Every POST to `/v1/systemone` is answered with the same calibrated `noul`
/// probability, and the request bodies are recorded so a test can assert which
/// question the harness asked.
async fn mock_judge(probability: f64) -> (String, Arc<StdMutex<Vec<Value>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let base = format!("http://{}", listener.local_addr().expect("addr"));
    let requests = Arc::new(StdMutex::new(Vec::new()));
    let seen = Arc::clone(&requests);
    tokio::spawn(async move {
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
            let value: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
            seen.lock().expect("lock").push(value.clone());
            // Answer under the name the harness asked with, the way System One
            // echoes the question key back.
            let name = value["questions"]
                .as_object()
                .and_then(|questions| questions.keys().next().cloned())
                .unwrap_or_else(|| "stuck".to_string());
            let mut answers = serde_json::Map::new();
            answers.insert(name, json!({"type": "noul", "noul": probability}));
            let reply_body = json!({"answers": Value::Object(answers)}).to_string();
            let reply = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{reply_body}",
                reply_body.len()
            );
            let _ = socket.write_all(reply.as_bytes()).await;
            let _ = socket.shutdown().await;
        }
    });
    (base, requests)
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
        subagent_model: None,
        api_key: "test-key".to_string(),
        workspace,
        general: false,
        approval: ApprovalMode::Auto,
        jev: None,
        session_dir: None,
        // Explicit, so the mock server only sees chat requests (0 would fetch `/models`).
        context_budget_tokens: 100_000,
        reasoning_effort: None,
        sandbox: false,
        mcp_servers: Vec::new(),
    }
}

#[tokio::test]
async fn general_session_answers_without_a_repository_or_forced_file_edits() {
    let workspace = tempfile::tempdir().expect("workspace");
    let (url, requests) = mock_server(vec![sse_text("Let's make a short plan.")]).await;
    let mut config = test_config(url, workspace.path().to_path_buf());
    config.general = true;
    let events = run_turns(config, &["Help me plan my day"]).await;
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, AgentEvent::ToolCallStarted { .. }))
    );
    let seen = requests.lock().expect("requests");
    let prompt = seen[0]["messages"][0]["content"]
        .as_str()
        .expect("system prompt");
    assert!(prompt.contains("general-purpose assistant"));
    assert!(prompt.contains("not attached to a coding project"));
    assert!(!workspace.path().join(".git").exists());
}

async fn run_one_turn(url: String, workspace: PathBuf, prompt: &str) -> Vec<AgentEvent> {
    run_one_turn_config(test_config(url, workspace), prompt).await
}

/// Like [`run_one_turn`], for a config the caller has already customised (a
/// JEV judge, an alternate model, ...).
async fn run_one_turn_config(config: AgentConfig, prompt: &str) -> Vec<AgentEvent> {
    let events = run_turns(config, &[prompt]).await;
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
            AgentEvent::FilesReverted { diffs, skipped, .. } => Some(format!(
                "reverted {} skipped {}",
                diffs
                    .iter()
                    .map(|d| d.path.as_str())
                    .collect::<Vec<_>>()
                    .join(","),
                skipped.join(",")
            )),
            AgentEvent::SessionOptions(_) | AgentEvent::SteeringAccepted { .. } => None,
            AgentEvent::SessionStopped { .. } => None,
            AgentEvent::JevVerdict { .. } => None,
            AgentEvent::TaskUpdated { .. } | AgentEvent::TaskAssessed { .. } => None,
            AgentEvent::SubagentStarted { .. }
            | AgentEvent::SubagentWorkspace { .. }
            | AgentEvent::SubagentEvent { .. } => None,
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
async fn incomplete_model_outcomes_never_mutate_files() {
    for reason in [None, Some("length"), Some("content_filter")] {
        let (_guard, ws) = temp_workspace();
        let mut chunks = vec![json!({"choices":[{"delta":{
            "content":"partial",
            "tool_calls":[{"index":0,"id":"unsafe","function":{
                "name":"write_file","arguments":json!({"path":"victim","content":"bad"}).to_string()
            }}]
        }}]})];
        if let Some(reason) = reason {
            chunks.push(json!({"choices":[{"delta":{},"finish_reason":reason}]}));
        }
        let body = sse(&chunks);
        let (url, _) = mock_server(vec![body; 8]).await;
        let events = run_one_turn(url, ws.clone(), "store this file").await;
        assert!(!ws.join("victim").exists());
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, AgentEvent::ToolCallStarted { .. }))
        );
        assert!(events.iter().any(|event| matches!(
            event,
            AgentEvent::TurnFinished {
                reason: TurnEndReason::Failed(_),
                ..
            }
        )));
    }
}

async fn assert_duplicate_tool_call_ids_rejected(reason: &str) {
    let (_guard, ws) = temp_workspace();
    let calls: Vec<Value> = [
        ("unique", "before"),
        ("duplicate", "one"),
        ("duplicate", "two"),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (id, path))| {
        json!({"index": index, "id": id, "type": "function",
                "function": {"name": "write_file",
                    "arguments": json!({"path": path, "content": "bad"}).to_string()}})
    })
    .collect();
    let (url, requests) = mock_server(vec![sse(&[json!({
        "choices": [{"delta": {"tool_calls": calls}, "finish_reason": reason}]
    })])])
    .await;
    let events = run_one_turn(url, ws.clone(), "store these files").await;
    for path in ["before", "one", "two"] {
        assert!(
            !ws.join(path).exists(),
            "duplicate provider IDs released file {path} before validation ({reason})"
        );
    }
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, AgentEvent::ToolCallStarted { .. }))
    );
    assert!(events.iter().any(|event| matches!(
        event,
        AgentEvent::TurnFinished {
            reason: TurnEndReason::Failed(message),
            ..
        } if message.contains("duplicate tool-call IDs")
    )));
    assert_eq!(requests.lock().expect("requests").len(), 1);
}

#[tokio::test]
async fn duplicate_tool_call_ids_with_stop_never_mutate_files() {
    assert_duplicate_tool_call_ids_rejected("stop").await;
}

#[tokio::test]
async fn duplicate_tool_call_ids_with_tool_calls_never_mutate_files() {
    assert_duplicate_tool_call_ids_rejected("tool_calls").await;
}

async fn assert_malformed_stream_json_rejected(reason: &str) {
    let (_guard, ws) = temp_workspace();
    let call = json!({"choices": [{"delta": {
        "content": "partial",
        "tool_calls": [{"index": 0, "id": "unsafe", "type": "function",
            "function": {"name": "write_file",
                "arguments": json!({"path": "victim", "content": "bad"}).to_string()}}]
    }}]});
    let terminal = json!({"choices": [{"delta": {}, "finish_reason": reason}]});
    let body =
        format!("data: {call}\n\ndata: {{\"choices\": [\n\ndata: {terminal}\n\ndata: [DONE]\n\n");
    let (url, requests) = mock_server(vec![body]).await;
    let events = run_one_turn(url, ws.clone(), "store victim").await;
    assert!(
        !ws.join("victim").exists(),
        "malformed provider JSON released a file mutation ({reason})"
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, AgentEvent::ToolCallStarted { .. }))
    );
    assert!(events.iter().any(|event| matches!(
        event,
        AgentEvent::TurnFinished {
            reason: TurnEndReason::Failed(message),
            ..
        } if message.contains("invalid JSON")
    )));
    assert_eq!(requests.lock().expect("requests").len(), 1);
}

#[tokio::test]
async fn malformed_stream_json_with_stop_never_mutates_files() {
    assert_malformed_stream_json_rejected("stop").await;
}

#[tokio::test]
async fn malformed_stream_json_with_tool_calls_never_mutates_files() {
    assert_malformed_stream_json_rejected("tool_calls").await;
}

#[tokio::test]
async fn valid_tool_calls_with_sse_keepalives_still_execute() {
    let (_guard, ws) = temp_workspace();
    let body = sse_tool_call(
        "write",
        "write_file",
        json!({"path": "victim", "content": "safe"}),
    )
    .replace(
        "\n\n",
        "\n\n: keepalive\n\nevent: ping\nid: ignored\nretry: 1000\n\ndata:\n\ndata:  \n\n",
    );
    let (url, _) = mock_server(vec![body, sse_text("Stored."), sse_text("Stored.")]).await;
    let events = run_one_turn(url, ws.clone(), "store victim").await;
    assert_eq!(
        std::fs::read_to_string(ws.join("victim")).expect("created"),
        "safe"
    );
    assert!(events.iter().any(|event| matches!(
        event,
        AgentEvent::TurnFinished {
            reason: TurnEndReason::Completed,
            ..
        }
    )));
}

#[tokio::test]
async fn valid_json_file_content_remains_a_string_through_the_harness() {
    let (_guard, ws) = temp_workspace();
    let content = "{\"x\":null,\"items\":[1,2]}";
    let (url, _) = mock_server(vec![
        sse_tool_call(
            "write",
            "write_file",
            json!({"path":"data.json","content":content}),
        ),
        sse_text("Stored."),
        sse_text("Stored."),
    ])
    .await;
    run_one_turn(url, ws.clone(), "store data.json").await;
    assert_eq!(
        std::fs::read_to_string(ws.join("data.json")).expect("created"),
        content
    );
}

#[tokio::test]
async fn full_loop_writes_runs_and_finishes() {
    let (_guard, ws) = temp_workspace();
    // `test`/`cat` are Unix shell builtins; the Windows runner checks the
    // same content with the interpreter form used across task-engine tests.
    let check: &str = if cfg!(windows) {
        r#"python -c "import pathlib; assert pathlib.Path('a.txt').read_text()=='hi\n'""#
    } else {
        "test \"$(cat a.txt)\" = hi"
    };
    let expected_call = format!("call run_command {check}");
    let (url, requests) = mock_server(vec![
        sse_tool_call(
            "c1",
            "write_file",
            json!({"path": "a.txt", "content": "hi\n"}),
        ),
        sse_tool_call("c2", "run_command", json!({"command": check})),
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
            expected_call.as_str(),
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
    let replayed = &requests[2]["messages"][3];
    assert_eq!(replayed["role"], "assistant");
    assert_eq!(replayed["reasoning_content"], "plan c1");
    assert_eq!(
        requests[2]["messages"][4],
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
async fn a_jev_verdict_that_confirms_an_edit_reaches_the_event_stream() {
    let (_guard, ws) = temp_workspace();
    let (url, _) = mock_server(vec![
        sse_tool_call(
            "c1",
            "write_file",
            json!({"path": "a.py", "content": "x = 1\n"}),
        ),
        sse_text("Done."),
        sse_text("No tests exist; nothing to run."),
    ])
    .await;
    let (judge_url, judged) = mock_judge(0.1).await;
    let mut config = test_config(url, ws);
    config.jev = Some(crate::JevConfig {
        api_key: "judge-key".to_string(),
        base_url: judge_url,
        model: "jev-test".to_string(),
    });
    let events = run_one_turn_config(config, "fix the value in a.py").await;

    assert_eq!(
        events.iter().find_map(|event| match event {
            AgentEvent::JevVerdict {
                question,
                probability,
                confirmed,
            } => Some((*question, *probability, *confirmed)),
            _ => None,
        }),
        Some((JevQuestion::Verified, 0.1, true))
    );
    assert!(events.iter().any(|event| matches!(
        event,
        AgentEvent::HarnessNudge {
            reason: NudgeReason::Verify,
            ..
        }
    )));

    let judged = judged.lock().expect("judge requests").clone();
    assert_eq!(judged.len(), 1);
    assert_eq!(judged[0]["model"], "jev-test");
    assert_eq!(
        judged[0]["questions"]
            .as_object()
            .expect("questions")
            .keys()
            .collect::<Vec<_>>(),
        vec!["verified"]
    );
}

#[tokio::test]
async fn a_jev_verdict_that_clears_an_edit_skips_the_nudge() {
    let (_guard, ws) = temp_workspace();
    let (url, _) = mock_server(vec![
        sse_tool_call(
            "c1",
            "write_file",
            json!({"path": "a.py", "content": "x = 1\n"}),
        ),
        sse_text("Done."),
    ])
    .await;
    let (judge_url, judged) = mock_judge(0.9).await;
    let mut config = test_config(url, ws);
    config.jev = Some(crate::JevConfig {
        api_key: "judge-key".to_string(),
        base_url: judge_url,
        model: "jev-test".to_string(),
    });
    let events = run_one_turn_config(config, "fix the value in a.py").await;

    assert_eq!(
        events.iter().find_map(|event| match event {
            AgentEvent::JevVerdict {
                question,
                probability,
                confirmed,
            } => Some((*question, *probability, *confirmed)),
            _ => None,
        }),
        Some((JevQuestion::Verified, 0.9, false))
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, AgentEvent::HarnessNudge { .. }))
    );
    assert_eq!(
        outline(&events),
        vec![
            "turn",
            "step 0",
            "call write_file a.py",
            "done ok=true",
            "step 1",
            "finished Completed",
        ]
    );
    assert_eq!(judged.lock().expect("judge requests").len(), 1);
}

#[tokio::test]
async fn a_jev_verdict_on_a_stuck_loop_reaches_the_event_stream() {
    let (_guard, ws) = temp_workspace();
    std::fs::write(ws.join("a.txt"), "hi\n").expect("seed file");
    let (url, _) = mock_server(vec![
        sse_tool_call("r1", "read_file", json!({"path": "a.txt"})),
        sse_tool_call("r2", "read_file", json!({"path": "a.txt"})),
        sse_text("It says hi."),
    ])
    .await;
    let (judge_url, judged) = mock_judge(0.9).await;
    let mut config = test_config(url, ws);
    config.jev = Some(crate::JevConfig {
        api_key: "judge-key".to_string(),
        base_url: judge_url,
        model: "jev-test".to_string(),
    });
    let events = run_one_turn_config(config, "what does a.txt say?").await;

    assert!(
        events.iter().any(|event| matches!(
            event,
            AgentEvent::JevVerdict {
                question: JevQuestion::Stuck,
                confirmed: true,
                ..
            }
        )),
        "the stuck verdict should reach the stream: {events:?}"
    );
    assert!(events.iter().any(|event| matches!(
        event,
        AgentEvent::HarnessNudge {
            reason: NudgeReason::Stuck,
            ..
        }
    )));
    let judged = judged.lock().expect("judge requests").clone();
    assert_eq!(
        judged[0]["questions"]
            .as_object()
            .expect("questions")
            .keys()
            .collect::<Vec<_>>(),
        vec!["stuck"]
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
        .filter(|m| m["role"] != "system")
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
async fn corrupt_history_is_preserved_and_blocks_provider_execution() {
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
    let handle = crate::spawn_session(config);
    let mut events = Vec::new();
    while let Ok(Ok(event)) =
        tokio::time::timeout(Duration::from_secs(5), handle.events.recv()).await
    {
        events.push(event);
    }
    assert!(
        matches!(&events[0], AgentEvent::Error(message) if message.contains("History") || message.contains("history"))
    );
    assert!(events.iter().any(|event| matches!(
        event,
        AgentEvent::SessionStopped {
            history_saved: false
        }
    )));
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, AgentEvent::TurnStarted { .. }))
    );
    assert!(requests.lock().unwrap().is_empty());
    assert_eq!(
        std::fs::read_to_string(sessions.path().join("history.json")).unwrap(),
        "{\"version\":1,\"turn_id\":3,\"messa"
    );
    assert!(!sessions.path().join("history.json.corrupt").exists());
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

fn fallback_error_response(status: u16, message: &str) -> String {
    let body = json!({"error": {"message": message}}).to_string();
    format!(
        "HTTP/1.1 {status} Test Error\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    )
}

async fn assert_effort_fallback_failure(response: String, expected: &str) {
    let (_guard, ws) = temp_workspace();
    let (url, requests) = mock_server(vec![
        fallback_error_response(400, "initial reasoning_effort rejection"),
        response,
    ])
    .await;
    let config = AgentConfig {
        reasoning_effort: Some(ReasoningEffort::High),
        ..test_config(url, ws)
    };
    let events = run_one_turn_config(config, "hello").await;
    let message = events
        .iter()
        .find_map(|event| match event {
            AgentEvent::TurnFinished {
                reason: TurnEndReason::Failed(message),
                ..
            } => Some(message),
            _ => None,
        })
        .expect("failed turn");
    assert!(
        message.contains(expected),
        "fallback diagnostic missing: {message}"
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event, AgentEvent::Error(message) if message.contains(expected)))
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, AgentEvent::ToolCallStarted { .. }))
    );
    let efforts: Vec<Value> = requests
        .lock()
        .expect("requests")
        .iter()
        .map(|request| request["reasoning_effort"].clone())
        .collect();
    assert_eq!(efforts, vec![json!("high"), Value::Null]);
}

#[tokio::test]
async fn reasoning_effort_fallback_reports_rejected_failure() {
    assert_effort_fallback_failure(
        fallback_error_response(422, "fallback rejected"),
        "fallback rejected",
    )
    .await;
}

#[tokio::test]
async fn reasoning_effort_fallback_reports_retryable_failure() {
    assert_effort_fallback_failure(
        fallback_error_response(500, "fallback retryable"),
        "fallback retryable",
    )
    .await;
}

#[tokio::test]
async fn reasoning_effort_fallback_reports_throttled_failure() {
    assert_effort_fallback_failure(
        fallback_error_response(429, "fallback throttled"),
        "fallback throttled",
    )
    .await;
}

#[tokio::test]
async fn reasoning_effort_fallback_reports_fatal_failure() {
    assert_effort_fallback_failure("data: {\n\n".into(), "invalid JSON").await;
}

#[tokio::test]
async fn reasoning_effort_fallback_failure_can_recover_without_poisoning_negotiation() {
    let (_guard, ws) = temp_workspace();
    let (url, requests) = mock_server(vec![
        fallback_error_response(400, "initial reasoning_effort rejection"),
        fallback_error_response(500, "fallback recovery failure"),
        sse_text("Recovered."),
    ])
    .await;
    let config = AgentConfig {
        reasoning_effort: Some(ReasoningEffort::High),
        ..test_config(url, ws)
    };
    let events = run_turns(config, &["hello", "hello again"]).await;
    let reasons: Vec<&TurnEndReason> = events
        .iter()
        .filter_map(|event| match event {
            AgentEvent::TurnFinished { reason, .. } => Some(reason),
            _ => None,
        })
        .collect();
    assert!(
        matches!(reasons.first(), Some(TurnEndReason::Failed(message))
            if message.contains("fallback recovery failure")),
        "fallback diagnostic missing: {reasons:?}"
    );
    assert!(matches!(reasons.get(1), Some(TurnEndReason::Completed)));
    let efforts: Vec<Value> = requests
        .lock()
        .expect("requests")
        .iter()
        .map(|request| request["reasoning_effort"].clone())
        .collect();
    assert_eq!(efforts, vec![json!("high"), Value::Null, json!("high")]);
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
        AgentEvent::SubagentStarted {
            call_id: "c".into(),
            session_id: "agent-1".into(),
            model: "child-model".into(),
        },
        AgentEvent::SubagentEvent {
            call_id: "c".into(),
            event: Box::new(AgentEvent::TextDelta("child text".into())),
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

fn sse_tool_calls(calls: &[(&str, &str, Value)]) -> String {
    let calls: Vec<Value> = calls
        .iter()
        .enumerate()
        .map(|(index, (id, name, args))| {
            json!({"index": index, "id": id, "type": "function",
                "function": {"name": name, "arguments": args.to_string()}})
        })
        .collect();
    sse(&[json!({"choices": [{"delta": {"tool_calls": calls}, "finish_reason": "tool_calls"}]})])
}

#[tokio::test]
async fn read_only_calls_in_one_response_run_together() {
    let (_guard, ws) = temp_workspace();
    std::fs::write(ws.join("a.txt"), "alpha\n").expect("write");
    std::fs::write(ws.join("b.txt"), "beta\n").expect("write");
    let (url, requests) = mock_server(vec![
        sse_tool_calls(&[
            ("r1", "read_file", json!({"path": "a.txt"})),
            ("r2", "grep", json!({"pattern": "beta"})),
            ("r3", "list_dir", json!({})),
        ]),
        sse_text("Read them."),
    ])
    .await;
    let events = run_one_turn(url, ws, "what do a.txt and b.txt say?").await;
    // All three start before any finishes, and results keep call order.
    assert_eq!(
        outline(&events),
        vec![
            "turn",
            "step 0",
            "call read_file a.txt",
            "call grep beta in .",
            "call list_dir .",
            "done ok=true",
            "done ok=true",
            "done ok=true",
            "step 1",
            "finished Completed",
        ]
    );
    let requests = requests.lock().expect("lock").clone();
    let ids: Vec<&str> = requests[1]["messages"]
        .as_array()
        .expect("messages")
        .iter()
        .filter_map(|m| m["tool_call_id"].as_str())
        .collect();
    assert_eq!(ids, ["r1", "r2", "r3"]);
}

#[tokio::test]
async fn open_plan_steps_get_a_nudge_before_the_turn_ends() {
    let (_guard, ws) = temp_workspace();
    let plan = |status: &str| {
        json!({"plan": [
            {"step": "Look around", "status": "completed"},
            {"step": "Write the summary", "status": status},
        ]})
    };
    let (url, requests) = mock_server(vec![
        sse_tool_call("p1", "update_plan", plan("in_progress")),
        sse_text("Here is a partial answer."),
        sse_tool_call("p2", "update_plan", plan("completed")),
        sse_text("Done."),
    ])
    .await;
    let events = run_one_turn(url, ws, "what is in this project?").await;
    assert_eq!(
        outline(&events),
        vec![
            "turn",
            "step 0",
            "call update_plan Plan: 1/2 done",
            "done ok=true",
            "step 1",
            "nudge Watchdog",
            "step 2",
            "call update_plan Plan: 2/2 done",
            "done ok=true",
            "step 3",
            "finished Completed",
        ]
    );
    let requests = requests.lock().expect("lock").clone();
    let tool_result = requests[1]["messages"]
        .as_array()
        .expect("messages")
        .iter()
        .find(|m| m["tool_call_id"] == "p1")
        .expect("plan result")
        .clone();
    assert_eq!(
        tool_result["content"],
        "Plan updated (1/2 closed):\n[x] Look around\n[>] Write the summary"
    );
}

#[tokio::test]
async fn messages_sent_during_a_turn_join_it() {
    let (_guard, ws) = temp_workspace();
    let (url, requests) = mock_server(vec![
        sse_tool_call("c1", "list_dir", json!({})),
        sse_text("Listed, and noted the follow-up."),
    ])
    .await;
    let handle = crate::spawn_session(test_config(url, ws));
    // The second arrives while the first turn is running.
    for text in ["list the files", "also mention the README"] {
        handle
            .ops
            .send(Op::UserMessage(text.to_string()))
            .await
            .expect("send");
    }
    let mut events = Vec::new();
    collect_turn(&handle, &mut events).await;
    let _ = handle.ops.send(Op::Shutdown).await;
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, AgentEvent::TurnStarted { .. }))
            .count(),
        1
    );
    let requests = requests.lock().expect("lock").clone();
    let users: Vec<&str> = requests[0]["messages"]
        .as_array()
        .expect("messages")
        .iter()
        .filter(|m| m["role"] == "user")
        .filter_map(|m| m["content"].as_str())
        .collect();
    assert_eq!(users, ["list the files", "also mention the README"]);
}

#[tokio::test]
async fn undo_restores_the_last_turns_files() {
    let (_guard, ws) = temp_workspace();
    std::fs::write(ws.join("b.txt"), "old\n").expect("write");
    std::fs::write(ws.join("c.txt"), "c\n").expect("write");
    let (url, requests) = mock_server(vec![
        sse_tool_calls(&[
            ("r1", "read_file", json!({"path": "b.txt"})),
            ("r2", "read_file", json!({"path": "c.txt"})),
        ]),
        sse_tool_calls(&[
            (
                "w1",
                "write_file",
                json!({"path": "a.txt", "content": "new file\n"}),
            ),
            (
                "e1",
                "edit_file",
                json!({"path": "b.txt", "old_string": "old", "new_string": "new"}),
            ),
            (
                "e2",
                "edit_file",
                json!({"path": "c.txt", "old_string": "c", "new_string": "C"}),
            ),
        ]),
        sse_tool_call("t1", "run_command", json!({"command": "test -f a.txt"})),
        sse_text("Done."),
        sse_text("Okay."),
    ])
    .await;
    let handle = crate::spawn_session(test_config(url, ws.clone()));
    let mut events = Vec::new();
    handle
        .ops
        .send(Op::UserMessage("make the changes".into()))
        .await
        .expect("send");
    collect_turn(&handle, &mut events).await;
    // The user edits c.txt after the agent: undo must leave it alone.
    std::fs::write(ws.join("c.txt"), "mine\n").expect("write");
    handle.ops.send(Op::UndoLastTurn).await.expect("undo");
    let reverted = loop {
        let event = tokio::time::timeout(Duration::from_secs(10), handle.events.recv())
            .await
            .expect("in time")
            .expect("open");
        if matches!(event, AgentEvent::FilesReverted { .. }) {
            break event;
        }
    };
    assert_eq!(
        outline(&[reverted]),
        ["reverted a.txt,b.txt skipped c.txt: it changed after the agent's edit"]
    );
    assert!(!ws.join("a.txt").exists());
    assert_eq!(
        std::fs::read_to_string(ws.join("b.txt")).expect("b"),
        "old\n"
    );
    assert_eq!(
        std::fs::read_to_string(ws.join("c.txt")).expect("c"),
        "mine\n"
    );

    // A second undo has nothing left; the model hears about the first.
    handle.ops.send(Op::UndoLastTurn).await.expect("undo");
    handle
        .ops
        .send(Op::UserMessage("thanks".into()))
        .await
        .expect("send");
    let mut more = Vec::new();
    collect_turn(&handle, &mut more).await;
    let _ = handle.ops.send(Op::Shutdown).await;
    assert!(more.iter().any(|e| matches!(e, AgentEvent::Error(m)
        if m == "There are no file changes by the agent to undo.")));
    let requests = requests.lock().expect("lock").clone();
    let last_user = requests.last().expect("request")["messages"]
        .as_array()
        .expect("messages")
        .iter()
        .rev()
        .find(|m| m["role"] == "user")
        .expect("user")["content"]
        .as_str()
        .expect("text")
        .to_string();
    assert!(
        last_user.starts_with("[Note: the user undid your file changes")
            && last_user.contains("a.txt, b.txt")
            && last_user.ends_with("thanks"),
        "{last_user}"
    );
}

#[tokio::test]
async fn a_reply_cut_off_mid_stream_retries_the_step() {
    let (_guard, ws) = temp_workspace();
    let cut = sse(&[
        json!({"choices": [{"delta": {"content": "Partial"}}]}),
        json!({"error": {"message": "upstream reset"}}),
    ]);
    let (url, requests) = mock_server(vec![cut, sse_text("Whole answer.")]).await;
    let events = run_one_turn(url, ws, "hello").await;
    assert_eq!(
        outline(&events),
        [
            "turn",
            "step 0",
            "error The model's reply was cut off (provider error: upstream reset). Retrying this step.",
            "step 1",
            "finished Completed",
        ]
    );
    assert_eq!(requests.lock().expect("lock").len(), 2);
}

#[test]
fn only_calls_that_can_move_content_rescan_before_stamping() {
    // Mutating tools and commands observe drift before their results are
    // stamped with a work revision.
    assert!(needs_rescan_before_stamping(tools::WRITE_FILE, true));
    assert!(needs_rescan_before_stamping(tools::RUN_COMMAND, true));
    // A subagent is not classified as mutating, but a child without its own
    // checkout edits this workspace.
    assert!(needs_rescan_before_stamping(tools::SPAWN_AGENT, false));
    // Inspection and bookkeeping are covered by the once-per-step scan: a
    // batch of reads must not re-read the whole evidence set per call.
    for name in [
        tools::READ_FILE,
        tools::GREP,
        tools::LIST_DIR,
        tools::UPDATE_PLAN,
        tools::REPORT_TASK,
    ] {
        assert!(
            !needs_rescan_before_stamping(name, false),
            "{name} cannot move content and must not force a per-call rescan"
        );
    }
}

#[tokio::test]
async fn a_read_only_task_sends_the_same_inspection_specs_on_every_step() {
    let (_guard, ws) = temp_workspace();
    std::fs::write(ws.join("a.txt"), "alpha\n").expect("write");
    let (url, requests) = mock_server(vec![
        sse_tool_calls(&[("r1", "read_file", json!({"path": "a.txt"}))]),
        sse_text("Inspected."),
    ])
    .await;
    let events = run_one_turn(url, ws, "Inspect a.txt. Do not modify anything.").await;
    assert!(
        events
            .iter()
            .any(|event| matches!(event, AgentEvent::TurnFinished { .. })),
        "the turn must finish"
    );
    let requests = requests.lock().expect("lock").clone();
    assert_eq!(requests.len(), 2);
    let names = |index: usize| -> Vec<String> {
        requests[index]["tools"]
            .as_array()
            .expect("tool specs")
            .iter()
            .map(|spec| {
                spec["function"]["name"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string()
            })
            .collect()
    };
    let first = names(0);
    assert!(first.contains(&tools::READ_FILE.to_string()));
    assert!(!first.contains(&tools::WRITE_FILE.to_string()));
    assert!(!first.contains(&tools::EDIT_FILE.to_string()));
    // The read-only variant is derived once; later steps must send exactly
    // the same specs rather than a rebuilt (and possibly drifting) copy.
    assert_eq!(requests[0]["tools"], requests[1]["tools"]);
    let run_command = requests[0]["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .find(|spec| spec["function"]["name"] == tools::RUN_COMMAND)
        .expect("run_command stays visible");
    assert!(
        run_command["function"]["description"]
            .as_str()
            .is_some_and(|description| description.starts_with("Inspect only")),
        "read-only run_command must carry the inspection-only description"
    );
}
