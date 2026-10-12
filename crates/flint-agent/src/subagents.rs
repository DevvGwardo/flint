//! Zed-style isolated child sessions: final-answer results, resumable ids,
//! model override/default/parent precedence, and a single delegation level.
//! Adapted behavior from zed-industries/zed; see NOTICE.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use async_channel::{Receiver, Sender};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use tokio_util::sync::CancellationToken;

use crate::approvals::Approvals;
use crate::mcp::McpHub;
use crate::protocol::{AgentConfig, AgentEvent, ReasoningEffort, TurnEndReason};
use crate::provider::Message;
use crate::session::Session;
use crate::tools::{ToolContext, ToolOutcome, head_tail};

pub(crate) const MAX_PARALLEL_SUBAGENTS: usize = 4;
const MAX_SUBAGENT_SESSIONS: usize = 32;
const OUTPUT_CHARS: usize = 20_000;

#[derive(Clone, Serialize, Deserialize)]
struct Metadata {
    model: String,
    #[serde(default)]
    effort: Option<ReasoningEffort>,
    #[serde(default)]
    worktree: Option<crate::worktree::Checkout>,
}

pub(super) struct Child {
    pub id: String,
    pub model: String,
    effort: Option<ReasoningEffort>,
    pub worktree: Option<crate::worktree::Checkout>,
    pub session: Session,
    events: Receiver<AgentEvent>,
}

pub(super) struct Subagents {
    config: AgentConfig,
    /// Each child gets `tools.child()`: the parent's workspace, sandbox and
    /// undo journal, with its own record of what it has read.
    tools: ToolContext,
    mcp: Option<Arc<McpHub>>,
    children: HashMap<String, Child>,
    saved: HashMap<String, Metadata>,
    active: HashSet<String>,
    active_workspaces: HashSet<std::path::PathBuf>,
    next_id: u64,
}

impl Subagents {
    pub fn set_jev(&mut self, config: Option<crate::JevConfig>) {
        self.config.jev = config.clone();
        for child in self.children.values_mut() {
            child.session.stage_jev(config.clone());
        }
    }

    pub fn new(config: AgentConfig, tools: ToolContext, mcp: Option<Arc<McpHub>>) -> Self {
        let mut saved = HashMap::new();
        let mut next_id = 1;
        if let Some(dir) = &config.session_dir
            && let Ok(entries) = std::fs::read_dir(dir.join("subagents"))
        {
            for entry in entries.flatten() {
                let id = entry.file_name().to_string_lossy().to_string();
                let Some(number) = id
                    .strip_prefix("agent-")
                    .and_then(|n| n.parse::<u64>().ok())
                else {
                    continue;
                };
                next_id = next_id.max(number.saturating_add(1));
                if saved.len() >= MAX_SUBAGENT_SESSIONS {
                    continue;
                }
                if let Ok(bytes) = std::fs::read(entry.path().join("subagent.json"))
                    && let Ok(meta) = serde_json::from_slice::<Metadata>(&bytes)
                    && !meta.model.trim().is_empty()
                {
                    saved.insert(id, meta);
                }
            }
        }
        Self {
            config,
            tools,
            mcp,
            children: HashMap::new(),
            saved,
            active: HashSet::new(),
            active_workspaces: HashSet::new(),
            next_id,
        }
    }

    /// Worktree scope is separate from edit approvals, including after restart.
    pub fn worktree_scope(&self, args: &Map<String, Value>) -> Option<String> {
        if let Some(request) = args.get("worktree").filter(|value| !value.is_null()) {
            return Some(format!("Prepare Git worktree and run subagent: {request}"));
        }
        let id = args.get("session_id")?.as_str()?.trim();
        let checkout = self
            .children
            .get(id)
            .and_then(|child| child.worktree.as_ref())
            .or_else(|| self.saved.get(id).and_then(|meta| meta.worktree.as_ref()))?;
        Some(format!(
            "Resume subagent in Git worktree {} ({})",
            checkout.workspace.display(),
            checkout.branch
        ))
    }

    pub async fn prepare(
        &mut self,
        args: &Map<String, Value>,
        models: Option<&Result<Vec<String>, String>>,
        approvals: Arc<Approvals>,
        effort: Option<ReasoningEffort>,
    ) -> Result<(Child, String), String> {
        let text = |name: &str| -> Result<Option<String>, String> {
            match args.get(name) {
                Some(Value::String(s)) => Ok(Some(s.trim().to_string()).filter(|s| !s.is_empty())),
                None | Some(Value::Null) => Ok(None),
                Some(_) => Err(format!("`{name}` must be a string.")),
            }
        };
        text("label")?.ok_or("`label` is required.")?;
        text("message")?.ok_or("`message` is required.")?;
        let message = args["message"]
            .as_str()
            .expect("validated message")
            .to_string();
        let session_id = text("session_id")?;
        let model = text("model")?;
        let request = crate::worktree::Request::parse(args)?;
        if session_id.is_some() && request.is_some() {
            return Err("worktree cannot be changed when resuming a subagent session.".into());
        }
        if session_id.is_some() && model.is_some() {
            return Err("model cannot be changed when resuming a subagent session.".to_string());
        }
        if let Some(id) = session_id {
            if self.active.contains(&id) {
                return Err(format!("Subagent {id} is already running in this batch."));
            }
            let checkout = self
                .children
                .get(&id)
                .and_then(|child| child.worktree.clone())
                .or_else(|| self.saved.get(&id).and_then(|meta| meta.worktree.clone()));
            if let Some(checkout) = &checkout {
                crate::worktree::validate(checkout).await?;
                if self.active_workspaces.contains(&checkout.workspace) {
                    return Err("Another subagent is already using this worktree.".into());
                }
            }
            let child = if let Some(child) = self.children.remove(&id) {
                child
            } else if let Some(meta) = self.saved.get(&id) {
                self.create(
                    id.clone(),
                    meta.model.clone(),
                    approvals,
                    meta.effort,
                    checkout,
                )?
            } else {
                return Err(format!("No subagent session found with id {id}."));
            };
            if let Some(checkout) = &child.worktree {
                self.active_workspaces.insert(checkout.workspace.clone());
            }
            self.active.insert(id);
            return Ok((child, message));
        }
        if self.children.len()
            + self
                .saved
                .keys()
                .filter(|id| !self.children.contains_key(*id))
                .count()
            + self
                .active
                .iter()
                .filter(|id| !self.saved.contains_key(*id))
                .count()
            >= MAX_SUBAGENT_SESSIONS
        {
            return Err(
                "Subagent session limit (32) reached; resume an existing session.".to_string(),
            );
        }
        if let Some(model) = &model {
            let ids = models
                .ok_or("Call list_models before selecting a model.")?
                .as_ref()
                .map_err(|error| format!("{error} Cannot validate explicit model `{model}`."))?;
            if !ids.contains(model) {
                return Err(format!(
                    "Model `{model}` is unavailable. Call list_models for available ids."
                ));
            }
        }
        let model = model
            .or_else(|| {
                self.config
                    .subagent_model
                    .as_ref()
                    .map(|model| model.trim().to_string())
                    .filter(|model| !model.is_empty())
            })
            .unwrap_or_else(|| self.config.model.clone());
        let checkout = if let Some(request) = request {
            if self.config.general && request.repository.is_none() {
                return Err(
                    "General-agent worktree tasks need an absolute `worktree.repository`.".into(),
                );
            }
            let checkout = crate::worktree::prepare(
                &request,
                &self.config.workspace,
                &crate::worktree::home()?,
            )
            .await?;
            if self.active_workspaces.contains(&checkout.workspace) {
                return Err("Another subagent is already using this worktree.".into());
            }
            Some(checkout)
        } else {
            None
        };
        let id = format!("agent-{}", self.next_id);
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or("Subagent id limit reached.")?;
        let child = self.create(id.clone(), model, approvals, effort, checkout)?;
        if let Some(checkout) = &child.worktree {
            self.active_workspaces.insert(checkout.workspace.clone());
        }
        self.active.insert(id);
        Ok((child, message))
    }

    fn create(
        &self,
        id: String,
        model: String,
        approvals: Arc<Approvals>,
        effort: Option<ReasoningEffort>,
        worktree: Option<crate::worktree::Checkout>,
    ) -> Result<Child, String> {
        let mut config = self.config.clone();
        config.model = model.clone();
        config.subagent_model = None;
        config.reasoning_effort = effort;
        if let Some(checkout) = &worktree {
            config.workspace = checkout.workspace.clone();
            config.general = false;
        }
        config.session_dir = config
            .session_dir
            .map(|dir| dir.join("subagents").join(&id));
        if let Some(dir) = &config.session_dir {
            std::fs::create_dir_all(dir).map_err(|err| format!("Cannot save subagent: {err}"))?;
            let bytes = serde_json::to_vec(&Metadata {
                model: model.clone(),
                effort,
                worktree: worktree.clone(),
            })
            .expect("metadata");
            let tmp = dir.join("subagent.json.tmp");
            std::fs::write(&tmp, bytes)
                .and_then(|()| std::fs::rename(tmp, dir.join("subagent.json")))
                .map_err(|err| format!("Cannot save subagent: {err}"))?;
        }
        let (events, events_rx) = async_channel::unbounded();
        let tools = if worktree.is_some() {
            let mut tools = ToolContext::new(config.workspace.clone(), config.sandbox);
            // Scope restrictions still propagate immediately across the family,
            // but this checkout has its own confined file/undo journal.
            tools.work_clock = self.tools.work_clock.clone();
            tools
        } else {
            self.tools.child()
        };
        let mut session = Session::new(
            config,
            events,
            approvals,
            Arc::new(Mutex::new(effort)),
            tools,
            // Parent MCP servers were started in its workspace. Do not expose
            // them as if they were scoped to this isolated checkout.
            if worktree.is_some() {
                None
            } else {
                self.mcp.clone()
            },
            true,
        );
        session.call_prefix = format!("{id}:");
        Ok(Child {
            id,
            model,
            effort,
            worktree,
            session,
            events: events_rx,
        })
    }

    pub fn put(&mut self, child: Child) {
        self.active.remove(&child.id);
        if let Some(checkout) = &child.worktree {
            self.active_workspaces.remove(&checkout.workspace);
        }
        self.saved.insert(
            child.id.clone(),
            Metadata {
                model: child.model.clone(),
                effort: child.effort,
                worktree: child.worktree.clone(),
            },
        );
        self.children.insert(child.id.clone(), child);
    }

    pub async fn flush(self) -> std::io::Result<()> {
        let mut result = Ok(());
        for (_, mut child) in self.children {
            if let Some(saver) = child.session.saver.take()
                && let Err(err) = saver.flush().await
            {
                result = Err(err);
            }
        }
        result
    }
}

pub(super) struct ChildResult {
    pub child: Child,
    pub outcome: ToolOutcome,
    pub work: Vec<AgentEvent>,
}

pub(super) async fn run_child(
    mut child: Child,
    message: String,
    call_id: &str,
    events: Sender<AgentEvent>,
    cancel: &CancellationToken,
    read_only: bool,
    restrictions: Vec<String>,
) -> ChildResult {
    child.session.inherited_read_only = read_only;
    child.session.inherited_restrictions = restrictions;
    let rx = child.events.clone();
    child.session.initialize_child_budget(cancel).await;
    let mut work = Vec::new();
    let mut forward = |event: AgentEvent| {
        if matches!(
            event,
            AgentEvent::ToolCallStarted { .. } | AgentEvent::ToolCallFinished { .. }
        ) {
            work.push(event.clone());
        }
        let event = if matches!(event, AgentEvent::ApprovalRequested { .. }) {
            // The parent's pinned approval controls answer all child calls.
            event
        } else {
            AgentEvent::SubagentEvent {
                call_id: call_id.to_string(),
                event: Box::new(event),
            }
        };
        let _ = events.try_send(event);
    };
    // Boxing breaks the parent/child loop's recursive future type. Children
    // do not get spawn_agent, just as Zed limits delegation to one level.
    let reason = {
        let mut turn = Box::pin(child.session.run_turn(message, cancel));
        loop {
            tokio::select! {
                reason = &mut turn => break reason,
                event = rx.recv() => {
                    if let Ok(event) = event {
                        forward(event);
                    }
                }
            }
        }
    };
    while let Ok(event) = rx.try_recv() {
        forward(event);
    }
    let execution_success = reason == TurnEndReason::Completed;
    let assessment = child
        .session
        .task
        .as_ref()
        .and_then(|task| task.assessment.as_ref());
    // A successfully returned report is not accepted child work. NeedsReview
    // is a usable report, but blocked/partial work is not task success.
    let success = execution_success
        && assessment.is_some_and(|assessment| {
            matches!(
                assessment.outcome,
                crate::task::TaskOutcome::Answered
                    | crate::task::TaskOutcome::NeedsReview
                    | crate::task::TaskOutcome::Accepted
            )
        });
    let output = child
        .session
        .history
        .iter()
        .rev()
        .find_map(|message| match message {
            Message::Assistant {
                content,
                tool_calls,
                ..
            } if tool_calls.is_empty() => Some(content.as_str()),
            _ => None,
        })
        .unwrap_or("");
    let result = if execution_success {
        json!({
            "session_id": child.id,
            "workspace": child.worktree.as_ref().map(|checkout| &checkout.workspace),
            "branch": child.worktree.as_ref().map(|checkout| &checkout.branch),
            "output": head_tail(output, OUTPUT_CHARS),
            "execution_success": true,
            "accepted": assessment.is_some_and(|assessment| assessment.outcome == crate::task::TaskOutcome::Accepted),
            "assessment": assessment,
            "deliverables": child.session.task.as_ref().map(|task| &task.files),
            "blockers": child.session.task.as_ref().map(|task| &task.blockers),
        })
    } else {
        json!({"session_id": child.id, "workspace": child.worktree.as_ref().map(|checkout| &checkout.workspace), "branch": child.worktree.as_ref().map(|checkout| &checkout.branch), "execution_success":false, "accepted":false, "assessment":assessment, "error": match reason {
            TurnEndReason::Interrupted => "Subagent interrupted.".to_string(),
            TurnEndReason::StepLimit => "Subagent reached the step limit.".to_string(),
            TurnEndReason::ContinuationLimit => child.session.task.as_ref()
                .and_then(|task| task.continuation_stop.as_ref())
                .map(crate::task::ContinuationStop::detail)
                .unwrap_or_else(|| "Subagent stopped with unresolved finish conditions.".to_string()),
            TurnEndReason::Failed(error) => error,
            TurnEndReason::Completed => unreachable!(),
        }})
    };
    ChildResult {
        child,
        outcome: ToolOutcome {
            output: result.to_string(),
            exit_code: None,
            success,
            diff: None,
        },
        work,
    }
}
