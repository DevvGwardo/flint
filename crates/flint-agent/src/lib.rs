//! flint's agent engine: a Chat Completions agent loop with local tools and
//! the harness (loop detection, verify-before-done, zero-edit watchdog,
//! tool-call repair, optional JEV judge), MCP tools and a command sandbox.
//! No UI code lives here.

mod approvals;
mod automatic_review;
#[cfg(test)]
mod bench_tests;
mod command_evidence;
pub mod config;
mod context;
mod file_io;
pub mod genui;
pub mod harness;
pub mod improvement;
pub mod mcp;
pub mod orchestration;
mod pending_turn;
mod persist;
pub mod prompt;
pub mod protocol;
pub mod provider;
pub mod rich;
pub mod secrets;
mod session;
mod subagents;
pub mod task;
pub mod tools;
pub mod watch;
mod worktree;

pub use protocol::*;
pub use task::*;

pub struct SavedTaskProjection {
    pub turn_id: u64,
    pub task: Option<TaskRecord>,
}

/// Reads only the native task projection, leaving damaged history in place.
/// This reconciles replay after a durable engine update whose UI event was
/// lost when the app closed.
pub fn saved_task(dir: &std::path::Path) -> Result<Option<(u64, TaskRecord)>, String> {
    Ok(saved_task_projection(dir)?.and_then(|saved| saved.task.map(|task| (saved.turn_id, task))))
}

/// Includes the native turn counter even for version-one histories without a
/// task record, so the UI can detect a crash between its independent writers.
pub fn saved_task_projection(dir: &std::path::Path) -> Result<Option<SavedTaskProjection>, String> {
    #[derive(serde::Deserialize)]
    struct SavedTask {
        version: u32,
        turn_id: u64,
        #[serde(default)]
        task: Option<TaskRecord>,
    }
    let directory = match orchestration::fs::Directory::open(dir) {
        Ok(directory) => directory,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => {
            return Err(
                "Couldn't read the saved native task. Existing history has been kept.".into(),
            );
        }
    };
    let Some(bytes) = directory
        .read_optional("history.json", orchestration::store::MAX_SNAPSHOT_BYTES)
        .map_err(|_| "Couldn't read the saved native task. Existing history has been kept.")?
    else {
        return Ok(None);
    };
    let saved: SavedTask = serde_json::from_slice(&bytes)
        .map_err(|_| "Couldn't read the saved native task. Existing history has been kept.")?;
    if !matches!(saved.version, 1..=3) {
        return Err(
            "The native task history version is unsupported. Existing history has been kept."
                .into(),
        );
    }
    if saved.version == 3 {
        orchestration::store::read_document(&directory).map_err(|error| error.to_string())?;
    }
    if let Some(task) = &saved.task {
        task.validate()?;
        if task.source != TaskSource::Native {
            return Err("Native history contains an external task record.".into());
        }
    }
    Ok(Some(SavedTaskProjection {
        turn_id: saved.turn_id,
        task: saved.task,
    }))
}

/// Explicit UI review when a native session is not running. The caller must
/// not use this concurrently with its engine writer.
pub fn review_saved_task(
    dir: &std::path::Path,
    workspace: &std::path::Path,
    review: &TaskReview,
) -> Result<TaskRecord, String> {
    let lease = orchestration::fs::WriterLease::acquire(dir)
        .map_err(|error| format!("Cannot review the saved task: {error}"))?;
    let (mut document, mut task, reason) = load_native_task_for_turn(&lease, review.turn_id)?;
    if task.refresh_files(workspace) {
        task.set_work_revision(task.work_revision.saturating_add(1));
        if let Some(reason) = reason {
            task.assess(reason, true);
        }
    }
    // Preserve newly observed drift even when the original review is stale,
    // so reopening details offers a fresh review rather than the same error.
    let result = task.review(review);
    persist::save_task_locked(&lease, &mut document, &task).map_err(|_| {
        "Couldn't save the task review. The conversation has been kept.".to_string()
    })?;
    result?;
    Ok(task)
}

/// An explicit scope edit for a stopped native session. See review_saved_task
/// for the single-writer precondition.
pub fn edit_saved_task(
    dir: &std::path::Path,
    turn_id: u64,
    edit: TaskEdit,
) -> Result<(u64, TaskRecord), String> {
    let lease = orchestration::fs::WriterLease::acquire(dir)
        .map_err(|error| format!("Cannot edit the saved task: {error}"))?;
    let (mut document, mut task, reason) = load_native_task_for_turn(&lease, turn_id)?;
    task.edit(edit)?;
    if let Some(reason) = reason {
        task.assess(reason, true);
    }
    persist::save_task_locked(&lease, &mut document, &task)
        .map_err(|_| "Couldn't save the task scope. The conversation has been kept.".to_string())?;
    Ok((turn_id, task))
}

fn load_native_task_for_turn(
    lease: &orchestration::fs::WriterLease,
    turn_id: u64,
) -> Result<(serde_json::Value, TaskRecord, Option<TurnEndReason>), String> {
    let document = orchestration::store::read_document(&lease.directory)
        .map_err(|error| error.to_string())?
        .ok_or("The saved native conversation is unavailable.")?;
    if document["turn_id"].as_u64() != Some(turn_id) {
        return Err("The saved conversation changed. Reopen its details.".into());
    }
    let task: TaskRecord = serde_json::from_value(
        document
            .get("task")
            .cloned()
            .filter(|task| !task.is_null())
            .ok_or("This older conversation has no native task record.")?,
    )
    .map_err(|_| "Invalid saved task. History has been kept.")?;
    if task.source != TaskSource::Native {
        return Err(
            "This history has no native task record. Existing history has been kept.".into(),
        );
    }
    let reason = task
        .assessment
        .as_ref()
        .map(|assessment| assessment.reason.clone());
    Ok((document, task, reason))
}
pub use session::MAX_STEPS_PER_TURN;

/// Both ends of a running session.
pub struct SessionHandle {
    pub ops: async_channel::Sender<Op>,
    pub events: async_channel::Receiver<AgentEvent>,
}

/// Starts a session on its own thread (with its own tokio runtime) and
/// returns its channels. The session ends on [`Op::Shutdown`] or when every
/// `ops` sender is dropped.
pub fn spawn_session(config: AgentConfig) -> SessionHandle {
    spawn_session_with_options(config, ExecutionOptions::default())
}

/// Tool availability controls, separate from approval and sandbox settings.
/// Defaults preserve the normal native engine behaviour.
#[derive(Debug, Clone, Copy)]
pub struct ExecutionOptions {
    pub allow_subagents: bool,
    /// Host advertises a bounded, data-only native interactive surface.
    pub interactive_ui: bool,
    /// Permit private, data-only learning of fixed verification reminders.
    pub improvement_hints: bool,
}

impl Default for ExecutionOptions {
    fn default() -> Self {
        Self {
            allow_subagents: true,
            interactive_ui: false,
            improvement_hints: false,
        }
    }
}

/// Starts the same native loop with explicitly restricted tool availability.
pub fn spawn_session_with_options(
    mut config: AgentConfig,
    options: ExecutionOptions,
) -> SessionHandle {
    if let Ok(canonical) = config.workspace.canonicalize() {
        config.workspace = canonical;
    }
    let (ops_tx, ops_rx) = async_channel::unbounded();
    let (events_tx, events_rx) = async_channel::unbounded();
    let spawned = std::thread::Builder::new()
        .name("flint-session".to_string())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(err) => {
                    let _ = events_tx.try_send(AgentEvent::Error(format!(
                        "cannot start the agent runtime: {err}"
                    )));
                    return;
                }
            };
            runtime.block_on(session::run(config, options, ops_rx, events_tx));
        });
    if let Err(err) = spawned {
        // The receiver is still returned; the front end sees a closed stream.
        eprintln!("flint: cannot start the session thread: {err}");
    }
    SessionHandle {
        ops: ops_tx,
        events: events_rx,
    }
}
