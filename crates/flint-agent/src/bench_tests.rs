//! Timed measurements of engine overhead. Skipped unless `FLINT_BENCH=1`:
//!
//! FLINT_BENCH=1 cargo test -p flint-agent --release bench_ -- --nocapture --test-threads=1

use std::time::Duration;
use std::time::Instant;

use serde_json::Value;
use serde_json::json;

use crate::provider::Message;
use crate::provider::RawToolCall;
use crate::provider::stream::CompletionBuilder;
use crate::provider::stream::SseParser;

pub(crate) fn enabled() -> bool {
    std::env::var("FLINT_BENCH").as_deref() == Ok("1")
}

/// Runs `f` `iterations` times and returns the median duration.
pub(crate) fn median<F: FnMut()>(iterations: usize, mut f: F) -> Duration {
    let mut samples: Vec<Duration> = (0..iterations)
        .map(|_| {
            let start = Instant::now();
            f();
            start.elapsed()
        })
        .collect();
    samples.sort();
    samples[samples.len() / 2]
}

/// A realistic long history: `turns` user turns, each with a tool call, a
/// 4k-char tool output, reasoning and a final answer.
pub(crate) fn synthetic_history(turns: usize) -> Vec<Message> {
    let mut history = vec![Message::System("system prompt ".repeat(200))];
    for turn in 0..turns {
        history.push(Message::User(format!(
            "turn {turn}: {}",
            "please fix the parser ".repeat(10)
        )));
        history.push(Message::Assistant {
            content: String::new(),
            reasoning: "Let me look at the file first. ".repeat(20),
            tool_calls: vec![RawToolCall {
                id: format!("call_{turn}"),
                name: "read_file".to_string(),
                arguments: json!({"path": format!("src/file_{turn}.rs")}).to_string(),
            }],
        });
        history.push(Message::Tool {
            call_id: format!("call_{turn}"),
            content: format!("{:>4}\tfn example() {{ let x = {turn}; }}\n", 1).repeat(100),
        });
        history.push(Message::Assistant {
            content: "Fixed the parser and ran the tests; all pass. ".repeat(6),
            reasoning: String::new(),
            tool_calls: Vec::new(),
        });
    }
    history
}

pub(crate) fn sse_body(content_events: usize) -> Vec<u8> {
    let mut body = String::new();
    for i in 0..content_events {
        let chunk = if i % 2 == 0 {
            json!({"choices": [{"delta": {"content": "token "}}]})
        } else {
            json!({"choices": [{"delta": {"reasoning_content": "think "}}]})
        };
        body.push_str(&format!("data: {chunk}\n\n"));
    }
    for _ in 0..content_events / 10 {
        let chunk = json!({"choices": [{"delta": {"tool_calls": [{"index": 0, "function": {"arguments": "{\\\"a\\\":1"}}]}}]});
        body.push_str(&format!("data: {chunk}\n\n"));
    }
    body.push_str("data: [DONE]\n\n");
    body.into_bytes()
}

#[test]
fn bench_sse_parse_throughput() {
    if !enabled() {
        return;
    }
    let body = sse_body(50_000);
    let events = 55_000;
    let elapsed = median(5, || {
        let mut parser = SseParser::default();
        let mut builder = CompletionBuilder::default();
        for piece in body.chunks(4096) {
            for payload in parser.push(piece) {
                if payload == "[DONE]" {
                    continue;
                }
                if let Ok(chunk) = serde_json::from_str::<Value>(&payload) {
                    builder.apply(&chunk);
                }
            }
        }
        std::hint::black_box(builder.finish());
    });
    let mb = body.len() as f64 / 1_048_576.0;
    eprintln!(
        "BENCH sse_parse: {:.1} MB in {elapsed:?} -> {:.0} MB/s, {:.2} M events/s",
        mb,
        mb / elapsed.as_secs_f64(),
        events as f64 / elapsed.as_secs_f64() / 1e6
    );
}

#[test]
fn bench_sse_coalesced_events() {
    if !enabled() {
        return;
    }
    // Network readers can coalesce many events in one chunk. The original
    // parser moved the remaining chunk once for every line it consumed.
    let body = b"data: token\n\n".repeat(20_000);
    let baseline = || {
        let mut buf = String::from_utf8_lossy(&body).into_owned();
        let mut data = Vec::new();
        let mut out = Vec::new();
        while let Some(pos) = buf.find('\n') {
            let line: String = buf.drain(..=pos).collect();
            let line = line.trim_end_matches(['\n', '\r']);
            if line.is_empty() {
                if !data.is_empty() {
                    out.push(data.join("\n"));
                    data.clear();
                }
            } else if let Some(rest) = line.strip_prefix("data:") {
                data.push(rest.strip_prefix(' ').unwrap_or(rest).to_string());
            }
        }
        out
    };
    assert_eq!(baseline(), SseParser::default().push(&body));
    let before = median(5, || {
        std::hint::black_box(baseline());
    });
    let after = median(5, || {
        std::hint::black_box(SseParser::default().push(&body));
    });
    eprintln!("BENCH sse_coalesced events=20000: before {before:?} -> after {after:?}");
}

#[test]
fn bench_request_build() {
    if !enabled() {
        return;
    }
    let tools = crate::tools::tool_specs();
    let tools_json = serde_json::to_string(&tools).unwrap_or_default();
    for turns in [20, 200] {
        let history = synthetic_history(turns);
        let before = median(20, || {
            std::hint::black_box(baseline_request_body(&history, &tools));
        });
        let request = crate::provider::ChatRequest {
            messages: &history,
            replay_reasoning_from: history.len(),
            tools_json: &tools_json,
            effort: None,
        };
        let after = median(20, || {
            std::hint::black_box(request.body("m"));
        });
        let kb = request.body("m").len() / 1024;
        eprintln!(
            "BENCH request_build turns={turns} ({kb} KB): before {before:?} -> after {after:?}"
        );
    }
}

#[test]
fn bench_history_save_load() {
    if !enabled() {
        return;
    }
    let dir = tempfile::tempdir().expect("tempdir");
    for turns in [20, 200] {
        let history = synthetic_history(turns);
        let snapshot_time = median(20, || {
            std::hint::black_box(crate::persist::snapshot(turns as u64, &history));
        });
        let bytes = crate::persist::snapshot(turns as u64, &history);
        let write_time = median(20, || {
            crate::persist::write_atomic(dir.path(), &bytes).expect("write");
        });
        let load_time = median(20, || {
            std::hint::black_box(crate::persist::load(dir.path()).expect("load"));
        });
        eprintln!(
            "BENCH history turns={turns} ({} KB): snapshot {snapshot_time:?} (on the session task), write {write_time:?} (background), load {load_time:?}",
            bytes.len() / 1024
        );
    }
}

/// The request path as first written: history -> Vec<Value> (clones every
/// string), json! (clones again), then serialize.
fn baseline_request_body(history: &[Message], tools: &[Value]) -> Vec<u8> {
    let wire: Vec<Value> = history.iter().map(|m| m.to_wire(false)).collect();
    let mut body = json!({
        "model": "m",
        "messages": wire.as_slice(),
        "stream": true,
        "stream_options": {"include_usage": true},
    });
    body["tools"] = Value::Array(tools.to_vec());
    body["tool_choice"] = json!("auto");
    serde_json::to_vec(&body).unwrap_or_default()
}
