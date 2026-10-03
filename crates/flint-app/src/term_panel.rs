//! Tabs of terminals (new shells start in the session's workspace), toggled
//! with ⌃`. The workspace docking layout controls placement and resizing.
//! The open state is kept in settings.

use std::path::PathBuf;

use flint_term::Size as TermSize;
use flint_term::SpawnConfig;
use flint_term::Terminal;
use gpui_kit::assets::IconName;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::app::FlintApp;
use crate::term_view::AgentTerminal;
use crate::term_view::TermView;
use crate::theme::palette;
use crate::theme::size;
use crate::ui;

pub const MIN_HEIGHT: f32 = 120.;
pub const DEFAULT_HEIGHT: f32 = 280.;
const TAB_BAR_HEIGHT: f32 = 34.;

/// The dock's state.
pub struct TermPanel {
    pub open: bool,
    pub height: f32,
    pub tabs: Vec<Entity<TermView>>,
    pub active: usize,
}

impl TermPanel {
    pub fn new(open: bool, height: f32) -> Self {
        Self {
            open,
            height: height.max(MIN_HEIGHT),
            tabs: Vec::new(),
            active: 0,
        }
    }

    pub fn active_view(&self) -> Option<&Entity<TermView>> {
        self.tabs.get(self.active)
    }
}

/// A rough first size; the view refits to its bounds on the first frame.
pub(crate) fn initial_size() -> TermSize {
    TermSize {
        cols: 100,
        rows: 16,
        cell_width: 8,
        cell_height: 18,
    }
}

impl FlintApp {
    /// ⌃` and the header button: show (starting a shell if there is none)
    /// or hide the dock.
    pub fn toggle_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.terminal.open = !self.terminal.open;
        if self.terminal.open {
            if self.terminal.tabs.is_empty() {
                self.new_terminal(None, None, window, cx);
            } else if let Some(view) = self.terminal.active_view() {
                let focus = view.read(cx).focus.clone();
                window.focus(&focus, cx);
            }
        } else {
            self.composer
                .update(cx, |state, cx| state.focus(window, cx));
        }
        self.save_terminal_settings();
        cx.notify();
    }

    /// Opens a new shell tab in `cwd` (default: the session's workspace),
    /// optionally with `typed` already on its command line.
    pub fn new_terminal(
        &mut self,
        cwd: Option<PathBuf>,
        typed: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let cwd = cwd.unwrap_or_else(|| self.session().workspace.clone());
        let spawned = Terminal::spawn(SpawnConfig {
            cwd: cwd.clone(),
            command: self.options.terminal_command.clone(),
            env: Vec::new(),
            size: initial_size(),
        });
        let terminal = match spawned {
            Ok(terminal) => terminal,
            Err(err) => {
                let ix = self.active;
                self.apply_event(
                    ix,
                    flint_agent::AgentEvent::Error(format!("Couldn't start a shell: {err:#}")),
                    cx,
                );
                return;
            }
        };
        let poll = self.options.terminal_poll;
        let view = cx.new(|cx| TermView::new(terminal, cwd, false, poll, cx));
        if let Some(text) = typed {
            view.read(cx).type_text(&text);
        }
        self.add_terminal_tab(view, window, cx);
    }

    /// Adds a tab (also used for read-only agent terminals) and shows it.
    pub fn add_terminal_tab(
        &mut self,
        view: Entity<TermView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.observe(&view, |_, _, cx| cx.notify()).detach();
        let focus = view.read(cx).focus.clone();
        self.terminal.tabs.push(view);
        self.terminal.active = self.terminal.tabs.len() - 1;
        self.terminal.open = true;
        window.focus(&focus, cx);
        self.save_terminal_settings();
        cx.notify();
    }

    /// Appends a tab and makes it the active one, without taking focus or
    /// opening the dock (an agent's command tab appears when the dock does).
    fn push_terminal_tab(&mut self, view: Entity<TermView>, cx: &mut Context<Self>) {
        cx.observe(&view, |_, _, cx| cx.notify()).detach();
        self.terminal.tabs.push(view);
        self.terminal.active = self.terminal.tabs.len() - 1;
    }

    /// Mirrors a command an agent started as a read-only tab, once per
    /// terminal. It is not shown: the command card's "Open in terminal"
    /// brings it up.
    pub(crate) fn agent_terminal_started(
        &mut self,
        uid: u64,
        terminal_id: String,
        label: String,
        cx: &mut Context<Self>,
    ) {
        let key: AgentTerminal = (uid, terminal_id);
        if self.agent_tab(&key, cx).is_some() {
            return;
        }
        let Some(ix) = self.session_index(uid) else {
            return;
        };
        let cwd = self.sessions[ix].workspace.clone();
        let poll = self.options.terminal_poll;
        let view = cx.new(|cx| TermView::mirror(label, key, cwd, poll, cx));
        self.push_terminal_tab(view, cx);
        cx.notify();
    }

    /// Appends (or replaces) the output of a mirrored agent terminal.
    pub(crate) fn agent_terminal_output(
        &mut self,
        uid: u64,
        terminal_id: &str,
        data: &str,
        replace: bool,
        cx: &mut Context<Self>,
    ) {
        let key = (uid, terminal_id.to_string());
        let Some(ix) = self.agent_tab(&key, cx) else {
            return;
        };
        let view = self.terminal.tabs[ix].clone();
        view.update(cx, |view, cx| view.feed(data, replace, cx));
    }

    /// Marks a mirrored agent terminal finished.
    pub(crate) fn agent_terminal_exited(
        &mut self,
        uid: u64,
        terminal_id: &str,
        exit_code: Option<i32>,
        cx: &mut Context<Self>,
    ) {
        let key = (uid, terminal_id.to_string());
        let Some(ix) = self.agent_tab(&key, cx) else {
            return;
        };
        let view = self.terminal.tabs[ix].clone();
        view.update(cx, |view, cx| view.mark_exited(exit_code, cx));
    }

    /// The tab showing a session's mirrored agent terminal, if it is open.
    pub(crate) fn agent_tab(&self, key: &AgentTerminal, cx: &App) -> Option<usize> {
        self.terminal
            .tabs
            .iter()
            .position(|view| view.read(cx).agent.as_ref() == Some(key))
    }

    /// Brings a mirrored agent terminal up (the command card's button).
    pub(crate) fn show_agent_terminal(
        &mut self,
        uid: u64,
        terminal_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let key = (uid, terminal_id.to_string());
        let Some(ix) = self.agent_tab(&key, cx) else {
            return false;
        };
        self.terminal.open = true;
        self.select_terminal_tab(ix, window, cx);
        true
    }

    /// Closes every tab mirroring a deleted session's commands.
    pub(crate) fn close_agent_terminals(&mut self, uid: u64, cx: &mut Context<Self>) {
        let mut removed = false;
        self.terminal
            .tabs
            .retain(|view| match &view.read(cx).agent {
                Some((session, _)) if *session == uid => {
                    removed = true;
                    false
                }
                _ => true,
            });
        if removed {
            if self.terminal.tabs.is_empty() {
                self.terminal.open = false;
            }
            self.terminal.active = self
                .terminal
                .active
                .min(self.terminal.tabs.len().saturating_sub(1));
            cx.notify();
        }
    }

    /// Sends the active terminal's text to the session's agent as a message.
    pub(crate) fn send_terminal_to_agent(&mut self, cx: &mut Context<Self>) {
        let Some(view) = self.terminal.active_view() else {
            return;
        };
        let (text, label) = {
            let view = view.read(cx);
            (view.terminal.buffer_text(), view.label())
        };
        if text.trim().is_empty() {
            return;
        }
        let (shown, message) = crate::turns::terminal_message(&label, &text);
        let ix = self.active;
        self.send_message(ix, shown, message, cx);
    }

    pub fn select_terminal_tab(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(view) = self.terminal.tabs.get(ix) {
            let focus = view.read(cx).focus.clone();
            self.terminal.active = ix;
            window.focus(&focus, cx);
            cx.notify();
        }
    }

    /// Closes a tab (its shell is stopped); the dock hides with the last one.
    pub fn close_terminal_tab(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if ix >= self.terminal.tabs.len() {
            return;
        }
        self.terminal.tabs.remove(ix);
        if self.terminal.tabs.is_empty() {
            self.terminal.open = false;
            self.terminal.active = 0;
            self.composer
                .update(cx, |state, cx| state.focus(window, cx));
            self.save_terminal_settings();
        } else {
            self.terminal.active = self.terminal.active.min(self.terminal.tabs.len() - 1);
            let ix = self.terminal.active;
            self.select_terminal_tab(ix, window, cx);
        }
        cx.notify();
    }

    /// Whether keyboard focus is in a terminal (app shortcuts step aside).
    pub fn terminal_focused(&self, window: &Window, cx: &App) -> bool {
        self.terminal.open
            && self
                .terminal
                .tabs
                .iter()
                .any(|view| view.read(cx).focus.is_focused(window))
    }

    fn save_terminal_settings(&mut self) {
        self.settings.terminal_open = self.terminal.open;
        self.settings.terminal_height = self.terminal.height;
        if !self.options.ephemeral() {
            self.settings.save(&self.home).ok();
        }
    }
}

/// The dock, when open.
pub fn render(app: &FlintApp, cx: &mut Context<FlintApp>) -> Option<AnyElement> {
    if !app.terminal.open {
        return None;
    }
    let p = palette();
    let tabs: Vec<_> = app
        .terminal
        .tabs
        .iter()
        .enumerate()
        .map(|(n, view)| {
            let active = n == app.terminal.active;
            let read_only = view.read(cx).read_only;
            div()
                .id(("terminal-tab", n))
                .h(px(26.))
                .pl(px(10.))
                .pr(px(4.))
                .rounded(px(7.))
                .flex()
                .items_center()
                .gap(px(6.))
                .cursor_pointer()
                .when(active, |tab| tab.bg(p.raised))
                .when(!active, |tab| tab.hover(|s| s.bg(hsla(0., 0., 1., 0.04))))
                .on_click(
                    cx.listener(move |this, _, window, cx| this.select_terminal_tab(n, window, cx)),
                )
                .child(ui::icon(
                    if read_only {
                        IconName::Bot
                    } else {
                        IconName::SquareTerminal
                    },
                    13.,
                    if active { p.text } else { p.text_subtle },
                ))
                .child(
                    div()
                        .max_w(px(220.))
                        .truncate()
                        .text_size(px(size::SM))
                        .text_color(if active { p.text } else { p.text_muted })
                        .child(view.read(cx).label()),
                )
                .child(
                    div()
                        .id(("terminal-close", n))
                        .size(px(18.))
                        .rounded(px(4.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .hover(|s| s.bg(p.border_strong))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            this.close_terminal_tab(n, window, cx);
                        }))
                        .child(ui::icon(IconName::X, 11., p.text_subtle))
                        .test_support(),
                )
                .test_support()
        })
        .collect();
    let small_button = |id: &'static str, icon: IconName| {
        div()
            .id(id)
            .size(px(26.))
            .rounded(px(7.))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .hover(|s| s.bg(p.raised))
            .child(ui::icon(icon, 14., p.text_muted))
    };
    let tab_bar = div()
        .h(px(TAB_BAR_HEIGHT))
        .flex_shrink_0()
        .px(px(8.))
        .flex()
        .items_center()
        .gap(px(4.))
        .border_b_1()
        .border_color(p.border)
        .child(crate::docking::handle(crate::docking::Panel::Terminal, cx))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(2.))
                .overflow_hidden()
                .children(tabs),
        )
        .child(
            small_button("terminal-new", IconName::Plus)
                .on_click(
                    cx.listener(|this, _, window, cx| this.new_terminal(None, None, window, cx)),
                )
                .test_support(),
        )
        .when(
            app.terminal
                .active_view()
                .is_some_and(|view| !view.read(cx).read_only),
            |bar| {
                bar.child(
                    small_button("terminal-send", IconName::Send)
                        .tooltip(|window, cx| {
                            Tooltip::new("Send this terminal to the agent").build(window, cx)
                        })
                        .on_click(cx.listener(|this, _, _, cx| this.send_terminal_to_agent(cx)))
                        .test_support(),
                )
            },
        )
        .child(div().flex_1())
        .child(ui::label("⌃`", size::XS, p.text_subtle))
        .child(
            small_button("terminal-hide", IconName::ChevronDown)
                .on_click(cx.listener(|this, _, window, cx| this.toggle_terminal(window, cx)))
                .test_support(),
        );
    Some(
        div()
            .id("terminal-panel")
            .size_full()
            .flex()
            .flex_col()
            .border_t_1()
            .border_color(p.border_strong)
            .bg(p.bg)
            .child(tab_bar)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .px(px(10.))
                    .pt(px(6.))
                    .children(app.terminal.active_view().cloned()),
            )
            .test_support()
            .into_any_element(),
    )
}
