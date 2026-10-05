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
//!   window at the display edge and record painted composer controls.

use std::path::PathBuf;
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
    use gpui_kit::{Styled as _, canvas, px};
    canvas(
        |bounds, _, _| bounds,
        move |bounds, _, window, _| {
            let visible =
                bounds
                    .intersect(&window.content_mask().bounds)
                    .intersect(&gpui_kit::Bounds {
                        origin: gpui_kit::Point::default(),
                        size: window.viewport_size(),
                    });
            if let Ok(mut controls) = COMPOSER_CONTROLS.lock()
                && let Some(controls) = controls.as_mut()
            {
                controls["painted"][name] = json!({
                    "x": f32::from(bounds.origin.x),
                    "y": f32::from(bounds.origin.y),
                    "width": f32::from(bounds.size.width),
                    "height": f32::from(bounds.size.height),
                    "fully_visible": visible == bounds
                        && bounds.size.width > px(0.)
                        && bounds.size.height > px(0.),
                    "window_active": window.is_window_active(),
                    "painted_at_ms": since_start_ms(),
                });
            }
        },
    )
    .absolute()
    .inset_0()
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
                    window.on_next_frame(move |_, cx| {
                        this.update(cx, |app, _| write_state(app)).ok();
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
    let Some(path) = env_path("FLINT_BP_STATE") else {
        return false;
    };
    std::fs::write(path, state_json(app).to_string()).is_ok()
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
