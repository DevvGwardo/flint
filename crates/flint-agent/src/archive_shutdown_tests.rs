use super::*;
use pretty_assertions::assert_eq;

/// Each Unix fixture and its sleeping descendant retain a loopback socket.
/// EOF therefore witnesses cleanup of both processes, not just the MCP parent.
#[cfg(unix)]
const BOOTSTRAP_MCP: &str = r#"
import json, pathlib, socket, subprocess, sys
role, address = sys.argv[1:3]
pathlib.Path(role + ".started").write_text("started")
host, port = address.rsplit(":", 1)
sock = socket.create_connection((host, int(port)))
child = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(60)"],
                         pass_fds=(sock.fileno(),))
for line in sys.stdin:
    m = json.loads(line)
    method, mid = m.get("method"), m.get("id")
    if method == "initialize":
        sock.sendall(b"R")
        if role == "stall":
            sock.recv(1)
        result = {"protocolVersion": "2025-06-18", "capabilities": {"tools": {}},
                  "serverInfo": {"name": role, "version": "1"}}
    elif method == "tools/list":
        result = {"tools": [{"name": "echo", "inputSchema": {"type": "object"}}]}
    else:
        continue
    print(json.dumps({"jsonrpc": "2.0", "id": mid, "result": result}), flush=True)
"#;

#[cfg(unix)]
fn bootstrap_mcp_config(
    workspace: &std::path::Path,
    address: std::net::SocketAddr,
    role: &str,
) -> crate::McpServerConfig {
    let script = workspace.join("bootstrap-mcp.py");
    std::fs::write(&script, BOOTSTRAP_MCP).unwrap();
    crate::McpServerConfig {
        name: role.into(),
        command: "python3".into(),
        args: vec![
            script.display().to_string(),
            role.into(),
            address.to_string(),
        ],
        env: Vec::new(),
    }
}

#[cfg(unix)]
async fn bootstrap_mcp_ready(listener: &TcpListener) -> tokio::net::TcpStream {
    tokio::time::timeout(Duration::from_secs(5), async {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut ready = [0];
        socket.read_exact(&mut ready).await.unwrap();
        assert_eq!(ready, [b'R']);
        socket
    })
    .await
    .expect("fixture must receive initialize before cancellation")
}

#[cfg(unix)]
async fn assert_bootstrap_mcp_cleaned(mut socket: tokio::net::TcpStream) {
    let mut byte = [0];
    let count = tokio::time::timeout(Duration::from_secs(2), socket.read(&mut byte))
        .await
        .expect("MCP parent and descendant must release their socket")
        .unwrap();
    assert_eq!(count, 0);
}

#[cfg(unix)]
async fn stop_during_mcp_bootstrap(close_intake: bool) {
    let (_guard, ws) = temp_workspace();
    let saved = tempfile::tempdir().unwrap();
    // No accept task: any provider lookup or turn leaves a connection visible.
    let provider = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let mut config = test_config(
        format!("http://{}/v1", provider.local_addr().unwrap()),
        ws.clone(),
    );
    config.context_budget_tokens = 0;
    config.session_dir = Some(saved.path().to_path_buf());
    config.mcp_servers = ["ready", "stall", "later"]
        .iter()
        .map(|role| bootstrap_mcp_config(&ws, address, role))
        .collect();
    let (ops, ops_rx) = async_channel::unbounded();
    let (events_tx, events_rx) = async_channel::unbounded();
    // Run the native session caller on this runtime so a failing regression
    // can abort it and reclaim its fixture processes before asserting.
    let task = tokio::spawn(run(
        config,
        crate::ExecutionOptions::default(),
        ops_rx,
        events_tx,
    ));
    let ready = bootstrap_mcp_ready(&listener).await;
    let stalled = bootstrap_mcp_ready(&listener).await;
    assert!(ws.join("ready.started").exists());
    assert!(ws.join("stall.started").exists());
    assert!(!ws.join("later.started").exists());
    ops.send(Op::UserMessage("Queued inspection.".into()))
        .await
        .unwrap();
    if close_intake {
        drop(ops);
    } else {
        ops.send(Op::Shutdown).await.unwrap();
    }
    let result = tokio::time::timeout(Duration::from_secs(2), async {
        let mut events = Vec::new();
        while let Ok(event) = events_rx.recv().await {
            events.push(event);
        }
        events
    })
    .await;
    if result.is_err() {
        task.abort();
        let _ = task.await;
    } else {
        task.await.unwrap();
    }
    // Assert cleanup even on the failing-before path; it cannot be excused
    // by a broken fixture or an unowned descendant holding stdout open.
    assert_bootstrap_mcp_cleaned(ready).await;
    assert_bootstrap_mcp_cleaned(stalled).await;
    let events = result.expect("shutdown/closed intake must cancel stalled MCP bootstrap");
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(
                event,
                AgentEvent::SessionStopped {
                    history_saved: true
                }
            ))
            .count(),
        1
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, AgentEvent::TurnStarted { .. }))
    );
    assert!(!ws.join("later.started").exists());
    assert!(
        tokio::time::timeout(Duration::from_millis(100), provider.accept())
            .await
            .is_err(),
        "neither model lookup nor queued work may reach the provider"
    );
    // Shutdown flushed the unused saver without inventing a conversation.
    assert!(persist::load(saved.path()).unwrap().is_none());
}

#[cfg(unix)]
#[tokio::test]
async fn shutdown_cancels_stalled_mcp_bootstrap_and_reclaims_owned_processes() {
    stop_during_mcp_bootstrap(false).await;
}

#[cfg(unix)]
#[tokio::test]
async fn closed_intake_cancels_stalled_mcp_bootstrap_and_reclaims_owned_processes() {
    stop_during_mcp_bootstrap(true).await;
}

#[cfg(unix)]
#[tokio::test]
async fn interrupt_during_mcp_bootstrap_preserves_inventory_and_queued_turn() {
    let (_guard, ws) = temp_workspace();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (url, requests) = mock_server(vec![sse_text("Ready.")]).await;
    let mut config = test_config(url, ws.clone());
    config.mcp_servers = vec![bootstrap_mcp_config(
        &ws,
        listener.local_addr().unwrap(),
        "stall",
    )];
    let handle = crate::spawn_session(config);
    let mut stalled = bootstrap_mcp_ready(&listener).await;
    handle.ops.send(Op::Interrupt).await.unwrap();
    handle
        .ops
        .send(Op::UserMessage("Queued inspection.".into()))
        .await
        .unwrap();
    // FIFO intake must consume Interrupt before this later message. Keep
    // initialization stalled until that has happened, rather than racing it.
    tokio::time::timeout(Duration::from_secs(2), async {
        while !handle.ops.is_empty() {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap();
    stalled.write_all(b"C").await.unwrap();
    let mut events = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), collect_turn(&handle, &mut events))
        .await
        .unwrap();
    assert_eq!(requests.lock().unwrap().len(), 1);
    assert!(
        requests.lock().unwrap()[0]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool["function"]["name"] == "mcp__stall__echo")
    );
    handle.ops.send(Op::Shutdown).await.unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while handle.events.recv().await.is_ok() {}
    })
    .await
    .unwrap();
    assert_bootstrap_mcp_cleaned(stalled).await;
}

#[tokio::test]
async fn shutdown_interrupts_initial_model_lookup_without_dispatching_queued_work() {
    let (_guard, ws) = temp_workspace();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        let mut chunk = [0; 1024];
        loop {
            let count = socket.read(&mut chunk).await.unwrap();
            assert!(count > 0);
            request.extend_from_slice(&chunk[..count]);
            if request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                break;
            }
        }
        assert!(request.starts_with(b"GET /v1/models "));
        ready_tx.send(()).unwrap();
        // Observe the client dropping the lookup instead of waiting for its
        // ten-second HTTP timeout. The server never supplies credentials/data.
        let _ = socket.read(&mut chunk).await;
    });
    let mut config = test_config(url, ws);
    config.context_budget_tokens = 0;
    let handle = crate::spawn_session(config);
    tokio::time::timeout(Duration::from_secs(5), ready_rx)
        .await
        .unwrap()
        .unwrap();
    handle
        .ops
        .send(Op::UserMessage("Queued inspection.".into()))
        .await
        .unwrap();
    handle.ops.send(Op::Shutdown).await.unwrap();
    let events = tokio::time::timeout(Duration::from_secs(2), async {
        let mut events = Vec::new();
        while let Ok(event) = handle.events.recv().await {
            events.push(event);
        }
        events
    })
    .await
    .expect("shutdown must cancel bootstrap lookup");
    assert!(events.iter().any(|event| matches!(
        event,
        AgentEvent::SessionStopped {
            history_saved: true
        }
    )));
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, AgentEvent::TurnStarted { .. }))
    );
    assert!(handle.ops.is_closed());
    tokio::time::timeout(Duration::from_secs(2), server)
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn shutdown_discards_queued_messages_and_reports_final_history_flush() {
    let (_guard, ws) = temp_workspace();
    let saved = tempfile::tempdir().unwrap();
    let (url, requests) = mock_server(vec![sse_tool_call(
        "pending",
        "write_file",
        json!({"path":"fixture.txt","content":"fixture"}),
    )])
    .await;
    let mut config = test_config(url, ws.clone());
    config.approval = ApprovalMode::AskForChanges;
    config.session_dir = Some(saved.path().to_path_buf());
    let handle = crate::spawn_session(config);
    handle
        .ops
        .send(Op::UserMessage("fixture".into()))
        .await
        .unwrap();
    loop {
        if matches!(
            handle.events.recv().await.unwrap(),
            AgentEvent::ApprovalRequested { .. }
        ) {
            break;
        }
    }
    handle
        .ops
        .send(Op::UserMessage("queued fixture".into()))
        .await
        .unwrap();
    handle.ops.send(Op::Shutdown).await.unwrap();
    let mut stopped = false;
    while let Ok(event) = tokio::time::timeout(Duration::from_secs(5), handle.events.recv())
        .await
        .unwrap()
    {
        if let AgentEvent::SessionStopped { history_saved } = event {
            assert!(history_saved);
            stopped = true;
        }
    }
    assert!(stopped);
    assert_eq!(requests.lock().unwrap().len(), 1);
    assert!(!ws.join("fixture.txt").exists());
    let history = persist::load(saved.path()).unwrap().unwrap();
    assert_eq!(history.turn_id, 1);
    assert!(
        !history
            .messages
            .iter()
            .any(|m| matches!(m, Message::User(s) if s == "queued fixture"))
    );
}
