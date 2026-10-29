use super::*;
use serde_json::json;

fn surface() -> SurfaceSnapshot {
    SurfaceSnapshot {
        surface_id: SURFACE_ID.into(),
        catalog_id: CATALOG_ID.into(),
        components: vec![
            Component::Column {
                id: "root".into(),
                children: vec!["title".into(), "open".into()],
            },
            Component::PlainText {
                id: "title".into(),
                text: TextValue::Binding(TextBinding {
                    path: "/title".into(),
                }),
                style: Some(TextStyle::Heading),
            },
            Component::Button {
                id: "open".into(),
                child: "label".into(),
                action: Action {
                    name: ActionName::OpenTask,
                },
            },
            Component::PlainText {
                id: "label".into(),
                text: TextValue::Literal("Open native task controls".into()),
                style: None,
            },
        ],
        data: BTreeMap::from([("title".into(), "Verified automatically".into())]),
    }
}

#[tokio::test]
async fn model_surface_tool_prepares_only_validated_data_without_workspace_effects() {
    let dir = tempfile::tempdir().unwrap();
    let context = crate::tools::ToolContext::new(dir.path().to_owned(), false);
    let cancel = tokio_util::sync::CancellationToken::new();
    let args = json!({"surface": surface()});
    let outcome = crate::tools::execute(
        &context,
        crate::tools::RENDER_UI,
        args.as_object().unwrap(),
        &|_| {},
        &cancel,
    )
    .await;
    assert!(outcome.success, "{}", outcome.output);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&outcome.output).unwrap(),
        args
    );
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    assert_eq!(
        crate::tools::tool_kind(crate::tools::RENDER_UI),
        crate::ToolKind::Read
    );
    let mut invalid = args.clone();
    invalid["surface"]["components"][2]["action"]["name"] = json!("accept_task");
    assert!(
        !crate::tools::execute(
            &context,
            crate::tools::RENDER_UI,
            invalid.as_object().unwrap(),
            &|_| {},
            &cancel
        )
        .await
        .success
    );
    let mut extra = args.clone();
    extra["command"] = json!("execute");
    assert!(
        !crate::tools::execute(
            &context,
            crate::tools::RENDER_UI,
            extra.as_object().unwrap(),
            &|_| {},
            &cancel
        )
        .await
        .success
    );
    cancel.cancel();
    assert!(
        !crate::tools::execute(
            &context,
            crate::tools::RENDER_UI,
            args.as_object().unwrap(),
            &|_| {},
            &cancel
        )
        .await
        .success
    );
}

#[test]
fn repeated_data_bindings_obey_the_total_rendered_text_budget() {
    let mut value = SurfaceSnapshot {
        surface_id: SURFACE_ID.into(),
        catalog_id: CATALOG_ID.into(),
        data: BTreeMap::from([("large".into(), "x".repeat(16 * 1024))]),
        components: vec![Component::Column {
            id: "root".into(),
            children: vec!["a".into(), "b".into()],
        }],
    };
    for (group, start, end) in [("a", 0, 32), ("b", 32, 64)] {
        value.components.push(Component::Column {
            id: group.into(),
            children: (start..end).map(|i| format!("leaf{i}")).collect(),
        });
    }
    for i in 0..64 {
        value.components.push(Component::PlainText {
            id: format!("leaf{i}"),
            text: TextValue::Binding(TextBinding {
                path: "/large".into(),
            }),
            style: None,
        });
    }
    value.validate().unwrap();
    if let Component::Column { children, .. } = &mut value.components[2] {
        children.push("leaf64".into());
    }
    value.components.push(Component::PlainText {
        id: "leaf64".into(),
        text: TextValue::Binding(TextBinding {
            path: "/large".into(),
        }),
        style: None,
    });
    assert!(value.validate().is_err());
}

fn snapshot() -> Envelope {
    Envelope::Snapshot {
        version: 1,
        generation: "fixture-generation".into(),
        sequence: 1,
        task: None,
        surface: surface(),
    }
}

#[test]
fn real_catalog_surface_roundtrips_with_valid_bindings_and_native_action_names() {
    let frame = snapshot();
    assert_eq!(Envelope::decode(&frame.encode().unwrap()).unwrap(), frame);
    assert_eq!(surface().action("open"), Some(ActionName::OpenTask));
    assert_eq!(surface().action("title"), None);
}

#[test]
fn unknown_functions_urls_catalogs_fields_and_privileged_actions_are_rejected() {
    for component in [
        json!({"id":"root","component":"Image","url":"https://example.invalid"}),
        json!({"id":"root","component":"PlainText","text":"unsafe","functionCall":{"name":"openUrl"}}),
        json!({"id":"root","component":"Button","child":"label","action":{"name":"accept"}}),
        json!({"id":"root","component":"Button","child":"label","action":{"name":"stage_prompt","command":"run arbitrary command"}}),
        json!({"id":"root","component":"PlainText","text":{"path":"/title","expression":"execute"}}),
    ] {
        let mut value = serde_json::to_value(snapshot()).unwrap();
        value["surface"]["components"] = json!([component]);
        assert!(Envelope::decode(&serde_json::to_vec(&value).unwrap()).is_err());
    }
    for (field, value) in [
        ("catalog_id", json!("unknown")),
        ("surface_id", json!("arbitrary")),
        ("theme", json!({"url":"unsafe"})),
    ] {
        let mut frame = serde_json::to_value(snapshot()).unwrap();
        frame["surface"][field] = value;
        assert!(Envelope::decode(&serde_json::to_vec(&frame).unwrap()).is_err());
    }
}

#[test]
fn cycles_missing_references_unreachable_nodes_and_duplicates_fail_before_rendering() {
    for failure in ["cycle", "missing", "unreachable", "duplicate", "root"] {
        let mut value = surface();
        match failure {
            "cycle" => {
                value.components[0] = Component::Column {
                    id: "root".into(),
                    children: vec!["root".into()],
                }
            }
            "missing" => {
                value.components[0] = Component::Column {
                    id: "root".into(),
                    children: vec!["missing".into()],
                }
            }
            "unreachable" => value.components.push(Component::Divider {
                id: "detached".into(),
            }),
            "duplicate" => value.components.push(value.components[1].clone()),
            "root" => {
                value.components.remove(0);
            }
            _ => unreachable!(),
        };
        assert!(value.validate().is_err(), "{failure}");
    }
}

#[test]
fn graph_depth_and_shared_descendant_depth_are_bounded() {
    for size in [24, 25] {
        let mut value = surface();
        value.components.clear();
        for index in 0..size {
            let id = if index == 0 {
                "root".into()
            } else {
                format!("node-{index}")
            };
            value.components.push(if index + 1 == size {
                Component::Divider { id }
            } else {
                Component::Card {
                    id,
                    child: format!("node-{}", index + 1),
                }
            });
        }
        assert_eq!(value.validate().is_ok(), size == 24);
    }
}

#[test]
fn frame_component_children_data_and_text_budgets_are_enforced() {
    let mut value = surface();
    value
        .data
        .insert("title".into(), "a".repeat(MAX_TEXT_BYTES + 1));
    assert!(value.validate().is_err());
    let mut value = surface();
    value.data = (0..65)
        .map(|index| (format!("key-{index}"), String::new()))
        .collect();
    assert!(value.validate().is_err());
    let mut value = surface();
    value.components[0] = Component::Column {
        id: "root".into(),
        children: vec!["title".into(); 65],
    };
    assert!(value.validate().is_err());
    let mut value = surface();
    value.components = vec![Component::Divider { id: "root".into() }; 257];
    assert!(value.validate().is_err());
    assert!(Envelope::decode(&vec![b' '; MAX_FRAME_BYTES + 1]).is_err());
}

#[test]
fn repeated_descendants_cannot_expand_a_small_graph_into_exponential_widgets() {
    let mut value = surface();
    value.components[0] = Component::Column {
        id: "root".into(),
        children: vec!["title".into(), "title".into(), "open".into()],
    };
    assert!(value.validate().is_err());
}

#[test]
fn invalid_bindings_identifiers_versions_and_counters_are_rejected() {
    for path in ["/missing", "/title/child", "../title", "/title~1child"] {
        let mut value = surface();
        value.components[1] = Component::PlainText {
            id: "title".into(),
            text: TextValue::Binding(TextBinding { path: path.into() }),
            style: None,
        };
        assert!(value.validate().is_err(), "{path}");
    }
    for (field, value) in [
        ("version", json!(2)),
        ("generation", json!("../socket")),
        ("sequence", json!(0)),
        ("sequence", json!(MAX_SAFE_INTEGER + 1)),
        ("unexpected", json!("ignored")),
    ] {
        let mut frame = serde_json::to_value(snapshot()).unwrap();
        frame[field] = value;
        assert!(Envelope::decode(&serde_json::to_vec(&frame).unwrap()).is_err());
    }
}

#[test]
fn deep_json_invalid_utf8_and_duplicate_headers_do_not_reach_the_sdk() {
    let nested = format!("{}0{}", "[".repeat(33), "]".repeat(33));
    assert!(Envelope::decode(nested.as_bytes()).is_err());
    assert!(Envelope::decode(&[0xff]).is_err());
    assert!(
        Envelope::decode(br#"{"kind":"ready","version":1,"version":1,"generation":"fixture"}"#)
            .is_err()
    );
}

#[test]
fn bootstrap_capabilities_and_task_content_are_never_in_debug_output() {
    let hello = Envelope::Hello {
        version: 1,
        generation: "fixture".into(),
        token: "private-fixture-capability".into(),
    };
    assert!(!format!("{hello:?}").contains("private-fixture-capability"));
    assert!(!format!("{:?}", snapshot()).contains("Verified automatically"));
}
