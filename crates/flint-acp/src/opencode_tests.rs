//! OpenCode's native ACP lifecycle, without provider access or a live CLI.

use flint_agent::{AgentEvent, ApprovalMode, Op, TurnEndReason};
use pretty_assertions::assert_eq;
use serde_json::{Value, json};

use crate::AcpAgent;
use crate::saved::load_saved;
use crate::test_support::{start_agent, start_agent_with_saved};

fn options(model: &str) -> Value {
    json!([
        {"id": "model", "name": "Model", "category": "model", "type": "select",
         "currentValue": model, "options": [
            {"value": "provider/model-a", "name": "Model A"},
            {"value": "provider/model-b", "name": "Model B"}]},
        {"id": "mode", "name": "Agent", "category": "mode", "type": "select",
         "currentValue": "build", "options": [
            {"value": "build", "name": "Build"}, {"value": "plan", "name": "Plan"}]}
    ])
}

#[tokio::test]
async fn opencode_streams_updates_changes_models_and_reopens_its_saved_session() {
    let (harness, mut fake) = start_agent(AcpAgent::OpenCode, ApprovalMode::Auto, Some("session"));
    let new = fake
        .handshake_with_options(options("provider/model-a"))
        .await;
    assert_eq!(new["params"]["cwd"], harness.workspace.to_str().unwrap());
    harness
        .until(|event| matches!(event, AgentEvent::SessionOptions(_)))
        .await;
    harness
        .send(Op::SetSessionOption {
            id: "model".into(),
            value: "provider/model-b".into(),
        })
        .await;
    let change = fake.expect("session/set_config_option").await;
    assert_eq!(change["params"]["configId"], "model");
    assert_eq!(change["params"]["value"], "provider/model-b");
    fake.respond(
        &change,
        json!({"configOptions": options("provider/model-b")}),
    )
    .await;
    harness
        .until(|event| {
            matches!(event, AgentEvent::SessionOptions(list)
        if list.iter().any(|option| option.id == "model" && option.current == "provider/model-b"))
        })
        .await;
    harness.send(Op::UserMessage("OpenCode task".into())).await;
    let prompt = fake.expect("session/prompt").await;
    assert_eq!(prompt["params"]["sessionId"], "s1");
    assert_eq!(prompt["params"]["prompt"][0]["text"], "OpenCode task");
    fake.update(json!({"sessionUpdate": "agent_message_chunk",
        "content": {"type": "text", "text": "OpenCode answer."}}))
        .await;
    fake.respond(&prompt, json!({"stopReason": "end_turn"}))
        .await;
    let events = harness.next_turn().await;
    assert!(
        events
            .iter()
            .any(|event| matches!(event, AgentEvent::TextDelta(text)
        if text == "OpenCode answer."))
    );
    assert!(events.iter().any(|event| matches!(
        event,
        AgentEvent::TurnFinished {
            turn_id: 1,
            reason: TurnEndReason::Completed
        }
    )));
    harness.send(Op::Shutdown).await;
    fake.closed().await;
    let saved = load_saved(&harness.workspace.join("session")).unwrap();
    assert_eq!(saved.agent, "opencode");
    assert_eq!(saved.session_id, "s1");
    assert_eq!(saved.turn_id, 1);
    assert_eq!(saved.options["model"], "provider/model-b");

    let (reopened, mut fake) =
        start_agent_with_saved(AcpAgent::OpenCode, ApprovalMode::Auto, saved);
    let init = fake.expect("initialize").await;
    fake.respond(
        &init,
        json!({"protocolVersion": 1, "agentCapabilities": {"loadSession": true}}),
    )
    .await;
    let load = fake.expect("session/load").await;
    assert_eq!(load["params"]["sessionId"], "s1");
    assert_eq!(load["params"]["cwd"], reopened.workspace.to_str().unwrap());
    fake.respond(&load, json!({"configOptions": options("provider/model-b")}))
        .await;
    reopened.send(Op::UserMessage("Continue".into())).await;
    let prompt = fake.expect("session/prompt").await;
    fake.respond(&prompt, json!({"stopReason": "end_turn"}))
        .await;
    assert!(reopened.next_turn().await.iter().any(|event| matches!(
        event,
        AgentEvent::TurnFinished {
            turn_id: 2,
            reason: TurnEndReason::Completed
        }
    )));
    reopened.send(Op::Shutdown).await;
    fake.closed().await;
}
