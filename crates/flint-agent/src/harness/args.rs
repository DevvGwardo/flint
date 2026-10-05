//! Best-effort repair of tool-call argument strings and tool names.
//!
//! Cheap models emit arguments that are *nearly* valid JSON: optional
//! trailing commas and containers left open after complete values. Every
//! repair is validated by re-parsing; when nothing parses, the caller gets the
//! parse error so the model sees a real failure instead of guessed arguments.

use serde_json::Map;
use serde_json::Value;

/// Result of [`repair_tool_args`].
#[derive(Debug, Clone, PartialEq)]
pub enum RepairedArgs {
    Ok {
        value: Map<String, Value>,
        /// True when the arguments differ from what the model sent.
        repaired: bool,
    },
    Invalid {
        error: String,
    },
}

/// Parses and repairs `raw` into an arguments object.
pub fn repair_tool_args(raw: &str) -> RepairedArgs {
    let trimmed = raw.trim();
    // A tool without parameters legitimately streams an empty string.
    if trimmed.is_empty() {
        return RepairedArgs::Ok {
            value: Map::new(),
            repaired: false,
        };
    }
    let direct_error = match serde_json::from_str::<Value>(trimmed) {
        Ok(value) => return finish(value, false),
        Err(err) => err.to_string(),
    };
    for candidate in repair_candidates(trimmed) {
        if let Ok(value) = serde_json::from_str::<Value>(&candidate) {
            return finish(value, true);
        }
    }
    RepairedArgs::Invalid {
        error: direct_error,
    }
}

fn finish(value: Value, repaired: bool) -> RepairedArgs {
    let Value::Object(map) = value else {
        return RepairedArgs::Invalid {
            error: "tool arguments must be a JSON object".to_string(),
        };
    };
    // A JSON-looking string is still a string (especially file content and
    // MCP inputs); null is a valid schema value. Repairs belong to the
    // individual tool's schema, never to arbitrary nested values.
    RepairedArgs::Ok {
        value: map,
        repaired,
    }
}

/// Candidate rewrites, most conservative first.
fn repair_candidates(input: &str) -> Vec<String> {
    let mut candidates = Vec::new();
    let no_trailing = strip_trailing_commas(input);
    if no_trailing != input {
        candidates.push(no_trailing.clone());
    }
    if let Some(closed) = close_unbalanced(&no_trailing) {
        // Closing an object can expose a comma that was trailing at the end.
        let re_stripped = strip_trailing_commas(&closed);
        if re_stripped != no_trailing {
            candidates.push(re_stripped);
        }
    }
    candidates
}

/// Drops a `,` that precedes `}` / `]` or ends the input. String-aware.
pub fn strip_trailing_commas(input: &str) -> String {
    let chars: Vec<char> = input.chars().collect();
    let mut out = String::with_capacity(input.len());
    let mut in_string = false;
    let mut escaped = false;
    for (i, &ch) in chars.iter().enumerate() {
        if in_string {
            out.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        if ch == '"' {
            in_string = true;
            out.push(ch);
            continue;
        }
        if ch == ',' {
            let next = chars[i + 1..].iter().find(|c| !c.is_whitespace());
            if matches!(next, None | Some('}') | Some(']')) {
                continue;
            }
        }
        out.push(ch);
    }
    out
}

/// Appends the closers needed to balance a truncated value. Returns `None`
/// when already balanced or when a mismatched closer shows deeper damage.
pub fn close_unbalanced(input: &str) -> Option<String> {
    let mut stack = Vec::new();
    let mut in_string = false;
    let mut escaped = false;
    for ch in input.chars() {
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
            '"' => in_string = true,
            '{' => stack.push('}'),
            '[' => stack.push(']'),
            '}' | ']' if stack.pop() != Some(ch) => return None,
            _ => {}
        }
    }
    if in_string || stack.is_empty() {
        return None;
    }
    let mut closed = input.to_string();
    while let Some(closer) = stack.pop() {
        closed.push(closer);
    }
    Some(closed)
}

/// `ReadFile`, `read-file`, `functions.read_file` -> `read_file` when offered.
pub fn closest_tool_name<'a>(name: &str, tool_names: &[&'a str]) -> Option<&'a str> {
    let key = normalize_name(name);
    tool_names
        .iter()
        .copied()
        .find(|candidate| normalize_name(candidate) == key)
}

fn normalize_name(name: &str) -> String {
    let last = name.rsplit(['.', ':', '/']).next().unwrap_or(name);
    let mut out = String::with_capacity(last.len() + 4);
    let mut prev: Option<char> = None;
    for ch in last.chars() {
        if ch.is_ascii_uppercase()
            && prev.is_some_and(|p| p.is_ascii_lowercase() || p.is_ascii_digit())
        {
            out.push('_');
        }
        out.push(if ch == '-' {
            '_'
        } else {
            ch.to_ascii_lowercase()
        });
        prev = Some(ch);
    }
    out
}

#[cfg(test)]
#[path = "args_tests.rs"]
mod tests;
