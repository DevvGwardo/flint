//! The "terminal output" ACP extension (used by Zed, Claude Code's and
//! Codex's adapters): when the client sets `clientCapabilities._meta.
//! terminal_output`, adapters report a command's terminal in tool-call
//! `_meta`:
//!
//! - `terminal_info { terminal_id, cwd }` when the command starts,
//! - `terminal_output { terminal_id, data }` (the whole output so far) or
//!   `terminal_output_delta { terminal_id, data }` (new output),
//! - `terminal_exit { terminal_id, exit_code, signal }` when it ends.

use std::path::PathBuf;

use serde_json::Map;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TermMeta {
    Info {
        terminal_id: String,
        cwd: Option<PathBuf>,
    },
    Output {
        terminal_id: String,
        data: String,
        replace: bool,
    },
    Exit {
        terminal_id: String,
        exit_code: Option<i32>,
    },
}

/// The terminal items in a tool call's `_meta`, in start/output/exit order.
pub fn parse(meta: &Map<String, Value>) -> Vec<TermMeta> {
    let id = |v: &Value| {
        v.get("terminal_id")
            .and_then(Value::as_str)
            .map(str::to_string)
    };
    let mut out = Vec::new();
    if let Some(info) = meta.get("terminal_info")
        && let Some(terminal_id) = id(info)
    {
        out.push(TermMeta::Info {
            terminal_id,
            cwd: info.get("cwd").and_then(Value::as_str).map(PathBuf::from),
        });
    }
    for (key, replace) in [("terminal_output", true), ("terminal_output_delta", false)] {
        if let Some(output) = meta.get(key)
            && let Some(terminal_id) = id(output)
        {
            out.push(TermMeta::Output {
                terminal_id,
                data: output
                    .get("data")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                replace,
            });
        }
    }
    if let Some(exit) = meta.get("terminal_exit")
        && let Some(terminal_id) = id(exit)
    {
        out.push(TermMeta::Exit {
            terminal_id,
            exit_code: exit
                .get("exit_code")
                .and_then(Value::as_i64)
                .and_then(|c| i32::try_from(c).ok()),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn parses_info_output_and_exit() {
        let meta = json!({
            "terminal_info": {"terminal_id": "t1", "cwd": "/w"},
            "terminal_output": {"terminal_id": "t1", "data": "ok\n"},
            "terminal_exit": {"terminal_id": "t1", "exit_code": 2, "signal": null},
            "other": 1
        });
        let Value::Object(meta) = meta else {
            panic!("object")
        };
        assert_eq!(
            parse(&meta),
            vec![
                TermMeta::Info {
                    terminal_id: "t1".into(),
                    cwd: Some("/w".into())
                },
                TermMeta::Output {
                    terminal_id: "t1".into(),
                    data: "ok\n".into(),
                    replace: true
                },
                TermMeta::Exit {
                    terminal_id: "t1".into(),
                    exit_code: Some(2)
                },
            ]
        );
        let Value::Object(delta) =
            json!({"terminal_output_delta": {"terminal_id": "t2", "data": "x"}})
        else {
            panic!("object")
        };
        assert_eq!(
            parse(&delta),
            vec![TermMeta::Output {
                terminal_id: "t2".into(),
                data: "x".into(),
                replace: false
            }]
        );
    }
}
