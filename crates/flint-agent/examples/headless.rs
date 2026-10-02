//! Runs one prompt in a directory and prints the event stream compactly.
//!
//! cargo run -p flint-agent --example headless -- <dir> "<prompt>"

use std::io::Write;
use std::path::PathBuf;

use flint_agent::AgentConfig;
use flint_agent::AgentEvent;
use flint_agent::Op;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let (Some(dir), Some(prompt)) = (args.next(), args.next()) else {
        anyhow::bail!("usage: headless <dir> <prompt>");
    };
    let config = AgentConfig::surplus_default(PathBuf::from(dir))?;
    eprintln!(
        "model {} at {}  jev={}",
        config.model,
        config.base_url,
        config.jev.is_some()
    );
    let handle = flint_agent::spawn_session(config);
    handle.ops.send_blocking(Op::UserMessage(prompt))?;
    let mut in_text = false;
    let mut out = std::io::stdout();
    while let Ok(event) = handle.events.recv_blocking() {
        let line = match &event {
            AgentEvent::TextDelta(text) => {
                in_text = true;
                write!(out, "{text}")?;
                out.flush()?;
                continue;
            }
            AgentEvent::ReasoningDelta(_) | AgentEvent::ToolOutputDelta { .. } => continue,
            AgentEvent::TurnStarted { turn_id } => format!("── turn {turn_id}"),
            AgentEvent::StepStarted { step, .. } => format!("── step {step}"),
            AgentEvent::ToolCallStarted { name, summary, .. } => format!("▶ {name}: {summary}"),
            AgentEvent::ToolCallFinished {
                success,
                exit_code,
                diff,
                duration_ms,
                output,
                ..
            } => {
                let mark = if *success { "✓" } else { "✗" };
                let diff = diff
                    .as_ref()
                    .map(|d| format!(" +{} -{}", d.added, d.removed))
                    .unwrap_or_default();
                let first = output.lines().last().unwrap_or_default();
                format!("  {mark} {duration_ms}ms exit={exit_code:?}{diff}  {first}")
            }
            AgentEvent::ApprovalRequested { summary, .. } => format!("? approval: {summary}"),
            AgentEvent::HarnessNudge { reason, .. } => format!("⚑ harness nudge: {reason:?}"),
            AgentEvent::ToolRepaired { tool, detail } => format!("⚒ repaired {tool}: {detail}"),
            AgentEvent::Usage(u) => format!(
                "  usage in={} cached={} out={} reasoning={}",
                u.input_tokens, u.cached_input_tokens, u.output_tokens, u.reasoning_tokens
            ),
            AgentEvent::TurnFinished { reason, .. } => format!("── finished: {reason:?}"),
            AgentEvent::Error(message) => format!("! {message}"),
            AgentEvent::ContextCompacted {
                before_tokens,
                after_tokens,
            } => {
                format!("⇣ context compacted {before_tokens} -> {after_tokens} tokens")
            }
        };
        if in_text {
            writeln!(out)?;
            in_text = false;
        }
        writeln!(out, "{line}")?;
        if matches!(event, AgentEvent::TurnFinished { .. }) {
            break;
        }
    }
    let _ = handle.ops.send_blocking(Op::Shutdown);
    Ok(())
}
