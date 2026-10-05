//! One ACP connection: `initialize`, a new or loaded session, then the
//! session loop in [`crate::live`]. `session/update`, permission and fs
//! requests are handled alongside. Generic over the byte streams so tests
//! can drive it with an in-process fake agent.

use std::collections::HashMap;
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
use agent_client_protocol::schema::v1::ClientCapabilities;
use agent_client_protocol::schema::v1::CreateTerminalRequest;
use agent_client_protocol::schema::v1::CreateTerminalResponse;
use agent_client_protocol::schema::v1::FileSystemCapabilities;
use agent_client_protocol::schema::v1::Implementation;
use agent_client_protocol::schema::v1::InitializeRequest;
use agent_client_protocol::schema::v1::KillTerminalRequest;
use agent_client_protocol::schema::v1::KillTerminalResponse;
use agent_client_protocol::schema::v1::LoadSessionRequest;
use agent_client_protocol::schema::v1::NewSessionRequest;
use agent_client_protocol::schema::v1::ReadTextFileRequest;
use agent_client_protocol::schema::v1::ReadTextFileResponse;
use agent_client_protocol::schema::v1::ReleaseTerminalRequest;
use agent_client_protocol::schema::v1::ReleaseTerminalResponse;
use agent_client_protocol::schema::v1::RequestPermissionRequest;
use agent_client_protocol::schema::v1::RequestPermissionResponse;
use agent_client_protocol::schema::v1::SessionId;
use agent_client_protocol::schema::v1::SessionNotification;
use agent_client_protocol::schema::v1::SessionUpdate;
use agent_client_protocol::schema::v1::TerminalOutputRequest;
use agent_client_protocol::schema::v1::TerminalOutputResponse;
use agent_client_protocol::schema::v1::WaitForTerminalExitRequest;
use agent_client_protocol::schema::v1::WaitForTerminalExitResponse;
use agent_client_protocol::schema::v1::WriteTextFileRequest;
use agent_client_protocol::schema::v1::WriteTextFileResponse;
use async_channel::Receiver;
use async_channel::Sender;
use flint_agent::AgentEvent;
use flint_agent::ApprovalDecision;
use flint_agent::ApprovalMode;
use flint_agent::Op;
use futures::AsyncRead;
use futures::AsyncWrite;

use crate::files::read_text;
use crate::files::rpc_error;
use crate::files::write_text;
use crate::launch::AcpAgent;
use crate::launch::describe;
use crate::live::Live;
use crate::mapper::Mapper;
use crate::options::Options;
use crate::saved::Saved;
use crate::saved::load_saved;
pub(crate) use crate::shared::Shared;
use crate::shared::permission_response;
use crate::terminals::Terminals;

/// What a runner needs besides the byte streams.
pub(crate) struct RunContext {
    pub agent: AcpAgent,
    pub workspace: PathBuf,
    pub session_dir: Option<PathBuf>,
    pub approval: ApprovalMode,
    pub agent_terminals: bool,
    pub preferred_options: std::collections::BTreeMap<String, String>,
    pub ops: Receiver<Op>,
    pub events: Sender<AgentEvent>,
    /// Last stderr lines of the adapter, for error messages.
    pub stderr: Arc<Mutex<String>>,
}

/// Runs the connection until Shutdown, the ops sender closing, or the agent
/// going away. Every failure is reported as an `Error` event.
pub(crate) async fn run<R, W>(reader: R, writer: W, ctx: RunContext)
where
    R: AsyncRead + Send + 'static,
    W: AsyncWrite + Send + 'static,
{
    // Observe Shutdown even while startup or a setting RPC owns the foreground.
    // Forward all other ops in order; do not discard queued user messages.
    let (ops_tx, ops) = async_channel::unbounded();
    let (shutdown_tx, shutdown) = tokio::sync::watch::channel(false);
    let (interrupts, _) = tokio::sync::watch::channel(0u64);
    let on_interrupt = interrupts.clone();
    let incoming = ctx.ops;
    let op_bridge = tokio::spawn(async move {
        while let Ok(op) = incoming.recv().await {
            let stopping = matches!(op, Op::Shutdown);
            if stopping {
                let _ = shutdown_tx.send(true);
            }
            if matches!(op, Op::Interrupt) {
                on_interrupt.send_modify(|epoch| *epoch = epoch.saturating_add(1));
            }
            if ops_tx.send(op).await.is_err() || stopping {
                return;
            }
        }
        let _ = shutdown_tx.send(true);
    });
    let saved = ctx
        .session_dir
        .as_deref()
        .and_then(load_saved)
        .filter(|s| s.agent == ctx.agent.id());
    let shared = Arc::new(Shared {
        agent: ctx.agent.clone(),
        workspace: ctx.workspace.clone(),
        events: ctx.events.clone(),
        stderr: Arc::clone(&ctx.stderr),
        mapper: Mutex::new(
            Mapper::new(&ctx.workspace, saved.as_ref().map_or(0, |s| s.turn_id))
                .with_label_prefix(&ctx.agent.label_prefix()),
        ),
        pending: Mutex::new(HashMap::new()),
        auto_approve: AtomicBool::new(ctx.approval == ApprovalMode::Auto),
        loading: AtomicBool::new(false),
        options: Mutex::new(Options::default()),
        options_changed: tokio::sync::watch::channel(0).0,
        terminals: Terminals::new(
            &ctx.workspace,
            ctx.events.clone(),
            &ctx.agent.label_prefix(),
        ),
        agent_terminals: ctx.agent_terminals,
        shutdown,
        interrupts,
        interrupts_seen: std::sync::atomic::AtomicU64::new(0),
    });

    let on_update = Arc::clone(&shared);
    let on_permission = Arc::clone(&shared);
    let on_read = Arc::clone(&shared);
    let on_write = Arc::clone(&shared);
    let main_shared = Arc::clone(&shared);
    let on_create = Arc::clone(&shared);
    let on_output = Arc::clone(&shared);
    let on_wait = Arc::clone(&shared);
    let on_kill = Arc::clone(&shared);
    let on_release = Arc::clone(&shared);
    let result = Client
        .builder()
        .name("flint")
        .on_receive_notification(
            async move |notification: SessionNotification, _cx| {
                match notification.update {
                    SessionUpdate::ConfigOptionUpdate(update) => {
                        on_update.set_options(&update.config_options);
                    }
                    update => {
                        if !on_update.loading.load(Ordering::Relaxed)
                            && let Some(events) = on_update.with_mapper(|m| m.update(update))
                        {
                            on_update.emit_all(events);
                        }
                    }
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
                // user decides, and the session loop answers it.
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
        .on_receive_request(
            async move |request: CreateTerminalRequest,
                        responder: Responder<CreateTerminalResponse>,
                        _cx| {
                match on_create.terminals.create(&request) {
                    Ok(id) => responder.respond(CreateTerminalResponse::new(id)),
                    Err(message) => {
                        // Never log command/argument/env values: a command may
                        // contain credentials. The UI and peer get the reason.
                        eprintln!(
                            "flint: WARN terminal/create failed ({}): {}",
                            crate::terminals::request_diagnostic(&request),
                            crate::terminals::diagnostic_reason(&request, &message),
                        );
                        let events = on_create
                            .with_mapper(|mapper| mapper.terminal_failed(&request, &message))
                            .unwrap_or_default();
                        on_create.emit_all(events);
                        responder.respond_with_error(rpc_error(format!(
                            "{message}. This is a Flint terminal creation error, not evidence that the shell is broken."
                        )))
                    }
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: TerminalOutputRequest,
                        responder: Responder<TerminalOutputResponse>,
                        _cx| {
                match on_output.terminals.output(&request.terminal_id.0) {
                    Ok((output, truncated, exit)) => responder.respond(
                        TerminalOutputResponse::new(output, truncated)
                            .exit_status(exit.map(|e| e.to_acp())),
                    ),
                    Err(message) => responder.respond_with_error(rpc_error(message)),
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: WaitForTerminalExitRequest,
                        responder: Responder<WaitForTerminalExitResponse>,
                        _cx| {
                // Waits off the dispatch loop so other messages keep flowing.
                let shared = Arc::clone(&on_wait);
                tokio::spawn(async move {
                    let _ = match shared.terminals.wait_for_exit(&request.terminal_id.0).await {
                        Ok(exit) => {
                            responder.respond(WaitForTerminalExitResponse::new(exit.to_acp()))
                        }
                        Err(message) => responder.respond_with_error(rpc_error(message)),
                    };
                });
                Ok(())
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: KillTerminalRequest,
                        responder: Responder<KillTerminalResponse>,
                        _cx| {
                match on_kill.terminals.kill(&request.terminal_id.0) {
                    Ok(()) => responder.respond(KillTerminalResponse::new()),
                    Err(message) => responder.respond_with_error(rpc_error(message)),
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: ReleaseTerminalRequest,
                        responder: Responder<ReleaseTerminalResponse>,
                        _cx| {
                match on_release.terminals.release(&request.terminal_id.0) {
                    Ok(()) => responder.respond(ReleaseTerminalResponse::new()),
                    Err(message) => responder.respond_with_error(rpc_error(message)),
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_with(ByteStreams::new(writer, reader), async move |cx| {
            let mut shutdown = main_shared.shutdown.clone();
            let live = tokio::select! {
                biased;
                _ = shutdown.wait_for(|stopped| *stopped) => None,
                live = open_session(cx, main_shared, ctx.session_dir, saved, ctx.preferred_options) => live,
            };
            if let Some(live) = live {
                live.run(ops).await;
            }
            Ok(())
        })
        .await;
    op_bridge.abort();
    shared.cancel_pending();
    shared.terminals.shutdown();
    if let Err(err) = result
        && !shared.events.is_closed()
    {
        let tail = shared.stderr_tail();
        shared.emit_all(vec![AgentEvent::Error(describe(
            &shared.agent,
            &err.message,
            None,
            &tail,
        ))]);
    }
}

/// `initialize`, then `session/load` (when the agent can and there is a
/// saved session) or `session/new`. Re-applies saved option choices when the
/// conversation couldn't be reopened. `None` after reporting a failure.
async fn open_session(
    cx: ConnectionTo<agent_client_protocol::Agent>,
    shared: Arc<Shared>,
    session_dir: Option<PathBuf>,
    saved: Option<Saved>,
    preferred_options: std::collections::BTreeMap<String, String>,
) -> Option<Live> {
    let agent = shared.agent.clone();
    let fail = |message: String| shared.emit_all(vec![AgentEvent::Error(message)]);
    let error = |err: agent_client_protocol::Error| {
        describe(
            &agent,
            &err.message,
            Some(i32::from(err.code).into()),
            &shared.stderr_tail(),
        )
    };

    let mut meta = serde_json::Map::new();
    meta.insert(
        "terminal_output".into(),
        serde_json::Value::Bool(shared.agent_terminals),
    );
    let capabilities = ClientCapabilities::new()
        .fs(FileSystemCapabilities::new()
            .read_text_file(true)
            .write_text_file(true))
        .terminal(true)
        .meta(meta);
    let init = shared
        .rpc(
            cx.send_request(
                InitializeRequest::new(ProtocolVersion::V1)
                    .client_capabilities(capabilities)
                    .client_info(Implementation::new("flint", env!("CARGO_PKG_VERSION"))),
            )
            .block_task(),
        )
        .await;
    let init = match init {
        Ok(init) => init,
        Err(err) => {
            fail(error(err));
            return None;
        }
    };

    let mut session_id: Option<SessionId> = None;
    if let Some(saved) = &saved {
        if init.agent_capabilities.load_session {
            shared.loading.store(true, Ordering::Relaxed);
            let loaded = shared
                .rpc(
                    cx.send_request(LoadSessionRequest::new(
                        saved.session_id.clone(),
                        shared.workspace.clone(),
                    ))
                    .block_task(),
                )
                .await;
            shared.loading.store(false, Ordering::Relaxed);
            match loaded {
                Ok(loaded) => {
                    session_id = Some(SessionId::new(saved.session_id.clone()));
                    shared.set_options(&loaded.config_options.unwrap_or_default());
                }
                Err(err) => fail(format!(
                    "{} couldn't reopen its earlier conversation ({}), so this continues in a \
                     new {0} session. Earlier messages stay visible, but the agent won't \
                     remember them.",
                    agent.name(),
                    err.message
                )),
            }
        } else {
            fail(format!(
                "{} can't reopen earlier conversations, so this continues in a new {0} \
                 session. Earlier messages stay visible, but the agent won't remember them.",
                agent.name()
            ));
        }
    }
    let reopened = session_id.is_some();
    let session_id = match session_id {
        Some(id) => id,
        None => match shared
            .rpc(
                cx.send_request(NewSessionRequest::new(shared.workspace.clone()))
                    .block_task(),
            )
            .await
        {
            Ok(created) => {
                shared.set_options(&created.config_options.unwrap_or_default());
                created.session_id
            }
            Err(err) => {
                fail(error(err));
                return None;
            }
        },
    };
    let mut live = Live::new(
        cx,
        Arc::clone(&shared),
        session_id,
        session_dir,
        saved.clone().unwrap_or_default().options,
    );
    // Keep the connection alive so the user can replace an unavailable
    // preference. Live gates prompts until a valid identity is chosen.
    if let Some(saved) = saved {
        let offered = shared.options();
        let identity: std::collections::BTreeMap<_, _> = saved
            .options
            .iter()
            .filter(|(id, _)| {
                id.as_str() == "model"
                    || id.as_str() == "provider"
                    || offered.get(id).is_some_and(|option| {
                        matches!(option.category.as_deref(), Some("model" | "provider"))
                    })
            })
            .map(|(id, value)| (id.clone(), value.clone()))
            .collect();
        live.restore_preferences(&identity).await;
        if !reopened {
            let other = saved
                .options
                .into_iter()
                .filter(|(id, _)| !identity.contains_key(id))
                .collect();
            live.reapply(&other).await;
        }
    } else {
        live.restore_preferences(&preferred_options).await;
    }
    live.save();
    Some(live)
}
