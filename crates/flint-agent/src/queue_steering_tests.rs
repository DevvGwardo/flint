use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn queue_steering_is_acknowledged_between_steps_without_cancelling_tools() {
    let (_dir, workspace) = temp_workspace();
    let (url, requests) = mock_server(vec![
        sse_tool_call(
            "working",
            "run_command",
            json!({"command": "sleep 0.15; printf 'fixture complete'"}),
        ),
        sse_text("Follow-up applied."),
    ])
    .await;
    let handle = crate::spawn_session(test_config(url, workspace));
    handle
        .ops
        .send(Op::UserMessage("Run the fixture.".into()))
        .await
        .unwrap();
    let mut events = Vec::new();
    let mut steered = false;
    loop {
        let event = tokio::time::timeout(Duration::from_secs(10), handle.events.recv())
            .await
            .unwrap()
            .unwrap();
        if matches!(&event, AgentEvent::ToolCallStarted { name, .. } if name == "run_command")
            && !steered
        {
            handle
                .ops
                .send(Op::SteerMessage {
                    id: 42,
                    text: "Use the new direction.".into(),
                    images: Vec::new(),
                })
                .await
                .unwrap();
            steered = true;
        }
        let finished = matches!(event, AgentEvent::TurnFinished { .. });
        events.push(event);
        if finished {
            break;
        }
    }
    assert!(steered);
    let ack = events
        .iter()
        .position(|event| matches!(event, AgentEvent::SteeringAccepted { id: 42 }))
        .unwrap();
    let tool = events
        .iter()
        .position(|event| matches!(event, AgentEvent::ToolCallFinished { success: true, .. }))
        .unwrap();
    assert!(tool < ack);
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, AgentEvent::TurnStarted { .. }))
            .count(),
        1
    );
    assert!(matches!(
        events.last(),
        Some(AgentEvent::TurnFinished {
            reason: TurnEndReason::Completed,
            ..
        })
    ));
    assert!(
        requests.lock().unwrap()[1]["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| {
                message["role"] == "user" && message["content"] == "Use the new direction."
            })
    );
    handle.ops.send(Op::Shutdown).await.unwrap();
    while tokio::time::timeout(Duration::from_secs(5), handle.events.recv())
        .await
        .unwrap()
        .is_ok()
    {}
}

#[tokio::test]
async fn queue_steering_delivered_to_an_idle_engine_starts_a_normal_turn() {
    let (_dir, workspace) = temp_workspace();
    let (url, requests) = mock_server(vec![sse_text("Applied.")]).await;
    let handle = crate::spawn_session(test_config(url, workspace));
    handle
        .ops
        .send(Op::SteerMessage {
            id: 7,
            text: "Late direction.".into(),
            images: Vec::new(),
        })
        .await
        .unwrap();
    let mut events = Vec::new();
    collect_turn(&handle, &mut events).await;
    assert!(
        events
            .iter()
            .any(|event| matches!(event, AgentEvent::SteeringAccepted { id: 7 }))
    );
    assert!(
        requests.lock().unwrap()[0]["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| {
                message["role"] == "user" && message["content"] == "Late direction."
            })
    );
    handle.ops.send(Op::Shutdown).await.unwrap();
    while tokio::time::timeout(Duration::from_secs(5), handle.events.recv())
        .await
        .unwrap()
        .is_ok()
    {}
}
