use super::*;
use pretty_assertions::{assert_eq, assert_ne};

fn prepared_surface() -> serde_json::Value {
    json!({"surface":{
        "surface_id":"task","catalog_id":"dev.hooter.safe.v1","data":{},
        "components":[
            {"id":"root","component":"Column","children":["title","open"]},
            {"id":"title","component":"PlainText","text":"Model-produced task preview"},
            {"id":"open","component":"Button","child":"label","action":{"name":"open_task"}},
            {"id":"label","component":"PlainText","text":"Open native controls"}
        ]
    }})
}

#[tokio::test]
async fn native_surface_capability_gates_both_advertisement_and_execution() {
    for enabled in [false, true] {
        let (_root, workspace) = temp_workspace();
        let (url, requests) = mock_server(vec![
            sse_tool_call("preview", "render_ui", prepared_surface()),
            sse_text("The task controls are explained above."),
        ])
        .await;
        let handle = crate::spawn_session_with_options(
            test_config(url, workspace.clone()),
            crate::ExecutionOptions {
                allow_subagents: true,
                interactive_ui: enabled,
                improvement_hints: false,
            },
        );
        handle
            .ops
            .send(Op::UserMessage(
                "Explain the task controls with a visual preview, without changing files.".into(),
            ))
            .await
            .unwrap();
        let mut events = Vec::new();
        collect_turn(&handle, &mut events).await;
        let (success, output) = events
            .iter()
            .find_map(|event| match event {
                AgentEvent::ToolCallFinished {
                    call_id,
                    success,
                    output,
                    ..
                } if call_id == "preview" => Some((*success, output)),
                _ => None,
            })
            .expect("surface tool result");
        assert_eq!(success, enabled);
        if enabled {
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(output).unwrap(),
                prepared_surface()
            );
        } else {
            assert!(output.contains("unavailable"), "{output}");
        }
        for request in requests.lock().unwrap().iter() {
            let tools = request["tools"].as_array().unwrap();
            assert_eq!(
                tools
                    .iter()
                    .any(|tool| tool["function"]["name"] == "render_ui"),
                enabled
            );
            assert!(
                tools
                    .iter()
                    .any(|tool| tool["function"]["name"] == "spawn_agent"),
                "Disabling GenUI must not disable unrelated delegation."
            );
        }
        assert!(!workspace.join("result.txt").exists());
        assert_ne!(
            assessed(&events).assessment.as_ref().unwrap().outcome,
            TaskOutcome::Accepted
        );
        handle.ops.send(Op::Shutdown).await.unwrap();
        while let Ok(event) = handle.events.recv().await {
            if matches!(event, AgentEvent::SessionStopped { .. }) {
                break;
            }
        }
    }
}
