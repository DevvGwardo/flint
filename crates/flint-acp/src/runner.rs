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
use agent_client_protocol::schema::v1::FileSystemCapabilities;
use agent_client_protocol::schema::v1::Implementation;
use agent_client_protocol::schema::v1::InitializeRequest;
use agent_client_protocol::schema::v1::LoadSessionRequest;
use agent_client_protocol::schema::v1::NewSessionRequest;
use agent_client_protocol::schema::v1::PermissionOption;
use agent_client_protocol::schema::v1::PermissionOptionKind;
use agent_client_protocol::schema::v1::ReadTextFileRequest;
use agent_client_protocol::schema::v1::ReadTextFileResponse;
use agent_client_protocol::schema::v1::RequestPermissionOutcome;
use agent_client_protocol::schema::v1::RequestPermissionRequest;
use agent_client_protocol::schema::v1::RequestPermissionResponse;
use agent_client_protocol::schema::v1::SelectedPermissionOutcome;
use agent_client_protocol::schema::v1::SessionConfigOption;
use agent_client_protocol::schema::v1::SessionId;
use agent_client_protocol::schema::v1::SessionNotification;
use agent_client_protocol::schema::v1::SessionUpdate;
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

type PendingPermission = (Responder<RequestPermissionResponse>, Vec<PermissionOption>);

/// State shared by the connection's handlers and the session loop.
pub(crate) struct Shared {
    pub agent: AcpAgent,
    pub workspace: PathBuf,
    pub events: Sender<AgentEvent>,
    pub stderr: Arc<Mutex<String>>,
    mapper: Mutex<Mapper>,
    pending: Mutex<HashMap<String, PendingPermission>>,
    auto_approve: AtomicBool,
    /// `session/load` replays history as updates; the UI already has it.
    loading: AtomicBool,
    options: Mutex<Options>,
}

impl Shared {
    pub fn emit_all(&self, events: Vec<AgentEvent>) {
        for event in events {
            let _ = self.events.try_send(event);
        }
    }

    pub fn with_mapper<T>(&self, f: impl FnOnce(&mut Mapper) -> T) -> Option<T> {
        self.mapper.lock().ok().map(|mut mapper| f(&mut mapper))
    }

    pub fn stderr_tail(&self) -> String {
        self.stderr.lock().map(|s| s.clone()).unwrap_or_default()
    }

    pub fn options(&self) -> Options {
        self.options.lock().map(|o| o.clone()).unwrap_or_default()
    }

    /// Stores the agent's options and tells the UI. When the agent has a
    /// permission mode, that mode decides approvals: flint stops answering
    /// permission requests on its own and shows each one.
    pub fn set_options(&self, raw: &[SessionConfigOption]) {
        let options = Options::from_acp(raw);
        let first_mode = options.has_mode() && !self.options().has_mode();
        if first_mode {
            self.auto_approve.store(false, Ordering::Relaxed);
        }
        let list = options.list.clone();
        if let Ok(mut slot) = self.options.lock() {
            *slot = options;
        }
        self.emit_all(vec![AgentEvent::SessionOptions(list)]);
    }

    /// Answers a pending permission with the option matching `decision`.
    pub fn resolve(&self, call_id: &str, decision: ApprovalDecision) {
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
    pub fn cancel_pending(&self) {
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
        stderr: Arc::clone(&ctx.stderr),
        mapper: Mutex::new(Mapper::new(
            &ctx.workspace,
            saved.as_ref().map_or(0, |s| s.turn_id),
        )),
        pending: Mutex::new(HashMap::new()),
        auto_approve: AtomicBool::new(ctx.approval == ApprovalMode::Auto),
        loading: AtomicBool::new(false),
        options: Mutex::new(Options::default()),
    });

    let on_update = Arc::clone(&shared);
    let on_permission = Arc::clone(&shared);
    let on_read = Arc::clone(&shared);
    let on_write = Arc::clone(&shared);
    let main_shared = Arc::clone(&shared);
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
        .connect_with(ByteStreams::new(writer, reader), async move |cx| {
            if let Some(live) = open_session(cx, main_shared, ctx.session_dir, saved).await {
                live.run(ctx.ops).await;
            }
            Ok(())
        })
        .await;
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
            fail(error(err));
            return None;
        }
    };

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
        None => match cx
            .send_request(NewSessionRequest::new(shared.workspace.clone()))
            .block_task()
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
        shared,
        session_id,
        session_dir,
        saved.clone().unwrap_or_default().options,
    );
    if !reopened && let Some(saved) = saved {
        live.reapply(&saved.options).await;
    }
    live.save();
    Some(live)
}
