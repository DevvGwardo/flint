//! One ACP connection: `initialize`, a new or loaded session, then a prompt
//! per user message, with `session/update`, permission and fs requests
//! handled alongside. Generic over the byte streams so tests can drive it
//! with an in-process fake agent.

use std::collections::HashMap;
use std::collections::VecDeque;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use agent_client_protocol::ByteStreams;
use agent_client_protocol::Client;
use agent_client_protocol::ConnectionTo;
use agent_client_protocol::Responder;
use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::schema::v1::CancelNotification;
use agent_client_protocol::schema::v1::ClientCapabilities;
use agent_client_protocol::schema::v1::ContentBlock;
use agent_client_protocol::schema::v1::FileSystemCapabilities;
use agent_client_protocol::schema::v1::Implementation;
use agent_client_protocol::schema::v1::InitializeRequest;
use agent_client_protocol::schema::v1::LoadSessionRequest;
use agent_client_protocol::schema::v1::NewSessionRequest;
use agent_client_protocol::schema::v1::PermissionOption;
use agent_client_protocol::schema::v1::PermissionOptionKind;
use agent_client_protocol::schema::v1::PromptRequest;
use agent_client_protocol::schema::v1::ReadTextFileRequest;
use agent_client_protocol::schema::v1::ReadTextFileResponse;
use agent_client_protocol::schema::v1::RequestPermissionOutcome;
use agent_client_protocol::schema::v1::RequestPermissionRequest;
use agent_client_protocol::schema::v1::RequestPermissionResponse;
use agent_client_protocol::schema::v1::SelectedPermissionOutcome;
use agent_client_protocol::schema::v1::SessionId;
use agent_client_protocol::schema::v1::SessionNotification;
use agent_client_protocol::schema::v1::StopReason;
use agent_client_protocol::schema::v1::TextContent;
use agent_client_protocol::schema::v1::WriteTextFileRequest;
use agent_client_protocol::schema::v1::WriteTextFileResponse;
use async_channel::Receiver;
use async_channel::Sender;
use flint_agent::AgentEvent;
use flint_agent::ApprovalDecision;
use flint_agent::ApprovalMode;
use flint_agent::Op;
use flint_agent::TurnEndReason;
use futures::AsyncRead;
use futures::AsyncWrite;
use serde::Deserialize;
use serde::Serialize;

use crate::launch::AcpAgent;
use crate::launch::looks_like_auth_error;
use crate::mapper::Mapper;

/// What a runner needs besides the byte streams.
pub(crate) struct RunContext {
    pub agent: AcpAgent,
    pub workspace: PathBuf,
    pub session_dir: Option<PathBuf>,
    pub approval: ApprovalMode,
    pub ops: Receiver<Op>,
    pub events: Sender<AgentEvent>,
    /// Last stderr lines of the adapter, for error messages.
    pub stderr: Arc<Mutex<String>>,
}

/// `session_dir/acp.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Saved {
    agent: String,
    session_id: String,
    turn_id: u64,
}

type PendingPermission = (Responder<RequestPermissionResponse>, Vec<PermissionOption>);

/// State shared by the connection's handlers and the turn loop.
struct Shared {
    agent: AcpAgent,
    workspace: PathBuf,
    events: Sender<AgentEvent>,
    mapper: Mutex<Mapper>,
    pending: Mutex<HashMap<String, PendingPermission>>,
    auto_approve: AtomicBool,
    /// `session/load` replays history as updates; the UI already has it.
    loading: AtomicBool,
}

impl Shared {
    fn emit_all(&self, events: Vec<AgentEvent>) {
        for event in events {
            let _ = self.events.try_send(event);
        }
    }

    fn with_mapper<T>(&self, f: impl FnOnce(&mut Mapper) -> T) -> Option<T> {
        self.mapper.lock().ok().map(|mut mapper| f(&mut mapper))
    }

    /// Answers a pending permission with the option matching `decision`.
    fn resolve(&self, call_id: &str, decision: ApprovalDecision) {
        let Some((responder, options)) =
            self.pending.lock().ok().and_then(|mut p| p.remove(call_id))
        else {
            return;
        };
        if decision == ApprovalDecision::ApproveAlways {
            self.auto_approve.store(true, Ordering::Relaxed);
        }
        let _ = responder.respond(permission_response(&options, decision));
    }

    /// Cancels every unanswered permission (the turn is being cancelled).
    fn cancel_pending(&self) {
        let pending: Vec<PendingPermission> = self
            .pending
            .lock()
            .map(|mut p| p.drain().map(|(_, v)| v).collect())
            .unwrap_or_default();
        for (responder, _) in pending {
            let _ = responder.respond(RequestPermissionResponse::new(
                RequestPermissionOutcome::Cancelled,
            ));
        }
    }
}

/// The ACP outcome for a flint decision: the closest option the agent offered.
pub(crate) fn permission_response(
    options: &[PermissionOption],
    decision: ApprovalDecision,
) -> RequestPermissionResponse {
    let preference: &[PermissionOptionKind] = match decision {
        ApprovalDecision::Approve => &[
            PermissionOptionKind::AllowOnce,
            PermissionOptionKind::AllowAlways,
        ],
        ApprovalDecision::ApproveAlways => &[
            PermissionOptionKind::AllowAlways,
            PermissionOptionKind::AllowOnce,
        ],
        ApprovalDecision::Deny => &[
            PermissionOptionKind::RejectOnce,
            PermissionOptionKind::RejectAlways,
        ],
    };
    let chosen = preference
        .iter()
        .find_map(|kind| options.iter().find(|option| option.kind == *kind));
    let outcome = match chosen {
        Some(option) => RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(
            option.option_id.clone(),
        )),
        None => RequestPermissionOutcome::Cancelled,
    };
    RequestPermissionResponse::new(outcome)
}

/// Runs the connection until Shutdown, the ops sender closing, or the agent
/// going away. Every failure is reported as an `Error` event.
pub(crate) async fn run<R, W>(reader: R, writer: W, ctx: RunContext)
where
    R: AsyncRead + Send + 'static,
    W: AsyncWrite + Send + 'static,
{
    let saved = ctx
        .session_dir
        .as_deref()
        .and_then(load_saved)
        .filter(|s| s.agent == ctx.agent.id());
    let shared = Arc::new(Shared {
        agent: ctx.agent.clone(),
        workspace: ctx.workspace.clone(),
        events: ctx.events.clone(),
        mapper: Mutex::new(Mapper::new(
            &ctx.workspace,
            saved.as_ref().map_or(0, |s| s.turn_id),
        )),
        pending: Mutex::new(HashMap::new()),
        auto_approve: AtomicBool::new(ctx.approval == ApprovalMode::Auto),
        loading: AtomicBool::new(false),
    });

    let on_update = Arc::clone(&shared);
    let on_permission = Arc::clone(&shared);
    let on_read = Arc::clone(&shared);
    let on_write = Arc::clone(&shared);
    let main_shared = Arc::clone(&shared);
    let stderr = Arc::clone(&ctx.stderr);
    let result = Client
        .builder()
        .name("flint")
        .on_receive_notification(
            async move |notification: SessionNotification, _cx| {
                if !on_update.loading.load(Ordering::Relaxed)
                    && let Some(events) = on_update.with_mapper(|m| m.update(notification.update))
                {
                    on_update.emit_all(events);
                }
                Ok(())
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_request(
            async move |request: RequestPermissionRequest,
                        responder: Responder<RequestPermissionResponse>,
                        _cx| {
                let id = request.tool_call.tool_call_id.0.to_string();
                if on_permission.auto_approve.load(Ordering::Relaxed) {
                    return responder.respond(permission_response(
                        &request.options,
                        ApprovalDecision::Approve,
                    ));
                }
                let events = on_permission
                    .with_mapper(|m| m.approval(&id, request.tool_call.fields))
                    .unwrap_or_default();
                // Park the responder; the dispatch loop moves on while the
                // user decides, and the turn loop answers it.
                if let Ok(mut pending) = on_permission.pending.lock() {
                    pending.insert(id, (responder, request.options));
                }
                on_permission.emit_all(events);
                Ok(())
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: ReadTextFileRequest,
                        responder: Responder<ReadTextFileResponse>,
                        _cx| {
                match read_text(&on_read.workspace, &request) {
                    Ok(content) => responder.respond(ReadTextFileResponse::new(content)),
                    Err(message) => responder.respond_with_error(rpc_error(message)),
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: WriteTextFileRequest,
                        responder: Responder<WriteTextFileResponse>,
                        _cx| {
                match write_text(&on_write.workspace, &request.path, &request.content) {
                    Ok((path, old)) => {
                        let events = on_write
                            .with_mapper(|m| {
                                m.file_written(&path, old.as_deref(), &request.content)
                            })
                            .unwrap_or_default();
                        on_write.emit_all(events);
                        responder.respond(WriteTextFileResponse::new())
                    }
                    Err(message) => responder.respond_with_error(rpc_error(message)),
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_with(ByteStreams::new(writer, reader), async move |cx| {
            session_loop(cx, main_shared, ctx.ops, ctx.session_dir, saved, &stderr).await;
            Ok(())
        })
        .await;
    if let Err(err) = result
        && !shared.events.is_closed()
    {
        let tail = ctx.stderr.lock().map(|s| s.clone()).unwrap_or_default();
        shared.emit_all(vec![AgentEvent::Error(describe(
            &shared.agent,
            &err.message,
            None,
            &tail,
        ))]);
    }
}

async fn session_loop(
    cx: ConnectionTo<agent_client_protocol::Agent>,
    shared: Arc<Shared>,
    ops: Receiver<Op>,
    session_dir: Option<PathBuf>,
    saved: Option<Saved>,
    stderr: &Mutex<String>,
) {
    let agent = shared.agent.clone();
    let tail = || stderr.lock().map(|s| s.clone()).unwrap_or_default();
    let fail = |message: String| shared.emit_all(vec![AgentEvent::Error(message)]);

    let capabilities = ClientCapabilities::new().fs(FileSystemCapabilities::new()
        .read_text_file(true)
        .write_text_file(true));
    let init = cx
        .send_request(
            InitializeRequest::new(ProtocolVersion::V1)
                .client_capabilities(capabilities)
                .client_info(Implementation::new("flint", env!("CARGO_PKG_VERSION"))),
        )
        .block_task()
        .await;
    let init = match init {
        Ok(init) => init,
        Err(err) => {
            fail(describe(
                &agent,
                &err.message,
                Some(i32::from(err.code).into()),
                &tail(),
            ));
            return;
        }
    };

    // Reopen the saved conversation when the agent can; otherwise start over.
    let mut session_id: Option<SessionId> = None;
    if let Some(saved) = &saved {
        if init.agent_capabilities.load_session {
            shared.loading.store(true, Ordering::Relaxed);
            let loaded = cx
                .send_request(LoadSessionRequest::new(
                    saved.session_id.clone(),
                    shared.workspace.clone(),
                ))
                .block_task()
                .await;
            shared.loading.store(false, Ordering::Relaxed);
            match loaded {
                Ok(_) => session_id = Some(SessionId::new(saved.session_id.clone())),
                Err(err) => fail(format!(
                    "{} couldn't reopen its earlier conversation ({}), so this continues in a new {0} session. \
                     Earlier messages stay visible, but the agent won't remember them.",
                    agent.name(),
                    err.message
                )),
            }
        } else {
            fail(format!(
                "{} can't reopen earlier conversations, so this continues in a new {0} session. \
                 Earlier messages stay visible, but the agent won't remember them.",
                agent.name()
            ));
        }
    }
    let session_id = match session_id {
        Some(id) => id,
        None => match cx
            .send_request(NewSessionRequest::new(shared.workspace.clone()))
            .block_task()
            .await
        {
            Ok(created) => created.session_id,
            Err(err) => {
                fail(describe(
                    &agent,
                    &err.message,
                    Some(i32::from(err.code).into()),
                    &tail(),
                ));
                return;
            }
        },
    };
    let save = |turn_id: u64| {
        if let Some(dir) = &session_dir {
            save_saved(
                dir,
                &Saved {
                    agent: agent.id(),
                    session_id: session_id.0.to_string(),
                    turn_id,
                },
            );
        }
    };
    save(shared.with_mapper(|m| m.turn_id()).unwrap_or(0));

    let mut queue: VecDeque<String> = VecDeque::new();
    let mut shutting_down = false;
    while !shutting_down {
        let text = match queue.pop_front() {
            Some(text) => text,
            None => match ops.recv().await {
                Ok(Op::UserMessage(text)) => text,
                Ok(Op::Approval { call_id, decision }) => {
                    shared.resolve(&call_id, decision);
                    continue;
                }
                Ok(Op::Interrupt | Op::SetReasoningEffort(_)) => continue,
                Ok(Op::Shutdown) | Err(_) => break,
            },
        };
        let opening = shared.with_mapper(Mapper::start_turn).unwrap_or_default();
        let turn_id = shared.with_mapper(|m| m.turn_id()).unwrap_or(0);
        shared.emit_all(opening);
        save(turn_id);

        let prompt = cx
            .send_request(PromptRequest::new(
                session_id.clone(),
                vec![ContentBlock::Text(TextContent::new(text))],
            ))
            .block_task();
        futures::pin_mut!(prompt);
        let mut cancelled = false;
        let result = loop {
            tokio::select! {
                result = &mut prompt => break result,
                op = ops.recv() => match op {
                    Ok(Op::UserMessage(text)) => queue.push_back(text),
                    Ok(Op::Approval { call_id, decision }) => shared.resolve(&call_id, decision),
                    Ok(Op::SetReasoningEffort(_)) => {}
                    Ok(Op::Interrupt) => {
                        cancelled = true;
                        shared.cancel_pending();
                        let _ = cx.send_notification(CancelNotification::new(session_id.clone()));
                    }
                    Ok(Op::Shutdown) | Err(_) => {
                        shutting_down = true;
                        shared.cancel_pending();
                        let _ = cx.send_notification(CancelNotification::new(session_id.clone()));
                        // Give the agent a moment to stop cleanly.
                        match tokio::time::timeout(std::time::Duration::from_secs(2), &mut prompt).await {
                            Ok(result) => break result,
                            Err(_) => return,
                        }
                    }
                },
            }
        };
        let (reason, closing) = match result {
            Ok(response) => {
                let reason = match response.stop_reason {
                    StopReason::EndTurn => TurnEndReason::Completed,
                    StopReason::Cancelled => TurnEndReason::Interrupted,
                    StopReason::MaxTurnRequests => TurnEndReason::StepLimit,
                    StopReason::MaxTokens => {
                        TurnEndReason::Failed("the agent hit its output token limit".to_string())
                    }
                    StopReason::Refusal => {
                        TurnEndReason::Failed("the agent refused to continue".to_string())
                    }
                    _ => {
                        TurnEndReason::Failed("the agent stopped for an unknown reason".to_string())
                    }
                };
                let mut closing = shared
                    .with_mapper(|m| m.close_open_calls(reason == TurnEndReason::Interrupted))
                    .unwrap_or_default();
                if let Some(usage) = &response.usage {
                    closing.push(Mapper::usage(usage));
                }
                (reason, closing)
            }
            Err(err) => {
                let message = describe(
                    &agent,
                    &err.message,
                    Some(i32::from(err.code).into()),
                    &tail(),
                );
                let mut closing = shared
                    .with_mapper(|m| m.close_open_calls(true))
                    .unwrap_or_default();
                let reason = if cancelled {
                    TurnEndReason::Interrupted
                } else {
                    closing.push(AgentEvent::Error(message.clone()));
                    TurnEndReason::Failed(message)
                };
                (reason, closing)
            }
        };
        shared.emit_all(closing);
        shared.emit_all(vec![AgentEvent::TurnFinished { turn_id, reason }]);
        save(turn_id);
    }
}

/// A user-facing explanation with the fix, from an agent error and the
/// adapter's recent stderr.
pub(crate) fn describe(agent: &AcpAgent, message: &str, code: Option<i64>, stderr: &str) -> String {
    let combined = format!("{message}\n{stderr}");
    let detail = message.trim();
    if looks_like_auth_error(code, &combined) {
        return format!(
            "{} isn't logged in ({detail}). {}",
            agent.name(),
            agent.login_hint()
        );
    }
    let tail: String = {
        let lines: Vec<&str> = stderr.lines().filter(|l| !l.trim().is_empty()).collect();
        lines[lines.len().saturating_sub(4)..].join(" · ")
    };
    if tail.is_empty() {
        format!("{} stopped responding: {detail}", agent.name())
    } else {
        format!(
            "{} stopped responding: {detail}. Adapter output: {tail}",
            agent.name()
        )
    }
}

fn rpc_error(message: String) -> agent_client_protocol::Error {
    let mut error = agent_client_protocol::Error::invalid_params();
    error.message = message;
    error
}

/// Resolves an agent-supplied path, refusing anything outside the workspace.
pub(crate) fn inside_workspace(workspace: &Path, path: &Path) -> Result<PathBuf, String> {
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        workspace.join(path)
    };
    let mut normalized = PathBuf::new();
    for component in joined.components() {
        match component {
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            std::path::Component::CurDir => {}
            other => normalized.push(other.as_os_str()),
        }
    }
    if normalized.starts_with(workspace) {
        Ok(normalized)
    } else {
        Err(format!(
            "{} is outside the workspace {}",
            path.display(),
            workspace.display()
        ))
    }
}

fn read_text(workspace: &Path, request: &ReadTextFileRequest) -> Result<String, String> {
    let path = inside_workspace(workspace, &request.path)?;
    let text = std::fs::read_to_string(&path)
        .map_err(|err| format!("cannot read {}: {err}", path.display()))?;
    let start = request
        .line
        .map_or(0, |line| line.saturating_sub(1) as usize);
    match (start, request.limit) {
        (0, None) => Ok(text),
        (start, limit) => {
            let lines = text.lines().skip(start);
            let picked: Vec<&str> = match limit {
                Some(limit) => lines.take(limit as usize).collect(),
                None => lines.collect(),
            };
            Ok(picked.join("\n"))
        }
    }
}

fn write_text(
    workspace: &Path,
    path: &Path,
    content: &str,
) -> Result<(PathBuf, Option<String>), String> {
    let path = inside_workspace(workspace, path)?;
    let old = std::fs::read_to_string(&path).ok();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| format!("cannot create {}: {err}", parent.display()))?;
    }
    std::fs::write(&path, content)
        .map_err(|err| format!("cannot write {}: {err}", path.display()))?;
    Ok((path, old))
}

fn load_saved(dir: &Path) -> Option<Saved> {
    serde_json::from_slice(&std::fs::read(dir.join("acp.json")).ok()?).ok()
}

fn save_saved(dir: &Path, saved: &Saved) {
    if std::fs::create_dir_all(dir).is_ok()
        && let Ok(bytes) = serde_json::to_vec_pretty(saved)
    {
        let tmp = dir.join("acp.json.tmp");
        if std::fs::write(&tmp, bytes).is_ok() {
            let _ = std::fs::rename(&tmp, dir.join("acp.json"));
        }
    }
}
