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

#[test]
fn terminal_failure_survives_the_agents_unknown_error_output() {
    let mut mapper = Mapper::new(Path::new("/workspace"), 0);
    let mut events = Vec::new();
    mapper.tool(
        "call".into(),
        ToolCallUpdateFields::new()
            .kind(AcpKind::Execute)
            .title("echo ok"),
        None,
        &mut events,
    );
    let request = CreateTerminalRequest::new("test", "echo ok");
    let diagnostic = mapper.terminal_failed(&request, "cannot use terminal cwd");
    assert!(
        matches!(&diagnostic[0], AgentEvent::ToolOutputDelta { call_id, .. } if call_id == "call")
    );
    events.clear();
    mapper.tool(
        "call".into(),
        ToolCallUpdateFields::new()
            .status(ToolCallStatus::Failed)
            .raw_output(serde_json::json!("Unknown error")),
        None,
        &mut events,
    );
    assert!(
        matches!(&events[0], AgentEvent::ToolCallFinished { output, success: false, .. }
        if output.contains("Unknown error")
            && output.lines().last() == Some("cannot use terminal cwd"))
    );
}

#[test]
fn ambiguous_terminal_failure_gets_its_own_row() {
    let mut mapper = Mapper::new(Path::new("/workspace"), 0);
    for id in ["first", "second"] {
        mapper.tool(
            id.into(),
            ToolCallUpdateFields::new()
                .kind(AcpKind::Execute)
                .title("echo ok"),
            None,
            &mut Vec::new(),
        );
    }
    let events = mapper.terminal_failed(&CreateTerminalRequest::new("test", "echo ok"), "bad cwd");
    assert!(
        matches!(&events[0], AgentEvent::ToolCallStarted { call_id, .. }
        if call_id.starts_with("flint-terminal-error-"))
    );
    assert!(
        mapper
            .calls
            .values()
            .all(|call| call.terminal_error.is_none())
    );
}

#[test]
fn unfinished_terminal_deltas_have_a_hard_storage_cap() {
    let mut mapper = Mapper::new(Path::new("/workspace"), 0);
    let mut events = Vec::new();
    mapper.tool(
        "call".into(),
        ToolCallUpdateFields::new(),
        None,
        &mut events,
    );
    let meta = serde_json::json!({
        "terminal_output_delta": {"terminal_id": "t1", "data": "界".repeat(4096)}
    });
    for _ in 0..128 {
        events.clear();
        mapper.terminal_meta("call", meta.as_object().unwrap(), &mut events);
        assert!(mapper.calls["call"].output.len() <= 1024 * 1024);
    }
    let closed = mapper.close_open_calls(false);
    assert!(
        matches!(&closed[0], AgentEvent::ToolCallFinished { output, .. }
        if output.contains("truncated"))
    );
}
