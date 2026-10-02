//! The root view's state and core behavior: sessions (each with its own
//! engine, running independently and addressed by a stable uid), the composer,
//! panels, and event pumping. Input menus live in `app_input.rs`, saved
//! sessions in `app_store.rs`, workspace/sidebar/settings actions in
//! `app_actions.rs`, and layout in `layout.rs`.

use std::path::PathBuf;
use std::time::Duration;
use std::time::Instant;

use flint_agent::ApprovalMode;
use flint_agent::ReasoningEffort;
use gpui_kit::component::command::CommandState;
use gpui_kit::component::input::InputEvent;
use gpui_kit::component::input::InputState;
use gpui_kit::component::input::TextareaState;
use gpui_kit::*;

use crate::engine;
use crate::session::Session;
use crate::settings::KeySources;
use crate::settings::Settings;

actions!(
    flint,
    [
        NewSession,
        NewClaudeSession,
        NewCodexSession,
        TogglePalette,
        ToggleChanges,
        ToggleSidebar,
        ToggleApproval,
        OpenWorkspace,
        OpenSettings,
        Interrupt,
        FocusComposer,
        RevealWorkspace,
        OpenTerminal,
        RenameSession,
        MenuUp,
        MenuDown,
        MenuAccept,
        MenuDismiss,
        DeleteSession,
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
    /// Automation: quit a few seconds after the first turn finishes.
    pub exit_after_turn: bool,
    /// Automation: replay this many synthetic turns (perf harness).
    pub demo_long: Option<usize>,
    /// Automation: stream deltas at this rate per second, then quit.
    pub stream_test: Option<u32>,
    /// Automation: scroll a long transcript, then quit.
    pub scroll_test: bool,
    pub open_settings: bool,
    pub open_mention: bool,
    pub open_slash: bool,
    pub select_change: bool,
    /// Settings and saved sessions live here (default `$FLINT_HOME` or `~/.flint`).
    pub home: Option<PathBuf>,
    /// API key file; overrides the `api_key_file` setting.
    pub key_path: Option<PathBuf>,
    /// Environment and legacy key file the key lookup may use; `None` means
    /// the process environment and `~/.fx/surplus.key`.
    pub key_sources: Option<KeySources>,
}

impl Options {
    /// Automation and demo runs neither load nor save sessions.
    pub(crate) fn ephemeral(&self) -> bool {
        self.demo || self.demo_long.is_some() || self.stream_test.is_some() || self.scroll_test
    }
}

/// Which sessions the sidebar lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionFilter {
    All,
    Running,
    Unread,
}

pub struct FlintApp {
    pub sessions: Vec<Session>,
    pub active: usize,
    /// Workspace new sessions start in.
    pub workspace: PathBuf,
    /// Where settings and saved sessions live.
    pub home: PathBuf,
    /// API key file override from the command line, ahead of the settings.
    pub key_path: Option<PathBuf>,
    pub key_sources: KeySources,
    pub settings: Settings,
    pub model: String,
    pub effort: Option<ReasoningEffort>,
    /// False once the endpoint said it ignores `reasoning_effort`.
    pub effort_supported: bool,
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
    pub mention: Option<crate::app_input::MentionMenu>,
    pub slash: Option<crate::app_input::SlashMenu>,
    /// Files attached with `@` for the next message.
    pub attachments: Vec<String>,
    pub settings_form: Option<crate::settings_view::SettingsForm>,
    pub help_open: bool,
    /// The agent picker above the composer is open.
    pub agent_menu: bool,
    /// Summary row whose answer was just copied, and when.
    pub copied: Option<(usize, Instant)>,
    /// Sidebar row with its context menu open.
    pub session_menu: Option<usize>,
    /// Session being renamed, with its title input.
    pub renaming: Option<(usize, Entity<InputState>)>,
    pub origin: Instant,
    pub(crate) focus: FocusHandle,
    /// Window drag from the header, armed on mouse down.
    pub(crate) drag_armed: bool,
    pub(crate) options: Options,
    pub(crate) next_uid: u64,
    pub(crate) file_index: std::collections::HashMap<PathBuf, Vec<String>>,
    pub(crate) subscriptions: Vec<Subscription>,
    /// Animation clock; runs only while a session is working.
    pub(crate) ticker: Option<Task<()>>,
    /// The composer had focus when the window went inactive; give it back on
    /// the next activation.
    pub(crate) refocus_composer: bool,
}

impl FlintApp {
    pub fn new(options: Options, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let workspace = options
            .workspace
            .clone()
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_default();
        let home = options
            .home
            .clone()
            .unwrap_or_else(crate::settings::flint_home);
        let key_path = options.key_path.clone();
        let key_sources = options.key_sources.clone().unwrap_or_default();
        let settings = Settings::load(&home, &key_sources);
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
                |this, _, event: &InputEvent, window, cx| match event {
                    InputEvent::PressEnter { shift: false, .. } => this.submit(window, cx),
                    InputEvent::Change => this.composer_changed(window, cx),
                    _ => {}
                },
            ),
            cx.subscribe(&search, |_, _, _: &InputEvent, cx| cx.notify()),
        ];
        let mut app = Self {
            sessions: Vec::new(),
            workspace: workspace.clone(),
            home,
            key_path,
            key_sources,
            active: 0,
            model: settings.model.clone(),
            effort: settings.reasoning_effort(),
            effort_supported: true,
            approval: settings.approval_mode(),
            settings,
            sidebar_open: true,
            sidebar_visible: true,
            changes_open: options.open_changes,
            selected_change: None,
            palette: None,
            composer,
            search,
            filter: SessionFilter::All,
            mention: None,
            slash: None,
            attachments: Vec::new(),
            settings_form: None,
            help_open: false,
            agent_menu: false,
            copied: None,
            session_menu: None,
            renaming: None,
            origin: Instant::now(),
            focus: cx.focus_handle(),
            drag_armed: false,
            options: options.clone(),
            next_uid: 0,
            file_index: Default::default(),
            subscriptions,
            ticker: None,
            refocus_composer: false,
        };
        if !options.ephemeral() {
            app.restore_sessions(cx);
        }
        let fresh = app.new_session_value(workspace);
        app.sessions.insert(0, fresh);
        app.active = 0;
        if options.demo {
            app.start_demo(cx);
        }
        if let Some(turns) = options.demo_long.or(options.scroll_test.then_some(200)) {
            app.load_long_session(turns, cx);
        }
        if let Some(rate) = options.stream_test {
            app.start_stream_test(rate, cx);
        }
        if options.scroll_test {
            app.start_scroll_test(cx);
        }
        if let Some(prompt) = options.prompt.clone().filter(|_| !options.demo) {
            app.composer
                .update(cx, |state, cx| state.set_value(prompt, window, cx));
            app.submit(window, cx);
        }
        crate::automation::schedule_dump(cx);
        if options.open_palette {
            app.open_palette(window, cx);
        } else if !crate::automation::background_launch() {
            app.composer.update(cx, |state, cx| state.focus(window, cx));
        }
        // gpui's input keeps its caret blink timer (and a repaint every
        // 500 ms) running while the window is inactive; it only hides the
        // caret. Release the composer's focus when the window goes inactive
        // so an idle background window never repaints, and give it back on
        // the next activation. A window that opened inactive (harness
        // background launch) also gets the composer on first activation.
        let sub = cx.observe_window_activation(window, |this, window, cx| {
            if window.is_window_active() {
                if this.refocus_composer || window.focused(cx).is_none() {
                    this.refocus_composer = false;
                    this.composer
                        .update(cx, |state, cx| state.focus(window, cx));
                }
            } else if this.composer.read(cx).focus_handle(cx).is_focused(window) {
                this.refocus_composer = true;
                window.blur(cx);
            }
        });
        app.subscriptions.push(sub);
        if options.open_settings {
            app.open_settings(window, cx);
        }
        if options.open_mention {
            app.open_mention_picker(window, cx);
        }
        if options.open_slash {
            app.composer
                .update(cx, |state, cx| state.set_value("/", window, cx));
            app.composer_changed(window, cx);
        }
        app
    }

    pub(crate) fn new_session_value(&mut self, workspace: PathBuf) -> Session {
        self.next_uid += 1;
        Session::new(self.next_uid, workspace)
    }

    pub fn session(&self) -> &Session {
        &self.sessions[self.active]
    }

    pub fn session_index(&self, uid: u64) -> Option<usize> {
        self.sessions.iter().position(|s| s.uid == uid)
    }

    pub fn now(&self) -> Duration {
        self.origin.elapsed()
    }

    pub fn branch(&self) -> Option<String> {
        engine::git_branch(&self.session().workspace)
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

    /// Copies a finished turn's answer and shows "Copied" for a moment.
    pub fn copy_answer(&mut self, ix: usize, text: String, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.copied = Some((ix, Instant::now()));
        self.sessions[self.active].list.remeasure_items(ix..ix + 1);
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(1600))
                .await;
            this.update(cx, |app, cx| {
                if app.copied.is_some_and(|(row, _)| row == ix) {
                    app.copied = None;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// Opens the changes panel on a file (or the first changed file).
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
            self.session_menu = None;
            self.composer
                .update(cx, |state, cx| state.focus(window, cx));
            cx.notify();
        }
    }

    pub fn select_change(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.selected_change = Some(ix);
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
                let session = self.new_session_value(self.workspace.clone());
                self.sessions.push(session);
                self.active = self.sessions.len() - 1;
            }
        }
        self.selected_change = None;
        self.composer
            .update(cx, |state, cx| state.focus(window, cx));
        cx.notify();
    }

    /// `/clear`: a fresh session in place of the active one (the old one stays
    /// saved and listed).
    pub(crate) fn clear_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.session().view.items.is_empty() {
            return;
        }
        let workspace = self.session().workspace.clone();
        let session = self.new_session_value(workspace);
        self.sessions.push(session);
        self.active = self.sessions.len() - 1;
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
