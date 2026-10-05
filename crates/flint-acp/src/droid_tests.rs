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
async fn preferred_model_is_confirmed_before_a_queued_first_prompt() {
    let (harness, mut fake) = crate::test_support::start_agent_with_preferences(
        AcpAgent::Droid,
        ApprovalMode::Auto,
        Some("session"),
        [("model".into(), "claude-sonnet-4-6".into())].into(),
    );
    harness
        .send(Op::UserMessage("queued before initialization".into()))
        .await;
    fake.handshake_with_options(options("gpt-6-sol", "high"))
        .await;
    let change = fake.expect("session/set_config_option").await;
    assert_eq!(change["params"]["value"], "claude-sonnet-4-6");
    fake.respond(&change, json!({})).await;
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(50), fake.recv())
            .await
            .is_err(),
        "an acknowledgement alone must not release the first prompt"
    );
    fake.update(json!({"sessionUpdate": "config_option_update",
        "configOptions": options("claude-sonnet-4-6", "high")}))
        .await;
    let prompt = fake.expect("session/prompt").await;
    fake.respond(&prompt, json!({"stopReason": "end_turn"}))
        .await;
    harness.next_turn().await;
    let saved = crate::saved::load_saved(&harness.workspace.join("session")).unwrap();
    assert_eq!(saved.options["model"], "claude-sonnet-4-6");
    harness.send(Op::Shutdown).await;
}

#[tokio::test]
async fn unavailable_preferred_model_blocks_prompts_until_the_user_replaces_it() {
    let (harness, mut fake) = crate::test_support::start_agent_with_preferences(
        AcpAgent::Droid,
        ApprovalMode::Auto,
        None,
        [("model".into(), "removed-model".into())].into(),
    );
    harness
        .send(Op::UserMessage("must not run with the default".into()))
        .await;
    fake.handshake_with_options(options("gpt-6-sol", "high"))
        .await;
    let error = harness
        .until(|event| matches!(event, AgentEvent::Error(_)))
        .await;
    assert!(matches!(error, AgentEvent::Error(message) if message.contains("no longer offers")));
    let error = harness
        .until(|event| matches!(event, AgentEvent::Error(_)))
        .await;
    assert!(matches!(error, AgentEvent::Error(message) if message.contains("No prompt was sent")));
    harness
        .send(Op::SetSessionOption {
            id: "model".into(),
            value: "claude-sonnet-4-6".into(),
        })
        .await;
    let change = fake.expect("session/set_config_option").await;
    fake.respond(
        &change,
        json!({"configOptions": options("claude-sonnet-4-6", "high")}),
    )
    .await;
    harness
        .send(Op::UserMessage("retry with selected model".into()))
        .await;
    let prompt = fake.expect("session/prompt").await;
    assert_eq!(
        prompt["params"]["prompt"][0]["text"],
        "retry with selected model"
    );
    fake.respond(&prompt, json!({"stopReason": "end_turn"}))
        .await;
    harness.next_turn().await;
    harness.send(Op::Shutdown).await;
}

#[tokio::test]
async fn preferences_apply_provider_before_its_model_list() {
    let provider_options = |provider: &str, model: &str| {
        json!([
            {"id": "provider", "name": "Provider", "type": "select", "currentValue": provider,
             "options": [{"value": "a", "name": "A"}, {"value": "b", "name": "B"}]},
            {"id": "model", "name": "Model", "category": "model", "type": "select", "currentValue": model,
             "options": [{"value": if provider == "a" { "a-model" } else { "b-model" }, "name": "Model"}]}
        ])
    };
    let (harness, mut fake) = crate::test_support::start_agent_with_preferences(
        AcpAgent::Droid,
        ApprovalMode::Auto,
        None,
        [
            ("model".into(), "b-model".into()),
            ("provider".into(), "b".into()),
        ]
        .into(),
    );
    harness.send(Op::UserMessage("go".into())).await;
    fake.handshake_with_options(provider_options("a", "a-model"))
        .await;
    let change = fake.expect("session/set_config_option").await;
    assert_eq!(change["params"]["configId"], "provider");
    fake.respond(
        &change,
        json!({"configOptions": provider_options("b", "b-model")}),
    )
    .await;
    let prompt = fake.expect("session/prompt").await;
    fake.respond(&prompt, json!({"stopReason": "end_turn"}))
        .await;
    harness.next_turn().await;
    harness.send(Op::Shutdown).await;
}

#[tokio::test(start_paused = true)]
async fn unconfirmed_preferred_model_times_out_without_sending_a_prompt() {
    let (harness, mut fake) = crate::test_support::start_agent_with_preferences(
        AcpAgent::Droid,
        ApprovalMode::Auto,
        Some("session"),
        [("model".into(), "claude-sonnet-4-6".into())].into(),
    );
    harness
        .send(Op::UserMessage("must wait for the model".into()))
        .await;
    fake.handshake_with_options(options("gpt-6-sol", "high"))
        .await;
    let change = fake.expect("session/set_config_option").await;
    fake.respond(&change, json!({})).await;
    // The saved accepted choice proves the RPC reply was handled before
    // advancing the confirmation deadline.
    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        loop {
            if crate::saved::load_saved(&harness.workspace.join("session")).is_some_and(|saved| {
                saved
                    .options
                    .get("model")
                    .is_some_and(|model| model == "claude-sonnet-4-6")
            }) {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    tokio::time::advance(std::time::Duration::from_secs(31)).await;
    let error = harness
        .until(|event| matches!(event, AgentEvent::Error(_)))
        .await;
    assert!(matches!(error, AgentEvent::Error(message) if message.contains("No prompt was sent")));
    harness.send(Op::Shutdown).await;
    fake.closed().await;
}

#[tokio::test]
async fn reopening_a_conversation_does_not_apply_another_sessions_model_default() {
    let (harness, mut fake) = crate::test_support::start_agent_with_preferences(
        AcpAgent::Droid,
        ApprovalMode::Auto,
        Some("session"),
        [("model".into(), "claude-sonnet-4-6".into())].into(),
    );
    crate::saved::save_saved(
        &harness.workspace.join("session"),
        &crate::saved::Saved {
            agent: "droid".into(),
            session_id: "saved-id".into(),
            options: [("model".into(), "gpt-6-sol".into())].into(),
            ..Default::default()
        },
    );
    let init = fake.expect("initialize").await;
    fake.respond(
        &init,
        json!({"protocolVersion": 1, "agentCapabilities": {"loadSession": true}}),
    )
    .await;
    let load = fake.expect("session/load").await;
    fake.respond(
        &load,
        json!({"configOptions": options("gpt-6-sol", "high")}),
    )
    .await;
    harness
        .send(Op::UserMessage("continue existing conversation".into()))
        .await;
    let prompt = fake.recv().await;
    assert_eq!(prompt["method"], "session/prompt");
    fake.respond(&prompt, json!({"stopReason": "end_turn"}))
        .await;
    harness.next_turn().await;
    harness.send(Op::Shutdown).await;
}

#[tokio::test]
async fn a_reopened_adapter_default_cannot_replace_the_saved_model() {
    let (harness, mut fake) = start_agent(AcpAgent::Droid, ApprovalMode::Auto, Some("session"));
    crate::saved::save_saved(
        &harness.workspace.join("session"),
        &crate::saved::Saved {
            agent: "droid".into(),
            session_id: "saved-id".into(),
            options: [("model".into(), "claude-sonnet-4-6".into())].into(),
            ..Default::default()
        },
    );
    harness
        .send(Op::UserMessage("continue with saved model".into()))
        .await;
    let init = fake.expect("initialize").await;
    fake.respond(
        &init,
        json!({"protocolVersion": 1, "agentCapabilities": {"loadSession": true}}),
    )
    .await;
    let load = fake.expect("session/load").await;
    fake.respond(
        &load,
        json!({"configOptions": options("gpt-6-sol", "high")}),
    )
    .await;
    let change = fake.recv().await;
    assert_eq!(change["method"], "session/set_config_option");
    assert_eq!(change["params"]["sessionId"], "saved-id");
    assert_eq!(change["params"]["value"], "claude-sonnet-4-6");
    fake.respond(
        &change,
        json!({"configOptions": options("claude-sonnet-4-6", "high")}),
    )
    .await;
    let prompt = fake.recv().await;
    assert_eq!(prompt["method"], "session/prompt");
    fake.respond(&prompt, json!({"stopReason": "end_turn"}))
        .await;
    harness.next_turn().await;
    harness.send(Op::Shutdown).await;
}

#[tokio::test]
async fn a_provider_change_updates_the_saved_conversations_model_pair() {
    let with_provider = |model: &str, provider: &str| {
        let mut raw = options(model, "high");
        raw.as_array_mut().unwrap().push(json!({
            "id": "provider", "name": "Provider", "type": "select", "currentValue": provider,
            "options": [{"value": "a", "name": "A"}, {"value": "b", "name": "B"}]
        }));
        raw
    };
    let (harness, mut fake) = start_agent(AcpAgent::Droid, ApprovalMode::Auto, Some("session"));
    fake.handshake_with_options(with_provider("gpt-6-sol", "a"))
        .await;
    harness
        .send(Op::SetSessionOption {
            id: "provider".into(),
            value: "b".into(),
        })
        .await;
    let change = fake.expect("session/set_config_option").await;
    fake.respond(
        &change,
        json!({"configOptions": with_provider("claude-sonnet-4-6", "b")}),
    )
    .await;
    harness
        .send(Op::UserMessage("use selected provider".into()))
        .await;
    let prompt = fake.expect("session/prompt").await;
    let saved = crate::saved::load_saved(&harness.workspace.join("session")).unwrap();
    assert_eq!(saved.options["provider"], "b");
    assert_eq!(saved.options["model"], "claude-sonnet-4-6");
    fake.respond(&prompt, json!({"stopReason": "end_turn"}))
        .await;
    harness.next_turn().await;
    harness.send(Op::Shutdown).await;
}

#[tokio::test]
async fn fixing_one_identity_choice_does_not_unblock_an_invalid_provider() {
    let with_provider = |model: &str| {
        let mut raw = options(model, "high");
        raw.as_array_mut().unwrap().push(json!({
            "id": "provider", "name": "Provider", "type": "select", "currentValue": "a",
            "options": [{"value": "a", "name": "A"}]
        }));
        raw
    };
    let (harness, mut fake) = crate::test_support::start_agent_with_preferences(
        AcpAgent::Droid,
        ApprovalMode::Auto,
        None,
        [
            ("model".into(), "gpt-6-sol".into()),
            ("provider".into(), "removed-provider".into()),
        ]
        .into(),
    );
    fake.handshake_with_options(with_provider("gpt-6-sol"))
        .await;
    harness
        .until(|event| matches!(event, AgentEvent::Error(_)))
        .await;
    harness
        .send(Op::SetSessionOption {
            id: "model".into(),
            value: "claude-sonnet-4-6".into(),
        })
        .await;
    let change = fake.expect("session/set_config_option").await;
    fake.respond(
        &change,
        json!({"configOptions": with_provider("claude-sonnet-4-6")}),
    )
    .await;
    let error = harness
        .until(|event| matches!(event, AgentEvent::Error(_)))
        .await;
    assert!(matches!(error, AgentEvent::Error(message) if message.contains("removed-provider")));
    harness
        .send(Op::UserMessage("must still be blocked".into()))
        .await;
    let error = harness
        .until(|event| matches!(event, AgentEvent::Error(_)))
        .await;
    assert!(matches!(error, AgentEvent::Error(message) if message.contains("No prompt was sent")));
    harness
        .send(Op::SetSessionOption {
            id: "provider".into(),
            value: "a".into(),
        })
        .await;
    let change = fake.expect("session/set_config_option").await;
    fake.respond(
        &change,
        json!({"configOptions": with_provider("claude-sonnet-4-6")}),
    )
    .await;
    harness
        .send(Op::UserMessage("all choices repaired".into()))
        .await;
    let prompt = fake.recv().await;
    assert_eq!(prompt["method"], "session/prompt");
    assert_eq!(
        prompt["params"]["prompt"][0]["text"],
        "all choices repaired"
    );
    fake.respond(&prompt, json!({"stopReason": "end_turn"}))
        .await;
    harness.next_turn().await;
    harness.send(Op::Shutdown).await;
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
async fn empty_setting_replies_keep_options_and_permissions_until_droid_updates_them() {
    for update_first in [false, true] {
        let (harness, mut fake) = start_agent(AcpAgent::Droid, ApprovalMode::Auto, Some("session"));
        fake.handshake_with_options(options("gpt-6-sol", "high"))
            .await;
        harness
            .until(|event| matches!(event, AgentEvent::SessionOptions(_)))
            .await;
        for (id, value) in [
            ("model", "claude-sonnet-4-6"),
            ("reasoning_effort", "low"),
            ("autonomy_level", "auto-high"),
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
            let mut raw = options(
                "claude-sonnet-4-6",
                if id == "model" { "high" } else { "low" },
            );
            if id == "autonomy_level" {
                raw[0]["currentValue"] = json!("auto-high");
            }
            let update = json!({"sessionUpdate": "config_option_update", "configOptions": raw});
            if update_first {
                fake.update(update.clone()).await;
            }
            // Droid acknowledges with {}, rather than returning configOptions.
            fake.respond(&request, json!({})).await;
            if !update_first {
                fake.update(update).await;
            }
        }
        harness.send(Op::UserMessage("Inspect.".into())).await;
        let prompt = fake.expect("session/prompt").await;
        fake.permission("permission", "command", "Run tests").await;
        let mut events = Vec::new();
        loop {
            let event = harness.next_event().await;
            let asked = matches!(event, AgentEvent::ApprovalRequested { .. });
            events.push(event);
            if asked {
                break;
            }
        }
        harness
            .send(Op::Approval {
                call_id: "command".into(),
                decision: ApprovalDecision::Deny,
            })
            .await;
        assert_eq!(
            fake.answered("permission").await["result"]["outcome"]["optionId"],
            "no"
        );
        fake.respond(&prompt, json!({"stopReason": "end_turn"}))
            .await;
        events.extend(harness.next_turn().await);
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, AgentEvent::Error(_)))
        );
        let list = events
            .iter()
            .filter_map(|event| match event {
                AgentEvent::SessionOptions(list) => Some(list),
                _ => None,
            })
            .next_back()
            .expect("updated options");
        assert_eq!(list[0].current, "auto-high");
        assert_eq!(list[1].current, "claude-sonnet-4-6");
        assert_eq!(list[2].current, "low");
        assert!(
            events
                .iter()
                .all(|event| !matches!(event, AgentEvent::SessionOptions(list) if list.is_empty()))
        );
        let saved = crate::saved::load_saved(&harness.workspace.join("session")).expect("saved");
        assert_eq!(saved.options["model"], "claude-sonnet-4-6");
        assert_eq!(saved.options["reasoning_effort"], "low");
        assert_eq!(saved.options["autonomy_level"], "auto-high");
        harness.send(Op::Shutdown).await;
    }
}

#[tokio::test]
async fn empty_setting_reply_without_an_update_preserves_the_last_known_options() {
    let (harness, mut fake) = start_agent(AcpAgent::Droid, ApprovalMode::Auto, Some("session"));
    fake.handshake_with_options(options("gpt-6-sol", "high"))
        .await;
    harness
        .until(|event| matches!(event, AgentEvent::SessionOptions(_)))
        .await;
    for (id, value) in [("model", "claude-sonnet-4-6"), ("reasoning_effort", "low")] {
        harness
            .send(Op::SetSessionOption {
                id: id.into(),
                value: value.into(),
            })
            .await;
        let request = fake.expect("session/set_config_option").await;
        fake.respond(&request, json!({})).await;
    }
    harness.send(Op::UserMessage("Inspect.".into())).await;
    let prompt = fake.expect("session/prompt").await;
    fake.respond(&prompt, json!({"stopReason": "end_turn"}))
        .await;
    let events = harness.next_turn().await;
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, AgentEvent::Error(_) | AgentEvent::SessionOptions(_)))
    );
    let saved = crate::saved::load_saved(&harness.workspace.join("session")).expect("saved");
    assert_eq!(saved.options["model"], "claude-sonnet-4-6");
    assert_eq!(saved.options["reasoning_effort"], "low");
    harness.send(Op::Shutdown).await;
}

#[tokio::test]
async fn rejected_droid_setting_is_not_saved_and_does_not_prevent_later_changes() {
    let (harness, mut fake) = start_agent(AcpAgent::Droid, ApprovalMode::Auto, Some("session"));
    fake.handshake_with_options(options("gpt-6-sol", "high"))
        .await;
    harness
        .until(|event| matches!(event, AgentEvent::SessionOptions(_)))
        .await;
    harness
        .send(Op::SetSessionOption {
            id: "autonomy_level".into(),
            value: "auto-high".into(),
        })
        .await;
    let request = fake.expect("session/set_config_option").await;
    fake.send(json!({"jsonrpc": "2.0", "id": request["id"],
        "error": {"code": -32602, "message": "Autonomy level not allowed by organization policy"}}))
        .await;
    assert_eq!(
        harness
            .until(|event| matches!(event, AgentEvent::Error(_)))
            .await,
        AgentEvent::Error(
            "Droid didn't change autonomy_level: Autonomy level not allowed by organization policy"
                .into()
        )
    );
    let saved = crate::saved::load_saved(&harness.workspace.join("session")).expect("saved");
    assert!(!saved.options.contains_key("autonomy_level"));
    harness
        .send(Op::SetSessionOption {
            id: "reasoning_effort".into(),
            value: "low".into(),
        })
        .await;
    let request = fake.expect("session/set_config_option").await;
    fake.respond(
        &request,
        json!({"configOptions": options("gpt-6-sol", "low")}),
    )
    .await;
    let event = harness
        .until(|event| matches!(event, AgentEvent::SessionOptions(_)))
        .await;
    assert!(
        matches!(event, AgentEvent::SessionOptions(list) if list[0].current == "normal" && list[2].current == "low")
    );
    harness.send(Op::Shutdown).await;
}

#[tokio::test]
async fn native_undo_is_rejected_without_stopping_the_droid_session() {
    let (harness, mut fake) = start_agent(AcpAgent::Droid, ApprovalMode::Auto, None);
    fake.handshake_with_options(options("gpt-6-sol", "high"))
        .await;
    harness
        .until(|event| matches!(event, AgentEvent::SessionOptions(_)))
        .await;
    let error =
        AgentEvent::Error("Undo is only available for Flint's own agent, not Droid.".into());
    harness.send(Op::UndoLastTurn).await;
    assert_eq!(
        harness
            .until(|event| matches!(event, AgentEvent::Error(_)))
            .await,
        error
    );
    harness.send(Op::UserMessage("Inspect.".into())).await;
    let prompt = fake.expect("session/prompt").await;
    harness.send(Op::UndoLastTurn).await;
    assert_eq!(
        harness
            .until(|event| matches!(event, AgentEvent::Error(_)))
            .await,
        error
    );
    fake.respond(&prompt, json!({"stopReason": "end_turn"}))
        .await;
    assert!(matches!(
        harness.next_turn().await.last(),
        Some(AgentEvent::TurnFinished {
            reason: TurnEndReason::Completed,
            ..
        })
    ));
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
