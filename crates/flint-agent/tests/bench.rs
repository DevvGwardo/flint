//! End-to-end overhead measurements. Skipped unless `FLINT_BENCH=1`:
//!
//! FLINT_BENCH=1 cargo test -p flint-agent --release --test bench -- --nocapture --test-threads=1

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::Instant;

use flint_agent::AgentConfig;
use flint_agent::AgentEvent;
use flint_agent::ApprovalMode;
use flint_agent::Op;
use serde_json::json;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

fn enabled() -> bool {
    std::env::var("FLINT_BENCH").as_deref() == Ok("1")
}

fn config(url: String, workspace: &Path) -> AgentConfig {
    let mut config = AgentConfig::new(
        workspace.to_path_buf(),
        url,
        "bench".to_string(),
        "bench".to_string(),
    );
    config.approval = ApprovalMode::Auto;
    config.jev = None;
    config
}

/// Serves `steps` tool-call responses (list_dir with a distinct depth/path),
/// then a final text answer. Responds instantly.
async fn mock(steps: usize) -> String {
    mock_with(steps, None).await
}

/// Like [`mock`]; when `command` is set, every tool call is that run_command.
async fn mock_with(steps: usize, command: Option<&'static str>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let url = format!("http://{}/v1", listener.local_addr().expect("addr"));
    let served = Arc::new(AtomicUsize::new(0));
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let served = Arc::clone(&served);
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut chunk = vec![0u8; 65536];
                loop {
                    let n = socket.read(&mut chunk).await.unwrap_or(0);
                    if n == 0 {
                        return;
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
                    if buf.len() < split + 4 + length {
                        continue;
                    }
                    buf.drain(..split + 4 + length);
                    let i = served.fetch_add(1, Ordering::SeqCst);
                    let mut body = String::new();
                    if i < steps {
                        let (name, arguments) = match command {
                            Some(command) => {
                                ("run_command", json!({"command": command}).to_string())
                            }
                            None => (
                                "list_dir",
                                json!({"path": ".", "depth": 1 + i % 3, "n": i}).to_string(),
                            ),
                        };
                        let call = json!({"choices": [{"delta": {"tool_calls": [{"index": 0, "id": format!("c{i}"),
                            "function": {"name": name, "arguments": arguments}}]}}]});
                        body.push_str(&format!("data: {call}\n\n"));
                    } else {
                        body.push_str(&format!(
                            "data: {}\n\n",
                            json!({"choices": [{"delta": {"content": "done"}}]})
                        ));
                    }
                    body.push_str(&format!(
                        "data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
                        json!({"choices": [{"delta": {}, "finish_reason": if i < steps { "tool_calls" } else { "stop" }}]}),
                        json!({"choices": [], "usage": {"prompt_tokens": 1000 + i, "completion_tokens": 10}})
                    ));
                    let reply = format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\n\r\n{body}",
                        body.len()
                    );
                    if socket.write_all(reply.as_bytes()).await.is_err() {
                        return;
                    }
                }
            });
        }
    });
    url
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bench_agent_step_overhead() {
    if !enabled() {
        return;
    }
    let steps = 50;
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("a.txt"), "x").expect("write");
    let url = mock(steps).await;
    let handle = flint_agent::spawn_session(config(url, dir.path()));
    let started = Instant::now();
    handle
        .ops
        .send(Op::UserMessage("list things".into()))
        .await
        .expect("send");
    let mut step_count = 0;
    loop {
        let event = tokio::time::timeout(Duration::from_secs(60), handle.events.recv())
            .await
            .expect("in time")
            .expect("open");
        match event {
            AgentEvent::StepStarted { .. } => step_count += 1,
            AgentEvent::TurnFinished { reason, .. } => {
                eprintln!("finished {reason:?}");
                break;
            }
            _ => {}
        }
    }
    let elapsed = started.elapsed();
    let _ = handle.ops.send(Op::Shutdown).await;
    eprintln!(
        "BENCH agent_step: {step_count} steps in {elapsed:?} -> {:?}/step (localhost mock, includes HTTP + list_dir)",
        elapsed / step_count
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bench_command_flood() {
    if !enabled() {
        return;
    }
    let dir = tempfile::tempdir().expect("tempdir");
    let streamed = AtomicUsize::new(0);
    let callbacks = AtomicUsize::new(0);
    let on_output = |chunk: String| {
        streamed.fetch_add(chunk.len(), Ordering::Relaxed);
        callbacks.fetch_add(1, Ordering::Relaxed);
    };
    let args = json!({"command": "yes 'flint flood line' | head -c 100000000"});
    let started = Instant::now();
    let outcome = flint_agent::tools::execute(
        &flint_agent::tools::ToolContext::new(dir.path().to_path_buf(), false),
        "run_command",
        args.as_object().expect("object"),
        &on_output,
        &CancellationToken::new(),
    )
    .await;
    eprintln!(
        "BENCH command_flood 100MB: {:?}, streamed {} MB in {} callbacks, model output {} chars, exit {:?}",
        started.elapsed(),
        streamed.load(Ordering::Relaxed) / 1_048_576,
        callbacks.load(Ordering::Relaxed),
        outcome.output.len(),
        outcome.exit_code
    );
}

/// A 100 MB command inside a real session, with a slow consumer that only
/// starts reading after the turn is over (worst case for the event queue).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bench_session_flood() {
    if !enabled() {
        return;
    }
    let dir = tempfile::tempdir().expect("tempdir");
    let url = mock_with(1, Some("yes 'flint flood line' | head -c 100000000")).await;
    let handle = flint_agent::spawn_session(config(url, dir.path()));
    let started = Instant::now();
    handle
        .ops
        .send(Op::UserMessage("flood".into()))
        .await
        .expect("send");
    // Let the engine run ahead of the consumer.
    tokio::time::sleep(Duration::from_secs(2)).await;
    let queued = handle.events.len();
    let (mut delta_bytes, mut delta_events) = (0usize, 0usize);
    loop {
        let event = tokio::time::timeout(Duration::from_secs(60), handle.events.recv())
            .await
            .expect("in time")
            .expect("open");
        match event {
            AgentEvent::ToolOutputDelta { chunk, .. } => {
                delta_bytes += chunk.len();
                delta_events += 1;
            }
            AgentEvent::TurnFinished { .. } => break,
            _ => {}
        }
    }
    let _ = handle.ops.send(Op::Shutdown).await;
    eprintln!(
        "BENCH session_flood 100MB: {:?}, {queued} events queued for the UI, {delta_events} output deltas totalling {} KB",
        started.elapsed(),
        delta_bytes / 1024
    );
}
