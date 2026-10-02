//! The root view's state and behavior: sessions (each with its own engine,
//! running independently), the composer, panels, and event pumping. Layout
//! lives in `layout.rs`.

use std::path::PathBuf;
use std::time::Duration;
use std::time::Instant;

use flint_agent::AgentEvent;
use flint_agent::ApprovalDecision;
use flint_agent::ApprovalMode;
use flint_agent::Op;
use flint_agent::TurnEndReason;
use gpui_kit::component::command::CommandState;
use gpui_kit::component::input::InputEvent;
use gpui_kit::component::input::InputState;
use gpui_kit::component::input::TextareaState;
use gpui_kit::*;

use crate::demo;
use crate::engine;
use crate::session::Session;

actions!(
    flint,
    [
        NewSession,
        TogglePalette,
        ToggleChanges,
        ToggleSidebar,
        ToggleApproval,
        OpenWorkspace,
        Interrupt,
        FocusComposer,
        RevealWorkspace,
        Quit,
    ]
);

/// Startup options parsed from the command line.
#[derive(Debug, Clone, Default)]
pub struct Options {
    pub workspace: Option<PathBuf>,
    pub demo: bool,
    /// Demo: stop after this many scripted events (mid-stream screenshots).
    pub demo_stop: Option<usize>,
    /// Demo: apply every event immediately.
    pub demo_instant: bool,
    /// Demo: ask for approval before the final test run.
    pub demo_approval: bool,
    /// Demo: expand the finished turn's work block.
    pub demo_expand: bool,
    /// Submit this message on startup (scripting and screenshots).
    pub prompt: Option<String>,
    pub open_palette: bool,
    pub open_changes: bool,
    /// Initial window size, e.g. `1100x800`.
    pub window_size: Option<(f32, f32)>,
    pub select_change: bool,
}

/// Which sessions the sidebar lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionFilter {
    All,
    Running,
    Unread,
}

/// Reasoning effort shown on the model chip. The engine protocol has no
/// effort setting yet, so this is display-only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effort {
    Low,
    Medium,
    High,
}

pub struct FlintApp {
    pub sessions: Vec<Session>,
    pub active: usize,
    /// Workspace new sessions start in.
    pub workspace: PathBuf,
    pub model: String,
    pub effort: Effort,
    pub approval: ApprovalMode,
    pub sidebar_open: bool,
    /// Whether the sidebar is on screen this frame (it auto-collapses when narrow).
    pub sidebar_visible: bool,
    pub changes_open: bool,
    pub selected_change: Option<usize>,
    pub palette: Option<Entity<CommandState>>,
    pub composer: Entity<TextareaState>,
    pub search: Entity<InputState>,
    pub filter: SessionFilter,
    pub notice_dismissed: bool,
    pub origin: Instant,
    pub(crate) focus: FocusHandle,
    /// Window drag from the header, armed on mouse down.
    pub(crate) drag_armed: bool,
    pub(crate) options: Options,
    _subscriptions: Vec<Subscription>,
    _ticker: Task<()>,
}

impl FlintApp {
    pub fn new(options: Options, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let workspace = options
            .workspace
            .clone()
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_default();
        let composer = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(1, 8)
                .submit_on_enter(true)
                .placeholder("Ask anything, @ to mention, / for actions")
        });
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search sessions"));
        let subscriptions = vec![
            cx.subscribe_in(
                &composer,
                window,
                |this, _, event: &InputEvent, window, cx| {
                    if let InputEvent::PressEnter { shift: false, .. } = event {
                        this.submit(window, cx);
                    }
                },
            ),
            cx.subscribe(&search, |_, _, _: &InputEvent, cx| cx.notify()),
        ];
        // Animation clock: fast while anything runs (status glyphs, timers).
        let ticker = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(120))
                    .await;
                let alive = this.update(cx, |app, cx| {
                    if app.sessions.iter().any(|s| s.view.running) {
                        cx.notify();
                    }
                });
                if alive.is_err() {
                    break;
                }
            }
        });
        let mut app = Self {
            sessions: vec![Session::new(workspace.clone())],
            workspace,
            active: 0,
            model: engine::model_name(),
            effort: Effort::Medium,
            approval: ApprovalMode::Auto,
            sidebar_open: true,
            sidebar_visible: true,
            changes_open: options.open_changes,
            selected_change: None,
            palette: None,
            composer,
            search,
            filter: SessionFilter::All,
            notice_dismissed: false,
            origin: Instant::now(),
            focus: cx.focus_handle(),
            drag_armed: false,
            options: options.clone(),
            _subscriptions: subscriptions,
            _ticker: ticker,
        };
        if options.demo {
            app.start_demo(cx);
        } else if let Some(prompt) = options.prompt.clone() {
            app.composer
                .update(cx, |state, cx| state.set_value(prompt, window, cx));
            app.submit(window, cx);
        }
        if options.open_palette {
            app.open_palette(window, cx);
        } else {
            app.composer.update(cx, |state, cx| state.focus(window, cx));
        }
        app
    }

    pub fn session(&self) -> &Session {
        &self.sessions[self.active]
    }

    pub fn now(&self) -> Duration {
        self.origin.elapsed()
    }

    pub fn branch(&self) -> Option<String> {
        engine::git_branch(&self.session().workspace)
    }

    fn start_demo(&mut self, cx: &mut Context<Self>) {
        let change = self.sessions[0].view.push_user(demo::PROMPT.to_string());
        self.sessions[0].apply(change);
        let mut beats = demo::script(self.options.demo_approval);
        if let Some(stop) = self.options.demo_stop {
            beats.truncate(stop);
        }
        self.play(0, beats, self.options.demo_instant, cx);
        // Other sessions working in the background, to show concurrency.
        for extra in demo::extra_sessions(&self.workspace) {
            let mut session = Session::new(extra.workspace);
            let change = session.view.push_user(extra.prompt.to_string());
            session.apply(change);
            self.sessions.push(session);
            let ix = self.sessions.len() - 1;
            self.play(ix, extra.beats, extra.instant, cx);
        }
    }

    /// Plays scripted events into a session with scripted time, so instant
    /// playback still shows real durations.
    fn play(&mut self, ix: usize, beats: Vec<demo::Beat>, instant: bool, cx: &mut Context<Self>) {
        let start = self.now();
        let select_change = self.options.select_change && ix == self.active;
        let expand = self.options.demo_expand;
        self.sessions[ix].pump = Some(cx.spawn(async move |this, cx| {
            let mut clock = start;
            for beat in beats {
                clock += beat.delay;
                if !instant {
                    cx.background_executor().timer(beat.delay).await;
                }
                if this
                    .update(cx, |app, cx| app.apply_event_at(ix, beat.event, clock, cx))
                    .is_err()
                {
                    return;
                }
            }
            this.update(cx, |app, cx| {
                if app.active == ix {
                    if select_change {
                        app.changes_open = true;
                        app.selected_change = Some(0);
                    }
                    if expand {
                        app.toggle_work(1, cx);
                    }
                }
                cx.notify();
            })
            .ok();
        }));
    }

    pub fn apply_event(&mut self, ix: usize, event: AgentEvent, cx: &mut Context<Self>) {
        let now = self.now();
        self.apply_event_at(ix, event, now, cx);
    }

    fn apply_event_at(
        &mut self,
        ix: usize,
        event: AgentEvent,
        now: Duration,
        cx: &mut Context<Self>,
    ) {
        let active = self.active;
        let Some(session) = self.sessions.get_mut(ix) else {
            return;
        };
        let finished = matches!(event, AgentEvent::TurnFinished { .. });
        let change = session.view.fold(event, now);
        session.apply(change);
        session.touched = Instant::now();
        if finished && ix != active {
            session.unread = true;
        }
        cx.notify();
    }

    pub fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.composer.read(cx).value().trim().to_string();
        if text.is_empty() {
            return;
        }
        let ix = self.active;
        if self.sessions[ix].view.running && self.sessions[ix].ops.is_none() {
            return;
        }
        self.composer
            .update(cx, |state, cx| state.set_value("", window, cx));
        let change = self.sessions[ix].view.push_user(text.clone());
        self.sessions[ix].apply(change);
        self.sessions[ix].touched = Instant::now();
        match self.ensure_engine(ix, cx) {
            Ok(()) => {
                if let Some(ops) = &self.sessions[ix].ops {
                    ops.try_send(Op::UserMessage(text)).ok();
                }
            }
            Err(err) => self.apply_event(ix, AgentEvent::Error(format!("{err:#}")), cx),
        }
        cx.notify();
    }

    /// Starts the engine for a session on its first message.
    fn ensure_engine(&mut self, ix: usize, cx: &mut Context<Self>) -> anyhow::Result<()> {
        if self.sessions[ix].ops.is_some() {
            return Ok(());
        }
        let config = engine::config_for(&self.sessions[ix].workspace, self.approval)?;
        let handle = flint_agent::spawn_session(config);
        let events = handle.events.clone();
        let session = &mut self.sessions[ix];
        session.ops = Some(handle.ops);
        session.pump = Some(cx.spawn(async move |this, cx| {
            while let Ok(event) = events.recv().await {
                if this
                    .update(cx, |app, cx| app.apply_event(ix, event, cx))
                    .is_err()
                {
                    break;
                }
            }
        }));
        Ok(())
    }

    pub fn interrupt(&mut self, cx: &mut Context<Self>) {
        let ix = self.active;
        if !self.sessions[ix].view.running {
            return;
        }
        if let Some(ops) = &self.sessions[ix].ops {
            ops.try_send(Op::Interrupt).ok();
            return;
        }
        // Demo: stop playback and close the turn.
        self.sessions[ix].pump = None;
        let turn_id = self.sessions[ix].view.turn_id;
        self.apply_event(
            ix,
            AgentEvent::TurnFinished {
                turn_id,
                reason: TurnEndReason::Interrupted,
            },
            cx,
        );
    }

    pub fn answer_approval(
        &mut self,
        call_id: String,
        decision: ApprovalDecision,
        cx: &mut Context<Self>,
    ) {
        let session = &mut self.sessions[self.active];
        let change = session.view.resolve_approval(&call_id, decision);
        session.apply(change);
        if let Some(ops) = &session.ops {
            ops.try_send(Op::Approval { call_id, decision }).ok();
        }
        if decision == ApprovalDecision::ApproveAlways {
            self.approval = ApprovalMode::Auto;
        }
        cx.notify();
    }

    /// Answers the oldest unanswered approval in the active session.
    pub fn answer_pending(&mut self, decision: ApprovalDecision, cx: &mut Context<Self>) {
        let pending = self
            .session()
            .view
            .items
            .iter()
            .find_map(|item| match item {
                crate::view_model::Item::Approval {
                    call_id,
                    decision: None,
                    ..
                } => Some(call_id.clone()),
                _ => None,
            });
        if let Some(call_id) = pending {
            self.answer_approval(call_id, decision, cx);
        }
    }

    pub fn toggle_item(&mut self, ix: usize, cx: &mut Context<Self>) {
        let session = &mut self.sessions[self.active];
        let change = session.view.toggle_expanded(ix);
        session.list.pause_following_tail();
        session.apply(change);
        cx.notify();
    }

    pub fn toggle_work(&mut self, ix: usize, cx: &mut Context<Self>) {
        let session = &mut self.sessions[self.active];
        let change = session.view.toggle_work(ix);
        session.list.pause_following_tail();
        session.apply(change);
        cx.notify();
    }

    pub fn feedback(&mut self, ix: usize, positive: bool, cx: &mut Context<Self>) {
        let session = &mut self.sessions[self.active];
        let change = session.view.set_feedback(ix, positive);
        session.apply(change);
        cx.notify();
    }

    /// Opens the changes panel on the first file a turn changed.
    pub fn review(&mut self, path: Option<String>, cx: &mut Context<Self>) {
        let changes = &self.session().view.changes;
        self.selected_change = path
            .and_then(|path| changes.iter().position(|f| f.path == path))
            .or(if changes.is_empty() { None } else { Some(0) });
        self.changes_open = true;
        cx.notify();
    }

    pub fn select_session(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if ix < self.sessions.len() {
            self.active = ix;
            self.sessions[ix].unread = false;
            self.selected_change = None;
            self.composer
                .update(cx, |state, cx| state.focus(window, cx));
            cx.notify();
        }
    }

    pub fn select_change(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.selected_change = if self.selected_change == Some(ix) {
            None
        } else {
            Some(ix)
        };
        self.changes_open = true;
        cx.notify();
    }

    pub(crate) fn new_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Reuse an untouched session in this workspace instead of stacking empty ones.
        let reusable = self.sessions.iter().position(|s| {
            s.view.items.is_empty() && s.ops.is_none() && s.workspace == self.workspace
        });
        match reusable {
            Some(ix) => self.active = ix,
            None => {
                self.sessions.push(Session::new(self.workspace.clone()));
                self.active = self.sessions.len() - 1;
            }
        }
        self.selected_change = None;
        self.composer
            .update(cx, |state, cx| state.focus(window, cx));
        cx.notify();
    }

    pub fn open_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let state = cx.new(|cx| CommandState::new(window, cx));
        state.update(cx, |state, cx| state.focus(window, cx));
        self.palette = Some(state);
        cx.notify();
    }

    pub fn close_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.palette = None;
        self.composer
            .update(cx, |state, cx| state.focus(window, cx));
        cx.notify();
    }

    pub fn toggle_approval(&mut self, cx: &mut Context<Self>) {
        self.approval = match self.approval {
            ApprovalMode::Auto => ApprovalMode::AskForChanges,
            ApprovalMode::AskForChanges => ApprovalMode::Auto,
        };
        cx.notify();
    }
}
