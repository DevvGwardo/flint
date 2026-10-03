//! Droid's native ACP options and saved session identity, without a live CLI.

use flint_agent::{AgentEvent, ApprovalDecision, ApprovalMode, ImageAttachment, Op, TurnEndReason};
use pretty_assertions::assert_eq;
use serde_json::{Value, json};

use crate::AcpAgent;
use crate::test_support::start_agent;

fn options(model: &str, effort: &str) -> Value {
    json!([
        {"id": "autonomy_level", "name": "Autonomy", "category": "mode", "type": "select",
         "currentValue": "normal", "options": [
            {"value": "normal", "name": "Normal"}, {"value": "spec", "name": "Spec"},
            {"value": "auto-low", "name": "Auto (Low)"}, {"value": "auto-medium", "name": "Auto (Medium)"},
            {"value": "auto-high", "name": "Auto (High)"}]},
        {"id": "model", "name": "Model", "category": "model", "type": "select",
         "currentValue": model, "options": [
            {"value": "gpt-6-sol", "name": "GPT-6 Sol"}, {"value": "claude-sonnet-4-6", "name": "Sonnet"}]},
        {"id": "reasoning_effort", "name": "Reasoning", "category": "thought_level", "type": "select",
         "currentValue": effort, "options": [
            {"value": "low", "name": "Low"}, {"value": "high", "name": "High"}]}
    ])
}

#[tokio::test]
async fn image_prompt_uses_acp_image_content_block() {
    let (harness, mut fake) = start_agent(AcpAgent::Droid, ApprovalMode::Auto, None);
    fake.handshake_with_options(options("gpt-6-sol", "high"))
        .await;
    harness
        .until(|event| matches!(event, AgentEvent::SessionOptions(_)))
        .await;
    harness
        .send(Op::UserMessageWithImages {
            text: "Inspect this".into(),
            images: vec![ImageAttachment {
                name: "screen.png".into(),
                mime_type: "image/png".into(),
                data: "aGVsbG8=".into(),
            }],
        })
        .await;
    let prompt = fake.expect("session/prompt").await;
    assert_eq!(
        prompt["params"]["prompt"],
        json!([
            {"type": "text", "text": "Inspect this"},
            {"type": "image", "data": "aGVsbG8=", "mimeType": "image/png"}
        ])
    );
    fake.respond(&prompt, json!({"stopReason": "end_turn"}))
        .await;
    harness.next_turn().await;
    harness.send(Op::Shutdown).await;
}

#[tokio::test]
async fn droid_options_and_permissions_round_trip_and_choices_are_saved() {
    let (harness, mut fake) = start_agent(AcpAgent::Droid, ApprovalMode::Auto, Some("session"));
    fake.handshake_with_options(options("gpt-6-sol", "high"))
        .await;
    let first = harness
        .until(|event| matches!(event, AgentEvent::SessionOptions(_)))
        .await;
    if let AgentEvent::SessionOptions(list) = first {
        assert_eq!(list[0].id, "autonomy_level");
        assert_eq!(list[1].current, "gpt-6-sol");
        assert_eq!(list[2].category.as_deref(), Some("thought_level"));
    }
    for (id, value, raw) in [
        (
            "model",
            "claude-sonnet-4-6",
            options("claude-sonnet-4-6", "high"),
        ),
        (
            "reasoning_effort",
            "low",
            options("claude-sonnet-4-6", "low"),
        ),
    ] {
        harness
            .send(Op::SetSessionOption {
                id: id.into(),
                value: value.into(),
            })
            .await;
        let request = fake.expect("session/set_config_option").await;
        assert_eq!(
            request["params"],
            json!({"sessionId": "s1", "configId": id, "value": value})
        );
        fake.respond(&request, json!({"configOptions": raw})).await;
        harness
            .until(|event| matches!(event, AgentEvent::SessionOptions(_)))
            .await;
    }
    harness
        .send(Op::UserMessage("Inspect the project.".into()))
        .await;
    let prompt = fake.expect("session/prompt").await;
    // Droid's own mode, not Flint's Auto default, decides when to ask.
    fake.permission("permission", "command", "Run tests").await;
    harness
        .until(|event| matches!(event, AgentEvent::ApprovalRequested { .. }))
        .await;
    harness
        .send(Op::Approval {
            call_id: "command".into(),
            decision: ApprovalDecision::Deny,
        })
        .await;
    assert_eq!(fake.recv().await["result"]["outcome"]["optionId"], "no");
    fake.update(json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "No command ran."}})).await;
    fake.respond(&prompt, json!({"stopReason": "end_turn"}))
        .await;
    let turn = harness.next_turn().await;
    assert_eq!(
        turn.last(),
        Some(&AgentEvent::TurnFinished {
            turn_id: 1,
            reason: TurnEndReason::Completed
        })
    );
    let saved = crate::saved::load_saved(&harness.workspace.join("session")).expect("saved");
    assert_eq!(saved.agent, "droid");
    assert_eq!(saved.session_id, "s1");
    assert_eq!(saved.options["model"], "claude-sonnet-4-6");
    assert_eq!(saved.options["reasoning_effort"], "low");
    harness.send(Op::Shutdown).await;
}

#[tokio::test]
async fn saved_droid_sessions_reopen_as_droid_and_keep_their_model() {
    let (harness, mut fake) = start_agent(
        AcpAgent::Droid,
        ApprovalMode::AskForChanges,
        Some("session"),
    );
    let dir = harness.workspace.join("session");
    std::fs::create_dir_all(&dir).expect("directory");
    std::fs::write(
        dir.join("acp.json"),
        json!({
            "agent": "droid", "session_id": "saved-droid", "turn_id": 2,
            "options": {"model": "claude-sonnet-4-6"}
        })
        .to_string(),
    )
    .expect("saved session");
    let init = fake.expect("initialize").await;
    fake.respond(
        &init,
        json!({"protocolVersion": 1, "agentCapabilities": {"loadSession": true}}),
    )
    .await;
    let load = fake.expect("session/load").await;
    assert_eq!(load["params"]["sessionId"], "saved-droid");
    fake.respond(
        &load,
        json!({"configOptions": options("claude-sonnet-4-6", "high")}),
    )
    .await;
    harness
        .until(|event| matches!(event, AgentEvent::SessionOptions(_)))
        .await;
    harness.send(Op::UserMessage("Continue.".into())).await;
    let prompt = fake.recv().await;
    assert_eq!(prompt["method"], "session/prompt");
    assert_eq!(prompt["params"]["sessionId"], "saved-droid");
    fake.respond(&prompt, json!({"stopReason": "end_turn"}))
        .await;
    let turn = harness.next_turn().await;
    assert_eq!(
        turn.last(),
        Some(&AgentEvent::TurnFinished {
            turn_id: 3,
            reason: TurnEndReason::Completed
        })
    );
    harness.send(Op::Shutdown).await;
}
