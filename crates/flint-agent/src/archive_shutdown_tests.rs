use super::*;
use pretty_assertions::assert_eq;

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
