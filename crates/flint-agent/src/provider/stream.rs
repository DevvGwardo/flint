//! Pure parsing for Chat Completions streams: SSE framing and assembly of
//! chunk deltas (text, reasoning, tool calls by index, trailing usage).

use std::time::Duration;

use serde_json::Value;

use crate::protocol::Usage;

pub const MAX_SSE_EVENT_BYTES: usize = 1024 * 1024;
pub const MAX_COMPLETION_BYTES: usize = 8 * 1024 * 1024;
const MAX_TOOL_CALLS: usize = 256;

/// Splits a byte stream into SSE `data:` payloads.
#[derive(Debug, Default)]
pub struct SseParser {
    buf: Vec<u8>,
    data: Vec<String>,
    skip_lf: bool,
    data_bytes: usize,
    exceeded: bool,
}

impl SseParser {
    /// Feeds bytes; returns every payload completed by them.
    pub fn push(&mut self, bytes: &[u8]) -> Vec<String> {
        if self.exceeded || self.buf.len().saturating_add(bytes.len()) > MAX_SSE_EVENT_BYTES {
            self.exceeded = true;
            self.buf.clear();
            self.data.clear();
            return Vec::new();
        }
        let previous_len = self.buf.len();
        self.buf.extend_from_slice(bytes);
        let mut out = Vec::new();
        let mut start = 0;
        // Decode complete lines, not network chunks, which may split UTF-8.
        // Scan only new bytes and compact once, avoiding repeated front drains.
        for pos in previous_len..self.buf.len() {
            let byte = self.buf[pos];
            if self.skip_lf {
                self.skip_lf = false;
                if byte == b'\n' {
                    start = pos + 1;
                    continue;
                }
            }
            if !matches!(byte, b'\n' | b'\r') {
                continue;
            }
            let line = String::from_utf8_lossy(&self.buf[start..pos]);
            if line.is_empty() {
                if !self.data.is_empty() {
                    out.push(self.data.join("\n"));
                    self.data.clear();
                    self.data_bytes = 0;
                }
            } else if let Some(rest) = line.strip_prefix("data:") {
                self.data_bytes = self.data_bytes.saturating_add(rest.len() + 1);
                if self.data_bytes > MAX_SSE_EVENT_BYTES {
                    self.exceeded = true;
                    self.data.clear();
                    break;
                }
                self.data
                    .push(rest.strip_prefix(' ').unwrap_or(rest).to_string());
            }
            // `event:`, `id:`, `retry:` and `:` comments carry nothing we need.
            start = pos + 1;
            self.skip_lf = byte == b'\r';
        }
        if start > 0 {
            self.buf.drain(..start);
        }
        out
    }

    /// Flushes a final event that wasn't followed by a blank line.
    pub fn finish(&mut self) -> Option<String> {
        if self.exceeded {
            return None;
        }
        let bytes = std::mem::take(&mut self.buf);
        let line = String::from_utf8_lossy(&bytes);
        self.skip_lf = false;
        if let Some(rest) = line.strip_prefix("data:") {
            if self.data_bytes.saturating_add(rest.len() + 1) > MAX_SSE_EVENT_BYTES {
                self.exceeded = true;
                self.data.clear();
                return None;
            }
            self.data
                .push(rest.strip_prefix(' ').unwrap_or(rest).to_string());
        }
        (!self.data.is_empty()).then(|| std::mem::take(&mut self.data).join("\n"))
    }

    pub fn exceeded(&self) -> bool {
        self.exceeded
    }
}

/// A delta worth showing as it arrives.
#[derive(Debug, Clone, PartialEq)]
pub enum StreamDelta {
    Reasoning(String),
    Text(String),
}

/// A tool call as the model sent it (arguments not yet parsed).
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RawToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

/// Everything one model call produced.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Completion {
    pub text: String,
    pub reasoning: String,
    pub tool_calls: Vec<RawToolCall>,
    pub finish_reason: Option<String>,
    pub usage: Option<Usage>,
    pub timing: Timing,
}

/// Latency of one model call, measured from sending the request.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Timing {
    /// Response headers received.
    pub headers: Option<Duration>,
    /// First body byte (first SSE data).
    pub first_byte: Option<Duration>,
    /// First text or reasoning delta.
    pub first_delta: Option<Duration>,
    pub total: Duration,
}

/// Accumulates chunks into a [`Completion`].
#[derive(Debug, Default)]
pub struct CompletionBuilder {
    completion: Completion,
    /// Wire index -> position in `tool_calls`.
    indices: Vec<(i64, usize)>,
    input_bytes: usize,
    exceeded: bool,
}

impl CompletionBuilder {
    /// Applies one parsed chunk and returns the deltas to display.
    pub fn apply(&mut self, chunk: &Value) -> Vec<StreamDelta> {
        // Includes ids, names, arguments, text and reasoning, not just what is
        // displayed. Account before extending any completion buffer.
        self.input_bytes = self.input_bytes.saturating_add(chunk.to_string().len());
        if self.exceeded || self.input_bytes > MAX_COMPLETION_BYTES {
            self.exceeded = true;
            return Vec::new();
        }
        let mut deltas = Vec::new();
        if let Some(usage) = chunk.get("usage").filter(|u| u.is_object()) {
            self.completion.usage = Some(parse_usage(usage));
        }
        let Some(choice) = chunk.get("choices").and_then(|c| c.get(0)) else {
            return deltas;
        };
        if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
            self.completion.finish_reason = Some(reason.to_string());
        }
        let Some(delta) = choice.get("delta").or_else(|| choice.get("message")) else {
            return deltas;
        };
        let reasoning = delta
            .get("reasoning_content")
            .or_else(|| delta.get("reasoning"))
            .and_then(Value::as_str);
        if let Some(text) = reasoning.filter(|t| !t.is_empty()) {
            self.completion.reasoning.push_str(text);
            deltas.push(StreamDelta::Reasoning(text.to_string()));
        }
        if let Some(text) = delta
            .get("content")
            .and_then(Value::as_str)
            .filter(|t| !t.is_empty())
        {
            self.completion.text.push_str(text);
            deltas.push(StreamDelta::Text(text.to_string()));
        }
        if let Some(calls) = delta.get("tool_calls").and_then(Value::as_array) {
            for (position, call) in calls.iter().enumerate() {
                self.apply_tool_call(position, call);
            }
        }
        deltas
    }

    fn apply_tool_call(&mut self, position: usize, call: &Value) {
        let index = call
            .get("index")
            .and_then(Value::as_i64)
            .unwrap_or(position as i64);
        let id = call
            .get("id")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty());
        let slot = match self.indices.iter().find(|(i, _)| *i == index) {
            // A new id at a known index means the provider reused indices.
            Some((_, slot))
                if id.is_none_or(|id| {
                    let previous = &self.completion.tool_calls[*slot].id;
                    previous.is_empty() || previous == id
                }) =>
            {
                *slot
            }
            Some(_) | None => {
                if self.completion.tool_calls.len() >= MAX_TOOL_CALLS {
                    self.exceeded = true;
                    return;
                }
                self.completion.tool_calls.push(RawToolCall::default());
                let slot = self.completion.tool_calls.len() - 1;
                self.indices.retain(|(i, _)| *i != index);
                self.indices.push((index, slot));
                slot
            }
        };
        let entry = &mut self.completion.tool_calls[slot];
        if let Some(id) = id {
            entry.id = id.to_string();
        }
        if let Some(function) = call.get("function") {
            if let Some(name) = function.get("name").and_then(Value::as_str)
                && entry.name.is_empty()
            {
                entry.name = name.to_string();
            }
            match function.get("arguments") {
                Some(Value::String(args)) => entry.arguments.push_str(args),
                // Some gateways send arguments as an object.
                Some(args @ Value::Object(_)) => entry.arguments.push_str(&args.to_string()),
                Some(Value::Null | Value::Bool(_) | Value::Number(_) | Value::Array(_)) | None => {}
            }
        }
    }

    /// The finished completion; calls without ids get synthetic ones.
    pub fn finish(mut self) -> Completion {
        for (i, call) in self.completion.tool_calls.iter_mut().enumerate() {
            if call.id.is_empty() {
                call.id = format!("call_{i}");
            }
        }
        self.completion.tool_calls.retain(|c| !c.name.is_empty());
        self.completion
    }

    pub fn exceeded(&self) -> bool {
        self.exceeded
    }
}

fn parse_usage(usage: &Value) -> Usage {
    let num = |v: Option<&Value>| v.and_then(Value::as_u64).unwrap_or(0);
    let cached = usage
        .get("prompt_tokens_details")
        .and_then(|d| d.get("cached_tokens"))
        .or_else(|| usage.get("prompt_cache_hit_tokens"));
    Usage {
        input_tokens: num(usage.get("prompt_tokens")),
        cached_input_tokens: num(cached),
        output_tokens: num(usage.get("completion_tokens")),
        reasoning_tokens: num(usage
            .get("completion_tokens_details")
            .and_then(|d| d.get("reasoning_tokens"))),
    }
}

#[cfg(test)]
#[path = "stream_tests.rs"]
mod tests;
