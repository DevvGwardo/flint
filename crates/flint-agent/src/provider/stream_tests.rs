use pretty_assertions::assert_eq;
use serde_json::json;

use super::*;

#[test]
fn sse_line_and_event_buffers_have_hard_limits() {
    let mut parser = SseParser::default();
    assert!(parser.push(&vec![b'x'; 1024 * 1024 + 1]).is_empty());
    assert!(parser.buf.is_empty());
    assert_eq!(parser.finish(), None);
    let mut parser = SseParser::default();
    let line = format!("data: {}\n", "x".repeat(64 * 1024));
    for _ in 0..17 {
        parser.push(line.as_bytes());
    }
    assert!(parser.data.is_empty());
}

#[test]
fn completion_and_tool_inventory_have_hard_limits() {
    let mut builder = CompletionBuilder::default();
    let chunk = json!({"choices":[{"delta":{"content":"x".repeat(256 * 1024)}}]});
    for _ in 0..33 {
        builder.apply(&chunk);
    }
    assert!(builder.finish().text.len() <= 8 * 1024 * 1024);
    let mut builder = CompletionBuilder::default();
    for index in 0..=256 {
        builder.apply(&json!({"choices":[{"delta":{"tool_calls":[
            {"index":index,"id":format!("c{index}"),"function":{"name":"read_file","arguments":"{}"}}
        ]}}]}));
    }
    assert_eq!(builder.finish().tool_calls.len(), 256);
}

#[test]
fn sse_frames_across_chunk_boundaries() {
    let mut parser = SseParser::default();
    assert_eq!(parser.push(b"data: {\"a\""), Vec::<String>::new());
    assert_eq!(
        parser.push(b":1}\r\n\r\n: keepalive\n\ndata: [DONE]\n\n"),
        vec!["{\"a\":1}".to_string(), "[DONE]".to_string()]
    );
    assert_eq!(parser.push(b"data: tail"), Vec::<String>::new());
    assert_eq!(parser.finish(), Some("tail".to_string()));
}

#[test]
fn sse_preserves_unicode_at_every_byte_boundary() {
    let body = "data: {\"text\":\"café 日本語 🦀\"}\n\n";
    for split in 0..=body.len() {
        let mut parser = SseParser::default();
        let mut payloads = parser.push(&body.as_bytes()[..split]);
        payloads.extend(parser.push(&body.as_bytes()[split..]));
        assert_eq!(payloads, vec!["{\"text\":\"café 日本語 🦀\"}"]);
        assert_eq!(parser.finish(), None);
    }
    let mut parser = SseParser::default();
    let payloads: Vec<_> = body
        .as_bytes()
        .chunks(1)
        .flat_map(|chunk| parser.push(chunk))
        .collect();
    assert_eq!(payloads, vec!["{\"text\":\"café 日本語 🦀\"}"]);
}

#[test]
fn sse_accepts_all_line_endings_and_preserves_data_spaces() {
    for ending in ["\n", "\r", "\r\n"] {
        let body = format!("data:  one  {ending}data: two{ending}{ending}");
        for split in 0..=body.len() {
            let mut parser = SseParser::default();
            let mut payloads = parser.push(&body.as_bytes()[..split]);
            payloads.extend(parser.push(&body.as_bytes()[split..]));
            assert_eq!(payloads, vec![" one  \ntwo"]);
        }
    }
    let mut parser = SseParser::default();
    parser.push(b"data:  tail  ");
    assert_eq!(parser.finish(), Some(" tail  ".into()));
    assert_eq!(parser.finish(), None);
}

#[test]
fn tool_call_id_can_arrive_after_its_name_and_arguments() {
    let mut builder = CompletionBuilder::default();
    builder.apply(&json!({"choices": [{"delta": {"tool_calls": [
        {"index": 0, "function": {"name": "read_file", "arguments": "{\"path\":"}}
    ]}}]}));
    builder.apply(&json!({"choices": [{"delta": {"tool_calls": [
        {"index": 0, "id": "late-id", "function": {"arguments": "\"a.rs\"}"}}
    ]}}]}));
    assert_eq!(
        builder.finish().tool_calls,
        vec![RawToolCall {
            id: "late-id".into(),
            name: "read_file".into(),
            arguments: "{\"path\":\"a.rs\"}".into(),
        }]
    );
}

#[test]
fn different_tool_call_ids_still_create_separate_calls_at_a_reused_index() {
    let mut builder = CompletionBuilder::default();
    for id in ["first", "second"] {
        builder.apply(&json!({"choices": [{"delta": {"tool_calls": [
            {"index": 0, "id": id, "function": {"name": "read_file", "arguments": "{}"}}
        ]}}]}));
    }
    let calls = builder.finish().tool_calls;
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].id, "first");
    assert_eq!(calls[1].id, "second");
}

#[test]
fn assembles_text_reasoning_tool_calls_and_usage() {
    let chunks = [
        json!({"choices": [{"delta": {"role": "assistant", "reasoning_content": "Think"}}]}),
        json!({"choices": [{"delta": {"reasoning_content": "ing.", "content": "I'll "}}]}),
        json!({"choices": [{"delta": {"content": "write it."}}]}),
        json!({"choices": [{"delta": {"tool_calls": [{"index": 0, "id": "c1", "type": "function", "function": {"name": "write_file", "arguments": "{\"path\":"}}]}}]}),
        json!({"choices": [{"delta": {"tool_calls": [{"index": 0, "function": {"arguments": "\"a.py\"}"}}]}}]}),
        json!({"choices": [{"delta": {"tool_calls": [{"index": 1, "id": "c2", "function": {"name": "run_command", "arguments": "{\"command\":\"ls\"}"}}]}}]}),
        json!({"choices": [{"delta": {}, "finish_reason": "tool_calls"}]}),
        json!({"choices": [], "usage": {"prompt_tokens": 100, "completion_tokens": 20,
            "prompt_tokens_details": {"cached_tokens": 64},
            "completion_tokens_details": {"reasoning_tokens": 7}}}),
    ];
    let mut builder = CompletionBuilder::default();
    let deltas: Vec<StreamDelta> = chunks.iter().flat_map(|c| builder.apply(c)).collect();
    assert_eq!(
        deltas,
        vec![
            StreamDelta::Reasoning("Think".into()),
            StreamDelta::Reasoning("ing.".into()),
            StreamDelta::Text("I'll ".into()),
            StreamDelta::Text("write it.".into()),
        ]
    );
    assert_eq!(
        builder.finish(),
        Completion {
            text: "I'll write it.".into(),
            reasoning: "Thinking.".into(),
            tool_calls: vec![
                RawToolCall {
                    id: "c1".into(),
                    name: "write_file".into(),
                    arguments: "{\"path\":\"a.py\"}".into()
                },
                RawToolCall {
                    id: "c2".into(),
                    name: "run_command".into(),
                    arguments: "{\"command\":\"ls\"}".into()
                },
            ],
            finish_reason: Some("tool_calls".into()),
            usage: Some(Usage {
                input_tokens: 100,
                cached_input_tokens: 64,
                output_tokens: 20,
                reasoning_tokens: 7
            }),
            timing: Timing::default(),
        }
    );
}

#[test]
fn tolerates_missing_index_and_ids() {
    let mut builder = CompletionBuilder::default();
    builder.apply(&json!({"choices": [{"delta": {"tool_calls": [
        {"function": {"name": "read_file", "arguments": {"path": "a"}}},
        {"function": {"name": "list_dir", "arguments": ""}}
    ]}, "finish_reason": "stop"}]}));
    let completion = builder.finish();
    assert_eq!(
        completion.tool_calls,
        vec![
            RawToolCall {
                id: "call_0".into(),
                name: "read_file".into(),
                arguments: "{\"path\":\"a\"}".into()
            },
            RawToolCall {
                id: "call_1".into(),
                name: "list_dir".into(),
                arguments: String::new()
            },
        ]
    );
}

#[test]
fn deepseek_cache_hit_field_counts_as_cached() {
    let mut builder = CompletionBuilder::default();
    builder.apply(&json!({"choices": [], "usage": {"prompt_tokens": 10, "completion_tokens": 1, "prompt_cache_hit_tokens": 8}}));
    assert_eq!(
        builder.finish().usage.map(|u| u.cached_input_tokens),
        Some(8)
    );
}
