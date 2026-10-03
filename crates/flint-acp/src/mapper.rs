//! Turns ACP `session/update` notifications into flint [`AgentEvent`]s.
//!
//! Pure state: no I/O, so the mapping is unit-tested on its own. It tracks
//! the tool calls of the current turn so a call is "started" once and
//! "finished" once, and so a file write the client performed during an edit
//! call is shown as that call's diff instead of a second row.

use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

use agent_client_protocol::schema::v1::ContentBlock;
use agent_client_protocol::schema::v1::SessionUpdate;
use agent_client_protocol::schema::v1::ToolCallContent;
use agent_client_protocol::schema::v1::ToolCallStatus;
use agent_client_protocol::schema::v1::ToolCallUpdateFields;
use agent_client_protocol::schema::v1::ToolKind as AcpKind;
use flint_agent::AgentEvent;
use flint_agent::FileDiff;
use flint_agent::ToolKind;
use flint_agent::Usage;
use flint_agent::tools::file_diff;
use flint_agent::tools::head_tail;
use serde_json::Value;

use crate::terminal_meta::TermMeta;

/// Characters of tool output kept for a row.
const OUTPUT_CHARS: usize = 8_000;

#[derive(Debug)]
struct Call {
    kind: ToolKind,
    title: String,
    started: Instant,
    output: String,
    diff: Option<FileDiff>,
    /// The diff came from our own fs write; the agent's copy is ignored.
    diff_from_write: bool,
    finished: bool,
    /// Exit code reported through the terminal extension.
    exit_code: Option<i32>,
}

/// Mapping state for one session.
#[derive(Debug)]
pub struct Mapper {
    workspace: std::path::PathBuf,
    turn_id: u64,
    step: u32,
    /// The last event was part of a tool call; the next text starts a step.
    after_tool: bool,
    calls: HashMap<String, Call>,
    order: Vec<String>,
    writes: u32,
    /// Prefix for terminal tab labels ("claude: npm test").
    label_prefix: String,
    /// Terminals already announced, by id.
    terminals: HashMap<String, String>,
}

impl Mapper {
    pub fn new(workspace: &Path, turn_id: u64) -> Self {
        Self {
            workspace: workspace.to_path_buf(),
            turn_id,
            step: 0,
            after_tool: false,
            calls: HashMap::new(),
            order: Vec::new(),
            writes: 0,
            label_prefix: "agent".to_string(),
            terminals: HashMap::new(),
        }
    }

    /// Terminal tabs are labelled `"{prefix}: {command}"`.
    pub fn with_label_prefix(mut self, prefix: &str) -> Self {
        self.label_prefix = prefix.to_string();
        self
    }

    pub fn turn_id(&self) -> u64 {
        self.turn_id
    }

    /// A prompt is about to be sent: the turn's opening events.
    pub fn start_turn(&mut self) -> Vec<AgentEvent> {
        self.turn_id += 1;
        self.step = 0;
        self.after_tool = false;
        self.calls.clear();
        self.order.clear();
        vec![
            AgentEvent::TurnStarted {
                turn_id: self.turn_id,
            },
            AgentEvent::StepStarted {
                turn_id: self.turn_id,
                step: 0,
            },
        ]
    }

    /// Text after a tool call opens a new step, like flint's own loop.
    fn maybe_new_step(&mut self, out: &mut Vec<AgentEvent>) {
        if self.after_tool {
            self.after_tool = false;
            self.step += 1;
            out.push(AgentEvent::StepStarted {
                turn_id: self.turn_id,
                step: self.step,
            });
        }
    }

    pub fn update(&mut self, update: SessionUpdate) -> Vec<AgentEvent> {
        let mut out = Vec::new();
        match update {
            SessionUpdate::AgentMessageChunk(chunk) => {
                if let Some(text) = text_of(&chunk.content) {
                    self.maybe_new_step(&mut out);
                    out.push(AgentEvent::TextDelta(text));
                }
            }
            SessionUpdate::AgentThoughtChunk(chunk) => {
                if let Some(text) = text_of(&chunk.content) {
                    self.maybe_new_step(&mut out);
                    out.push(AgentEvent::ReasoningDelta(text));
                }
            }
            SessionUpdate::ToolCall(call) => {
                let fields = ToolCallUpdateFields::new()
                    .kind(call.kind)
                    .status(call.status)
                    .title(call.title)
                    .content(call.content)
                    .raw_input(call.raw_input)
                    .raw_output(call.raw_output);
                self.tool(
                    call.tool_call_id.to_string(),
                    fields,
                    call.meta.as_ref(),
                    &mut out,
                );
            }
            SessionUpdate::ToolCallUpdate(update) => {
                self.tool(
                    update.tool_call_id.to_string(),
                    update.fields,
                    update.meta.as_ref(),
                    &mut out,
                );
            }
            SessionUpdate::UsageUpdate(_)
            | SessionUpdate::UserMessageChunk(_)
            | SessionUpdate::Plan(_)
            | SessionUpdate::AvailableCommandsUpdate(_)
            | SessionUpdate::CurrentModeUpdate(_)
            | SessionUpdate::ConfigOptionUpdate(_)
            | SessionUpdate::SessionInfoUpdate(_) => {}
            // Unstable or future variants carry nothing flint shows.
            _ => {}
        }
        out
    }

    /// Starts a call the first time it is seen; finishes it on a terminal status.
    fn tool(
        &mut self,
        id: String,
        fields: ToolCallUpdateFields,
        meta: Option<&serde_json::Map<String, Value>>,
        out: &mut Vec<AgentEvent>,
    ) {
        self.after_tool = true;
        if !self.calls.contains_key(&id) {
            let kind = fields.kind.map_or(ToolKind::Other, map_kind);
            let title = fields.title.clone().unwrap_or_else(|| "tool".to_string());
            out.push(AgentEvent::ToolCallStarted {
                call_id: id.clone(),
                name: fields
                    .name
                    .clone()
                    .unwrap_or_else(|| kind_name(kind).to_string()),
                kind,
                args: fields.raw_input.clone().unwrap_or(Value::Null),
                summary: title.clone(),
            });
            self.calls.insert(
                id.clone(),
                Call {
                    kind,
                    title,
                    started: Instant::now(),
                    output: String::new(),
                    diff: None,
                    diff_from_write: false,
                    finished: false,
                    exit_code: None,
                },
            );
            self.order.push(id.clone());
        }
        let workspace = self.workspace.clone();
        let Some(call) = self.calls.get_mut(&id) else {
            return;
        };
        if let Some(title) = fields.title {
            call.title = title;
        }
        if let Some(content) = fields.content {
            let mut text = String::new();
            for item in content {
                match item {
                    ToolCallContent::Content(content) => {
                        if let Some(chunk) = text_of(&content.content) {
                            text.push_str(&chunk);
                        }
                    }
                    ToolCallContent::Diff(diff) if !call.diff_from_write => {
                        let path = relative(&workspace, &diff.path);
                        call.diff =
                            Some(file_diff(&path, diff.old_text.as_deref(), &diff.new_text));
                    }
                    ToolCallContent::Diff(_) | ToolCallContent::Terminal(_) => {}
                    _ => {}
                }
            }
            if !text.is_empty() {
                call.output = text;
            }
        }
        if call.output.is_empty()
            && let Some(raw) = fields.raw_output.as_ref()
        {
            call.output = raw_output_text(raw);
        }
        if let Some(meta) = meta {
            self.terminal_meta(&id, meta, out);
        }
        let Some(call) = self.calls.get_mut(&id) else {
            return;
        };
        let done = match fields.status {
            Some(ToolCallStatus::Completed) => Some(true),
            Some(ToolCallStatus::Failed) => Some(false),
            Some(ToolCallStatus::Pending | ToolCallStatus::InProgress) | None => None,
            Some(_) => None,
        };
        if let Some(success) = done
            && !call.finished
        {
            call.finished = true;
            out.push(finished_event(&id, call, success));
        }
    }

    /// Applies the terminal extension's `_meta` for a tool call: announces
    /// the terminal, forwards its output (also kept as the call's output)
    /// and its exit code.
    fn terminal_meta(
        &mut self,
        call_id: &str,
        meta: &serde_json::Map<String, Value>,
        out: &mut Vec<AgentEvent>,
    ) {
        for item in crate::terminal_meta::parse(meta) {
            let (TermMeta::Info { terminal_id, .. }
            | TermMeta::Output { terminal_id, .. }
            | TermMeta::Exit { terminal_id, .. }) = &item;
            let terminal_id = terminal_id.clone();
            if !self.terminals.contains_key(&terminal_id) {
                let cwd = match &item {
                    TermMeta::Info { cwd, .. } => cwd.clone(),
                    TermMeta::Output { .. } | TermMeta::Exit { .. } => None,
                };
                let title = self
                    .calls
                    .get(call_id)
                    .map(|c| c.title.clone())
                    .unwrap_or_default();
                self.terminals
                    .insert(terminal_id.clone(), call_id.to_string());
                out.push(AgentEvent::TerminalStarted {
                    terminal_id: terminal_id.clone(),
                    call_id: Some(call_id.to_string()),
                    label: format!("{}: {title}", self.label_prefix),
                    cwd,
                });
            }
            match item {
                TermMeta::Info { .. } => {}
                TermMeta::Output { data, replace, .. } => {
                    if let Some(call) = self.calls.get_mut(call_id) {
                        if replace {
                            call.output = data.clone();
                        } else {
                            call.output.push_str(&data);
                        }
                    }
                    out.push(AgentEvent::TerminalOutput {
                        terminal_id,
                        data,
                        replace,
                    });
                }
                TermMeta::Exit { exit_code, .. } => {
                    if let Some(call) = self.calls.get_mut(call_id) {
                        call.exit_code = exit_code;
                    }
                    out.push(AgentEvent::TerminalExited {
                        terminal_id,
                        exit_code,
                    });
                }
            }
        }
    }

    /// The client wrote a file for the agent (`fs/write_text_file`). It
    /// becomes the diff of the edit call in flight, or its own edit row.
    pub fn file_written(&mut self, path: &Path, old: Option<&str>, new: &str) -> Vec<AgentEvent> {
        let shown = relative(&self.workspace, path);
        let diff = file_diff(&shown, old, new);
        let in_flight = self
            .order
            .iter()
            .rev()
            .find(|id| {
                self.calls
                    .get(*id)
                    .is_some_and(|c| !c.finished && c.kind == ToolKind::Edit)
            })
            .cloned();
        if let Some(id) = in_flight
            && let Some(call) = self.calls.get_mut(&id)
        {
            call.diff = Some(diff);
            call.diff_from_write = true;
            return Vec::new();
        }
        self.writes += 1;
        let call_id = format!("fs-write-{}", self.writes);
        let output = format!("Wrote {shown} (+{} -{})", diff.added, diff.removed);
        vec![
            AgentEvent::ToolCallStarted {
                call_id: call_id.clone(),
                name: "write_text_file".to_string(),
                kind: ToolKind::Edit,
                args: serde_json::json!({ "path": shown }),
                summary: shown,
            },
            AgentEvent::ToolCallFinished {
                call_id,
                output,
                exit_code: None,
                success: true,
                diff: Some(diff),
                duration_ms: 0,
            },
        ]
    }

    /// A permission prompt: makes sure its call has a row first.
    pub fn approval(&mut self, id: &str, fields: ToolCallUpdateFields) -> Vec<AgentEvent> {
        let mut out = Vec::new();
        self.tool(id.to_string(), fields, None, &mut out);
        let (kind, summary) = self
            .calls
            .get(id)
            .map_or((ToolKind::Other, String::new()), |c| {
                (c.kind, c.title.clone())
            });
        out.push(AgentEvent::ApprovalRequested {
            call_id: id.to_string(),
            kind,
            summary,
        });
        out
    }

    /// Closes calls the agent never finished (cancel, crash, end of turn).
    pub fn close_open_calls(&mut self, interrupted: bool) -> Vec<AgentEvent> {
        let mut out = Vec::new();
        for id in &self.order {
            if let Some(call) = self.calls.get_mut(id)
                && !call.finished
            {
                call.finished = true;
                if interrupted && call.output.is_empty() {
                    call.output = "Interrupted.".to_string();
                }
                out.push(finished_event(id, call, !interrupted));
            }
        }
        out
    }

    /// Usage reported with the prompt response.
    pub fn usage(usage: &agent_client_protocol::schema::v1::Usage) -> AgentEvent {
        AgentEvent::Usage(Usage {
            input_tokens: usage.input_tokens,
            cached_input_tokens: usage.cached_read_tokens.unwrap_or(0),
            output_tokens: usage.output_tokens,
            reasoning_tokens: usage.thought_tokens.unwrap_or(0),
        })
    }
}

fn finished_event(id: &str, call: &Call, success: bool) -> AgentEvent {
    AgentEvent::ToolCallFinished {
        call_id: id.to_string(),
        output: head_tail(&call.output, OUTPUT_CHARS),
        exit_code: call.exit_code,
        success,
        diff: call.diff.clone(),
        duration_ms: u64::try_from(call.started.elapsed().as_millis()).unwrap_or(u64::MAX),
    }
}

/// flint's kind for an ACP tool kind.
pub fn map_kind(kind: AcpKind) -> ToolKind {
    match kind {
        AcpKind::Read => ToolKind::Read,
        AcpKind::Edit | AcpKind::Delete | AcpKind::Move => ToolKind::Edit,
        AcpKind::Search | AcpKind::Fetch => ToolKind::Search,
        AcpKind::Execute => ToolKind::Command,
        AcpKind::Think | AcpKind::SwitchMode | AcpKind::Other => ToolKind::Other,
        _ => ToolKind::Other,
    }
}

fn kind_name(kind: ToolKind) -> &'static str {
    match kind {
        ToolKind::Command => "execute",
        ToolKind::Read => "read",
        ToolKind::Edit => "edit",
        ToolKind::Search => "search",
        ToolKind::Other => "tool",
    }
}

fn text_of(block: &ContentBlock) -> Option<String> {
    match block {
        ContentBlock::Text(text) if !text.text.is_empty() => Some(text.text.clone()),
        ContentBlock::ResourceLink(link) => Some(format!("[{}]({})", link.name, link.uri)),
        _ => None,
    }
}

/// Text of a raw tool output (adapters send strings or `{output|stdout}`).
fn raw_output_text(raw: &Value) -> String {
    match raw {
        Value::String(text) => text.clone(),
        Value::Object(map) => ["output", "stdout", "formatted_output", "aggregated_output"]
            .iter()
            .find_map(|key| map.get(*key).and_then(Value::as_str))
            .map_or_else(|| raw.to_string(), str::to_string),
        Value::Null => String::new(),
        Value::Bool(_) | Value::Number(_) | Value::Array(_) => raw.to_string(),
    }
}

/// Workspace-relative display path.
pub fn relative(workspace: &Path, path: &Path) -> String {
    path.strip_prefix(workspace)
        .unwrap_or(path)
        .display()
        .to_string()
}

#[cfg(test)]
#[path = "mapper_tests.rs"]
mod tests;
