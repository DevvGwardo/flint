//! Test support: an in-process fake ACP agent that speaks raw JSON-RPC lines
//! over a duplex pipe, and a harness around the runner.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use flint_agent::AgentEvent;
use flint_agent::ApprovalMode;
use flint_agent::Op;
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
pub(crate) struct Fake {
    pub lines: tokio::io::Lines<BufReader<ReadHalf<DuplexStream>>>,
    pub out: WriteHalf<DuplexStream>,
}

impl Fake {
    pub async fn recv(&mut self) -> Value {
        let line = tokio::time::timeout(Duration::from_secs(10), self.lines.next_line())
            .await
            .expect("client message in time")
            .expect("read")
            .expect("open pipe");
        serde_json::from_str(&line).expect("json")
    }

    /// Next message with this method (skips others).
    pub async fn expect(&mut self, method: &str) -> Value {
        loop {
            let message = self.recv().await;
            if message["method"] == method {
                return message;
            }
        }
    }

    /// The client's answer to the request with this id.
    pub async fn answered(&mut self, id: &str) -> Value {
        loop {
            let message = self.recv().await;
            if message["method"].is_null() && message["id"] == id {
                return message;
            }
        }
    }

    pub async fn send(&mut self, value: Value) {
        let mut line = value.to_string();
        line.push('\n');
        self.out.write_all(line.as_bytes()).await.expect("write");
    }

    pub async fn respond(&mut self, request: &Value, result: Value) {
        self.send(json!({"jsonrpc": "2.0", "id": request["id"], "result": result}))
            .await;
    }

    pub async fn update(&mut self, update: Value) {
        self.send(json!({"jsonrpc": "2.0", "method": "session/update",
            "params": {"sessionId": "s1", "update": update}}))
            .await;
    }

    /// initialize + session/new answered with these `configOptions`.
    pub async fn handshake_with_options(&mut self, options: Value) -> Value {
        let init = self.expect("initialize").await;
        self.respond(
            &init,
            json!({"protocolVersion": 1, "agentCapabilities": {"loadSession": false}}),
        )
        .await;
        let new = self.expect("session/new").await;
        self.respond(&new, json!({"sessionId": "s1", "configOptions": options}))
            .await;
        new
    }

    /// initialize + session/new.
    pub async fn handshake(&mut self) {
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

    pub async fn permission(&mut self, id: &str, call: &str, title: &str) {
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

pub(crate) struct Harness {
    pub ops: async_channel::Sender<Op>,
    pub events: async_channel::Receiver<AgentEvent>,
    pub stderr: Arc<Mutex<String>>,
    pub _dir: tempfile::TempDir,
    pub workspace: PathBuf,
}

impl Harness {
    pub async fn next_event(&self) -> AgentEvent {
        tokio::time::timeout(Duration::from_secs(10), self.events.recv())
            .await
            .expect("event in time")
            .expect("open")
    }

    pub async fn next_turn(&self) -> Vec<AgentEvent> {
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

    pub async fn until(&self, pred: impl Fn(&AgentEvent) -> bool) -> AgentEvent {
        loop {
            let event = self.next_event().await;
            if pred(&event) {
                return event;
            }
        }
    }

    pub async fn send(&self, op: Op) {
        self.ops.send(op).await.expect("send");
    }
}

pub(crate) fn start(approval: ApprovalMode) -> (Harness, Fake) {
    start_with(approval, None)
}

/// Like [`start`], with a session directory (relative to the temp dir).
pub(crate) fn start_with(approval: ApprovalMode, session_dir: Option<&str>) -> (Harness, Fake) {
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
        session_dir: session_dir.map(|d| workspace.join(d)),
        approval,
        agent_terminals: true,
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

pub(crate) fn outline(events: &[AgentEvent]) -> Vec<String> {
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
            AgentEvent::SessionOptions(list) => format!(
                "options {}",
                list.iter()
                    .map(|o| format!("{}={}", o.id, o.current))
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            other => format!("{other:?}"),
        })
        .collect()
}
