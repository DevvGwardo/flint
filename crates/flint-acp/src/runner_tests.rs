//! The runner against an in-process fake ACP agent that speaks raw
//! JSON-RPC lines over a duplex pipe.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use flint_agent::AgentEvent;
use flint_agent::ApprovalDecision;
use flint_agent::ApprovalMode;
use flint_agent::Op;
use flint_agent::ToolKind;
use flint_agent::TurnEndReason;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncWriteExt;
use tokio::io::BufReader;
use tokio::io::DuplexStream;
use tokio::io::ReadHalf;
use tokio::io::WriteHalf;
use tokio_util::compat::TokioAsyncReadCompatExt;
use tokio_util::compat::TokioAsyncWriteCompatExt;

use crate::launch::AcpAgent;
use crate::runner::RunContext;

/// The agent side of the pipe.
struct Fake {
    lines: tokio::io::Lines<BufReader<ReadHalf<DuplexStream>>>,
    out: WriteHalf<DuplexStream>,
}

impl Fake {
    async fn recv(&mut self) -> Value {
        let line = tokio::time::timeout(Duration::from_secs(10), self.lines.next_line())
            .await
            .expect("client message in time")
            .expect("read")
            .expect("open pipe");
        serde_json::from_str(&line).expect("json")
    }

    /// Next message with this method (skips others).
    async fn expect(&mut self, method: &str) -> Value {
        loop {
            let message = self.recv().await;
            if message["method"] == method {
                return message;
            }
        }
    }

    async fn send(&mut self, value: Value) {
        let mut line = value.to_string();
        line.push('\n');
        self.out.write_all(line.as_bytes()).await.expect("write");
    }

    async fn respond(&mut self, request: &Value, result: Value) {
        self.send(json!({"jsonrpc": "2.0", "id": request["id"], "result": result}))
            .await;
    }

    async fn update(&mut self, update: Value) {
        self.send(json!({"jsonrpc": "2.0", "method": "session/update",
            "params": {"sessionId": "s1", "update": update}}))
            .await;
    }

    /// initialize + session/new.
    async fn handshake(&mut self) {
        let init = self.expect("initialize").await;
        assert_eq!(
            init["params"]["clientCapabilities"]["fs"],
            json!({"readTextFile": true, "writeTextFile": true})
        );
        self.respond(
            &init,
            json!({"protocolVersion": 1, "agentCapabilities": {"loadSession": false}, "authMethods": []}),
        )
        .await;
        let new = self.expect("session/new").await;
        self.respond(&new, json!({"sessionId": "s1"})).await;
    }

    async fn permission(&mut self, id: &str, call: &str, title: &str) {
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": "session/request_permission", "params": {
            "sessionId": "s1",
            "options": [
                {"optionId": "yes", "name": "Allow", "kind": "allow_once"},
                {"optionId": "always", "name": "Always", "kind": "allow_always"},
                {"optionId": "no", "name": "Reject", "kind": "reject_once"}
            ],
            "toolCall": {"toolCallId": call, "title": title, "kind": "execute", "status": "pending"}}}))
            .await;
    }
}

struct Harness {
    ops: async_channel::Sender<Op>,
    events: async_channel::Receiver<AgentEvent>,
    stderr: Arc<Mutex<String>>,
    _dir: tempfile::TempDir,
    workspace: PathBuf,
}

impl Harness {
    async fn next_event(&self) -> AgentEvent {
        tokio::time::timeout(Duration::from_secs(10), self.events.recv())
            .await
            .expect("event in time")
            .expect("open")
    }

    async fn next_turn(&self) -> Vec<AgentEvent> {
        let mut events = Vec::new();
        loop {
            let event = self.next_event().await;
            let done = matches!(event, AgentEvent::TurnFinished { .. });
            events.push(event);
            if done {
                return events;
            }
        }
    }

    async fn until(&self, pred: impl Fn(&AgentEvent) -> bool) -> AgentEvent {
        loop {
            let event = self.next_event().await;
            if pred(&event) {
                return event;
            }
        }
    }

    async fn send(&self, op: Op) {
        self.ops.send(op).await.expect("send");
    }
}

fn start(approval: ApprovalMode) -> (Harness, Fake) {
    let dir = tempfile::tempdir().expect("tempdir");
    let workspace = dir.path().canonicalize().expect("canonical");
    let (client_side, agent_side) = tokio::io::duplex(1 << 20);
    let (client_read, client_write) = tokio::io::split(client_side);
    let (agent_read, agent_write) = tokio::io::split(agent_side);
    let (ops_tx, ops_rx) = async_channel::unbounded();
    let (events_tx, events_rx) = async_channel::unbounded();
    let stderr = Arc::new(Mutex::new(String::new()));
    let context = RunContext {
        agent: AcpAgent::ClaudeCode,
        workspace: workspace.clone(),
        session_dir: None,
        approval,
        ops: ops_rx,
        events: events_tx,
        stderr: Arc::clone(&stderr),
    };
    tokio::spawn(crate::runner::run(
        client_read.compat(),
        client_write.compat_write(),
        context,
    ));
    let fake = Fake {
        lines: BufReader::new(agent_read).lines(),
        out: agent_write,
    };
    let harness = Harness {
        ops: ops_tx,
        events: events_rx,
        stderr,
        _dir: dir,
        workspace,
    };
    (harness, fake)
}

fn outline(events: &[AgentEvent]) -> Vec<String> {
    events
        .iter()
        .map(|event| match event {
            AgentEvent::TurnStarted { turn_id } => format!("turn {turn_id}"),
            AgentEvent::StepStarted { step, .. } => format!("step {step}"),
            AgentEvent::ReasoningDelta(text) => format!("thinking {text}"),
            AgentEvent::TextDelta(text) => format!("text {text}"),
            AgentEvent::ToolCallStarted {
                call_id,
                kind,
                summary,
                ..
            } => format!("start {call_id} {kind:?} {summary}"),
            AgentEvent::ToolCallFinished {
                call_id,
                output,
                success,
                diff,
                ..
            } => format!(
                "finish {call_id} ok={success} {output:?}{}",
                diff.as_ref()
                    .map(|d| format!(" diff {} +{} -{}", d.path, d.added, d.removed))
                    .unwrap_or_default()
            ),
            AgentEvent::ApprovalRequested {
                call_id,
                kind,
                summary,
            } => format!("approve? {call_id} {kind:?} {summary}"),
            AgentEvent::Usage(u) => format!("usage {} {}", u.input_tokens, u.output_tokens),
            AgentEvent::TurnFinished { reason, .. } => format!("finished {reason:?}"),
            AgentEvent::Error(message) => format!("error {message}"),
            other => format!("{other:?}"),
        })
        .collect()
}

#[tokio::test]
async fn maps_a_turn_of_updates() {
    let (harness, mut fake) = start(ApprovalMode::Auto);
    fake.handshake().await;
    harness.send(Op::UserMessage("make hello.py".into())).await;
    let prompt = fake.expect("session/prompt").await;
    assert_eq!(
        prompt["params"]["prompt"],
        json!([{"type": "text", "text": "make hello.py"}])
    );
    let hello = format!("{}/hello.py", harness.workspace.display());
    fake.update(json!({"sessionUpdate": "agent_thought_chunk", "content": {"type": "text", "text": "plan"}})).await;
    fake.update(json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "Writing it."}})).await;
    fake.update(
        json!({"sessionUpdate": "tool_call", "toolCallId": "t1", "title": "Write hello.py",
        "kind": "edit", "status": "pending", "rawInput": {"file_path": "hello.py"}}),
    )
    .await;
    fake.update(
        json!({"sessionUpdate": "tool_call_update", "toolCallId": "t1", "status": "completed",
        "content": [{"type": "diff", "path": hello, "oldText": null, "newText": "print('hi')\n"}]}),
    )
    .await;
    fake.update(
        json!({"sessionUpdate": "tool_call", "toolCallId": "t2", "title": "python3 hello.py",
        "kind": "execute", "status": "in_progress"}),
    )
    .await;
    fake.update(
        json!({"sessionUpdate": "tool_call_update", "toolCallId": "t2", "status": "completed",
        "content": [{"type": "content", "content": {"type": "text", "text": "hi\n"}}]}),
    )
    .await;
    fake.update(json!({"sessionUpdate": "plan", "entries": []}))
        .await;
    fake.update(json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "Done."}})).await;
    fake.respond(
        &prompt,
        json!({"stopReason": "end_turn", "usage": {"totalTokens": 30, "inputTokens": 20, "outputTokens": 10}}),
    )
    .await;

    assert_eq!(
        outline(&harness.next_turn().await),
        vec![
            "turn 1",
            "step 0",
            "thinking plan",
            "text Writing it.",
            "start t1 Edit Write hello.py",
            "finish t1 ok=true \"\" diff hello.py +1 -0",
            "start t2 Command python3 hello.py",
            "finish t2 ok=true \"hi\\n\"",
            "step 1",
            "text Done.",
            "usage 20 10",
            "finished Completed",
        ]
    );
}

#[tokio::test]
async fn permission_round_trip_picks_the_matching_option() {
    let (harness, mut fake) = start(ApprovalMode::AskForChanges);
    fake.handshake().await;
    harness.send(Op::UserMessage("clean up".into())).await;
    let prompt = fake.expect("session/prompt").await;

    fake.permission("p1", "t1", "rm -rf build").await;
    let approval = harness
        .until(|e| matches!(e, AgentEvent::ApprovalRequested { .. }))
        .await;
    assert_eq!(
        approval,
        AgentEvent::ApprovalRequested {
            call_id: "t1".into(),
            kind: ToolKind::Command,
            summary: "rm -rf build".into()
        }
    );
    harness
        .send(Op::Approval {
            call_id: "t1".into(),
            decision: ApprovalDecision::Deny,
        })
        .await;
    let answer = fake.recv().await;
    assert_eq!(answer["id"], "p1");
    assert_eq!(
        answer["result"]["outcome"],
        json!({"outcome": "selected", "optionId": "no"})
    );

    // "Always" sticks: the next request is answered without asking.
    fake.permission("p2", "t2", "ls").await;
    harness
        .until(|e| matches!(e, AgentEvent::ApprovalRequested { call_id, .. } if call_id == "t2"))
        .await;
    harness
        .send(Op::Approval {
            call_id: "t2".into(),
            decision: ApprovalDecision::ApproveAlways,
        })
        .await;
    assert_eq!(fake.recv().await["result"]["outcome"]["optionId"], "always");
    fake.permission("p3", "t3", "pwd").await;
    assert_eq!(fake.recv().await["result"]["outcome"]["optionId"], "yes");

    fake.respond(&prompt, json!({"stopReason": "end_turn"}))
        .await;
    let events = harness.next_turn().await;
    assert_eq!(
        events.last(),
        Some(&AgentEvent::TurnFinished {
            turn_id: 1,
            reason: TurnEndReason::Completed
        })
    );
}

#[tokio::test]
async fn interrupt_sends_cancel_and_ends_interrupted() {
    let (harness, mut fake) = start(ApprovalMode::AskForChanges);
    fake.handshake().await;
    harness.send(Op::UserMessage("long task".into())).await;
    let prompt = fake.expect("session/prompt").await;
    fake.update(
        json!({"sessionUpdate": "tool_call", "toolCallId": "t1", "title": "sleep 100",
        "kind": "execute", "status": "in_progress"}),
    )
    .await;
    fake.permission("p1", "t2", "rm x").await;
    harness
        .until(|e| matches!(e, AgentEvent::ApprovalRequested { .. }))
        .await;
    harness.send(Op::Interrupt).await;
    // The pending permission is cancelled, then session/cancel arrives.
    let cancelled = fake.recv().await;
    assert_eq!(
        cancelled["result"]["outcome"],
        json!({"outcome": "cancelled"})
    );
    let cancel = fake.expect("session/cancel").await;
    assert_eq!(cancel["params"]["sessionId"], "s1");
    fake.respond(&prompt, json!({"stopReason": "cancelled"}))
        .await;
    let events = harness.next_turn().await;
    let tail: Vec<String> = outline(&events).into_iter().rev().take(3).collect();
    assert_eq!(
        tail,
        vec![
            "finished Interrupted",
            "finish t2 ok=false \"Interrupted.\"",
            "finish t1 ok=false \"Interrupted.\"",
        ]
    );
}

#[tokio::test]
async fn fs_requests_stay_in_the_workspace_and_writes_become_diffs() {
    let (harness, mut fake) = start(ApprovalMode::Auto);
    fake.handshake().await;
    std::fs::write(harness.workspace.join("a.txt"), "one\ntwo\nthree\n").expect("write");
    harness.send(Op::UserMessage("edit a.txt".into())).await;
    let prompt = fake.expect("session/prompt").await;
    let ws = harness.workspace.display().to_string();

    fake.send(
        json!({"jsonrpc": "2.0", "id": 1, "method": "fs/read_text_file",
        "params": {"sessionId": "s1", "path": format!("{ws}/a.txt"), "line": 2, "limit": 1}}),
    )
    .await;
    assert_eq!(fake.recv().await["result"]["content"], "two");
    fake.send(
        json!({"jsonrpc": "2.0", "id": 2, "method": "fs/read_text_file",
        "params": {"sessionId": "s1", "path": "/etc/hosts"}}),
    )
    .await;
    let refused = fake.recv().await;
    assert!(
        refused["error"]["message"]
            .as_str()
            .is_some_and(|m| m.contains("outside the workspace")),
        "{refused}"
    );

    // A write during an edit call becomes that call's diff.
    fake.update(
        json!({"sessionUpdate": "tool_call", "toolCallId": "t1", "title": "Edit a.txt",
        "kind": "edit", "status": "in_progress"}),
    )
    .await;
    fake.send(json!({"jsonrpc": "2.0", "id": 3, "method": "fs/write_text_file",
        "params": {"sessionId": "s1", "path": format!("{ws}/a.txt"), "content": "one\n2\nthree\n"}}))
        .await;
    assert_eq!(fake.recv().await["result"], json!({}));
    fake.update(
        json!({"sessionUpdate": "tool_call_update", "toolCallId": "t1", "status": "completed"}),
    )
    .await;
    // A write outside any edit call gets its own row.
    fake.send(
        json!({"jsonrpc": "2.0", "id": 4, "method": "fs/write_text_file",
        "params": {"sessionId": "s1", "path": format!("{ws}/new/b.txt"), "content": "b\n"}}),
    )
    .await;
    fake.recv().await;
    fake.respond(&prompt, json!({"stopReason": "end_turn"}))
        .await;

    let events = harness.next_turn().await;
    let rows: Vec<String> = outline(&events)
        .into_iter()
        .filter(|r| r.starts_with("finish ") || r.starts_with("start "))
        .collect();
    assert_eq!(
        rows,
        vec![
            "start t1 Edit Edit a.txt",
            "finish t1 ok=true \"\" diff a.txt +1 -1",
            "start fs-write-1 Edit new/b.txt",
            "finish fs-write-1 ok=true \"Wrote new/b.txt (+1 -0)\" diff new/b.txt +1 -0",
        ]
    );
    assert_eq!(
        std::fs::read_to_string(harness.workspace.join("a.txt")).expect("read"),
        "one\n2\nthree\n"
    );
}

#[tokio::test]
async fn crash_mid_turn_reports_a_clear_error() {
    let (harness, mut fake) = start(ApprovalMode::Auto);
    fake.handshake().await;
    harness.send(Op::UserMessage("hi".into())).await;
    fake.expect("session/prompt").await;
    fake.update(
        json!({"sessionUpdate": "tool_call", "toolCallId": "t1", "title": "ls",
        "kind": "execute", "status": "in_progress"}),
    )
    .await;
    if let Ok(mut stderr) = harness.stderr.lock() {
        stderr.push_str("Error: Please run /login to authenticate\n");
    }
    drop(fake);
    let events = harness.next_turn().await;
    let error = events.iter().find_map(|e| match e {
        AgentEvent::Error(message) => Some(message.clone()),
        _ => None,
    });
    assert!(
        error
            .as_deref()
            .is_some_and(|m| m.contains("isn't logged in") && m.contains("Run `claude` once")),
        "{error:?}"
    );
    assert!(matches!(
        events.last(),
        Some(AgentEvent::TurnFinished {
            reason: TurnEndReason::Failed(_),
            ..
        })
    ));
    assert!(outline(&events).contains(&"finish t1 ok=false \"Interrupted.\"".to_string()));
}

#[tokio::test]
async fn auth_error_on_new_session_says_how_to_log_in() {
    let (harness, mut fake) = start(ApprovalMode::Auto);
    let init = fake.expect("initialize").await;
    fake.respond(
        &init,
        json!({"protocolVersion": 1, "agentCapabilities": {}}),
    )
    .await;
    let new = fake.expect("session/new").await;
    fake.send(json!({"jsonrpc": "2.0", "id": new["id"],
        "error": {"code": -32000, "message": "Authentication required"}}))
        .await;
    let error = harness.until(|e| matches!(e, AgentEvent::Error(_))).await;
    assert_eq!(
        error,
        AgentEvent::Error(
            "Claude Code isn't logged in (Authentication required). Run `claude` once in a \
             terminal to log in, then retry."
                .into()
        )
    );
}
