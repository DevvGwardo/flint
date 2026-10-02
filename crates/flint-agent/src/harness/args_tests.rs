use pretty_assertions::assert_eq;
use serde_json::json;

use super::*;

fn ok(value: Value, repaired: bool) -> RepairedArgs {
    let Value::Object(value) = value else {
        panic!("expected object")
    };
    RepairedArgs::Ok { value, repaired }
}

#[test]
fn parses_clean_and_empty_arguments() {
    assert_eq!(
        repair_tool_args(r#"{"path":"a.rs"}"#),
        ok(json!({"path": "a.rs"}), false)
    );
    assert_eq!(repair_tool_args("  "), ok(json!({}), false));
}

#[test]
fn drops_nulls_and_expands_encoded_arrays() {
    assert_eq!(
        repair_tool_args(r#"{"path":"a.rs","limit":null,"paths":"[\"a\",\"b\"]"}"#),
        ok(json!({"path": "a.rs", "paths": ["a", "b"]}), true)
    );
}

#[test]
fn fixes_trailing_commas_and_truncation() {
    assert_eq!(repair_tool_args(r#"{"a":1,}"#), ok(json!({"a": 1}), true));
    assert_eq!(
        repair_tool_args(r#"{"command":"ls -la","items":[1,2"#),
        ok(json!({"command": "ls -la", "items": [1, 2]}), true)
    );
    assert_eq!(
        repair_tool_args(r#"{"content":"hello, world"#),
        ok(json!({"content": "hello, world"}), true)
    );
}

#[test]
fn keeps_commas_inside_strings() {
    assert_eq!(strip_trailing_commas(r#"{"a":"x,}"}"#), r#"{"a":"x,}"}"#);
}

#[test]
fn rejects_hopeless_input_and_non_objects() {
    assert!(matches!(
        repair_tool_args("{bad"),
        RepairedArgs::Invalid { .. }
    ));
    assert!(matches!(
        repair_tool_args("[1,2]"),
        RepairedArgs::Invalid { .. }
    ));
    assert_eq!(close_unbalanced("{]"), None);
}

#[test]
fn matches_tool_names_loosely() {
    let names = ["read_file", "write_file", "run_command"];
    assert_eq!(closest_tool_name("ReadFile", &names), Some("read_file"));
    assert_eq!(
        closest_tool_name("functions.run_command", &names),
        Some("run_command")
    );
    assert_eq!(closest_tool_name("write-file", &names), Some("write_file"));
    assert_eq!(closest_tool_name("grep", &names), None);
}
