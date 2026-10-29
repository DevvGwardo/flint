use super::*;
use pretty_assertions::assert_eq;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::test]
async fn interrupt_before_turn_started_during_blocked_bootstrap_never_runs_inference() {
    let (_root, workspace) = temp_workspace();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let release = std::sync::Arc::new(tokio::sync::Notify::new());
    let gate = release.clone();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        loop {
            let mut buffer = [0; 1024];
            let count = socket.read(&mut buffer).await.unwrap();
            assert!(count > 0);
            request.extend_from_slice(&buffer[..count]);
            if request.windows(4).any(|part| part == b"\r\n\r\n") {
                break;
            }
        }
        assert!(request.starts_with(b"GET "));
        ready_tx.send(()).unwrap();
        gate.notified().await;
        let body = br#"{"data":[]}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = socket.write_all(response.as_bytes()).await;
        let _ = socket.write_all(body).await;
        // No completion connection should ever arrive for the cancelled input.
        assert!(
            tokio::time::timeout(Duration::from_millis(100), listener.accept())
                .await
                .is_err()
        );
    });
    let mut config = test_config(url, workspace.clone());
    config.context_budget_tokens = 0;
    let handle = crate::spawn_session(config);
    tokio::time::timeout(Duration::from_secs(10), ready_rx)
        .await
        .unwrap()
        .unwrap();
    handle
        .ops
        .send(Op::UserMessage("Create a result.txt file.".into()))
        .await
        .unwrap();
    handle.ops.send(Op::Interrupt).await.unwrap();
    let events = tokio::time::timeout(Duration::from_secs(3), async {
        let mut events = Vec::new();
        loop {
            let event = handle.events.recv().await.unwrap();
            let done = matches!(event, AgentEvent::TurnFinished { .. });
            events.push(event);
            if done {
                break events;
            }
        }
    })
    .await
    .expect("pending Stop must finish without waiting for the blocked catalog");
    assert!(events.iter().any(|event| matches!(
        event,
        AgentEvent::TurnFinished {
            reason: TurnEndReason::Interrupted,
            ..
        }
    )));
    assert!(!events.iter().any(|event| matches!(
        event,
        AgentEvent::StepStarted { .. } | AgentEvent::ToolCallStarted { .. }
    )));
    assert!(!workspace.join("result.txt").exists());
    assert_eq!(
        assessed(&events).assessment.as_ref().unwrap().reason,
        TurnEndReason::Interrupted
    );
    release.notify_one();
    server.await.unwrap();
    handle.ops.send(Op::Shutdown).await.unwrap();
    loop {
        if matches!(
            handle.events.recv().await.unwrap(),
            AgentEvent::SessionStopped {
                history_saved: true
            }
        ) {
            break;
        }
    }
}
