//! The root view: owns sessions, the composer and panel state, routes actions,
//! and pumps engine (or demo) events into the active session.

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
use gpui_kit::component::input::TextareaState;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::demo;
use crate::engine;
use crate::session::Session;
use crate::theme::palette;

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
    /// Submit this message on startup (scripting and screenshots).
    pub prompt: Option<String>,
    pub open_palette: bool,
    pub open_changes: bool,
    pub select_change: bool,
}

pub struct FlintApp {
    pub sessions: Vec<Session>,
    pub active: usize,
    pub workspace: PathBuf,
    pub branch: Option<String>,
    pub model: String,
    pub approval: ApprovalMode,
    pub sidebar_open: bool,
    pub changes_open: bool,
    pub selected_change: Option<usize>,
    pub palette: Option<Entity<CommandState>>,
    pub composer: Entity<TextareaState>,
    pub origin: Instant,
    focus: FocusHandle,
    options: Options,
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
                .auto_grow(1, 10)
                .submit_on_enter(true)
                .placeholder("Ask flint to build, fix, or explain something…")
        });
        let subscriptions = vec![cx.subscribe_in(
            &composer,
            window,
            |this, _, event: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { shift: false, .. } = event {
                    this.submit(window, cx);
                }
            },
        )];
        // Redraw once a second while a turn runs so the elapsed time ticks.
        let ticker = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(500))
                    .await;
                let alive = this.update(cx, |app, cx| {
                    if app.session().view.running {
                        cx.notify();
                    }
                });
                if alive.is_err() {
                    break;
                }
            }
        });
        let mut app = Self {
            branch: engine::git_branch(&workspace),
            workspace,
            sessions: vec![Session::new()],
            active: 0,
            model: engine::model_name(),
            approval: ApprovalMode::Auto,
            sidebar_open: true,
            changes_open: options.open_changes,
            selected_change: None,
            palette: None,
            composer,
            origin: Instant::now(),
            focus: cx.focus_handle(),
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

    fn start_demo(&mut self, cx: &mut Context<Self>) {
        let ix = self.active;
        let start = self.now();
        let session = &mut self.sessions[ix];
        session.demo = true;
        let change = session.view.push_user(demo::PROMPT.to_string());
        session.apply(change);
        let mut beats = demo::script(self.options.demo_approval);
        if let Some(stop) = self.options.demo_stop {
            beats.truncate(stop);
        }
        let instant = self.options.demo_instant;
        let select_change = self.options.select_change;
        session.pump = Some(cx.spawn(async move |this, cx| {
            // Scripted time, so instant playback still shows real durations.
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
            if select_change {
                this.update(cx, |app, cx| {
                    app.selected_change = Some(0);
                    cx.notify();
                })
                .ok();
            }
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
        let Some(session) = self.sessions.get_mut(ix) else {
            return;
        };
        let change = session.view.fold(event, now);
        session.apply(change);
        cx.notify();
    }

    pub fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.composer.read(cx).value().trim().to_string();
        if text.is_empty() {
            return;
        }
        if self.session().view.running && self.session().ops.is_none() {
            return;
        }
        self.composer
            .update(cx, |state, cx| state.set_value("", window, cx));
        let ix = self.active;
        let change = self.sessions[ix].view.push_user(text.clone());
        self.sessions[ix].apply(change);
        if let Err(err) = self.ensure_engine(ix, cx) {
            let now = self.now();
            let change = self.sessions[ix]
                .view
                .fold(AgentEvent::Error(format!("{err:#}")), now);
            self.sessions[ix].apply(change);
        } else if let Some(ops) = &self.sessions[ix].ops {
            ops.try_send(Op::UserMessage(text)).ok();
        }
        cx.notify();
    }

    /// Starts the engine for a session on its first message.
    fn ensure_engine(&mut self, ix: usize, cx: &mut Context<Self>) -> anyhow::Result<()> {
        if self.sessions[ix].ops.is_some() {
            return Ok(());
        }
        let config = engine::config_for(&self.workspace, self.approval)?;
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

    pub fn toggle_item(&mut self, ix: usize, cx: &mut Context<Self>) {
        let session = &mut self.sessions[self.active];
        let change = session.view.toggle_expanded(ix);
        session.list.pause_following_tail();
        session.apply(change);
        cx.notify();
    }

    pub fn select_session(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix < self.sessions.len() {
            self.active = ix;
            self.selected_change = None;
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

    fn new_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Reuse an untouched session instead of stacking empty ones.
        if let Some(ix) = self.sessions.iter().position(|s| s.view.items.is_empty()) {
            self.active = ix;
        } else {
            self.sessions.push(Session::new());
            self.active = self.sessions.len() - 1;
        }
        self.selected_change = None;
        self.composer
            .update(cx, |state, cx| state.focus(window, cx));
        cx.notify();
    }

    fn open_workspace(&mut self, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Open workspace".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            this.update(cx, |app, cx| {
                app.branch = engine::git_branch(&path);
                app.workspace = path;
                // Engines are bound to a workspace; new messages start fresh ones.
                app.sessions.push(Session::new());
                app.active = app.sessions.len() - 1;
                cx.notify();
            })
            .ok();
        })
        .detach();
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

impl Render for FlintApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette();
        let body = h_resizable("flint-body")
            .child(resizable_panel().child(crate::transcript::render_main(self, window, cx)))
            .child(
                resizable_panel()
                    .size(px(400.))
                    .size_range(px(300.)..px(760.))
                    .visible(self.changes_open)
                    .child(crate::changes_panel::render(self, cx)),
            );
        div()
            .id("flint")
            .key_context("FlintApp")
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &NewSession, window, cx| this.new_session(window, cx)))
            .on_action(cx.listener(|this, _: &TogglePalette, window, cx| {
                if this.palette.is_some() {
                    this.close_palette(window, cx);
                } else {
                    this.open_palette(window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &ToggleChanges, _, cx| {
                this.changes_open = !this.changes_open;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ToggleSidebar, _, cx| {
                this.sidebar_open = !this.sidebar_open;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ToggleApproval, _, cx| this.toggle_approval(cx)))
            .on_action(cx.listener(|this, _: &OpenWorkspace, _, cx| this.open_workspace(cx)))
            .on_action(cx.listener(|this, _: &Interrupt, _, cx| this.interrupt(cx)))
            .on_action(cx.listener(|this, _: &FocusComposer, window, cx| {
                this.composer
                    .update(cx, |state, cx| state.focus(window, cx));
            }))
            .size_full()
            .flex()
            .flex_col()
            .bg(p.bg)
            .text_color(p.text)
            .text_size(px(crate::theme::size::BASE))
            .child(crate::title_bar::render(self, cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .when(self.sidebar_open, |row| {
                        row.child(crate::sidebar::render(self, cx))
                    })
                    .child(div().flex_1().min_w_0().h_full().child(body)),
            )
            .child(crate::status_bar::render(self, cx))
            .when_some(self.palette.clone(), |root, state| {
                root.child(crate::palette::render(&state, cx))
            })
    }
}
