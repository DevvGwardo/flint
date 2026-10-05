//! A Model Context Protocol client for stdio servers: start each configured
//! server, list its tools, and offer them to the model as
//! `mcp__<server>__<tool>`. JSON-RPC 2.0, one message per line.
//!
//! Servers start once per session and are shared with its subagents. A
//! server that fails to start or list its tools is reported and skipped; it
//! never stops the session.

use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::time::Duration;

use serde_json::Map;
use serde_json::Value;
use serde_json::json;
use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::io::BufReader;
use tokio::process::Child;
use tokio::process::ChildStdin;
use tokio::process::Command;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use crate::protocol::McpServerConfig;
use crate::tools::ToolOutcome;
use crate::tools::head_tail;

/// Prefix of every MCP tool name the model sees.
pub const PREFIX: &str = "mcp__";
const PROTOCOL_VERSION: &str = "2025-06-18";
const START_TIMEOUT: Duration = Duration::from_secs(20);
const CALL_TIMEOUT: Duration = Duration::from_secs(300);
/// Characters of a tool result the model sees.
const OUTPUT_CHARS: usize = 20_000;
/// Longest tool name Chat Completions endpoints accept.
const MAX_NAME_CHARS: usize = 64;
const MAX_FRAME_BYTES: usize = 1024 * 1024;
const MAX_PENDING: usize = 256;
const MAX_TOOLS: usize = 500;
const MAX_SCHEMA_BYTES: usize = 64 * 1024;
const MAX_INVENTORY_BYTES: usize = 4 * 1024 * 1024;
const MAX_LIST_PAGES: usize = 32;
const MAX_SERVERS: usize = 16;

type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value, String>>>>>;

/// Dropping a request future must not leave a sender in the shared inbox.
struct PendingRequest {
    id: u64,
    pending: Pending,
    stdin: Arc<tokio::sync::Mutex<ChildStdin>>,
    notify: bool,
}

impl Drop for PendingRequest {
    fn drop(&mut self) {
        let removed = self
            .pending
            .lock()
            .ok()
            .and_then(|mut p| p.remove(&self.id))
            .is_some();
        if !removed || !self.notify {
            return;
        }
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let stdin = Arc::clone(&self.stdin);
        let id = self.id;
        runtime.spawn(async move {
            // Best effort, including when the server stopped reading stdin.
            let _ = tokio::time::timeout(Duration::from_secs(1), async move {
                let mut line = serde_json::to_vec(&json!({
                    "jsonrpc":"2.0", "method":"notifications/cancelled",
                    "params":{"requestId":id, "reason":"client request cancelled"}
                }))
                .unwrap_or_default();
                line.push(b'\n');
                let mut stdin = stdin.lock().await;
                stdin.write_all(&line).await?;
                stdin.flush().await
            })
            .await;
        });
    }
}

/// One running server.
struct Server {
    name: String,
    stdin: Arc<tokio::sync::Mutex<ChildStdin>>,
    pending: Pending,
    next_id: AtomicU64,
    /// Held so the process is killed when the hub is dropped.
    _child: Child,
}

/// One tool of one server.
#[derive(Debug, Clone)]
pub struct McpTool {
    /// `mcp__<server>__<tool>`, as offered to the model.
    pub full_name: String,
    server: usize,
    /// The server's own name for it.
    name: String,
    description: String,
    input_schema: Value,
}

/// Every started server and its tools.
#[derive(Default)]
pub struct McpHub {
    servers: Vec<Server>,
    tools: Vec<McpTool>,
}

impl std::fmt::Debug for McpHub {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpHub")
            .field(
                "servers",
                &self.servers.iter().map(|s| &s.name).collect::<Vec<_>>(),
            )
            .field("tools", &self.tools.len())
            .finish()
    }
}

impl McpHub {
    /// Starts every server in `workspace`. Returns the hub and a message for
    /// each server that could not be used.
    pub async fn start(configs: &[McpServerConfig], workspace: &Path) -> (Self, Vec<String>) {
        let mut hub = Self::default();
        let mut errors = Vec::new();
        let mut inventory_bytes = 0usize;
        for (config_at, config) in configs.iter().enumerate() {
            if config_at >= MAX_SERVERS {
                errors.push("MCP server inventory exceeds the 16-server limit.".into());
                break;
            }
            match tokio::time::timeout(START_TIMEOUT, start_server(config, workspace)).await {
                Ok(Ok((server, tools))) => {
                    let index = hub.servers.len();
                    hub.servers.push(server);
                    for (name, description, input_schema) in tools {
                        let size = name.len() + description.len() + input_schema.to_string().len();
                        if hub.tools.len() >= MAX_TOOLS
                            || inventory_bytes.saturating_add(size) > MAX_INVENTORY_BYTES
                        {
                            errors.push(
                                "MCP tool inventory exceeds the 500-tool / 4 MiB limit.".into(),
                            );
                            break;
                        }
                        let full_name = tool_name(&config.name, &name);
                        if hub.tools.iter().any(|t| t.full_name == full_name) {
                            continue;
                        }
                        hub.tools.push(McpTool {
                            full_name,
                            server: index,
                            name,
                            description,
                            input_schema,
                        });
                        inventory_bytes += size;
                    }
                }
                Ok(Err(error)) => errors.push(format!("MCP server `{}`: {error}", config.name)),
                Err(_) => errors.push(format!(
                    "MCP server `{}` did not start within {}s.",
                    config.name,
                    START_TIMEOUT.as_secs()
                )),
            }
        }
        (hub, errors)
    }

    pub fn tools(&self) -> &[McpTool] {
        &self.tools
    }

    /// Chat Completions function specs for every tool.
    pub fn specs(&self) -> Vec<Value> {
        self.tools
            .iter()
            .map(|tool| {
                let mut parameters = match &tool.input_schema {
                    Value::Object(schema) => schema.clone(),
                    _ => Map::new(),
                };
                parameters
                    .entry("type")
                    .or_insert_with(|| Value::String("object".into()));
                parameters
                    .entry("properties")
                    .or_insert_with(|| Value::Object(Map::new()));
                parameters.remove("$schema");
                let server = &self.servers[tool.server].name;
                let description: String = format!("[MCP {server}] {}", tool.description)
                    .chars()
                    .take(1_000)
                    .collect();
                json!({
                    "type": "function",
                    "function": {
                        "name": tool.full_name,
                        "description": description,
                        "parameters": Value::Object(parameters),
                    }
                })
            })
            .collect()
    }

    /// Calls the tool the model knows as `full_name`.
    pub async fn call(
        &self,
        full_name: &str,
        args: &Map<String, Value>,
        cancel: &CancellationToken,
    ) -> ToolOutcome {
        let Some(tool) = self.tools.iter().find(|t| t.full_name == full_name) else {
            return ToolOutcome::error(format!("unknown MCP tool `{full_name}`"));
        };
        let server = &self.servers[tool.server];
        let request = server.request(
            "tools/call",
            json!({"name": tool.name, "arguments": Value::Object(args.clone())}),
        );
        let result = tokio::select! {
            result = tokio::time::timeout(CALL_TIMEOUT, request) => match result {
                Ok(result) => result,
                Err(_) => Err(format!("no answer within {}s", CALL_TIMEOUT.as_secs())),
            },
            () = cancel.cancelled() => Err("Interrupted.".to_string()),
        };
        match result {
            Ok(result) => {
                let failed = result.get("isError").and_then(Value::as_bool) == Some(true);
                let text = result_text(&result);
                ToolOutcome {
                    output: head_tail(&text, OUTPUT_CHARS),
                    exit_code: None,
                    success: !failed,
                    diff: None,
                }
            }
            Err(error) => ToolOutcome::error(format!("MCP `{}`: {error}", server.name)),
        }
    }
}

impl Server {
    async fn send(&self, message: &Value) -> Result<(), String> {
        let mut line = serde_json::to_vec(message).map_err(|e| e.to_string())?;
        if line.len() > MAX_FRAME_BYTES {
            return Err("MCP request exceeds the 1 MiB frame limit".into());
        }
        line.push(b'\n');
        let mut stdin = self.stdin.lock().await;
        stdin
            .write_all(&line)
            .await
            .and(stdin.flush().await)
            .map_err(|e| format!("cannot write to the server: {e}"))
    }

    async fn request(&self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        {
            let mut pending = self
                .pending
                .lock()
                .map_err(|_| "MCP pending inbox poisoned")?;
            if pending.len() >= MAX_PENDING {
                return Err("too many pending MCP requests".into());
            }
            pending.insert(id, tx);
        }
        let mut cleanup = PendingRequest {
            id,
            pending: Arc::clone(&self.pending),
            stdin: Arc::clone(&self.stdin),
            notify: true,
        };
        let sent = self
            .send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))
            .await;
        if let Err(error) = sent {
            cleanup.notify = false;
            return Err(error);
        }
        rx.await
            .unwrap_or_else(|_| Err("the server exited".to_string()))
    }
}

type ToolInfo = (String, String, Value);

async fn start_server(
    config: &McpServerConfig,
    workspace: &Path,
) -> Result<(Server, Vec<ToolInfo>), String> {
    let mut command = Command::new(&config.command);
    command
        .args(&config.args)
        .envs(config.env.iter().map(|(k, v)| (k, v)))
        .current_dir(workspace)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let mut child = command
        .spawn()
        .map_err(|e| format!("cannot start `{}`: {e}", config.command))?;
    let stdin = child.stdin.take().ok_or("no stdin")?;
    let stdout = child.stdout.take().ok_or("no stdout")?;
    let pending: Pending = Arc::default();
    let reader_pending = Arc::clone(&pending);
    let (replies_tx, replies_rx) = async_channel::bounded::<Value>(16);
    tokio::spawn(async move {
        let mut reader = BufReader::new(stdout);
        let mut exit_error = "the server exited".to_string();
        loop {
            let mut line = Vec::new();
            let count = match (&mut reader)
                .take(MAX_FRAME_BYTES as u64 + 1)
                .read_until(b'\n', &mut line)
                .await
            {
                Ok(0) | Err(_) => break,
                Ok(count) => count,
            };
            if count > MAX_FRAME_BYTES {
                exit_error = "MCP input exceeds the 1 MiB frame limit".into();
                break;
            }
            let Ok(message) = serde_json::from_slice::<Value>(&line) else {
                continue;
            };
            let has_method = message.get("method").is_some();
            match message.get("id").and_then(Value::as_u64) {
                // A response to one of our requests.
                Some(id) if !has_method => {
                    let reply = match message.get("error") {
                        Some(error) => Err(error
                            .get("message")
                            .and_then(Value::as_str)
                            .map_or_else(|| error.to_string(), str::to_string)),
                        None => Ok(message.get("result").cloned().unwrap_or(Value::Null)),
                    };
                    let waiting = reader_pending.lock().ok().and_then(|mut p| p.remove(&id));
                    if let Some(waiting) = waiting {
                        let _ = waiting.send(reply);
                    }
                }
                // A request from the server (roots, sampling, …): unsupported.
                _ if has_method
                    && message.get("id").is_some()
                    && replies_tx
                        .try_send(json!({"jsonrpc": "2.0", "id": message["id"],
                            "error": {"code": -32601, "message": "not supported by flint"}}))
                        .is_err() =>
                {
                    exit_error = "too many MCP server requests".into();
                    break;
                }
                _ => {}
            }
        }
        // The server exited: fail every waiting request.
        if let Ok(mut pending) = reader_pending.lock() {
            for (_, waiting) in pending.drain() {
                let _ = waiting.send(Err(exit_error.clone()));
            }
        }
    });
    let stdin = Arc::new(tokio::sync::Mutex::new(stdin));
    // Answer the server's own requests for as long as it runs.
    let replies_stdin = Arc::clone(&stdin);
    tokio::spawn(async move {
        while let Ok(reply) = replies_rx.recv().await {
            let mut line = serde_json::to_vec(&reply).unwrap_or_default();
            line.push(b'\n');
            let mut stdin = replies_stdin.lock().await;
            if stdin.write_all(&line).await.is_err() {
                break;
            }
            let _ = stdin.flush().await;
        }
    });
    let server = Server {
        name: config.name.clone(),
        stdin,
        pending,
        next_id: AtomicU64::new(1),
        _child: child,
    };
    server
        .request(
            "initialize",
            json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": {"name": "flint", "version": env!("CARGO_PKG_VERSION")},
            }),
        )
        .await
        .map_err(|e| format!("initialize failed: {e}"))?;
    server
        .send(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))
        .await?;
    let mut tools = Vec::new();
    let mut cursor: Option<String> = None;
    let mut cursors = std::collections::HashSet::new();
    let mut pages = 0;
    let mut inventory_bytes = 0usize;
    loop {
        pages += 1;
        if pages > MAX_LIST_PAGES {
            return Err("MCP tools/list exceeds the 32-page limit".into());
        }
        let params = match &cursor {
            Some(cursor) => json!({"cursor": cursor}),
            None => json!({}),
        };
        let listing = server
            .request("tools/list", params)
            .await
            .map_err(|e| format!("tools/list failed: {e}"))?;
        for tool in listing
            .get("tools")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(name) = tool.get("name").and_then(Value::as_str) else {
                continue;
            };
            let schema = tool.get("inputSchema").cloned().unwrap_or(Value::Null);
            let description = tool
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let schema_bytes = schema.to_string().len();
            let size = name.len() + description.len() + schema_bytes;
            if tools.len() >= MAX_TOOLS
                || name.len() > 256
                || description.len() > 4096
                || schema_bytes > MAX_SCHEMA_BYTES
                || inventory_bytes.saturating_add(size) > MAX_INVENTORY_BYTES
            {
                return Err("MCP tool/schema inventory exceeds its safety limits".into());
            }
            inventory_bytes += size;
            tools.push((name.to_string(), description.to_string(), schema));
        }
        cursor = listing
            .get("nextCursor")
            .and_then(Value::as_str)
            .map(str::to_string);
        if cursor.is_none() {
            break;
        }
        if !cursors.insert(cursor.clone()) {
            return Err("MCP tools/list repeated a pagination cursor".into());
        }
    }
    Ok((server, tools))
}

/// `mcp__<server>__<tool>`, limited to the characters and length tool names
/// may have.
pub fn tool_name(server: &str, tool: &str) -> String {
    let clean = |text: &str| -> String {
        text.chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .collect()
    };
    let name = format!("{PREFIX}{}__{}", clean(server), clean(tool));
    name.chars().take(MAX_NAME_CHARS).collect()
}

/// The text of a `tools/call` result: text blocks joined, other content
/// named, structured content as JSON when there is no text.
fn result_text(result: &Value) -> String {
    let mut parts = Vec::new();
    for block in result
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        match block.get("type").and_then(Value::as_str) {
            Some("text") => {
                if let Some(text) = block.get("text").and_then(Value::as_str) {
                    parts.push(text.to_string());
                }
            }
            Some("resource") => {
                let resource = &block["resource"];
                match resource.get("text").and_then(Value::as_str) {
                    Some(text) => parts.push(text.to_string()),
                    None => parts.push(format!(
                        "[resource {}]",
                        resource.get("uri").and_then(Value::as_str).unwrap_or("")
                    )),
                }
            }
            Some(other) => parts.push(format!("[{other} content omitted]")),
            None => {}
        }
    }
    if parts.is_empty()
        && let Some(structured) = result.get("structuredContent")
    {
        return structured.to_string();
    }
    if parts.is_empty() {
        "(no output)".to_string()
    } else {
        parts.join("\n")
    }
}

#[cfg(test)]
#[path = "mcp_tests.rs"]
mod tests;
