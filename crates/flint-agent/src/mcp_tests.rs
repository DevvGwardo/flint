use pretty_assertions::assert_eq;
use serde_json::json;

use super::*;

/// A minimal MCP server: one `echo` tool, one `fail` tool, and a request
/// back to the client during startup (which flint must answer).
const SERVER: &str = r#"
import json, sys
def send(m):
    sys.stdout.write(json.dumps(m) + "\n"); sys.stdout.flush()
for line in sys.stdin:
    m = json.loads(line)
    method, mid = m.get("method"), m.get("id")
    if method == "initialize":
        send({"jsonrpc": "2.0", "id": "srv-1", "method": "roots/list"})
        send({"jsonrpc": "2.0", "id": mid, "result": {"protocolVersion": "2025-06-18",
              "capabilities": {"tools": {}}, "serverInfo": {"name": "t", "version": "1"}}})
    elif method == "tools/list" and not (m.get("params") or {}).get("cursor"):
        send({"jsonrpc": "2.0", "id": mid, "result": {"nextCursor": "p2", "tools": [
            {"name": "echo", "description": "Echo text.", "inputSchema": {"type": "object",
             "properties": {"text": {"type": "string"}}, "required": ["text"]}}]}})
    elif method == "tools/list":
        send({"jsonrpc": "2.0", "id": mid, "result": {"tools": [
            {"name": "fail", "description": "Always fails."}]}})
    elif method == "tools/call":
        p = m["params"]
        if p["name"] == "echo":
            send({"jsonrpc": "2.0", "id": mid, "result": {"content": [
                {"type": "text", "text": "echo: " + p["arguments"]["text"]},
                {"type": "image", "data": "", "mimeType": "image/png"}]}})
        else:
            send({"jsonrpc": "2.0", "id": mid, "result": {"isError": True,
                  "content": [{"type": "text", "text": "it broke"}]}})
    elif mid is not None and method is None:
        pass  # our answer to roots/list
"#;

fn config(name: &str, dir: &Path) -> McpServerConfig {
    let script = dir.join("server.py");
    std::fs::write(&script, SERVER).expect("script");
    McpServerConfig {
        name: name.to_string(),
        command: "python3".into(),
        args: vec![script.display().to_string()],
        env: Vec::new(),
    }
}

#[tokio::test]
async fn lists_and_calls_tools_of_a_stdio_server() {
    let dir = tempfile::tempdir().expect("tempdir");
    let missing = McpServerConfig {
        name: "gone".into(),
        command: "/nonexistent/mcp-server".into(),
        args: Vec::new(),
        env: Vec::new(),
    };
    let (hub, errors) = McpHub::start(&[config("demo.srv", dir.path()), missing], dir.path()).await;
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(
        errors[0].starts_with("MCP server `gone`: cannot start"),
        "{errors:?}"
    );
    let names: Vec<&str> = hub.tools().iter().map(|t| t.full_name.as_str()).collect();
    assert_eq!(names, ["mcp__demo_srv__echo", "mcp__demo_srv__fail"]);

    let specs = hub.specs();
    assert_eq!(
        specs[0]["function"]["description"],
        "[MCP demo.srv] Echo text."
    );
    assert_eq!(
        specs[0]["function"]["parameters"]["required"],
        json!(["text"])
    );
    // A tool without a schema still gets a valid object schema.
    assert_eq!(
        specs[1]["function"]["parameters"],
        json!({"type": "object", "properties": {}})
    );

    let cancel = CancellationToken::new();
    let args = json!({"text": "hi"});
    let echoed = hub
        .call(
            "mcp__demo_srv__echo",
            args.as_object().expect("object"),
            &cancel,
        )
        .await;
    assert!(echoed.success);
    assert_eq!(echoed.output, "echo: hi\n[image content omitted]");
    let failed = hub.call("mcp__demo_srv__fail", &Map::new(), &cancel).await;
    assert!(!failed.success);
    assert_eq!(failed.output, "it broke");
}

#[test]
fn tool_names_are_sanitized_and_bounded() {
    assert_eq!(tool_name("git hub", "list.prs"), "mcp__git_hub__list_prs");
    assert_eq!(tool_name("s", &"x".repeat(100)).chars().count(), 64);
}

#[tokio::test]
async fn dropped_request_removes_pending_and_notifies_server() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cfg = config("drop", dir.path());
    let script = SERVER.replace(
        "    elif method == \"tools/call\":",
        "    elif method == \"hang\":\n        pass\n    elif method == \"notifications/cancelled\":\n        open(\"cancelled.json\", \"w\").write(json.dumps(m[\"params\"]))\n    elif method == \"tools/call\":",
    );
    std::fs::write(dir.path().join("server.py"), script).expect("script");
    let (server, _) = start_server(&cfg, dir.path()).await.expect("server");
    let mut request = Box::pin(server.request("hang", json!({})));
    assert!(
        tokio::time::timeout(Duration::from_millis(50), request.as_mut())
            .await
            .is_err()
    );
    assert_eq!(server.pending.lock().expect("pending").len(), 1);
    drop(request);
    assert!(server.pending.lock().expect("pending").is_empty());
    tokio::time::timeout(Duration::from_secs(2), async {
        while !dir.path().join("cancelled.json").exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("cancellation notification");
    let notification: Value = serde_json::from_slice(
        &std::fs::read(dir.path().join("cancelled.json")).expect("notification"),
    )
    .expect("json");
    assert!(notification["requestId"].is_u64());
}

#[tokio::test]
async fn oversized_mcp_frame_fails_without_waiting_for_newline() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cfg = config("oversize", dir.path());
    std::fs::write(dir.path().join("server.py"),
        "import sys\nsys.stdin.readline()\nsys.stdout.write('x' * (1024 * 1024 + 1)); sys.stdout.flush()\nsys.stdin.readline()\n"
    ).expect("script");
    let result = tokio::time::timeout(Duration::from_secs(2), start_server(&cfg, dir.path()))
        .await
        .expect("bounded frame");
    assert!(result.is_err());
}

#[tokio::test]
async fn schema_and_tool_inventories_are_bounded() {
    for tools in [
        json!([{"name":"huge", "inputSchema":{"description":"x".repeat(64 * 1024 + 1)}}]),
        Value::Array((0..501).map(|i| json!({"name":format!("t{i}")})).collect()),
    ] {
        let dir = tempfile::tempdir().expect("tempdir");
        let cfg = config("inventory", dir.path());
        let script = format!(
            "import sys,json\nfor line in sys.stdin:\n m=json.loads(line)\n if m.get('id') is not None:\n  result={{'tools':json.loads({:?})}} if m['method']=='tools/list' else {{}}\n  print(json.dumps({{'jsonrpc':'2.0','id':m['id'],'result':result}}),flush=True)\n",
            tools.to_string()
        );
        std::fs::write(dir.path().join("server.py"), script).expect("script");
        let result = tokio::time::timeout(Duration::from_secs(2), start_server(&cfg, dir.path()))
            .await
            .expect("bounded inventory");
        assert!(result.is_err());
    }
}
