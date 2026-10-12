//! NDJSON bridge used by tools/blueprint/orchestrator.py.
//! Provider credentials belong to the supervising proxy, never this process.

use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::time::Duration;

use flint_agent::{
    AgentConfig, AgentEvent, ApprovalMode, ExecutionOptions, JevConfig, Op, ReasoningEffort,
    spawn_session_with_options,
};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Run {
    workspace: PathBuf,
    base_url: String,
    model: String,
    proxy_key: String,
    prompt: String,
    allow_subagents: bool,
    judge: bool,
    timeout_secs: u64,
    denied_read_canary: PathBuf,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let line = std::io::stdin()
        .lock()
        .lines()
        .next()
        .ok_or_else(|| anyhow::anyhow!("missing run specification"))??;
    let run: Run = serde_json::from_str(&line)?;
    anyhow::ensure!((1..=600).contains(&run.timeout_secs), "invalid deadline");
    anyhow::ensure!(
        matches!(
            std::fs::read(&run.denied_read_canary),
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied
        ),
        "the supervisor must apply the verified outer containment profile"
    );
    // macOS cannot apply a second restrictive sandbox inside this outer one.
    // The outer profile also restricts file tools and network, not just shell
    // writes. This changes no production defaults or persistent settings.
    let mut config = AgentConfig::new(
        run.workspace,
        run.base_url.clone(),
        run.model.clone(),
        run.proxy_key.clone(),
    );
    config.approval = ApprovalMode::Auto;
    config.sandbox = false;
    config.subagent_model = Some(run.model.clone());
    config.reasoning_effort = Some(ReasoningEffort::Low);
    config.context_budget_tokens = 64_000;
    config.jev = run.judge.then(|| JevConfig {
        base_url: run
            .base_url
            .trim_end_matches('/')
            .trim_end_matches("/v1")
            .to_string(),
        model: run.model,
        api_key: run.proxy_key,
    });
    let handle = spawn_session_with_options(
        config,
        ExecutionOptions {
            allow_subagents: run.allow_subagents,
        },
    );
    handle.ops.send(Op::UserMessage(run.prompt)).await?;
    let mut deadline = tokio::time::Instant::now() + Duration::from_secs(run.timeout_secs);
    let mut stopping = false;
    let mut output = std::io::stdout().lock();
    loop {
        let event = match tokio::time::timeout_at(deadline, handle.events.recv()).await {
            Ok(Ok(event)) => event,
            Ok(Err(_)) => anyhow::bail!("event stream closed before shutdown acknowledgement"),
            Err(_) if !stopping => {
                handle.ops.send(Op::Shutdown).await?;
                stopping = true;
                deadline = tokio::time::Instant::now() + Duration::from_secs(5);
                writeln!(output, "{{\"EvalDeadlineExceeded\":true}}")?;
                output.flush()?;
                continue;
            }
            Err(_) => anyhow::bail!("shutdown acknowledgement deadline exceeded"),
        };
        serde_json::to_writer(&mut output, &event)?;
        writeln!(output)?;
        output.flush()?;
        if matches!(event, AgentEvent::TurnFinished { .. }) && !stopping {
            handle.ops.send(Op::Shutdown).await?;
            stopping = true;
            deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        }
        if matches!(event, AgentEvent::SessionStopped { .. }) {
            break;
        }
    }
    Ok(())
}
