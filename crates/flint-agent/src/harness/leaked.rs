//! Detects tool calls that a model wrote into its text instead of calling the
//! tool: `<invoke name="run_command">`, `<function=read_file>`,
//! `<tool_call>{…}</tool_call>`, or a bare `{"name": …, "arguments": …}`
//! block. Only offered tool names count, so prose mentioning a tool is not a
//! leak.

use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

/// A tool call found in assistant text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeakedCall {
    pub name: String,
    pub form: LeakForm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeakForm {
    Xml,
    FunctionTag,
    Json,
}

static XML_INVOKE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)<\s*(?:invoke|function_call|tool_use)\b[^>]*\bname\s*=\s*["']?([A-Za-z0-9_.:-]+)"#,
    )
    .expect("valid regex")
});
static FUNCTION_TAG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)<\s*function\s*=\s*["']?([A-Za-z0-9_.:-]+)"#).expect("valid regex")
});
static TOOL_CALL_BLOCK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?is)<\s*tool_call\s*>(.*?)(?:<\s*/\s*tool_call\s*>|$)").expect("valid regex")
});

/// The first leaked call to one of `tool_names` in `text`, if any.
pub fn detect_leaked_tool_call(text: &str, tool_names: &[&str]) -> Option<LeakedCall> {
    if tool_names.is_empty() || text.trim().is_empty() {
        return None;
    }
    let offered = |name: &str| tool_names.contains(&name);
    for caps in XML_INVOKE.captures_iter(text) {
        if offered(&caps[1]) {
            return Some(LeakedCall {
                name: caps[1].to_string(),
                form: LeakForm::Xml,
            });
        }
    }
    for caps in FUNCTION_TAG.captures_iter(text) {
        if offered(&caps[1]) {
            return Some(LeakedCall {
                name: caps[1].to_string(),
                form: LeakForm::FunctionTag,
            });
        }
    }
    for caps in TOOL_CALL_BLOCK.captures_iter(text) {
        if let Some(name) = json_call_name(&caps[1], tool_names) {
            return Some(LeakedCall {
                name,
                form: LeakForm::Json,
            });
        }
    }
    balanced_json_objects(text)
        .into_iter()
        .find_map(|candidate| json_call_name(candidate, tool_names))
        .map(|name| LeakedCall {
            name,
            form: LeakForm::Json,
        })
}

/// The nudge sent when a call leaked into text.
pub fn leaked_call_nudge(call: &LeakedCall) -> String {
    format!(
        "You wrote a `{}` call as text instead of calling the tool, so nothing ran. \
         Call the tool through the tool-calling interface, then continue.",
        call.name
    )
}

fn json_call_name(candidate: &str, tool_names: &[&str]) -> Option<String> {
    let Ok(Value::Object(record)) = serde_json::from_str::<Value>(candidate.trim()) else {
        return None;
    };
    let function = record.get("function").and_then(Value::as_object);
    let name = record
        .get("name")
        .or_else(|| record.get("tool"))
        .or_else(|| function.and_then(|f| f.get("name")))
        .and_then(Value::as_str)?;
    if !tool_names.contains(&name) {
        return None;
    }
    // A schema declaration (`parameters: {type: "object", properties}`) is not a call.
    if let Some(params) = record.get("parameters").and_then(Value::as_object)
        && params.get("type").and_then(Value::as_str) == Some("object")
        && params.contains_key("properties")
    {
        return None;
    }
    let has_args = ["arguments", "args", "input", "parameters"]
        .iter()
        .any(|key| record.contains_key(*key))
        || function.is_some();
    has_args.then(|| name.to_string())
}

/// Top-level balanced `{…}` regions, string- and escape-aware.
fn balanced_json_objects(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut start = None;
    let mut in_string = false;
    let mut escaped = false;
    for (i, ch) in text.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' if depth > 0 => in_string = true,
            '{' => {
                if depth == 0 {
                    start = Some(i);
                }
                depth += 1;
            }
            '}' if depth > 0 => {
                depth -= 1;
                if depth == 0
                    && let Some(s) = start.take()
                {
                    out.push(&text[s..=i]);
                }
            }
            _ => {}
        }
    }
    out
}

#[cfg(test)]
#[path = "leaked_tests.rs"]
mod tests;
