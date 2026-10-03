//! Blueprint UI tests: one test per user-facing promise, driving the real
//! FlintApp views in a headless window with scripted engine events (channels
//! attached through `FlintApp::attach_engine`, no model involved).
//! `tools/blueprint/interact.py` turns each test into a report check.

use flint_agent::AgentEvent;
use flint_agent::ApprovalDecision;
use flint_agent::ApprovalMode;
use flint_agent::FileDiff;
use flint_agent::Op;
use flint_agent::SessionHandle;
use flint_agent::ToolKind;
use flint_agent::TurnEndReason;
use flint_app::app::FlintApp;
use flint_app::app::Options;
use flint_app::settings::KeySources;
use flint_app::view_model::Item;
use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    AnyWindowHandle, App, AppContext as _, Bounds, ElementId, Entity, Focusable as _, Point,
    ScrollDelta, TestAppContext, Window, WindowBounds, WindowOptions, point, px, size,
};
use serde_json::json;

struct Ui {
    window: AnyWindowHandle,
    app: Entity<FlintApp>,
}

struct Engine {
    ops: async_channel::Receiver<Op>,
    events: async_channel::Sender<AgentEvent>,
}

impl Engine {
    /// Sends one event and lets the pump (which coalesces bursts on an 8 ms
    /// timer) deliver it.
    fn send(&self, cx: &mut TestAppContext, event: AgentEvent) {
        self.events.try_send(event).unwrap();
        settle(cx);
    }

    fn sent(&self) -> Vec<Op> {
        std::iter::from_fn(|| self.ops.try_recv().ok()).collect()
    }
}

/// Runs pending work and advances the test clock past the pump's batching
/// delay, twice, so queued events and their redraws all land.
fn settle(cx: &mut TestAppContext) {
    for _ in 0..2 {
        cx.run_until_parked();
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(20));
    }
    cx.run_until_parked();
}

/// A fresh, empty directory under the system temp dir.
fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "flint-ui-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Options isolated from the real machine: a temp workspace, settings home
/// and key file.
fn test_options() -> Options {
    let home = temp_dir("home");
    let key = home.join("api.key");
    std::fs::write(&key, "test-key").unwrap();
    Options {
        workspace: Some(temp_dir("ws")),
        home: Some(home),
        key_path: Some(key),
        // No environment keys and no ~/.fx fallback: independent of this machine.
        key_sources: Some(KeySources::none()),
        ..Options::default()
    }
}

fn open(cx: &mut TestAppContext) -> Ui {
    open_with(cx, test_options())
}

fn open_with(cx: &mut TestAppContext, options: Options) -> Ui {
    cx.update(flint_app::init);
    let (window, app) = cx.update(|cx| {
        let bounds = Bounds {
            origin: Point::default(),
            size: size(px(1440.), px(900.)),
        };
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            cx,
            |window, cx| cx.new(|cx| FlintApp::new(options, window, cx)),
        )
        .unwrap()
    });
    cx.run_until_parked();
    Ui { window, app }
}

impl Ui {
    fn with<R>(&self, cx: &mut TestAppContext, f: impl FnOnce(&mut Window, &mut App) -> R) -> R {
        cx.update_window(self.window, |_, window, cx| f(window, cx))
            .unwrap()
    }

    fn press(&self, cx: &mut TestAppContext, key: &str) {
        self.with(cx, |window, cx| window.press(key, cx));
        cx.run_until_parked();
    }

    fn input(&self, cx: &mut TestAppContext, text: &str) {
        self.with(cx, |window, cx| window.input(text, cx));
        cx.run_until_parked();
    }

    fn click(&self, cx: &mut TestAppContext, id: impl Into<ElementId>) {
        let id = id.into();
        self.with(cx, |window, cx| window.click(id, cx));
        cx.run_until_parked();
    }

    fn read<R>(&self, cx: &mut TestAppContext, f: impl FnOnce(&FlintApp, &App) -> R) -> R {
        cx.read(|cx| f(self.app.read(cx), cx))
    }

    /// Attaches scripted engine channels to the active session.
    fn engine(&self, cx: &mut TestAppContext) -> Engine {
        let (ops_tx, ops_rx) = async_channel::unbounded();
        let (events_tx, events_rx) = async_channel::unbounded();
        self.app.update(cx, |app, cx| {
            let ix = app.active;
            app.attach_engine(
                ix,
                SessionHandle {
                    ops: ops_tx,
                    events: events_rx,
                },
                cx,
            );
        });
        Engine {
            ops: ops_rx,
            events: events_tx,
        }
    }

    fn composer_text(&self, cx: &mut TestAppContext) -> String {
        self.read(cx, |app, cx| app.composer.read(cx).value().to_string())
    }
}

fn tool_started(call_id: &str, kind: ToolKind, summary: &str) -> AgentEvent {
    AgentEvent::ToolCallStarted {
        call_id: call_id.into(),
        name: "tool".into(),
        kind,
        args: json!({}),
        summary: summary.into(),
    }
}

fn tool_finished(call_id: &str, output: &str, diff: Option<FileDiff>) -> AgentEvent {
    AgentEvent::ToolCallFinished {
        call_id: call_id.into(),
        output: output.into(),
        exit_code: Some(0),
        success: true,
        diff,
        duration_ms: 20,
    }
}

/// A complete turn that edits `src/a.rs` and answers.
fn finished_turn(ui: &Ui, cx: &mut TestAppContext, engine: &Engine) {
    ui.input(cx, "fix it");
    ui.press(cx, "enter");
    for event in [
        AgentEvent::TurnStarted { turn_id: 1 },
        AgentEvent::StepStarted {
            turn_id: 1,
            step: 1,
        },
        AgentEvent::ReasoningDelta("look".into()),
        tool_started("r1", ToolKind::Read, "src/a.rs"),
        tool_finished("r1", "fn a() {}", None),
        tool_started("e1", ToolKind::Edit, "src/a.rs"),
        tool_finished(
            "e1",
            "ok",
            Some(FileDiff {
                path: "src/a.rs".into(),
                unified: "@@ -1 +1 @@\n-fn a() {}\n+fn a() -> u8 { 1 }\n".into(),
                added: 1,
                removed: 1,
                created: false,
            }),
        ),
        AgentEvent::TextDelta("Fixed `a`.".into()),
        AgentEvent::TurnFinished {
            turn_id: 1,
            reason: TurnEndReason::Completed,
        },
    ] {
        engine.send(cx, event);
    }
}

#[gpui_kit::test]
fn enter_sends_the_message(cx: &mut TestAppContext) {
    let ui = open(cx);
    let engine = ui.engine(cx);
    ui.input(cx, "hello there");
    ui.press(cx, "enter");
    assert!(matches!(engine.sent().as_slice(), [Op::UserMessage(text)] if text == "hello there"));
    assert_eq!(ui.composer_text(cx), "");
    let first = ui.read(cx, |app, _| app.session().view.items.first().cloned());
    assert_eq!(first, Some(Item::User("hello there".into())));
}

#[gpui_kit::test]
fn shift_enter_adds_a_newline(cx: &mut TestAppContext) {
    let ui = open(cx);
    let engine = ui.engine(cx);
    ui.input(cx, "line one");
    ui.press(cx, "shift-enter");
    ui.input(cx, "line two");
    assert_eq!(ui.composer_text(cx), "line one\nline two");
    assert!(engine.sent().is_empty());
}

#[gpui_kit::test]
fn empty_input_does_not_send(cx: &mut TestAppContext) {
    let ui = open(cx);
    let engine = ui.engine(cx);
    ui.input(cx, "   ");
    ui.press(cx, "enter");
    assert!(engine.sent().is_empty());
    assert!(ui.read(cx, |app, _| app.session().view.items.is_empty()));
}

#[gpui_kit::test]
fn composer_is_focused_on_launch(cx: &mut TestAppContext) {
    let ui = open(cx);
    let app = ui.app.clone();
    let focused = ui.with(cx, |window, cx| {
        app.read(cx).composer.focus_handle(cx).is_focused(window)
    });
    assert!(focused);
}

#[gpui_kit::test]
fn shift_tab_cycles_the_approval_mode(cx: &mut TestAppContext) {
    let ui = open(cx);
    assert_eq!(ui.read(cx, |app, _| app.approval), ApprovalMode::Auto);
    ui.press(cx, "shift-tab");
    assert_eq!(
        ui.read(cx, |app, _| app.approval),
        ApprovalMode::AskForChanges
    );
    ui.press(cx, "shift-tab");
    assert_eq!(ui.read(cx, |app, _| app.approval), ApprovalMode::Auto);
}

#[gpui_kit::test]
fn cmd_k_opens_the_palette_and_escape_closes_it(cx: &mut TestAppContext) {
    let ui = open(cx);
    ui.press(cx, "cmd-k");
    assert!(ui.read(cx, |app, _| app.palette.is_some()));
    ui.press(cx, "escape");
    assert!(ui.read(cx, |app, _| app.palette.is_none()));
}

#[gpui_kit::test]
fn palette_enter_runs_the_selected_command(cx: &mut TestAppContext) {
    let ui = open(cx);
    ui.press(cx, "cmd-k");
    ui.input(cx, "changes panel");
    ui.press(cx, "enter");
    assert!(ui.read(cx, |app, _| app.changes_open));
    assert!(ui.read(cx, |app, _| app.palette.is_none()));
}

#[gpui_kit::test]
fn cmd_j_toggles_the_changes_panel(cx: &mut TestAppContext) {
    let ui = open(cx);
    ui.press(cx, "cmd-j");
    assert!(ui.read(cx, |app, _| app.changes_open));
    ui.press(cx, "cmd-j");
    assert!(ui.read(cx, |app, _| !app.changes_open));
}

#[gpui_kit::test]
fn cmd_n_creates_a_session(cx: &mut TestAppContext) {
    let ui = open(cx);
    let _engine = ui.engine(cx);
    ui.input(cx, "first task");
    ui.press(cx, "enter");
    ui.press(cx, "cmd-n");
    assert_eq!(
        ui.read(cx, |app, _| (app.sessions.len(), app.active)),
        (2, 1)
    );
}

#[gpui_kit::test]
fn stop_and_cmd_period_send_interrupt(cx: &mut TestAppContext) {
    let ui = open(cx);
    let engine = ui.engine(cx);
    ui.input(cx, "long task");
    ui.press(cx, "enter");
    engine.send(cx, AgentEvent::TurnStarted { turn_id: 1 });
    engine.sent();
    ui.press(cx, "cmd-.");
    assert!(matches!(engine.sent().as_slice(), [Op::Interrupt]));
    ui.click(cx, "stop");
    assert!(matches!(engine.sent().as_slice(), [Op::Interrupt]));
}

#[gpui_kit::test]
fn approval_buttons_send_the_right_op(cx: &mut TestAppContext) {
    for (button, decision) in [
        ("approve-button", ApprovalDecision::Approve),
        ("always-button", ApprovalDecision::ApproveAlways),
        ("deny-button", ApprovalDecision::Deny),
    ] {
        let ui = open(cx);
        let engine = ui.engine(cx);
        ui.input(cx, "deploy");
        ui.press(cx, "enter");
        engine.send(cx, AgentEvent::TurnStarted { turn_id: 1 });
        engine.send(
            cx,
            AgentEvent::ApprovalRequested {
                call_id: "c1".into(),
                kind: ToolKind::Command,
                summary: "rm -rf build".into(),
            },
        );
        engine.sent();
        ui.click(cx, button);
        let sent = engine.sent();
        assert!(
            matches!(sent.as_slice(), [Op::Approval { call_id, decision: d }] if call_id == "c1" && *d == decision),
            "{button}: {sent:?}"
        );
    }
}

#[gpui_kit::test]
fn worked_for_expands_and_collapses(cx: &mut TestAppContext) {
    let ui = open(cx);
    let engine = ui.engine(cx);
    finished_turn(&ui, cx, &engine);
    let header = ui.read(cx, |app, _| app.session().view.turns[0].header.unwrap());
    ui.click(cx, ("worked", header));
    assert!(ui.read(cx, |app, _| app.session().view.turns[0].expanded));
    ui.click(cx, ("worked", header));
    assert!(ui.read(cx, |app, _| !app.session().view.turns[0].expanded));
}

#[gpui_kit::test]
fn clicking_a_tool_row_expands_its_output(cx: &mut TestAppContext) {
    let ui = open(cx);
    let engine = ui.engine(cx);
    ui.input(cx, "run tests");
    ui.press(cx, "enter");
    engine.send(cx, AgentEvent::TurnStarted { turn_id: 1 });
    engine.send(cx, tool_started("c1", ToolKind::Command, "npm test"));
    engine.send(
        cx,
        tool_finished("c1", "line 1\nline 2\nline 3\nline 4", None),
    );
    let ix = ui.read(cx, |app, _| app.session().view.items.len() - 1);
    ui.click(cx, ("tool", ix));
    let expanded = ui.read(cx, |app, _| match &app.session().view.items[ix] {
        Item::Tool(call) => call.expanded,
        _ => false,
    });
    assert!(expanded);
}

#[gpui_kit::test]
fn review_opens_the_changes_panel_on_that_file(cx: &mut TestAppContext) {
    let ui = open(cx);
    let engine = ui.engine(cx);
    finished_turn(&ui, cx, &engine);
    let end = ui.read(cx, |app, _| app.session().view.turns[0].end.unwrap());
    ui.click(cx, ("review-button", end));
    assert_eq!(
        ui.read(cx, |app, _| (app.changes_open, app.selected_change)),
        (true, Some(0))
    );
}

#[gpui_kit::test]
fn switching_sessions_keeps_a_background_session_running(cx: &mut TestAppContext) {
    let ui = open(cx);
    let engine = ui.engine(cx);
    ui.input(cx, "background work");
    ui.press(cx, "enter");
    engine.send(cx, AgentEvent::TurnStarted { turn_id: 1 });
    ui.press(cx, "cmd-n");
    assert_eq!(ui.read(cx, |app, _| app.active), 1);
    engine.send(cx, tool_started("c1", ToolKind::Command, "make"));
    engine.send(
        cx,
        AgentEvent::TurnFinished {
            turn_id: 1,
            reason: TurnEndReason::Completed,
        },
    );
    let (items, unread) = ui.read(cx, |app, _| {
        (app.sessions[0].view.items.len(), app.sessions[0].unread)
    });
    assert!(
        items >= 3,
        "background session kept folding events: {items} items"
    );
    assert!(unread, "finished background session is marked unread");
    ui.click(cx, ("session", 0usize));
    assert_eq!(
        ui.read(cx, |app, _| (app.active, app.sessions[0].unread)),
        (0, false)
    );
}

#[gpui_kit::test]
fn transcript_follows_output_until_scrolled_up(cx: &mut TestAppContext) {
    let ui = open(cx);
    let engine = ui.engine(cx);
    ui.input(cx, "stream");
    ui.press(cx, "enter");
    engine.send(cx, AgentEvent::TurnStarted { turn_id: 1 });
    for n in 0..60 {
        engine.send(
            cx,
            tool_started(
                &format!("c{n}"),
                ToolKind::Read,
                &format!("src/file_{n}.rs"),
            ),
        );
    }
    ui.with(cx, |window, cx| window.render_frame(cx));
    assert!(ui.read(cx, |app, _| app.session().list.is_following_tail()));
    ui.with(cx, |window, cx| {
        window.scroll(
            "transcript",
            ScrollDelta::Pixels(point(px(0.), px(400.))),
            cx,
        )
    });
    cx.run_until_parked();
    assert!(ui.read(cx, |app, _| !app.session().list.is_following_tail()));
}

// ---- Part B fixes -------------------------------------------------------

fn approval_requested(engine: &Engine, cx: &mut TestAppContext, call_id: &str) {
    engine.send(
        cx,
        AgentEvent::ApprovalRequested {
            call_id: call_id.into(),
            kind: ToolKind::Command,
            summary: "npm publish".into(),
        },
    );
}

fn running_turn(ui: &Ui, cx: &mut TestAppContext) -> Engine {
    let engine = ui.engine(cx);
    ui.input(cx, "do the thing");
    ui.press(cx, "enter");
    engine.send(cx, AgentEvent::TurnStarted { turn_id: 1 });
    engine.sent();
    engine
}

#[gpui_kit::test]
fn pending_approval_is_pinned_just_above_the_composer(cx: &mut TestAppContext) {
    let ui = open(cx);
    let engine = running_turn(&ui, cx);
    approval_requested(&engine, cx, "c1");
    let (card, stop) = ui.with(cx, |window, _| {
        (
            window.find("approval-card").bounds(),
            window.find("stop").bounds(),
        )
    });
    assert!(
        card.bottom() <= stop.top(),
        "card {card:?} must sit above the composer {stop:?}"
    );
    assert!(
        card.top() > px(450.),
        "card is in the lower half, near the composer: {card:?}"
    );
}

#[gpui_kit::test]
fn y_a_n_answer_an_approval_from_an_empty_composer(cx: &mut TestAppContext) {
    for (key, decision) in [
        ("y", ApprovalDecision::Approve),
        ("a", ApprovalDecision::ApproveAlways),
        ("n", ApprovalDecision::Deny),
    ] {
        let ui = open(cx);
        let engine = running_turn(&ui, cx);
        approval_requested(&engine, cx, "c1");
        ui.press(cx, key);
        let sent = engine.sent();
        assert!(
            matches!(sent.as_slice(), [Op::Approval { decision: d, .. }] if *d == decision),
            "{key}: {sent:?}"
        );
        assert_eq!(ui.composer_text(cx), "", "{key} must not be typed");
    }
}

#[gpui_kit::test]
fn typing_y_with_text_in_the_composer_does_not_approve(cx: &mut TestAppContext) {
    let ui = open(cx);
    let engine = running_turn(&ui, cx);
    approval_requested(&engine, cx, "c1");
    ui.input(cx, "wait, why");
    assert!(engine.sent().is_empty());
    assert!(ui.read(cx, |app, _| app.session().view.pending_approval().is_some()));
}

#[gpui_kit::test]
fn cmd_enter_approves_the_pending_request(cx: &mut TestAppContext) {
    let ui = open(cx);
    let engine = running_turn(&ui, cx);
    approval_requested(&engine, cx, "c1");
    ui.press(cx, "cmd-enter");
    assert!(matches!(
        engine.sent().as_slice(),
        [Op::Approval {
            decision: ApprovalDecision::Approve,
            ..
        }]
    ));
}

#[gpui_kit::test]
fn background_approval_shows_needs_approval_in_the_sidebar(cx: &mut TestAppContext) {
    let ui = open(cx);
    let engine = running_turn(&ui, cx);
    ui.press(cx, "cmd-n");
    approval_requested(&engine, cx, "c1");
    let status = ui.read(cx, |app, _| app.sessions[0].status());
    assert_eq!(status, flint_app::session::Status::NeedsApproval);
}

/// A workspace with two source files and a gitignored secret.
fn mention_workspace() -> std::path::PathBuf {
    let ws = temp_dir("mention");
    std::fs::create_dir_all(ws.join("src")).unwrap();
    std::fs::write(ws.join("src/main.rs"), "fn main() { println!(\"hi\"); }\n").unwrap();
    std::fs::write(ws.join("src/lib.rs"), "pub fn lib() {}\n").unwrap();
    std::fs::write(ws.join(".gitignore"), "secret.txt\n").unwrap();
    std::fs::write(ws.join("secret.txt"), "password\n").unwrap();
    ws
}

#[gpui_kit::test]
fn at_opens_a_file_picker_and_picking_attaches_the_file(cx: &mut TestAppContext) {
    let ui = open_with(
        cx,
        Options {
            workspace: Some(mention_workspace()),
            ..test_options()
        },
    );
    let engine = ui.engine(cx);
    ui.input(cx, "explain @mai");
    let results = ui.read(cx, |app, _| app.mention.as_ref().map(|m| m.results.clone()));
    assert_eq!(
        results.as_ref().and_then(|r| r.first()).map(String::as_str),
        Some("src/main.rs")
    );
    ui.press(cx, "enter");
    assert_eq!(ui.composer_text(cx), "explain @src/main.rs ");
    assert_eq!(
        ui.read(cx, |app, _| app.attachments.clone()),
        vec!["src/main.rs".to_string()]
    );
    ui.press(cx, "enter");
    let sent = engine.sent();
    let [Op::UserMessage(message)] = sent.as_slice() else {
        panic!("expected one message, got {sent:?}");
    };
    assert!(message.starts_with("explain @src/main.rs"));
    assert!(message.contains(
        "[Attached file src/main.rs (1 lines)]\n```\nfn main() { println!(\"hi\"); }\n```"
    ));
}

#[gpui_kit::test]
fn the_file_picker_respects_gitignore(cx: &mut TestAppContext) {
    let ui = open_with(
        cx,
        Options {
            workspace: Some(mention_workspace()),
            ..test_options()
        },
    );
    ui.input(cx, "@secret");
    let results = ui.read(cx, |app, _| app.mention.as_ref().map(|m| m.results.clone()));
    assert_eq!(results, Some(vec![]));
}

#[gpui_kit::test]
fn plus_opens_the_same_file_picker(cx: &mut TestAppContext) {
    let ui = open_with(
        cx,
        Options {
            workspace: Some(mention_workspace()),
            ..test_options()
        },
    );
    // "+" opens the project menu; its last row is "Attach file…".
    ui.click(cx, "attach");
    let attach = ui.read(cx, |app, _| app.project_items().len() - 1);
    ui.click(cx, ("project-item", attach));
    let menu = ui.read(cx, |app, _| {
        app.mention.as_ref().map(|m| (m.inline, m.results.len()))
    });
    assert_eq!(
        menu,
        Some((false, 2)),
        "src/main.rs and src/lib.rs; secret.txt is gitignored"
    );
}

#[gpui_kit::test]
fn slash_opens_a_keyboard_navigable_command_menu(cx: &mut TestAppContext) {
    let ui = open(cx);
    ui.input(cx, "/");
    assert!(ui.read(cx, |app, _| app.slash.is_some()));
    ui.press(cx, "down");
    ui.press(cx, "down");
    ui.press(cx, "enter");
    assert!(
        ui.read(cx, |app, _| app.settings_form.is_some()),
        "third command is /model"
    );
    assert_eq!(ui.composer_text(cx), "");
}

#[gpui_kit::test]
fn slash_review_opens_the_changes_panel(cx: &mut TestAppContext) {
    let ui = open(cx);
    ui.input(cx, "/rev");
    ui.press(cx, "enter");
    assert!(ui.read(cx, |app, _| app.changes_open && app.slash.is_none()));
}

#[gpui_kit::test]
fn settings_save_to_config_and_are_read_on_startup(cx: &mut TestAppContext) {
    let options = test_options();
    let home = options.home.clone().unwrap();
    let ui = open_with(cx, options.clone());
    ui.press(cx, "cmd-,");
    assert!(ui.read(cx, |app, _| app.settings_form.is_some()));
    ui.click(cx, ("settings-approval", 1usize));
    ui.click(cx, ("settings-effort", 2usize));
    ui.click(cx, "settings-save");
    assert!(ui.read(cx, |app, _| app.settings_form.is_none()));
    let saved = std::fs::read_to_string(home.join("config.toml")).unwrap();
    assert!(
        saved.contains("approval = \"ask\"") && saved.contains("effort = \"high\""),
        "{saved}"
    );
    let again = open_with(cx, options);
    assert_eq!(
        again.read(cx, |app, _| (app.approval, app.effort)),
        (
            ApprovalMode::AskForChanges,
            Some(flint_agent::ReasoningEffort::High)
        )
    );
}

#[gpui_kit::test]
fn escape_closes_settings_without_saving(cx: &mut TestAppContext) {
    let options = test_options();
    let home = options.home.clone().unwrap();
    let ui = open_with(cx, options);
    ui.press(cx, "cmd-,");
    ui.click(cx, ("settings-approval", 1usize));
    ui.press(cx, "escape");
    assert!(ui.read(cx, |app, _| app.settings_form.is_none()
        && app.approval == ApprovalMode::Auto));
    assert!(!home.join("config.toml").exists());
}

#[gpui_kit::test]
fn sessions_are_saved_and_listed_after_a_restart(cx: &mut TestAppContext) {
    let options = test_options();
    let ui = open_with(cx, options.clone());
    let engine = ui.engine(cx);
    ui.input(cx, "persist me");
    ui.press(cx, "enter");
    for event in [
        AgentEvent::TurnStarted { turn_id: 1 },
        AgentEvent::TextDelta("Saved.".into()),
        AgentEvent::TurnFinished {
            turn_id: 1,
            reason: TurnEndReason::Completed,
        },
    ] {
        engine.send(cx, event);
    }
    let again = open_with(cx, options);
    let restored = again.read(cx, |app, _| {
        app.sessions
            .iter()
            .find(|s| s.title() == "persist me")
            .map(|s| (s.status(), s.view.items.len(), s.dir.is_some()))
    });
    assert_eq!(restored, Some((flint_app::session::Status::Done, 3, true)));
}

#[gpui_kit::test]
fn rename_and_delete_from_the_session_context_menu(cx: &mut TestAppContext) {
    let options = test_options();
    let ui = open_with(cx, options.clone());
    let _engine = ui.engine(cx);
    ui.input(cx, "rename me");
    ui.press(cx, "enter");
    let dir = ui.read(cx, |app, _| app.session().dir.clone().unwrap());
    ui.with(cx, |window, cx| window.right_click(("session", 0usize), cx));
    cx.run_until_parked();
    ui.click(cx, ("rename-session", 0usize));
    assert!(ui.read(cx, |app, _| app.renaming.is_some()));
    ui.press(cx, "cmd-a");
    ui.input(cx, "Better title");
    ui.press(cx, "enter");
    assert_eq!(
        ui.read(cx, |app, _| app.sessions[0].title()),
        "Better title"
    );
    let meta = std::fs::read_to_string(dir.join("meta.json")).unwrap();
    assert!(meta.contains("Better title"), "{meta}");
    ui.with(cx, |window, cx| window.right_click(("session", 0usize), cx));
    cx.run_until_parked();
    ui.click(cx, ("delete-session", 0usize));
    assert!(ui.read(cx, |app, _| {
        app.sessions.iter().all(|s| s.title() != "Better title")
    }));
    assert!(!dir.exists(), "deleting removes the saved session");
}

#[gpui_kit::test]
fn the_effort_chip_sends_set_reasoning_effort(cx: &mut TestAppContext) {
    let ui = open(cx);
    let engine = running_turn(&ui, cx);
    ui.click(cx, "effort-chip");
    assert!(matches!(
        engine.sent().as_slice(),
        [Op::SetReasoningEffort(Some(
            flint_agent::ReasoningEffort::High
        ))]
    ));
}

#[gpui_kit::test]
fn the_effort_chip_hides_when_the_endpoint_ignores_it(cx: &mut TestAppContext) {
    let ui = open(cx);
    let engine = running_turn(&ui, cx);
    engine.send(
        cx,
        AgentEvent::Error(
            "This model endpoint doesn't accept reasoning_effort; continuing without it.".into(),
        ),
    );
    assert!(!ui.read(cx, |app, _| app.effort_supported));
    assert!(ui.with(cx, |window, _| window.try_find("effort-chip").is_none()));
}

#[gpui_kit::test]
fn clicking_a_guard_nudge_expands_its_message(cx: &mut TestAppContext) {
    let ui = open(cx);
    let engine = running_turn(&ui, cx);
    engine.send(
        cx,
        AgentEvent::HarnessNudge {
            reason: flint_agent::NudgeReason::Verify,
            message: "You modified files but haven't run the tests. Run them and continue.".into(),
        },
    );
    let ix = ui.read(cx, |app, _| app.session().view.items.len() - 1);
    ui.click(cx, ("nudge", ix));
    let expanded = ui.read(cx, |app, _| {
        matches!(
            app.session().view.items[ix],
            Item::Nudge { expanded: true, .. }
        )
    });
    assert!(expanded);
}

#[gpui_kit::test]
fn context_compaction_shows_a_row(cx: &mut TestAppContext) {
    let ui = open(cx);
    let engine = running_turn(&ui, cx);
    engine.send(
        cx,
        AgentEvent::ContextCompacted {
            before_tokens: 92_000,
            after_tokens: 41_000,
        },
    );
    let last = ui.read(cx, |app, _| app.session().view.items.last().cloned());
    assert_eq!(
        last,
        Some(Item::Compacted {
            before_tokens: 92_000,
            after_tokens: 41_000
        })
    );
}

#[gpui_kit::test]
fn changes_show_one_combined_diff_per_file(cx: &mut TestAppContext) {
    use flint_app::session::combined;
    let ws = temp_dir("combined");
    let original = "one\ntwo\nthree\n";
    let v1 = "one\nTWO\nthree\n";
    let v2 = "one\nTWO\nthree\nfour\n";
    std::fs::write(ws.join("a.txt"), original).unwrap();
    let ui = open_with(
        cx,
        Options {
            workspace: Some(ws.clone()),
            ..test_options()
        },
    );
    let engine = running_turn(&ui, cx);
    for (n, (before, after)) in [(original, v1), (v1, v2)].into_iter().enumerate() {
        std::fs::write(ws.join("a.txt"), after).unwrap();
        let (unified, added, removed) = combined(before, after, "a.txt");
        let id = format!("e{n}");
        engine.send(cx, tool_started(&id, ToolKind::Edit, "a.txt"));
        engine.send(
            cx,
            tool_finished(
                &id,
                "ok",
                Some(FileDiff {
                    path: "a.txt".into(),
                    unified,
                    added,
                    removed,
                    created: false,
                }),
            ),
        );
    }
    engine.send(
        cx,
        AgentEvent::TurnFinished {
            turn_id: 1,
            reason: TurnEndReason::Completed,
        },
    );
    let (expected, added, removed) = combined(original, v2, "a.txt");
    let file = ui.read(cx, |app, _| app.session().view.changes[0].clone());
    assert_eq!(
        (file.combined, file.added, file.removed),
        (Some(expected), added, removed)
    );
    let turn = ui.read(cx, |app, _| {
        (
            app.session().view.turns[0].added,
            app.session().view.turns[0].removed,
        )
    });
    assert_eq!(turn, (2, 1), "the files-changed card counts the net change");
}

#[gpui_kit::test]
fn the_changes_panel_lists_files_only_when_there_are_several(cx: &mut TestAppContext) {
    let ui = open(cx);
    let engine = running_turn(&ui, cx);
    let edit = |n: usize, path: &str| {
        tool_finished(
            &format!("e{n}"),
            "ok",
            Some(FileDiff {
                path: path.into(),
                unified: format!("--- a/{path}\n+++ b/{path}\n@@ -1 +1 @@\n-a\n+b\n"),
                added: 1,
                removed: 1,
                created: false,
            }),
        )
    };
    engine.send(cx, tool_started("e0", ToolKind::Edit, "x.rs"));
    engine.send(cx, edit(0, "x.rs"));
    ui.press(cx, "cmd-j");
    assert!(ui.with(cx, |window, _| {
        window.try_find(("change", 0usize)).is_none()
    }));
    assert!(ui.with(cx, |window, _| window.try_find("open-in-editor").is_some()));
    engine.send(cx, tool_started("e1", ToolKind::Edit, "y.rs"));
    engine.send(cx, edit(1, "y.rs"));
    assert!(ui.with(cx, |window, _| {
        window.try_find(("change", 1usize)).is_some()
    }));
}

#[gpui_kit::test]
fn the_header_title_uses_the_available_width(cx: &mut TestAppContext) {
    let ui = open(cx);
    let _engine = ui.engine(cx);
    ui.input(
        cx,
        &"Refactor the session store so restore is incremental ".repeat(4),
    );
    ui.press(cx, "enter");
    let width = ui.with(cx, |window, _| {
        window.find("session-title").bounds().size.width
    });
    assert!(width > px(380.), "title width {width:?}");
}

#[gpui_kit::test]
fn the_welcome_tip_is_plain_language_and_its_dismissal_is_remembered(cx: &mut TestAppContext) {
    let options = test_options();
    let ui = open_with(cx, options.clone());
    assert!(ui.with(cx, |window, _| window.try_find("welcome-tip").is_some()));
    ui.click(cx, "dismiss-tip");
    let again = open_with(cx, options);
    assert!(again.with(cx, |window, _| window.try_find("welcome-tip").is_none()));
}

#[gpui_kit::test]
fn copy_shows_copied_and_fills_the_clipboard(cx: &mut TestAppContext) {
    let ui = open(cx);
    let engine = ui.engine(cx);
    finished_turn(&ui, cx, &engine);
    let end = ui.read(cx, |app, _| app.session().view.turns[0].end.unwrap());
    ui.click(cx, ("copy-button", end));
    assert!(ui.read(cx, |app, _| app.copied.is_some_and(|(row, _)| row == end)));
    let clip = cx.read_from_clipboard().and_then(|item| item.text());
    assert_eq!(clip.as_deref(), Some("Fixed `a`."));
}

#[gpui_kit::test]
fn a_missing_key_shows_a_card_that_opens_settings(cx: &mut TestAppContext) {
    let ui = open_with(
        cx,
        Options {
            key_path: Some(temp_dir("nokey").join("api.key")),
            ..test_options()
        },
    );
    ui.input(cx, "hello");
    ui.press(cx, "enter");
    let (ix, message) = ui.read(cx, |app, _| {
        let view = &app.session().view;
        view.items
            .iter()
            .enumerate()
            .find_map(|(ix, item)| match item {
                Item::Error(message) => Some((ix, message.clone())),
                _ => None,
            })
            .unwrap()
    });
    assert!(message.starts_with("No API key found"), "{message}");
    assert!(!message.contains("test-key"));
    ui.click(cx, ("error-settings", ix));
    assert!(ui.read(cx, |app, _| app.settings_form.is_some()));
}

#[gpui_kit::test]
fn an_unreachable_endpoint_shows_a_card_with_retry(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let options = test_options();
    let home = options.home.clone().unwrap();
    std::fs::write(
        home.join("config.toml"),
        "model = \"test-model\"\nbase_url = \"http://127.0.0.1:9/v1\"\n",
    )
    .unwrap();
    let ui = open_with(cx, options);
    ui.input(cx, "hello");
    ui.press(cx, "enter");
    let started = std::time::Instant::now();
    let error = loop {
        settle(cx);
        let error = ui.read(cx, |app, _| {
            app.session()
                .view
                .items
                .iter()
                .enumerate()
                .find_map(|(ix, item)| match item {
                    Item::Error(message) => Some((ix, message.clone())),
                    _ => None,
                })
        });
        if let Some(error) = error {
            break error;
        }
        assert!(
            started.elapsed() < std::time::Duration::from_secs(30),
            "no error from the engine"
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    };
    assert!(
        ui.with(cx, |window, _| window
            .try_find(("error-retry", error.0))
            .is_some()),
        "retry offered for: {}",
        error.1
    );
    assert!(ui.with(cx, |window, _| {
        window.try_find(("error-settings", error.0)).is_some()
    }));
}

#[gpui_kit::test]
fn a_model_error_offers_retry_which_resends(cx: &mut TestAppContext) {
    let ui = open(cx);
    let engine = running_turn(&ui, cx);
    engine.send(
        cx,
        AgentEvent::Error("HTTP 400: model `nope` does not exist".into()),
    );
    engine.send(
        cx,
        AgentEvent::TurnFinished {
            turn_id: 1,
            reason: TurnEndReason::Failed("HTTP 400".into()),
        },
    );
    let ix = ui.read(cx, |app, _| {
        app.session()
            .view
            .items
            .iter()
            .position(|item| matches!(item, Item::Error(_)))
            .unwrap()
    });
    ui.click(cx, ("error-retry", ix));
    assert!(matches!(engine.sent().as_slice(), [Op::UserMessage(text)] if text == "do the thing"));
}

#[gpui_kit::test]
fn files_changed_counts_survive_edits_landing_before_the_ui_reads_them(cx: &mut TestAppContext) {
    use flint_app::session::combined;
    let ws = temp_dir("race");
    let original = "def area_of_rect(w, h):\n    return w * h\n\nx = area_of_rect(1, 2)\n";
    let v1 = "def rectangle_area(w, h):\n    return w * h\n\nx = area_of_rect(1, 2)\n";
    let v2 = "def rectangle_area(w, h):\n    return w * h\n\nx = rectangle_area(1, 2)\n";
    // The engine already applied both edits before the UI sees the first one.
    std::fs::write(ws.join("geo.py"), v2).unwrap();
    let ui = open_with(
        cx,
        Options {
            workspace: Some(ws),
            ..test_options()
        },
    );
    let engine = running_turn(&ui, cx);
    for (n, (before, after)) in [(original, v1), (v1, v2)].into_iter().enumerate() {
        let (unified, added, removed) = combined(before, after, "geo.py");
        let id = format!("e{n}");
        engine
            .events
            .try_send(tool_started(&id, ToolKind::Edit, "geo.py"))
            .unwrap();
        engine
            .events
            .try_send(tool_finished(
                &id,
                "ok",
                Some(FileDiff {
                    path: "geo.py".into(),
                    unified,
                    added,
                    removed,
                    created: false,
                }),
            ))
            .unwrap();
    }
    engine.send(
        cx,
        AgentEvent::TurnFinished {
            turn_id: 1,
            reason: TurnEndReason::Completed,
        },
    );
    let turn = ui.read(cx, |app, _| {
        (
            app.session().view.turns[0].added,
            app.session().view.turns[0].removed,
        )
    });
    assert_eq!(
        turn,
        (2, 2),
        "net change is two lines replaced, as git diff reports"
    );
}

#[gpui_kit::test]
fn agent_picker_sets_a_fresh_session_and_opens_a_new_one_after_start(cx: &mut TestAppContext) {
    use flint_agent::AgentKind;
    let ui = open(cx);
    // A fresh session takes the picked agent; the chip shows it.
    ui.click(cx, "model-chip");
    assert!(ui.read(cx, |app, _| app.agent_menu));
    ui.click(cx, ("agent-item", 1usize));
    assert_eq!(
        ui.read(cx, |app, _| (
            app.agent_menu,
            app.sessions.len(),
            app.session().agent
        )),
        (false, 1, AgentKind::ClaudeCode)
    );
    assert_eq!(
        ui.read(cx, |app, _| app.agent_label(app.session().agent)),
        "Claude Code"
    );

    // Once it has started, the session keeps its agent; /agent opens a new one.
    let _engine = ui.engine(cx);
    ui.input(cx, "first task");
    ui.press(cx, "enter");
    ui.input(cx, "/agent codex");
    ui.press(cx, "escape");
    ui.press(cx, "enter");
    let agents = ui.read(cx, |app, _| {
        (
            app.sessions.iter().map(|s| s.agent).collect::<Vec<_>>(),
            app.active,
        )
    });
    assert_eq!(agents, (vec![AgentKind::ClaudeCode, AgentKind::Codex], 1));
    assert_eq!(ui.composer_text(cx), "");
}

fn agent_options(mode: &str) -> AgentEvent {
    use flint_agent::OptionChoice;
    use flint_agent::SessionOption;
    let option =
        |id: &str, category: Option<&str>, current: &str, choices: &[(&str, &str)]| SessionOption {
            id: id.to_string(),
            name: id.to_string(),
            description: None,
            category: category.map(str::to_string),
            current: current.to_string(),
            choices: choices
                .iter()
                .map(|(value, name)| OptionChoice {
                    value: (*value).to_string(),
                    name: (*name).to_string(),
                    description: Some(format!("about {name}")),
                })
                .collect(),
        };
    AgentEvent::SessionOptions(vec![
        option(
            "mode",
            Some("mode"),
            mode,
            &[
                ("default", "Manual"),
                ("acceptEdits", "Accept Edits"),
                ("plan", "Plan"),
                ("bypassPermissions", "Bypass"),
            ],
        ),
        option(
            "model",
            Some("model"),
            "default",
            &[("default", "Default"), ("opus", "Opus")],
        ),
        option(
            "effort",
            Some("thought_level"),
            "high",
            &[("low", "Low"), ("high", "High")],
        ),
        option(
            "fast",
            Some("model_config"),
            "off",
            &[("on", "On"), ("off", "Off")],
        ),
        option(
            "agent",
            None,
            "default",
            &[("default", "Default"), ("reviewer", "Reviewer")],
        ),
    ])
}

/// A Claude Code session with scripted options attached.
fn acp_session(cx: &mut TestAppContext) -> (Ui, Engine) {
    let ui = open(cx);
    ui.app.update(cx, |app, _| {
        app.sessions[0].agent = flint_agent::AgentKind::ClaudeCode
    });
    let engine = ui.engine(cx);
    engine.send(cx, agent_options("default"));
    (ui, engine)
}

fn has(ui: &Ui, cx: &mut TestAppContext, id: &str) -> bool {
    let id = id.to_string();
    ui.with(cx, move |window, cx| {
        window.render_frame(cx);
        window.try_find(ElementId::Name(id.into())).is_some()
    })
}

#[gpui_kit::test]
fn agent_option_chips_render_and_replace_auto_run(cx: &mut TestAppContext) {
    let (ui, _engine) = acp_session(cx);
    for id in [
        "option-model",
        "option-reasoning",
        "option-mode",
        "option-fast",
        "option-more",
    ] {
        assert!(has(&ui, cx, id), "{id} missing");
    }
    assert!(!has(&ui, cx, "approval-hint"));
    assert!(!has(&ui, cx, "effort-chip"));
}

#[gpui_kit::test]
fn option_menus_send_set_session_option(cx: &mut TestAppContext) {
    let (ui, engine) = acp_session(cx);
    // Keyboard: open the model menu, move down, Enter.
    ui.click(cx, "option-model");
    assert!(has(&ui, cx, "option-menu"));
    ui.press(cx, "down");
    ui.press(cx, "enter");
    assert!(
        matches!(engine.sent().as_slice(), [Op::SetSessionOption { id, value }] if id == "model" && value == "opus")
    );
    assert!(!has(&ui, cx, "option-menu"));
    // Mouse: reasoning menu, first row.
    ui.click(cx, "option-reasoning");
    ui.click(cx, ("option-item", 0usize));
    assert!(
        matches!(engine.sent().as_slice(), [Op::SetSessionOption { id, value }] if id == "effort" && value == "low")
    );
    // Fast is a toggle.
    ui.click(cx, "option-fast");
    assert!(
        matches!(engine.sent().as_slice(), [Op::SetSessionOption { id, value }] if id == "fast" && value == "on")
    );
    // More lists the remaining options; picking one opens its choices.
    ui.click(cx, "option-more");
    ui.click(cx, ("option-item", 0usize));
    ui.click(cx, ("option-item", 1usize));
    assert!(
        matches!(engine.sent().as_slice(), [Op::SetSessionOption { id, value }] if id == "agent" && value == "reviewer")
    );
}

#[gpui_kit::test]
fn shift_tab_cycles_the_agents_mode(cx: &mut TestAppContext) {
    let (ui, engine) = acp_session(cx);
    ui.press(cx, "shift-tab");
    assert!(
        matches!(engine.sent().as_slice(), [Op::SetSessionOption { id, value }] if id == "mode" && value == "acceptEdits")
    );
    engine.send(cx, agent_options("plan"));
    // From Plan it wraps to Manual, skipping Bypass.
    ui.press(cx, "shift-tab");
    assert!(
        matches!(engine.sent().as_slice(), [Op::SetSessionOption { id, value }] if id == "mode" && value == "default")
    );
    // flint's auto-run is untouched.
    assert_eq!(ui.read(cx, |app, _| app.approval), ApprovalMode::Auto);
}

#[gpui_kit::test]
fn plus_menu_lists_folders_and_picking_one_sets_the_workspace(cx: &mut TestAppContext) {
    use flint_app::project_menu::ProjectItem;
    let ui = open(cx);
    let other = temp_dir("other-project");
    // A saved session elsewhere makes that folder a recent one.
    ui.app.update(cx, |app, _| {
        app.sessions
            .push(flint_app::session::Session::new(9_999, other.clone()));
    });
    ui.click(cx, "attach");
    let items = ui.read(cx, |app, _| app.project_items());
    assert_eq!(
        items,
        vec![
            ProjectItem::OpenFolder,
            ProjectItem::Recent(other.clone()),
            ProjectItem::AttachFile,
        ]
    );
    for n in 0..3usize {
        assert!(ui.with(cx, move |w, _| w.try_find(("project-item", n)).is_some()));
    }
    // An unstarted session moves to the chosen folder.
    ui.click(cx, ("project-item", 1usize));
    assert_eq!(
        ui.read(cx, |app, _| (
            app.session().workspace.clone(),
            app.sessions.len(),
            app.project_menu
        )),
        (other.clone(), 2, None)
    );

    // A started session stays put; the folder opens in a new session.
    let _engine = ui.engine(cx);
    ui.input(cx, "first task");
    ui.press(cx, "enter");
    let first = ui.read(cx, |app, _| app.sessions[1].workspace.clone());
    let elsewhere = temp_dir("elsewhere");
    ui.app
        .update(cx, |app, cx| app.set_project_folder(elsewhere.clone(), cx));
    assert_eq!(
        ui.read(cx, |app, _| (
            app.sessions.len(),
            app.session().workspace.clone(),
            app.sessions[1].workspace.clone()
        )),
        (3, elsewhere, first)
    );
}

#[gpui_kit::test]
fn welcome_folder_chip_opens_the_project_menu(cx: &mut TestAppContext) {
    let ui = open(cx);
    ui.click(cx, "welcome-folder");
    assert!(has(&ui, cx, "project-menu"));
    ui.press(cx, "escape");
    assert!(!has(&ui, cx, "project-menu"));
}

#[gpui_kit::test]
fn open_picker_does_not_push_the_logo_into_the_header(cx: &mut TestAppContext) {
    let ui = open(cx);
    let bounds = |ui: &Ui, cx: &mut TestAppContext, id: &'static str| {
        ui.with(cx, move |window, cx| {
            window.render_frame(cx);
            window.find(ElementId::Name(id.into())).bounds()
        })
    };
    let header = bounds(&ui, cx, "header");
    let before = bounds(&ui, cx, "welcome-logo");
    ui.click(cx, "attach");
    let attach = ui.read(cx, |app, _| app.project_items().len() - 1);
    ui.click(cx, ("project-item", attach));
    assert!(ui.read(cx, |app, _| app.mention.is_some()));
    let after = bounds(&ui, cx, "welcome-logo");
    assert_eq!(after.origin, before.origin, "the picker moved the hero");
    assert!(
        after.origin.y >= header.origin.y + header.size.height,
        "logo {after:?} overlaps header {header:?}"
    );
}

#[gpui_kit::test]
fn starting_agent_shows_a_status_line_and_greyed_chips(cx: &mut TestAppContext) {
    let ui = open(cx);
    ui.app.update(cx, |app, _| {
        app.sessions[0].agent = flint_agent::AgentKind::Codex
    });
    let engine = ui.engine(cx);
    assert!(ui.read(cx, |app, _| app.session().agent_starting()));
    assert!(has(&ui, cx, "agent-starting"));
    assert!(ui.with(cx, |w, _| {
        w.try_find(("option-placeholder", 0usize)).is_some()
    }));
    engine.send(cx, agent_options("default"));
    assert!(!has(&ui, cx, "agent-starting"));
    assert!(has(&ui, cx, "option-model"));
}

#[gpui_kit::test]
fn slash_model_and_mode_open_the_agents_menus(cx: &mut TestAppContext) {
    use flint_app::session_options::MenuTarget;
    let (ui, _engine) = acp_session(cx);
    ui.input(cx, "/model");
    ui.press(cx, "enter");
    assert_eq!(
        ui.read(cx, |app, _| app
            .option_menu
            .as_ref()
            .map(|m| m.target.clone())),
        Some(MenuTarget::Option("model".into()))
    );
    ui.press(cx, "escape");
    ui.input(cx, "/mode");
    ui.press(cx, "enter");
    assert_eq!(
        ui.read(cx, |app, _| app
            .option_menu
            .as_ref()
            .map(|m| m.target.clone())),
        Some(MenuTarget::Option("mode".into()))
    );
}

fn terminal_options() -> Options {
    Options {
        terminal_command: Some(("/bin/sh".into(), Vec::new())),
        terminal_poll: true,
        ..test_options()
    }
}

/// Waits (real time) until the active terminal shows a line satisfying `pred`.
fn wait_for_terminal(ui: &Ui, cx: &mut TestAppContext, pred: impl Fn(&str) -> bool) -> Vec<String> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        settle(cx);
        let lines = ui.read(cx, |app, cx| {
            app.terminal
                .active_view()
                .map(|v| v.read(cx).terminal.snapshot().text_lines())
                .unwrap_or_default()
        });
        if lines.iter().any(|l| pred(l)) || std::time::Instant::now() > deadline {
            return lines;
        }
        std::thread::sleep(std::time::Duration::from_millis(30));
    }
}

#[gpui_kit::test]
fn ctrl_backtick_toggles_a_terminal_in_the_workspace(cx: &mut TestAppContext) {
    let ui = open_with(cx, terminal_options());
    assert!(!has(&ui, cx, "terminal-panel"));
    ui.press(cx, "ctrl-`");
    let (open, tabs, cwd, workspace) = ui.read(cx, |app, cx| {
        (
            app.terminal.open,
            app.terminal.tabs.len(),
            app.terminal.active_view().map(|v| v.read(cx).cwd.clone()),
            app.session().workspace.clone(),
        )
    });
    assert_eq!((open, tabs, cwd), (true, 1, Some(workspace)));
    assert!(has(&ui, cx, "terminal-panel"));
    ui.click(cx, "terminal-new");
    assert_eq!(ui.read(cx, |app, _| app.terminal.tabs.len()), 2);
    ui.press(cx, "ctrl-`");
    assert!(!has(&ui, cx, "terminal-panel"));
}

#[gpui_kit::test]
fn typing_in_the_terminal_runs_commands(cx: &mut TestAppContext) {
    let ui = open_with(cx, terminal_options());
    ui.press(cx, "ctrl-`");
    settle(cx);
    ui.input(cx, "echo hi-$((40+2))");
    ui.press(cx, "enter");
    let lines = wait_for_terminal(&ui, cx, |l| l == "hi-42");
    assert!(lines.iter().any(|l| l == "hi-42"), "{lines:?}");
    // Escape and Shift+Tab go to the shell, not to flint.
    let approval = ui.read(cx, |app, _| app.approval);
    ui.press(cx, "shift-tab");
    assert_eq!(ui.read(cx, |app, _| app.approval), approval);
}

#[gpui_kit::test]
fn terminal_copy_and_paste(cx: &mut TestAppContext) {
    let ui = open_with(cx, terminal_options());
    ui.press(cx, "ctrl-`");
    settle(cx);
    ui.input(cx, "echo copy-me");
    ui.press(cx, "enter");
    let lines = wait_for_terminal(&ui, cx, |l| l == "copy-me");
    let row = lines
        .iter()
        .position(|l| l == "copy-me")
        .expect("output row");
    ui.read(cx, |app, cx| {
        let view = app.terminal.active_view().expect("tab").read(cx);
        view.terminal.start_selection(row, 0, false, 1);
        view.terminal.update_selection(row, 6, true);
    });
    ui.press(cx, "cmd-c");
    let copied = cx.read(|cx| cx.read_from_clipboard().and_then(|item| item.text()));
    assert_eq!(copied.as_deref(), Some("copy-me"));

    cx.update(|cx| {
        cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(
            "echo pasted-$((1+1))".into(),
        ))
    });
    ui.press(cx, "cmd-v");
    ui.press(cx, "enter");
    let lines = wait_for_terminal(&ui, cx, |l| l == "pasted-2");
    assert!(lines.iter().any(|l| l == "pasted-2"), "{lines:?}");
}

/// A running command card (the transcript's second row, after the message).
fn running_command(ui: &Ui, cx: &mut TestAppContext, engine: &Engine, command: &str) -> usize {
    ui.input(cx, "run it");
    ui.press(cx, "enter");
    engine.send(cx, AgentEvent::TurnStarted { turn_id: 1 });
    engine.send(cx, tool_started("c1", ToolKind::Command, command));
    engine.sent();
    ui.read(cx, |app, _| {
        app.session()
            .view
            .items
            .iter()
            .position(|item| matches!(item, Item::Tool(_)))
            .expect("tool row")
    })
}

#[gpui_kit::test]
fn a_command_card_opens_in_a_terminal(cx: &mut TestAppContext) {
    let ui = open_with(cx, terminal_options());
    let engine = ui.engine(cx);
    let row = running_command(&ui, cx, &engine, "npm test");
    assert!(!has(&ui, cx, "terminal-panel"));
    ui.click(cx, ("tool-terminal", row));
    let (open, tabs, read_only) = ui.read(cx, |app, cx| {
        (
            app.terminal.open,
            app.terminal.tabs.len(),
            app.terminal
                .active_view()
                .map(|view| view.read(cx).read_only),
        )
    });
    assert_eq!((open, tabs, read_only), (true, 1, Some(false)));
    // The command is on the new shell's prompt, ready to run.
    let lines = wait_for_terminal(&ui, cx, |l| l.contains("npm test"));
    assert!(lines.iter().any(|l| l.contains("npm test")), "{lines:?}");
}

#[gpui_kit::test]
fn a_command_card_sends_its_output_to_the_agent(cx: &mut TestAppContext) {
    let ui = open_with(cx, terminal_options());
    let engine = ui.engine(cx);
    let row = running_command(&ui, cx, &engine, "npm test");
    engine.send(cx, tool_finished("c1", "2 tests failed\n", None));
    engine.sent();
    ui.click(cx, ("tool-send", row));
    assert!(
        matches!(
            engine.sent().as_slice(),
            [Op::UserMessage(message)]
                if message.contains("2 tests failed") && message.contains("npm test")
        ),
        "output not sent back"
    );
    // It shows in the transcript as a user message; the output itself is
    // what was sent, in a fenced block.
    let last = ui.read(cx, |app, _| app.session().view.items.last().cloned());
    assert!(
        matches!(&last, Some(Item::User(text)) if text.contains("output of `npm test`")),
        "{last:?}"
    );
}

#[gpui_kit::test]
fn an_agents_command_mirrors_into_a_read_only_tab(cx: &mut TestAppContext) {
    let ui = open_with(cx, terminal_options());
    let engine = ui.engine(cx);
    let row = running_command(&ui, cx, &engine, "npm test");
    engine.send(
        cx,
        AgentEvent::TerminalStarted {
            terminal_id: "t1".into(),
            call_id: Some("c1".into()),
            label: "claude: npm test".into(),
            cwd: None,
        },
    );
    // Mirrored, but not in the way until the card asks for it.
    assert_eq!(
        ui.read(cx, |app, _| (app.terminal.open, app.terminal.tabs.len())),
        (false, 1)
    );
    engine.send(
        cx,
        AgentEvent::TerminalOutput {
            terminal_id: "t1".into(),
            data: "2 tests failed\n".into(),
            replace: false,
        },
    );
    ui.click(cx, ("tool-terminal", row));
    assert!(ui.read(cx, |app, _| app.terminal.open));
    let lines = wait_for_terminal(&ui, cx, |l| l == "2 tests failed");
    assert!(lines.iter().any(|l| l == "2 tests failed"), "{lines:?}");
    // A whole-output update replaces the screen instead of appending.
    engine.send(
        cx,
        AgentEvent::TerminalOutput {
            terminal_id: "t1".into(),
            data: "all green\n".into(),
            replace: true,
        },
    );
    let lines = wait_for_terminal(&ui, cx, |l| l == "all green");
    assert!(!lines.iter().any(|l| l == "2 tests failed"), "{lines:?}");
    engine.send(
        cx,
        AgentEvent::TerminalExited {
            terminal_id: "t1".into(),
            exit_code: Some(1),
        },
    );
    let label = ui.read(cx, |app, cx| {
        app.terminal
            .active_view()
            .map(|view| view.read(cx).label())
            .unwrap_or_default()
    });
    assert_eq!(label, "claude: npm test (exit 1)");
}

#[gpui_kit::test]
fn a_mirrored_command_tab_is_never_typed_into(cx: &mut TestAppContext) {
    let ui = open_with(cx, terminal_options());
    let engine = ui.engine(cx);
    running_command(&ui, cx, &engine, "npm test");
    engine.send(
        cx,
        AgentEvent::TerminalStarted {
            terminal_id: "t1".into(),
            call_id: Some("c1".into()),
            label: "claude: npm test".into(),
            cwd: None,
        },
    );
    ui.click(cx, "terminal");
    assert!(ui.read(cx, |app, _| app.terminal.open));
    ui.input(cx, "echo nope");
    ui.press(cx, "enter");
    let lines = ui.read(cx, |app, cx| {
        app.terminal
            .active_view()
            .map(|view| view.read(cx).terminal.snapshot().text_lines())
            .unwrap_or_default()
    });
    assert!(!lines.iter().any(|l| l.contains("nope")), "{lines:?}");
    // Its tab offers no "send to agent": there is nothing to type.
    assert!(!has(&ui, cx, "terminal-send"));
}

#[gpui_kit::test]
fn the_sidebar_opens_the_terminal_dock(cx: &mut TestAppContext) {
    let ui = open_with(cx, terminal_options());
    assert!(!ui.read(cx, |app, _| app.terminal.open));
    ui.click(cx, "terminal");
    assert!(ui.read(cx, |app, _| app.terminal.open));
    assert_eq!(ui.read(cx, |app, _| app.terminal.tabs.len()), 1);
    ui.click(cx, "terminal");
    assert!(!ui.read(cx, |app, _| app.terminal.open));
}
