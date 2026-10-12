//! Automation-only hooks for the blueprint harness (`tools/blueprint/`). None of
//! this runs unless a `FLINT_BP_*` variable or an automation flag asks for it.
//!
//! - `FLINT_BP_STATE=<path>`: when a turn finishes, write a JSON dump of every
//!   session (status, transcript item kinds, changes, usage, guard events,
//!   time to first token).
//! - `FLINT_BP_FRAMES=<path>`: record per-frame timing (render cost and the
//!   interval since the previous frame) and write it on exit.
//! - `FLINT_BP_TIMING=<path>`: write the first-frame timestamp once.
//! - `FLINT_BP_QUEUE_EDITOR=1`: with a state dump and demo queue, open a long
//!   synthetic instruction for native editor screenshots.
//! - `FLINT_BP_CAPTURE=1`: with a timed state dump, position the background
//!   window at the display edge and dump the state actually painted.

use std::path::PathBuf;
use std::sync::LazyLock;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::time::Instant;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use serde_json::Value;
use serde_json::json;

use crate::app::FlintApp;
use crate::session::Status;
use crate::view_model::Item;

static START: OnceLock<Instant> = OnceLock::new();
static FRAMES: Mutex<Vec<(f64, f64)>> = Mutex::new(Vec::new());
static FIRST_FRAME: OnceLock<()> = OnceLock::new();
static STREAM_RATE: Mutex<f64> = Mutex::new(0.);
static EVENT_BATCHES: Mutex<Vec<(usize, f64)>> = Mutex::new(Vec::new());
static COMPOSER_CONTROLS: Mutex<Option<Value>> = Mutex::new(None);
static PAINTED_STATE: Mutex<Option<Value>> = Mutex::new(None);
static TASK_CONTROLS: LazyLock<Mutex<serde_json::Map<String, Value>>> =
    LazyLock::new(|| Mutex::new(serde_json::Map::new()));

pub fn capture_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        env_path("FLINT_BP_STATE").is_some()
            && std::env::var("FLINT_BP_DUMP_AFTER_MS")
                .ok()
                .and_then(|ms| ms.parse::<u64>().ok())
                .is_some()
            && std::env::var("FLINT_BP_CAPTURE").ok().as_deref() == Some("1")
    })
}

pub(crate) fn begin_composer_capture(running: bool, tasks: usize) {
    if capture_enabled()
        && let Ok(mut controls) = COMPOSER_CONTROLS.lock()
    {
        *controls = Some(json!({
            "running": running,
            "tasks": tasks,
            "painted": {},
        }));
    }
}

pub(crate) fn control_probe(name: &'static str) -> impl gpui_kit::IntoElement {
    use gpui_kit::{Styled as _, canvas};
    canvas(
        |bounds, _, _| bounds,
        move |bounds, _, window, _| {
            if let Ok(mut controls) = COMPOSER_CONTROLS.lock()
                && let Some(controls) = controls.as_mut()
            {
                controls["painted"][name] = painted_bounds(bounds, window);
            }
        },
    )
    .absolute()
    .inset_0()
}

pub(crate) fn task_control_probe(name: &'static str) -> impl gpui_kit::IntoElement {
    use gpui_kit::{Styled as _, canvas};
    canvas(
        |bounds, _, _| bounds,
        move |bounds, _, window, _| {
            if !capture_enabled() {
                return;
            }
            if let Ok(mut controls) = TASK_CONTROLS.lock() {
                controls.insert(name.into(), painted_bounds(bounds, window));
            }
        },
    )
    .absolute()
    .inset_0()
}

fn painted_bounds(bounds: gpui_kit::Bounds<gpui_kit::Pixels>, window: &gpui_kit::Window) -> Value {
    let visible = bounds
        .intersect(&window.content_mask().bounds)
        .intersect(&gpui_kit::Bounds {
            origin: gpui_kit::Point::default(),
            size: window.viewport_size(),
        });
    json!({
        "x":f32::from(bounds.origin.x),"y":f32::from(bounds.origin.y),
        "width":f32::from(bounds.size.width),"height":f32::from(bounds.size.height),
        "fully_visible":visible==bounds && bounds.size.width>gpui_kit::px(0.) && bounds.size.height>gpui_kit::px(0.),
        "window_active":window.is_window_active(),"painted_at_ms":since_start_ms(),
    })
}

/// Freeze the render's model state, then add control geometry during paint.
/// A later model update cannot substitute an unpainted scene in the dump.
pub(crate) fn capture_probe(app: &FlintApp) -> impl gpui_kit::IntoElement {
    use gpui_kit::{Styled as _, canvas};
    let state = state_json(app);
    canvas(
        |bounds, _, _| bounds,
        move |bounds, _, window, _| {
            let mut state = state;
            state["capture"] = painted_bounds(bounds, window);
            if let Ok(mut painted) = PAINTED_STATE.lock() {
                *painted = Some(state);
            }
        },
    )
    .absolute()
    .inset_0()
}

/// Offline task-state captures, only with explicit demo and capture flags.
/// No provider, local tool execution or saved user session is involved.
pub(crate) fn seed_task_scene(
    app: &mut FlintApp,
    window: &mut gpui_kit::Window,
    cx: &mut gpui_kit::Context<FlintApp>,
) -> bool {
    use flint_agent::{
        AgentEvent, CommandEvidence, FileEvidence, FileState, ReviewDecision, TaskOutcome,
        TaskRecord, TaskReview, TaskSource, TurnEndReason,
    };
    if !app.options.demo || !app.options.demo_instant || !capture_enabled() {
        return false;
    }
    let Ok(scene) = std::env::var("FLINT_BP_TASK_SCENE") else {
        return false;
    };
    if ![
        "partial",
        "review",
        "blocked",
        "external",
        "accepted",
        "stale",
        "editing",
        "confirmation",
        "queue",
        "historical",
    ]
    .contains(&scene.as_str())
    {
        return false;
    }
    let ix = app.active;
    let uid = app.sessions[ix].uid;
    let mut task = TaskRecord::new(
        1,
        "Recover actual method bodies from the input, not only class metadata.\n\
         Required file: method-bodies.c\nRequired check: python3 validate_metadata.py\n\
         Acceptance: Recovered pseudocode reflects the method instructions."
            .into(),
        7,
    );
    task.files = vec![
        FileEvidence {
            path: "metadata.json".into(),
            state: FileState::Present {
                bytes: 412,
                fingerprint: 11,
            },
            reported: true,
        },
        FileEvidence {
            path: "method-bodies.c".into(),
            state: if ["partial", "queue", "historical"].contains(&scene.as_str()) {
                FileState::Missing
            } else {
                FileState::Present {
                    bytes: 912,
                    fingerprint: 12,
                }
            },
            reported: false,
        },
    ];
    task.checks.push(CommandEvidence {
        id: 1,
        call_id: "fixture-check".into(),
        command: "python3 validate_metadata.py".into(),
        contract_revision: 1,
        work_revision: 7,
        started_revision: 7,
        success: true,
        exit_code: Some(0),
        detail: "Metadata schema validated. This does not validate method-body meaning.".into(),
        verification: true,
    });
    task.observed_edits = 1;
    if scene == "blocked" {
        task.add_blocker("The input binary is unavailable.", "agent report");
    }
    if scene == "stale" {
        task.set_work_revision(8);
    }
    task.assess(TurnEndReason::Completed, true);
    if scene == "external" {
        task = TaskRecord::unverified(
            1,
            task.contract.original_request,
            TurnEndReason::Completed,
            TaskSource::External,
        );
        app.sessions[ix].agent = flint_agent::AgentKind::Droid;
    }
    if scene == "accepted" {
        task.review(&TaskReview {
            turn_id: 1,
            task_id: 1,
            contract_revision: 1,
            work_revision: 7,
            decision: ReviewDecision::Accept,
        })
        .expect("fixture review");
    }
    let change = app.sessions[ix]
        .view
        .push_user(task.contract.original_request.clone());
    app.sessions[ix].apply(change);
    app.apply_event(ix, AgentEvent::TurnStarted { turn_id: 1 }, cx);
    app.apply_event(ix,AgentEvent::TextDelta("Synthetic offline rendering fixture. Metadata was extracted; method-body meaning needs user review.".into()),cx);
    app.apply_event(
        ix,
        AgentEvent::TaskAssessed {
            turn_id: 1,
            task: Box::new(task),
        },
        cx,
    );
    app.apply_event(
        ix,
        AgentEvent::TurnFinished {
            turn_id: 1,
            reason: TurnEndReason::Completed,
        },
        cx,
    );
    // This synthetic stopped fixture has no engine/writer. Mirror its
    // shutdown acknowledgment so editor/confirmation captures exercise the
    // real controls without weakening production writer checks.
    app.apply_event(
        ix,
        AgentEvent::SessionStopped {
            history_saved: true,
        },
        cx,
    );
    if scene == "queue" {
        let queue = &mut app.sessions[ix].prompt_queue;
        queue
            .enqueue(crate::prompt_queue::Prompt::new(
                "Run the next task".into(),
                "Run the next task".into(),
                Vec::new(),
            ))
            .expect("fixture queue");
        queue.paused = true;
        queue.review_hold = Some(format!(
            "{}. Review the task before running queued work.",
            TaskOutcome::Partial.label()
        ));
        app.queue_popover = true;
        app.request_queue_override(None, cx);
    } else {
        if scene == "historical" {
            app.apply_event(ix, AgentEvent::TurnStarted { turn_id: 2 }, cx);
            app.apply_event(
                ix,
                AgentEvent::TextDelta("Synthetic next-turn answer.".into()),
                cx,
            );
            app.apply_event(
                ix,
                AgentEvent::TurnFinished {
                    turn_id: 2,
                    reason: TurnEndReason::Completed,
                },
                cx,
            );
        }
        app.open_task_details(crate::subagent_ui::Conversation::parent(uid), 1, window, cx);
        if scene == "editing" {
            app.edit_task_scope(window, cx);
        }
        if scene == "confirmation" {
            app.request_task_acceptance(cx);
        }
        // Background capture must not acquire keyboard focus or blink a caret.
        if background_launch() {
            window.blur(cx);
        }
    }
    cx.notify();
    true
}

pub fn record_event_batch(count: usize, started: Instant) {
    if frames_enabled()
        && let Ok(mut batches) = EVENT_BATCHES.lock()
    {
        batches.push((count, started.elapsed().as_secs_f64() * 1000.));
    }
}

/// The delta rate a stream test actually achieved (written with the frames).
pub fn note_stream_rate(per_second: f64) {
    if let Ok(mut rate) = STREAM_RATE.lock() {
        *rate = per_second;
    }
}

/// Called first thing in `main`, so frame timestamps are relative to launch.
pub fn mark_process_start() {
    START.get_or_init(Instant::now);
}

fn since_start_ms() -> f64 {
    START.get_or_init(Instant::now).elapsed().as_secs_f64() * 1000.
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

pub(crate) fn seed_queue_editor(
    app: &mut FlintApp,
    window: &mut gpui_kit::Window,
    cx: &mut gpui_kit::Context<FlintApp>,
) {
    if !app.options.demo
        || !app.options.demo_queue
        || env_path("FLINT_BP_STATE").is_none()
        || std::env::var("FLINT_BP_QUEUE_EDITOR").ok().as_deref() != Some("1")
    {
        return;
    }
    let Some(prompt) = app.sessions[app.active].prompt_queue.items.front_mut() else {
        return;
    };
    *prompt = prompt.with_text(format!("{}\n{}", "界🙂".repeat(256), "line\n".repeat(12)));
    let id = prompt.id;
    app.edit_queued_prompt(id, window, cx);
}

fn frames_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| env_path("FLINT_BP_FRAMES").is_some())
}

/// Records one root render: when it started and how long it took.
pub fn record_frame(started: Instant) {
    if FIRST_FRAME.set(()).is_ok()
        && let Some(path) = env_path("FLINT_BP_TIMING")
    {
        let epoch_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs_f64() * 1000.)
            .unwrap_or_default();
        let body = json!({
            "first_frame_epoch_ms": epoch_ms,
            "main_to_first_frame_ms": since_start_ms(),
        });
        std::fs::write(path, body.to_string()).ok();
    }
    if frames_enabled() {
        let at = since_start_ms();
        let cost = started.elapsed().as_secs_f64() * 1000.;
        if let Ok(mut frames) = FRAMES.lock() {
            frames.push((at, cost));
        }
    }
}

/// Writes recorded frames (`[[at_ms, render_ms], ...]`) to `FLINT_BP_FRAMES`.
pub fn write_frames(label: &str) {
    let Some(path) = env_path("FLINT_BP_FRAMES") else {
        return;
    };
    let frames = FRAMES.lock().map(|f| f.clone()).unwrap_or_default();
    let rate = STREAM_RATE.lock().map(|r| *r).unwrap_or_default();
    let batches = EVENT_BATCHES.lock().map(|b| b.clone()).unwrap_or_default();
    let body = json!({
        "label": label, "frames": frames, "deltas_per_second": rate,
        "event_batches": batches,
    });
    std::fs::write(path, body.to_string()).ok();
}

/// With `FLINT_BP_STATE` and `FLINT_BP_DUMP_AFTER_MS`, dumps the state once
/// the UI has settled (used by the state sweep).
pub fn schedule_dump(window: &gpui_kit::Window, cx: &mut gpui_kit::Context<FlintApp>) {
    use gpui_kit::AppContext as _;
    let Some(ms) = std::env::var("FLINT_BP_DUMP_AFTER_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
    else {
        return;
    };
    let handle = window.window_handle();
    cx.spawn(async move |this, cx| {
        cx.background_executor()
            .timer(std::time::Duration::from_millis(ms))
            .await;
        if capture_enabled() {
            cx.update_window(handle, |_, window, _| {
                // The next-frame callback runs before drawing. Wait for the
                // following frame so the dump describes a completed paint.
                window.on_next_frame(move |window, _| {
                    window.on_next_frame(move |_, _| {
                        write_capture_state();
                    });
                });
                window.refresh();
            })
            .ok();
        } else {
            this.update(cx, |app, _| dump_state(app)).ok();
        }
    })
    .detach();
}

/// Writes the session dump to `FLINT_BP_STATE`; returns whether it did.
pub fn dump_state(app: &FlintApp) -> bool {
    // Demo background turns can finish before the timed capture's first
    // paint. Only its scheduled post-paint dump may signal capture readiness.
    if capture_enabled() {
        return false;
    }
    write_state(app)
}

fn write_state(app: &FlintApp) -> bool {
    if env_path("FLINT_BP_STATE").is_none() {
        return false;
    }
    write_json_state(&state_json(app))
}

fn write_capture_state() -> bool {
    let Some(mut state) = PAINTED_STATE.lock().ok().and_then(|mut state| state.take()) else {
        return false;
    };
    // Deferred overlays paint after the root probe. Read their geometry only
    // once the whole frame has finished, without rereading the live model.
    state["composer_controls"] = COMPOSER_CONTROLS
        .lock()
        .ok()
        .and_then(|controls| controls.clone())
        .into();
    state["task_controls"] = TASK_CONTROLS
        .lock()
        .map(|controls| controls.clone())
        .unwrap_or_default()
        .into();
    write_json_state(&state)
}

fn write_json_state(state: &Value) -> bool {
    let Some(path) = env_path("FLINT_BP_STATE") else {
        return false;
    };
    std::fs::write(path, state.to_string()).is_ok()
}

pub fn state_json(app: &FlintApp) -> Value {
    let sessions: Vec<Value> = app
        .sessions
        .iter()
        .enumerate()
        .map(|(ix, session)| {
            let view = &session.view;
            let kinds: Vec<&str> = view.items.iter().map(item_kind).collect();
            let guard: Vec<Value> = view
                .items
                .iter()
                .filter_map(|item| match item {
                    Item::Nudge {
                        reason, message, ..
                    } => Some(json!({ "reason": format!("{reason:?}"), "message": message })),
                    Item::Repair { tool, detail } => {
                        Some(json!({ "reason": "Repair", "message": format!("{tool}: {detail}") }))
                    }
                    _ => None,
                })
                .collect();
            let turns: Vec<Value> = view
                .turns
                .iter()
                .map(|turn| {
                    let reason = turn.end.and_then(|end| match view.items.get(end) {
                        Some(Item::TurnSummary { reason, .. }) => Some(format!("{reason:?}")),
                        _ => None,
                    });
                    json!({
                        "duration_ms": turn.duration.as_millis() as u64,
                        "files": turn.files,
                        "added": turn.added,
                        "removed": turn.removed,
                        "nudges": turn.nudges,
                        "end_reason": reason,
                        "outcome": turn.task.as_ref().and_then(|task| task.assessment.as_ref()).map(|assessment| assessment.outcome),
                    })
                })
                .collect();
            json!({
                "index": ix,
                "active": ix == app.active,
                "title": session.title(),
                "workspace": session.workspace.display().to_string(),
                "status": status_name(session.status()),
                "queued_prompts": session.prompt_queue.items.len(),
                "queue_paused": session.prompt_queue.paused,
                "queue_review_hold": session.prompt_queue.review_hold,
                "task": view.task,
                "queue_editing": app.queue_edit.as_ref().is_some_and(|edit| edit.uid == session.uid),
                "steering_pending": session.steering_pending.is_some(),
                "item_kinds": kinds,
                "changes": view.changes.iter().map(|f| json!({
                    "path": f.path, "added": f.added, "removed": f.removed, "created": f.created,
                })).collect::<Vec<_>>(),
                "turns": turns,
                "guard_events": guard,
                "usage": {
                    "input": view.session_usage.input_tokens,
                    "cached": view.session_usage.cached_input_tokens,
                    "output": view.session_usage.output_tokens,
                    "reasoning": view.session_usage.reasoning_tokens,
                },
                "pending_approvals": view.pending_approvals,
                "first_token_ms": session.first_token.map(|d| d.as_millis() as u64),
                "first_text_ms": session.first_text.map(|d| d.as_millis() as u64),
            })
        })
        .collect();
    let active = &app.session().view;
    json!({
        "sessions": sessions,
        "composer_controls": COMPOSER_CONTROLS.lock().ok().and_then(|controls| controls.clone()),
        "changes_open": app.changes_open,
        "selected_change": app.selected_change,
        "palette_open": app.palette.is_some(),
        "settings_open": app.settings_form.is_some(),
        "task_details_open": app.task_panel.is_some(),
        "task_details_turn_id": app.task_panel.as_ref().map(|panel|panel.turn_id),
        "task_scope_editing": app.task_panel.as_ref().is_some_and(|panel|panel.draft.is_some()),
        "task_accept_confirming": app.task_panel.as_ref().is_some_and(|panel|panel.confirm_accept.is_some()),
        "task_controls": TASK_CONTROLS.lock().map(|controls|controls.clone()).unwrap_or_default(),
        "mention_open": app.mention.as_ref().is_some_and(|m| !m.results.is_empty()),
        "slash_open": app.slash.is_some(),
        "pinned_approval": active.has_pending_approval(),
        "effort_supported": app.effort_supported,
        "work_expanded": active.turns.iter().any(|t| t.expanded),
        "sidebar_visible": app.sidebar_visible,
    })
}

pub fn item_kind(item: &Item) -> &'static str {
    match item {
        Item::User(_) => "user",
        Item::Assistant { .. } => "assistant",
        Item::Thinking { .. } => "thinking",
        Item::Tool(_) => "tool",
        Item::Nudge { .. } => "nudge",
        Item::Jev { .. } => "jev",
        Item::Repair { .. } => "repair",
        Item::Approval { .. } => "approval",
        Item::Error(_) => "error",
        Item::Compacted { .. } => "compacted",
        Item::Reverted { .. } => "reverted",
        Item::TurnSummary { .. } => "summary",
    }
}

fn status_name(status: Status) -> &'static str {
    match status {
        Status::Idle => "idle",
        Status::Running => "running",
        Status::Starting => "starting",
        Status::Failed { seen: false } => "failed",
        Status::Failed { seen: true } => "failed_seen",
        Status::Stopped => "stopped",
        Status::NeedsApproval => "needs_approval",
        Status::NeedsReview => "needs_review",
        Status::Blocked => "blocked",
        Status::Unread => "unread",
        Status::Done => "done",
    }
}

/// Whether this launch must stay in the background (`FLINT_BP_NO_ACTIVATE`,
/// set by the blueprint harness so windows never steal focus): the window
/// opens without taking focus, so nothing is key yet. The composer is
/// focused on first activation instead.
pub fn background_launch() -> bool {
    static BACKGROUND: OnceLock<bool> = OnceLock::new();
    *BACKGROUND.get_or_init(|| std::env::var_os("FLINT_BP_NO_ACTIVATE").is_some())
}
