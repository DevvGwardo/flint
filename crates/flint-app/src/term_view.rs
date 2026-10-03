//! One terminal tab: a [`flint_term::Terminal`] shown and driven in GPUI.
//!
//! Output wakes the view through a coalesced event; the grid is snapshotted
//! and painted on the next frame only (an idle terminal never repaints).
//! Special keys and Ctrl/Alt combinations are encoded on key down; plain
//! text, including IME composition, arrives through the input handler.

use std::ops::Range;
use std::path::PathBuf;
use std::sync::LazyLock;
use std::time::Duration;
use std::time::Instant;

use flint_term::Mods;
use flint_term::Size as TermSize;
use flint_term::TermEvent;
use flint_term::Terminal;
use gpui_kit::*;
use regex::Regex;

use crate::term_paint::Metrics;
use crate::term_paint::PaintState;

actions!(terminal, [TerminalShiftTab]);

/// Keys encoded on key down rather than delivered as text.
const NAMED_KEYS: &[&str] = &[
    "up",
    "down",
    "left",
    "right",
    "home",
    "end",
    "pageup",
    "pagedown",
    "insert",
    "delete",
    "enter",
    "tab",
    "escape",
    "backspace",
    "f1",
    "f2",
    "f3",
    "f4",
    "f5",
    "f6",
    "f7",
    "f8",
    "f9",
    "f10",
    "f11",
    "f12",
];

static URL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(https?|file)://[^\s<>"'`)\]}]+"#).expect("url regex"));

pub struct TermView {
    pub terminal: Terminal,
    pub focus: FocusHandle,
    pub cwd: PathBuf,
    /// Title set by the program (OSC 0/2), if any.
    pub title: Option<String>,
    /// Agent output mirrors: shown, never typed into.
    pub read_only: bool,
    /// Fixed label (e.g. "claude: npm test") instead of title/cwd.
    pub fixed_label: Option<String>,
    /// Set on a mirror of a command an agent is running: the session it
    /// belongs to and the agent's terminal id.
    pub agent: Option<AgentTerminal>,
    pub exited: Option<Option<i32>>,
    bell_until: Option<Instant>,
    /// IME text being composed.
    marked: Option<String>,
    metrics: Option<Metrics>,
    rows: usize,
    cols: usize,
    scroll_accum: f32,
    dragging: bool,
    _pump: Task<()>,
}

/// Identifies a mirrored agent command: its session and the agent's id.
pub type AgentTerminal = (u64, String);

/// Clears the screen, the scrollback and homes the cursor.
const RESET: &[u8] = b"\x1b[2J\x1b[3J\x1b[H";

impl TermView {
    /// `poll` checks for output on a frame timer instead of waiting on the
    /// PTY thread's wakeups (tests only; the app waits, so idle costs nothing).
    pub fn new(
        terminal: Terminal,
        cwd: PathBuf,
        read_only: bool,
        poll: bool,
        cx: &mut Context<Self>,
    ) -> Self {
        let events = terminal.events();
        let pump = cx.spawn(async move |this, cx| {
            if poll {
                loop {
                    cx.background_executor()
                        .timer(Duration::from_millis(16))
                        .await;
                    while let Ok(event) = events.try_recv() {
                        if this
                            .update(cx, |view, cx| view.on_event(event, cx))
                            .is_err()
                        {
                            return;
                        }
                    }
                }
            }
            while let Ok(event) = events.recv().await {
                if this
                    .update(cx, |view, cx| view.on_event(event, cx))
                    .is_err()
                {
                    break;
                }
            }
        });
        let size = terminal.size();
        Self {
            terminal,
            focus: cx.focus_handle(),
            cwd,
            title: None,
            read_only,
            fixed_label: None,
            agent: None,
            exited: None,
            bell_until: None,
            marked: None,
            metrics: None,
            rows: usize::from(size.rows),
            cols: usize::from(size.cols),
            scroll_accum: 0.,
            dragging: false,
            _pump: pump,
        }
    }

    /// A read-only mirror of a command an agent runs: flint feeds it the
    /// output the agent reports, there is no process behind it.
    pub fn mirror(
        label: String,
        agent: AgentTerminal,
        cwd: PathBuf,
        poll: bool,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut view = Self::new(
            Terminal::detached(crate::term_panel::initial_size()),
            cwd,
            true,
            poll,
            cx,
        );
        view.fixed_label = Some(label);
        view.agent = Some(agent);
        view
    }

    /// Appends output the agent reported for this mirror (`replace` sends the
    /// command's whole output so far, which resets the screen first).
    pub fn feed(&mut self, data: &str, replace: bool, cx: &mut Context<Self>) {
        if replace {
            self.terminal.feed(RESET);
        }
        self.terminal.feed(data.as_bytes());
        cx.notify();
    }

    /// The mirrored command finished.
    pub fn mark_exited(&mut self, code: Option<i32>, cx: &mut Context<Self>) {
        self.exited = Some(code);
        cx.notify();
    }

    /// Tab label: a fixed label, the program's title, or the folder.
    pub fn label(&self) -> String {
        let base = self.fixed_label.clone().unwrap_or_else(|| {
            self.title
                .clone()
                .filter(|t| !t.trim().is_empty())
                .unwrap_or_else(|| crate::session::folder_name(&self.cwd))
        });
        match self.exited {
            Some(Some(code)) if code != 0 => format!("{base} (exit {code})"),
            Some(_) => format!("{base} (done)"),
            None => base,
        }
    }

    fn on_event(&mut self, event: TermEvent, cx: &mut Context<Self>) {
        match event {
            TermEvent::Wakeup => {}
            TermEvent::Title(title) => self.title = Some(title),
            TermEvent::Bell => {
                self.bell_until = Some(Instant::now() + Duration::from_millis(180));
                cx.spawn(async move |this, cx| {
                    cx.background_executor()
                        .timer(Duration::from_millis(200))
                        .await;
                    this.update(cx, |_, cx| cx.notify()).ok();
                })
                .detach();
            }
            TermEvent::Exited(code) => self.exited = Some(code),
        }
        cx.notify();
    }

    pub fn write(&self, bytes: impl Into<Vec<u8>>) {
        if !self.read_only {
            self.terminal.write(bytes);
        }
    }

    /// Types `text` into the shell without pressing Enter.
    pub fn type_text(&self, text: &str) {
        self.write(flint_term::paste_bytes(
            text,
            self.terminal.bracketed_paste(),
        ));
    }

    pub fn copy(&self, cx: &mut Context<Self>) -> bool {
        match self.terminal.selection_text() {
            Some(text) => {
                cx.write_to_clipboard(ClipboardItem::new_string(text));
                true
            }
            None => false,
        }
    }

    pub fn paste_clipboard(&self, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.type_text(&text);
        }
    }

    fn key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let m = keystroke.modifiers;
        if m.platform {
            let handled = match keystroke.key.as_str() {
                "c" => self.copy(cx),
                "v" => {
                    self.paste_clipboard(cx);
                    true
                }
                _ => false,
            };
            if handled {
                cx.stop_propagation();
            }
            return;
        }
        if self.read_only {
            return;
        }
        let key = keystroke.key.as_str();
        let single = key.chars().count() == 1 || key == "space";
        if NAMED_KEYS.contains(&key) || (single && (m.control || m.alt)) {
            let mods = Mods {
                shift: m.shift,
                ctrl: m.control,
                alt: m.alt,
            };
            if let Some(bytes) = flint_term::encode_key(key, mods, self.terminal.app_cursor()) {
                self.terminal.clear_selection();
                self.write(bytes);
                cx.stop_propagation();
                cx.notify();
            }
        }
    }

    fn mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        let Some(metrics) = self.metrics else {
            return;
        };
        let (row, col, right) = metrics.cell_at(event.position, self.cols, self.rows);
        if event.modifiers.platform {
            if let Some(url) = self.url_at(row, col) {
                cx.open_url(&url);
            }
            return;
        }
        // 1 click: an empty selection that dragging extends; 2: word; 3: line.
        self.terminal
            .start_selection(row, col, right, event.click_count.clamp(1, 3) as u8);
        self.dragging = true;
        cx.notify();
    }

    fn mouse_move(&mut self, event: &MouseMoveEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if !self.dragging || event.pressed_button != Some(MouseButton::Left) {
            return;
        }
        if let Some(metrics) = self.metrics {
            let (row, col, right) = metrics.cell_at(event.position, self.cols, self.rows);
            self.terminal.update_selection(row, col, right);
            cx.notify();
        }
    }

    fn mouse_up(&mut self, _: &MouseUpEvent, _window: &mut Window, _cx: &mut Context<Self>) {
        self.dragging = false;
    }

    fn scroll(&mut self, event: &ScrollWheelEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(metrics) = self.metrics else {
            return;
        };
        let line = metrics.cell.height;
        self.scroll_accum += event.delta.pixel_delta(line).y / line;
        let lines = self.scroll_accum.trunc() as i32;
        if lines == 0 {
            return;
        }
        self.scroll_accum -= lines as f32;
        if self.terminal.alt_screen() && !self.read_only {
            // Full-screen programs scroll with arrow keys.
            let key = if lines > 0 { "up" } else { "down" };
            let bytes = flint_term::encode_key(key, Mods::default(), self.terminal.app_cursor())
                .unwrap_or_default();
            self.write(bytes.repeat(lines.unsigned_abs() as usize));
        } else {
            self.terminal.scroll(lines);
        }
        cx.notify();
    }

    fn url_at(&self, row: usize, col: usize) -> Option<String> {
        let line = self.terminal.snapshot().text_lines().get(row)?.clone();
        URL.find_iter(&line).find_map(|m| {
            let start = line[..m.start()].chars().count();
            let end = start + m.as_str().chars().count();
            (col >= start && col < end).then(|| m.as_str().to_string())
        })
    }

    /// Fits the grid to `bounds` (tells the program on change).
    fn fit(&mut self, bounds: Bounds<Pixels>, metrics: Metrics) {
        let (cols, rows) = metrics.grid(bounds);
        let size = TermSize {
            cols,
            rows,
            cell_width: f32::from(metrics.cell.width).round() as u16,
            cell_height: f32::from(metrics.cell.height).round() as u16,
        };
        self.terminal.resize(size);
        self.cols = usize::from(cols);
        self.rows = usize::from(rows);
        self.metrics = Some(Metrics {
            origin: bounds.origin,
            ..metrics
        });
    }
}

impl Focusable for TermView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for TermView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = cx.entity();
        let measured = Metrics::measure(window);
        let palette = self.terminal.palette.clone();
        div()
            .id("terminal-view")
            .key_context("Terminal")
            .track_focus(&self.focus)
            .size_full()
            .bg(crate::term_paint::color(palette.background))
            .cursor(CursorStyle::IBeam)
            .on_key_down(cx.listener(Self::key_down))
            .on_action(
                cx.listener(|this, _: &TerminalShiftTab, _, _| this.write(b"\x1b[Z".to_vec())),
            )
            .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
            .on_mouse_move(cx.listener(Self::mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::mouse_up))
            .on_scroll_wheel(cx.listener(Self::scroll))
            .child(
                canvas(
                    {
                        let view = view.clone();
                        move |bounds, window, cx| {
                            view.update(cx, |this, _| {
                                this.fit(bounds, measured);
                                // Acknowledge before snapshotting, so output
                                // arriving after this frame wakes the next.
                                this.terminal.acknowledge_wakeup();
                                let snapshot = this.terminal.snapshot();
                                let state = PaintState {
                                    focused: this.focus.is_focused(window),
                                    marked: this.marked.clone(),
                                    bell: this.bell_until.is_some_and(|t| Instant::now() < t),
                                    palette: this.terminal.palette.clone(),
                                };
                                (snapshot, this.metrics.unwrap_or(measured), state)
                            })
                        }
                    },
                    move |bounds, (snapshot, metrics, state), window, cx| {
                        crate::term_paint::paint(bounds, &snapshot, &metrics, &state, window, cx);
                        let focus = view.read(cx).focus.clone();
                        window.handle_input(
                            &focus,
                            ElementInputHandler::new(bounds, view.clone()),
                            cx,
                        );
                    },
                )
                .size_full(),
            )
    }
}

impl EntityInputHandler for TermView {
    fn text_for_range(
        &mut self,
        _range: Range<usize>,
        _adjusted: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        Some(String::new())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: 0..0,
            reversed: false,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.marked.as_ref().map(|m| 0..m.encode_utf16().count())
    }

    fn unmark_text(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.marked = None;
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        _range: Option<Range<usize>>,
        text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.marked = None;
        if !text.is_empty() {
            self.terminal.clear_selection();
            self.write(text.as_bytes().to_vec());
        }
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        _range: Option<Range<usize>>,
        new_text: &str,
        _new_selected_range: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.marked = (!new_text.is_empty()).then(|| new_text.to_string());
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        _range_utf16: Range<usize>,
        _element_bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        // The IME candidate window sits at the cursor.
        let metrics = self.metrics?;
        let cursor = self.terminal.snapshot().cursor;
        Some(metrics.cell_bounds(cursor.row, cursor.col, 1))
    }

    fn character_index_for_point(
        &mut self,
        _point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        None
    }

    fn accepts_text_input(&self, _window: &mut Window, _cx: &mut Context<Self>) -> bool {
        !self.read_only
    }
}
