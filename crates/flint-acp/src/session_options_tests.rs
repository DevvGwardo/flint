//! Agent session options (`configOptions`) against the fake agent.

use flint_agent::AgentEvent;
use flint_agent::ApprovalMode;
use flint_agent::Op;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;

use crate::test_support::start;
use crate::test_support::start_with;

fn claude_options(mode: &str, model: &str, fast: bool) -> Value {
    json!([
        {"id": "mode", "name": "Mode", "category": "mode", "type": "select", "currentValue": mode,
         "options": [{"value": "default", "name": "Manual"}, {"value": "acceptEdits", "name": "Accept Edits"},
                     {"value": "plan", "name": "Plan", "description": "Read-only planning"}]},
        {"id": "model", "name": "Model", "category": "model", "type": "select", "currentValue": model,
         "options": [{"value": "default", "name": "Default"}, {"value": "opus", "name": "Opus"}]},
        {"id": "fast", "name": "Fast", "category": "model_config", "type": "boolean", "currentValue": fast}
    ])
}

fn summary(event: &AgentEvent) -> Vec<(String, String)> {
    match event {
        AgentEvent::SessionOptions(list) => list
            .iter()
            .map(|o| (o.id.clone(), o.current.clone()))
            .collect(),
        other => panic!("expected options, got {other:?}"),
    }
}

fn pairs(items: &[(&str, &str)]) -> Vec<(String, String)> {
    items
        .iter()
        .map(|(a, b)| ((*a).to_string(), (*b).to_string()))
        .collect()
}

#[tokio::test]
async fn shutdown_interrupts_an_unanswered_option_change() {
    let (harness, mut fake) = start(ApprovalMode::Auto);
    fake.handshake_with_options(claude_options("default", "default", false))
        .await;
    harness
        .until(|e| matches!(e, AgentEvent::SessionOptions(_)))
        .await;
    harness
        .send(Op::SetSessionOption {
            id: "mode".into(),
            value: "plan".into(),
        })
        .await;
    fake.expect("session/set_config_option").await;
    harness.send(Op::Shutdown).await;
    fake.closed().await;
}

#[tokio::test(start_paused = true)]
async fn interrupt_is_not_blocked_by_a_setting_rpc_during_a_turn() {
    let (harness, mut fake) = start(ApprovalMode::Auto);
    fake.handshake_with_options(claude_options("default", "default", false))
        .await;
    harness
        .until(|e| matches!(e, AgentEvent::SessionOptions(_)))
        .await;
    harness.send(Op::UserMessage("wait".into())).await;
    fake.expect("session/prompt").await;
    harness
        .send(Op::SetSessionOption {
            id: "mode".into(),
            value: "plan".into(),
        })
        .await;
    fake.expect("session/set_config_option").await;
    harness.send(Op::Interrupt).await;
    fake.expect("session/cancel").await;
    let events = harness.next_turn().await;
    assert!(matches!(
        events.last(),
        Some(AgentEvent::TurnFinished {
            reason: flint_agent::TurnEndReason::Interrupted,
            ..
        })
    ));
    fake.closed().await;
}

#[tokio::test]
async fn options_are_emitted_and_set_round_trips() {
    let (harness, mut fake) = start(ApprovalMode::AskForChanges);
    fake.handshake_with_options(claude_options("default", "default", false))
        .await;
    let first = harness
        .until(|e| matches!(e, AgentEvent::SessionOptions(_)))
        .await;
    assert_eq!(
        summary(&first),
        pairs(&[("mode", "default"), ("model", "default"), ("fast", "false")])
    );
    if let AgentEvent::SessionOptions(list) = &first {
        assert_eq!(
            list[0].choices[2].description.as_deref(),
            Some("Read-only planning")
        );
    }

    harness
        .send(Op::SetSessionOption {
            id: "mode".into(),
            value: "plan".into(),
        })
        .await;
    let request = fake.expect("session/set_config_option").await;
    assert_eq!(
        request["params"],
        json!({"sessionId": "s1", "configId": "mode", "value": "plan"})
    );
    fake.respond(
        &request,
        json!({"configOptions": claude_options("plan", "default", false)}),
    )
    .await;
    let after = harness
        .until(|e| matches!(e, AgentEvent::SessionOptions(_)))
        .await;
    assert_eq!(
        summary(&after),
        pairs(&[("mode", "plan"), ("model", "default"), ("fast", "false")])
    );

    // Booleans go back to the agent as booleans.
    harness
        .send(Op::SetSessionOption {
            id: "fast".into(),
            value: "true".into(),
        })
        .await;
    let request = fake.expect("session/set_config_option").await;
    assert_eq!(request["params"]["value"], json!(true));
    assert_eq!(request["params"]["type"], json!("boolean"));
    fake.respond(
        &request,
        json!({"configOptions": claude_options("plan", "default", true)}),
    )
    .await;
    harness
        .until(|e| matches!(e, AgentEvent::SessionOptions(_)))
        .await;

    // A value the agent didn't offer is refused locally.
    harness
        .send(Op::SetSessionOption {
            id: "model".into(),
            value: "gpt".into(),
        })
        .await;
    let error = harness.until(|e| matches!(e, AgentEvent::Error(_))).await;
    assert_eq!(
        error,
        AgentEvent::Error("Claude Code has no setting model = gpt.".into())
    );
}

#[tokio::test]
async fn config_option_update_notifications_are_forwarded() {
    let (harness, mut fake) = start(ApprovalMode::AskForChanges);
    fake.handshake_with_options(claude_options("default", "default", false))
        .await;
    harness
        .until(|e| matches!(e, AgentEvent::SessionOptions(_)))
        .await;
    fake.update(json!({"sessionUpdate": "config_option_update", "configOptions": claude_options("acceptEdits", "opus", false)}))
        .await;
    let update = harness
        .until(|e| matches!(e, AgentEvent::SessionOptions(_)))
        .await;
    assert_eq!(
        summary(&update),
        pairs(&[
            ("mode", "acceptEdits"),
            ("model", "opus"),
            ("fast", "false")
        ])
    );
}

#[tokio::test]
async fn the_agents_mode_decides_approvals_even_with_auto_run() {
    let (harness, mut fake) = start(ApprovalMode::Auto);
    fake.handshake_with_options(claude_options("default", "default", false))
        .await;
    harness.send(Op::UserMessage("edit".into())).await;
    let prompt = fake.expect("session/prompt").await;
    fake.permission("p1", "t1", "Edit a.rs").await;
    let asked = harness
        .until(|e| matches!(e, AgentEvent::ApprovalRequested { .. }))
        .await;
    assert!(matches!(asked, AgentEvent::ApprovalRequested { call_id, .. } if call_id == "t1"));
    harness
        .send(Op::Approval {
            call_id: "t1".into(),
            decision: flint_agent::ApprovalDecision::Approve,
        })
        .await;
    assert_eq!(fake.recv().await["result"]["outcome"]["optionId"], "yes");
    fake.respond(&prompt, json!({"stopReason": "end_turn"}))
        .await;
    harness.next_turn().await;
}

#[tokio::test]
async fn saved_choices_are_reapplied_when_the_session_cant_be_reopened() {
    let (harness, mut fake) = start_with(ApprovalMode::AskForChanges, Some("session"));
    let dir = harness.workspace.join("session");
    std::fs::create_dir_all(&dir).expect("mkdir");
    std::fs::write(
        dir.join("acp.json"),
        json!({"agent": "claude_code", "session_id": "old", "turn_id": 3,
               "options": {"model": "opus", "mode": "default"}})
        .to_string(),
    )
    .expect("write");
    // No loadSession: a new session, then only the differing choice is set.
    fake.handshake_with_options(claude_options("default", "default", false))
        .await;
    let request = fake.expect("session/set_config_option").await;
    assert_eq!(
        request["params"],
        json!({"sessionId": "s1", "configId": "model", "value": "opus"})
    );
    fake.respond(
        &request,
        json!({"configOptions": claude_options("default", "opus", false)}),
    )
    .await;

    let error = harness.until(|e| matches!(e, AgentEvent::Error(_))).await;
    assert!(
        matches!(&error, AgentEvent::Error(m) if m.contains("can't reopen earlier conversations"))
    );
    let reapplied = harness
        .until(|e| matches!(e, AgentEvent::SessionOptions(list) if list.iter().any(|o| o.current == "opus")))
        .await;
    assert_eq!(
        summary(&reapplied),
        pairs(&[("mode", "default"), ("model", "opus"), ("fast", "false")])
    );

    // The new session id and the choices are saved for next time.
    harness.send(Op::Shutdown).await;
    let saved: Value = loop {
        if let Ok(text) = std::fs::read_to_string(dir.join("acp.json"))
            && let Ok(saved) = serde_json::from_str::<Value>(&text)
            && saved["session_id"] == "s1"
        {
            break saved;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    };
    assert_eq!(
        saved["options"],
        json!({"model": "opus", "mode": "default"})
    );
    assert_eq!(saved["turn_id"], 3);
}

#[tokio::test]
async fn a_reopened_session_keeps_the_agents_own_values() {
    let (harness, mut fake) = start_with(ApprovalMode::AskForChanges, Some("session"));
    let dir = harness.workspace.join("session");
    std::fs::create_dir_all(&dir).expect("mkdir");
    std::fs::write(
        dir.join("acp.json"),
        json!({"agent": "claude_code", "session_id": "old", "turn_id": 1, "options": {"model": "opus"}}).to_string(),
    )
    .expect("write");
    let init = fake.expect("initialize").await;
    fake.respond(
        &init,
        json!({"protocolVersion": 1, "agentCapabilities": {"loadSession": true}}),
    )
    .await;
    let load = fake.expect("session/load").await;
    assert_eq!(load["params"]["sessionId"], "old");
    fake.respond(
        &load,
        json!({"configOptions": claude_options("default", "opus", false)}),
    )
    .await;
    let options = harness
        .until(|e| matches!(e, AgentEvent::SessionOptions(_)))
        .await;
    assert_eq!(
        summary(&options),
        pairs(&[("mode", "default"), ("model", "opus"), ("fast", "false")])
    );
    // Nothing is re-set: the next request the agent sees is the prompt.
    harness.send(Op::UserMessage("hi".into())).await;
    let next = fake.recv().await;
    assert_eq!(next["method"], "session/prompt");
}
