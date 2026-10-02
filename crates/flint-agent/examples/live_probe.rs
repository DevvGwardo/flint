//! Live measurements against the local Surplus shim (deepseek-v4.1-flash):
//! time to first token, reasoning_effort, and provider cache hits across a
//! context compaction.
//!
//! cargo run -p flint-agent --release --example live_probe -- [ttft|effort|cache]...

use std::path::PathBuf;
use std::time::Duration;

use flint_agent::AgentConfig;
use flint_agent::AgentEvent;
use flint_agent::Op;
use flint_agent::ReasoningEffort;
use flint_agent::provider::ChatRequest;
use flint_agent::provider::Completion;
use flint_agent::provider::Message;
use flint_agent::provider::Provider;
use tokio_util::sync::CancellationToken;

fn percentile(values: &mut [f64], p: f64) -> f64 {
    values.sort_by(f64::total_cmp);
    let rank = ((values.len() - 1) as f64 * p).round() as usize;
    values[rank]
}

fn ms(d: Option<Duration>) -> f64 {
    d.map_or(f64::NAN, |d| d.as_secs_f64() * 1000.0)
}

async fn call(provider: &Provider, prompt: &str, effort: Option<ReasoningEffort>) -> Completion {
    let messages = vec![
        Message::System("You are a concise assistant.".to_string()),
        Message::User(prompt.to_string()),
    ];
    let request = ChatRequest {
        messages: &messages,
        replay_reasoning_from: messages.len(),
        tools_json: "",
        effort,
    };
    provider
        .complete(&request, &mut |_| {}, &CancellationToken::new())
        .await
        .expect("completion")
}

async fn ttft(provider: &Provider) {
    let (mut headers, mut first_byte, mut first_delta, mut total) =
        (vec![], vec![], vec![], vec![]);
    for i in 0..5 {
        let c = call(
            provider,
            &format!("Name three rivers in Europe. ({i})"),
            None,
        )
        .await;
        eprintln!(
            "  call {i}: headers {:.0}ms, first byte {:.0}ms, first delta {:.0}ms, total {:.0}ms",
            ms(c.timing.headers),
            ms(c.timing.first_byte),
            ms(c.timing.first_delta),
            c.timing.total.as_secs_f64() * 1000.0
        );
        headers.push(ms(c.timing.headers));
        first_byte.push(ms(c.timing.first_byte));
        first_delta.push(ms(c.timing.first_delta));
        total.push(c.timing.total.as_secs_f64() * 1000.0);
    }
    for (name, values) in [
        ("headers", &mut headers),
        ("first byte", &mut first_byte),
        ("first delta", &mut first_delta),
        ("total", &mut total),
    ] {
        println!(
            "TTFT {name}: median {:.0}ms, p95 {:.0}ms",
            percentile(values, 0.5),
            percentile(values, 0.95)
        );
    }
}

async fn effort(provider: &Provider, hard: bool) {
    let prompt = if hard {
        "Find all integer pairs (x, y) with 1 <= x <= y <= 50 such that x*y + x + y is a perfect \
         square. How many pairs are there? Answer with the number only."
    } else {
        "A train leaves at 9:17 and arrives at 13:05 after crossing two time zones eastward \
         (+1h each). How long was the trip in minutes? Answer with the number only."
    };
    for level in [
        None,
        Some(ReasoningEffort::Low),
        Some(ReasoningEffort::Medium),
        Some(ReasoningEffort::High),
    ] {
        let (mut chars, mut out_tokens, mut latency) = (vec![], vec![], vec![]);
        let mut answers = Vec::new();
        for _ in 0..3 {
            let c = call(provider, prompt, level).await;
            chars.push(c.reasoning.chars().count() as f64);
            out_tokens.push(c.usage.map_or(0, |u| u.output_tokens) as f64);
            latency.push(c.timing.total.as_secs_f64() * 1000.0);
            answers.push(c.text.trim().to_string());
        }
        println!(
            "EFFORT {:<6}: reasoning chars median {:.0} {chars:?}, output tokens median {:.0}, latency median {:.0}ms, answers {answers:?}, accepted={}",
            level.map_or("none", ReasoningEffort::as_str),
            percentile(&mut chars.clone(), 0.5),
            percentile(&mut out_tokens, 0.5),
            percentile(&mut latency, 0.5),
            provider.effort_supported()
        );
    }
}

/// A real multi-turn session with a small budget so compaction happens;
/// prints per-call cache hit rates around it.
async fn cache() {
    let dir = tempfile::tempdir().expect("tempdir");
    for i in 0..8 {
        let body: String = (0..300)
            .map(|n| format!("record {i}-{n}: value {}\n", n * 7 % 13))
            .collect();
        std::fs::write(dir.path().join(format!("data{i}.txt")), body).expect("write");
    }
    let mut config = AgentConfig::surplus_default(PathBuf::from(dir.path())).expect("config");
    config.context_budget_tokens = 14_000;
    let handle = flint_agent::spawn_session(config);
    let mut rows = Vec::new();
    for i in 0..8 {
        handle
            .ops
            .send(Op::UserMessage(format!(
                "Read data{i}.txt with read_file and tell me how many records have value 3. Don't run commands."
            )))
            .await
            .expect("send");
        let mut previous = flint_agent::Usage::default();
        loop {
            match handle.events.recv().await.expect("event") {
                AgentEvent::Usage(u) => {
                    let input = u.input_tokens - previous.input_tokens;
                    let cached = u.cached_input_tokens - previous.cached_input_tokens;
                    rows.push(format!(
                        "turn {i}: prompt {input} tokens, cached {cached} ({:.0}%)",
                        100.0 * cached as f64 / input.max(1) as f64
                    ));
                    previous = u;
                }
                AgentEvent::ContextCompacted {
                    before_tokens,
                    after_tokens,
                } => {
                    rows.push(format!(
                        "---- compacted {before_tokens} -> {after_tokens} (estimate)"
                    ));
                }
                AgentEvent::TurnFinished { reason, .. } => {
                    rows.push(format!("turn {i} finished {reason:?}"));
                    break;
                }
                AgentEvent::Error(message) => rows.push(format!("error: {message}")),
                _ => {}
            }
        }
    }
    let _ = handle.ops.send(Op::Shutdown).await;
    for row in rows {
        println!("CACHE {row}");
    }
}

#[tokio::main]
async fn main() {
    let config = AgentConfig::surplus_default(std::env::temp_dir()).expect("config");
    let provider = Provider::new(&config.base_url, &config.model, &config.api_key);
    let modes: Vec<String> = std::env::args().skip(1).collect();
    let all = modes.is_empty();
    if all || modes.iter().any(|m| m == "ttft") {
        ttft(&provider).await;
    }
    if all || modes.iter().any(|m| m == "effort") {
        effort(&provider, false).await;
    }
    if modes.iter().any(|m| m == "effort-hard") {
        effort(&provider, true).await;
    }
    if all || modes.iter().any(|m| m == "cache") {
        cache().await;
    }
}
