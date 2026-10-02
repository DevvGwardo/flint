use agent_client_protocol::schema::v1::ToolKind as AcpKind;
use pretty_assertions::assert_eq;

use super::*;

#[test]
fn acp_kinds_map_to_flint_kinds() {
    let pairs = [
        (AcpKind::Read, ToolKind::Read),
        (AcpKind::Edit, ToolKind::Edit),
        (AcpKind::Delete, ToolKind::Edit),
        (AcpKind::Move, ToolKind::Edit),
        (AcpKind::Search, ToolKind::Search),
        (AcpKind::Fetch, ToolKind::Search),
        (AcpKind::Execute, ToolKind::Command),
        (AcpKind::Think, ToolKind::Other),
        (AcpKind::Other, ToolKind::Other),
    ];
    for (acp, flint) in pairs {
        assert_eq!(map_kind(acp), flint);
    }
}

#[test]
fn raw_output_shapes() {
    assert_eq!(raw_output_text(&serde_json::json!("plain")), "plain");
    assert_eq!(
        raw_output_text(&serde_json::json!({"stdout": "out", "code": 0})),
        "out"
    );
    assert_eq!(raw_output_text(&serde_json::Value::Null), "");
}
