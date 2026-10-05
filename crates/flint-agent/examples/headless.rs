//! Runs one prompt in a directory and prints the event stream compactly.
//!
//! cargo run -p flint-agent --example headless -- <dir> "<prompt>"

use std::io::Write;
use std::path::PathBuf;

use flint_agent::AgentConfig;
use flint_agent::AgentEvent;
use flint_agent::Op;
use flint_agent::TurnEndReason;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let (Some(dir), Some(prompt)) = (args.next(), args.next()) else {
        anyhow::bail!("usage: headless <dir> <prompt>");
    };
    let config = AgentConfig::from_env(PathBuf::from(dir))?;
    eprintln!(
        "model {} at {}  jev={}",
        config.model,
        config.base_url,
        config.jev.is_some()
    );
    let handle = flint_agent::spawn_session(config);
    handle.ops.send(Op::UserMessage(prompt)).await?;
    let mut in_text = false;
    let mut out = std::io::stdout();
    let mut shutdown_sent = false;
    let mut completed = false;
    let mut failure = None;
    let mut deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(900);
    loop {
        let event = match tokio::time::timeout_at(deadline, handle.events.recv()).await {
            Ok(Ok(event)) => event,
            Ok(Err(_)) => anyhow::bail!("session event stream closed before SessionStopped"),
            Err(_) if !shutdown_sent => {
                failure = Some("turn exceeded the 900-second deadline".to_string());
                handle.ops.send(Op::Shutdown).await?;
                shutdown_sent = true;
                deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
                continue;
            }
            Err(_) => anyhow::bail!("session did not acknowledge shutdown within 10 seconds"),
        };
        let line = match &event {
            AgentEvent::TextDelta(text) => {
                in_text = true;
                write!(out, "{text}")?;
                out.flush()?;
                continue;
            }
            AgentEvent::ReasoningDelta(_) | AgentEvent::ToolOutputDelta { .. } => continue,
            AgentEvent::TurnStarted { turn_id } => format!("── turn {turn_id}"),
            AgentEvent::SteeringAccepted { id } => format!("steering accepted: {id}"),
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
            AgentEvent::SessionOptions(options) => format!("options: {}", options.len()),
            AgentEvent::FilesReverted { diffs, skipped, .. } => {
                format!(
                    "↶ reverted {} file(s), skipped {}",
                    diffs.len(),
                    skipped.len()
                )
            }
            AgentEvent::SessionStopped { history_saved } => {
                format!("session stopped: history_saved={history_saved}")
            }
            AgentEvent::SubagentStarted {
                session_id, model, ..
            } => {
                format!("subagent: {session_id} ({model})")
            }
            AgentEvent::SubagentEvent { .. } => continue,
            AgentEvent::TerminalStarted { label, .. } => format!("terminal: {label}"),
            AgentEvent::TerminalOutput { .. } | AgentEvent::TerminalExited { .. } => continue,
        };
        if in_text {
            writeln!(out)?;
            in_text = false;
        }
        writeln!(out, "{line}")?;
        if let AgentEvent::TurnFinished { reason, .. } = &event
            && !shutdown_sent
        {
            completed = matches!(reason, TurnEndReason::Completed);
            if !completed {
                failure = Some(format!("turn failed: {reason:?}"));
            }
            handle.ops.send(Op::Shutdown).await?;
            shutdown_sent = true;
            deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
        }
        if matches!(event, AgentEvent::SessionStopped { .. }) {
            break;
        }
    }
    if let Some(failure) = failure {
        anyhow::bail!(failure);
    }
    anyhow::ensure!(completed, "session stopped without a completed turn");
    Ok(())
}
