//! A terminal: alacritty's parser and grid, fed either by a real PTY
//! (alacritty's event loop reads it on a background thread) or by flint
//! itself (a read-only "detached" terminal for agent output).
//!
//! The UI is told about changes through [`TermEvent`]s; wakeups are
//! coalesced so a flood of output produces at most one pending wakeup, and
//! the UI repaints when it next renders a frame.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use alacritty_terminal::event::Event;
use alacritty_terminal::event::EventListener;
use alacritty_terminal::event::WindowSize;
use alacritty_terminal::event_loop::EventLoop;
use alacritty_terminal::event_loop::EventLoopSender;
use alacritty_terminal::event_loop::Msg;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::grid::Scroll;
use alacritty_terminal::index::Column;
use alacritty_terminal::index::Line;
use alacritty_terminal::index::Point;
use alacritty_terminal::index::Side;
use alacritty_terminal::selection::Selection;
use alacritty_terminal::selection::SelectionType;
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::Config;
use alacritty_terminal::term::Term;
use alacritty_terminal::term::TermMode;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::tty;
use alacritty_terminal::vte::ansi::Processor;
use alacritty_terminal::vte::ansi::StdSyncHandler;
use async_channel::Receiver;
use async_channel::Sender;

use crate::palette::Palette;
use crate::snapshot::Snapshot;
use crate::snapshot::build;

/// Lines of scrollback kept.
pub const SCROLLBACK_LINES: usize = 10_000;

/// Grid size plus the pixel size of a cell (programs can ask for it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Size {
    pub cols: u16,
    pub rows: u16,
    pub cell_width: u16,
    pub cell_height: u16,
}

impl Size {
    fn window(self) -> WindowSize {
        WindowSize {
            num_lines: self.rows.max(1),
            num_cols: self.cols.max(2),
            cell_width: self.cell_width.max(1),
            cell_height: self.cell_height.max(1),
        }
    }

    fn term(self) -> TermSize {
        TermSize::new(usize::from(self.cols.max(2)), usize::from(self.rows.max(1)))
    }
}

/// What the UI hears from a terminal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TermEvent {
    /// Output arrived; repaint on the next frame.
    Wakeup,
    Title(String),
    Bell,
    /// The shell exited, with its code when known.
    Exited(Option<i32>),
}

/// How to start a shell in a PTY.
#[derive(Debug, Clone)]
pub struct SpawnConfig {
    pub cwd: PathBuf,
    /// Program and arguments; `None` runs `$SHELL -l` (or `/bin/zsh -l`).
    pub command: Option<(String, Vec<String>)>,
    pub env: Vec<(String, String)>,
    pub size: Size,
}

/// Forwards alacritty events to the UI, coalescing wakeups.
#[derive(Clone)]
struct Listener {
    tx: Sender<TermEvent>,
    queued: Receiver<TermEvent>,
    delivery: Arc<std::sync::Mutex<()>>,
    pending: Arc<AtomicBool>,
    pty_writer: Arc<FairMutex<Option<EventLoopSender>>>,
}

impl Listener {
    fn deliver(&self, mut event: TermEvent) {
        if let TermEvent::Title(title) = &mut event
            && title.len() > 4000
        {
            let mut end = 4000;
            while !title.is_char_boundary(end) {
                end -= 1;
            }
            title.truncate(end);
        }
        // Serialize producers, not the UI consumer. Collapse only pending
        // informational events of the same kind; preserve every exit event.
        // There are at most one wakeup, title and bell plus the child's exit.
        let Ok(_delivery) = self.delivery.lock() else {
            return;
        };
        let mut retained = Vec::new();
        while let Ok(queued) = self.queued.try_recv() {
            let superseded = matches!(
                (&queued, &event),
                (TermEvent::Title(_), TermEvent::Title(_))
                    | (TermEvent::Bell, TermEvent::Bell)
                    | (TermEvent::Wakeup, TermEvent::Wakeup)
            );
            if !superseded {
                retained.push(queued);
            }
        }
        for queued in retained {
            let _ = self.tx.try_send(queued);
        }
        let _ = self.tx.try_send(event);
    }
}

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        let out = match event {
            Event::Wakeup => {
                if self.pending.swap(true, Ordering::AcqRel) {
                    return;
                }
                TermEvent::Wakeup
            }
            Event::Title(title) => TermEvent::Title(title),
            Event::ResetTitle => TermEvent::Title(String::new()),
            Event::Bell => TermEvent::Bell,
            Event::ChildExit(status) => TermEvent::Exited(status.code()),
            // Replies the program asked for (cursor position, device
            // attributes, …) go back to the PTY.
            Event::PtyWrite(text) => {
                if let Some(writer) = self.pty_writer.lock().as_ref() {
                    let _ = writer.send(Msg::Input(text.into_bytes().into()));
                }
                return;
            }
            Event::Exit
            | Event::MouseCursorDirty
            | Event::ClipboardStore(..)
            | Event::ClipboardLoad(..)
            | Event::ColorRequest(..)
            | Event::TextAreaSizeRequest(..)
            | Event::CursorBlinkingChange => return,
        };
        self.deliver(out);
    }
}

enum Backend {
    Pty(EventLoopSender),
    /// Fed by flint; `parser` turns bytes into grid changes.
    Detached(Box<FairMutex<Processor<StdSyncHandler>>>),
}

/// One terminal.
pub struct Terminal {
    term: Arc<FairMutex<Term<Listener>>>,
    backend: Backend,
    listener: Listener,
    events: Receiver<TermEvent>,
    size: Size,
    pub palette: Palette,
}

impl Terminal {
    /// Starts a shell in a PTY.
    pub fn spawn(config: SpawnConfig) -> anyhow::Result<Self> {
        let (program, args) = config.command.unwrap_or_else(|| {
            let shell = std::env::var("SHELL")
                .ok()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "/bin/zsh".to_string());
            (shell, vec!["-l".to_string()])
        });
        let mut env: std::collections::HashMap<String, String> = config.env.into_iter().collect();
        env.entry("TERM".into())
            .or_insert_with(|| "xterm-256color".into());
        env.entry("COLORTERM".into())
            .or_insert_with(|| "truecolor".into());
        env.entry("TERM_PROGRAM".into())
            .or_insert_with(|| "flint".into());
        // Windows' `Options` has an extra field (`escape_args`).
        #[allow(clippy::needless_update)]
        let options = tty::Options {
            shell: Some(tty::Shell::new(program, args)),
            working_directory: Some(config.cwd),
            drain_on_exit: true,
            env,
            ..tty::Options::default()
        };
        let pty = tty::new(&options, config.size.window(), 0)?;
        let (mut terminal, listener) = Self::with_term(config.size);
        let event_loop = EventLoop::new(
            Arc::clone(&terminal.term),
            listener.clone(),
            pty,
            true,
            false,
        )?;
        let sender = event_loop.channel();
        *listener.pty_writer.lock() = Some(sender.clone());
        event_loop.spawn();
        terminal.backend = Backend::Pty(sender);
        Ok(terminal)
    }

    /// A terminal with no process: flint feeds it with [`Terminal::feed`].
    pub fn detached(size: Size) -> Self {
        Self::with_term(size).0
    }

    fn with_term(size: Size) -> (Self, Listener) {
        let (tx, events) = async_channel::unbounded();
        let listener = Listener {
            tx,
            queued: events.clone(),
            delivery: Arc::new(std::sync::Mutex::new(())),
            pending: Arc::new(AtomicBool::new(false)),
            pty_writer: Arc::new(FairMutex::new(None)),
        };
        let config = Config {
            scrolling_history: SCROLLBACK_LINES,
            ..Config::default()
        };
        let term = Term::new(config, &size.term(), listener.clone());
        let terminal = Self {
            term: Arc::new(FairMutex::new(term)),
            backend: Backend::Detached(Box::new(FairMutex::new(Processor::new()))),
            listener: listener.clone(),
            events,
            size,
            palette: Palette::default(),
        };
        (terminal, listener)
    }

    /// The event stream (wakeups, title, bell, exit).
    pub fn events(&self) -> Receiver<TermEvent> {
        self.events.clone()
    }

    /// The UI took the latest frame; the next output wakes it again.
    pub fn acknowledge_wakeup(&self) {
        self.listener.pending.store(false, Ordering::Release);
    }

    pub fn is_detached(&self) -> bool {
        matches!(self.backend, Backend::Detached(_))
    }

    /// Sends bytes to the program (keys, paste). No-op when detached.
    pub fn write(&self, bytes: impl Into<Vec<u8>>) {
        if let Backend::Pty(sender) = &self.backend {
            let bytes: Vec<u8> = bytes.into();
            if !bytes.is_empty() {
                // Typing jumps back to the live screen, like other terminals.
                self.term.lock().scroll_display(Scroll::Bottom);
                let _ = sender.send(Msg::Input(bytes.into()));
            }
        }
    }

    /// Feeds output to a detached terminal (as if a program printed it).
    ///
    /// There is no tty in between, so this does the tty's `onlcr` itself:
    /// every `\n` becomes `\r\n`. Without it, output captured from a pipe
    /// (an agent's command) staircases, each line starting where the last
    /// one ended. A `\r` already there is harmless doubled.
    pub fn feed(&self, bytes: &[u8]) {
        if let Backend::Detached(parser) = &self.backend {
            let mut translated = Vec::with_capacity(bytes.len());
            for &b in bytes {
                if b == b'\n' {
                    translated.push(b'\r');
                }
                translated.push(b);
            }
            let mut term = self.term.lock();
            parser.lock().advance(&mut *term, &translated);
            drop(term);
            self.listener.send_event(Event::Wakeup);
        }
    }

    /// Application-cursor mode (DECCKM), for encoding arrow keys.
    pub fn app_cursor(&self) -> bool {
        self.term.lock().mode().contains(TermMode::APP_CURSOR)
    }

    /// Bracketed-paste mode, for wrapping pastes.
    pub fn bracketed_paste(&self) -> bool {
        self.term.lock().mode().contains(TermMode::BRACKETED_PASTE)
    }

    /// A full-screen program (vim, less, htop) is using the alternate screen.
    pub fn alt_screen(&self) -> bool {
        self.term.lock().mode().contains(TermMode::ALT_SCREEN)
    }

    pub fn size(&self) -> Size {
        self.size
    }

    /// Resizes the grid and tells the program (SIGWINCH).
    pub fn resize(&mut self, size: Size) {
        if size == self.size {
            return;
        }
        self.size = size;
        self.term.lock().resize(size.term());
        if let Backend::Pty(sender) = &self.backend {
            let _ = sender.send(Msg::Resize(size.window()));
        }
    }

    /// Scrolls the view by `lines` (positive = back into history).
    pub fn scroll(&self, lines: i32) {
        self.term.lock().scroll_display(Scroll::Delta(lines));
    }

    pub fn scroll_to_bottom(&self) {
        self.term.lock().scroll_display(Scroll::Bottom);
    }

    pub fn snapshot(&self) -> Snapshot {
        build(&self.term.lock(), &self.palette)
    }

    /// Grid point for a visible cell (row 0 = top of the view).
    fn point(&self, row: usize, col: usize) -> Point {
        let term = self.term.lock();
        let offset = term.grid().display_offset() as i32;
        Point::new(
            Line(row as i32 - offset),
            Column(col.min(term.columns() - 1)),
        )
    }

    /// Starts a selection at a visible cell; `kind` 1 = chars, 2 = word, 3 = line.
    pub fn start_selection(&self, row: usize, col: usize, right_half: bool, kind: u8) {
        let point = self.point(row, col);
        let side = if right_half { Side::Right } else { Side::Left };
        let ty = match kind {
            2 => SelectionType::Semantic,
            3 => SelectionType::Lines,
            _ => SelectionType::Simple,
        };
        self.term.lock().selection = Some(Selection::new(ty, point, side));
    }

    pub fn update_selection(&self, row: usize, col: usize, right_half: bool) {
        let point = self.point(row, col);
        let side = if right_half { Side::Right } else { Side::Left };
        if let Some(selection) = self.term.lock().selection.as_mut() {
            selection.update(point, side);
        }
    }

    pub fn clear_selection(&self) {
        self.term.lock().selection = None;
    }

    pub fn selection_text(&self) -> Option<String> {
        self.term
            .lock()
            .selection_to_string()
            .filter(|s| !s.is_empty())
    }

    /// The whole buffer (scrollback and screen) as text, trailing blanks trimmed.
    pub fn buffer_text(&self) -> String {
        let term = self.term.lock();
        let top = Point::new(Line(-(term.history_size() as i32)), Column(0));
        let bottom = Point::new(
            Line(term.screen_lines() as i32 - 1),
            Column(term.columns() - 1),
        );
        let text = term.bounds_to_string(top, bottom);
        text.lines()
            .map(str::trim_end)
            .collect::<Vec<_>>()
            .join("\n")
            .trim_end()
            .to_string()
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        if let Backend::Pty(sender) = &self.backend {
            let _ = sender.send(Msg::Shutdown);
        }
    }
}

#[cfg(test)]
#[path = "term_tests.rs"]
mod tests;
