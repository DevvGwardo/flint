//! The terminal dock at the bottom of the main column: tabs of terminals
//! (new shells start in the session's workspace), toggled with ⌃`,
//! resized by dragging its top edge. Height and open state are kept in
//! settings.

use std::path::PathBuf;

use flint_term::Size as TermSize;
use flint_term::SpawnConfig;
use flint_term::Terminal;
use gpui_kit::assets::IconName;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::app::FlintApp;
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
    /// Drag in progress on the top edge: (pointer y at start, height at start).
    pub resizing: Option<(Pixels, f32)>,
}

impl TermPanel {
    pub fn new(open: bool, height: f32) -> Self {
        Self {
            open,
            height: height.max(MIN_HEIGHT),
            tabs: Vec::new(),
            active: 0,
            resizing: None,
        }
    }

    pub fn active_view(&self) -> Option<&Entity<TermView>> {
        self.tabs.get(self.active)
    }
}

/// A rough first size; the view refits to its bounds on the first frame.
fn initial_size() -> TermSize {
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

    pub fn start_terminal_resize(&mut self, y: Pixels) {
        self.terminal.resizing = Some((y, self.terminal.height));
    }

    /// Mouse moved while dragging the dock's edge.
    pub fn drag_terminal_edge(&mut self, y: Pixels, window: &Window, cx: &mut Context<Self>) {
        let Some((start_y, start_height)) = self.terminal.resizing else {
            return;
        };
        let max = (f32::from(window.viewport_size().height) - 240.).max(MIN_HEIGHT);
        self.terminal.height = (start_height + f32::from(start_y - y)).clamp(MIN_HEIGHT, max);
        cx.notify();
    }

    pub fn end_terminal_resize(&mut self) {
        if self.terminal.resizing.take().is_some() {
            self.save_terminal_settings();
        }
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
    let tabs = app.terminal.tabs.iter().enumerate().map(|(n, view)| {
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
    });
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
        .child(div().flex_1())
        .child(ui::label("⌃`", size::XS, p.text_subtle))
        .child(
            small_button("terminal-hide", IconName::ChevronDown)
                .on_click(cx.listener(|this, _, window, cx| this.toggle_terminal(window, cx)))
                .test_support(),
        );
    // The top edge is the resize handle.
    let edge = div()
        .id("terminal-edge")
        .h(px(5.))
        .w_full()
        .flex_shrink_0()
        .cursor(CursorStyle::ResizeUpDown)
        .hover(|s| s.bg(p.accent.opacity(0.4)))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, event: &MouseDownEvent, _, _| {
                this.start_terminal_resize(event.position.y)
            }),
        )
        .test_support();
    Some(
        div()
            .id("terminal-panel")
            .h(px(app.terminal.height))
            .w_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .border_t_1()
            .border_color(p.border_strong)
            .bg(p.bg)
            .child(edge)
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
