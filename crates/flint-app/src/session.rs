//! One conversation: its view-model, its virtual-list state, its workspace,
//! and its link to the engine (or the demo player). Sessions run
//! independently, so several can work at once in the background.

use std::path::PathBuf;
use std::time::Instant;

use flint_agent::Op;
use gpui_kit::*;

use crate::view_model::Change;
use crate::view_model::SessionView;

pub struct Session {
    pub view: SessionView,
    pub list: ListState,
    pub workspace: PathBuf,
    /// Last time something happened, for the sidebar's relative time.
    pub touched: Instant,
    /// A turn finished while the session was not on screen.
    pub unread: bool,
    /// Engine ops channel, once the session has started an engine.
    pub ops: Option<async_channel::Sender<Op>>,
    /// Events pump (engine) or demo playback; dropping it stops either.
    pub pump: Option<Task<()>>,
}

/// The sidebar's live status glyph for a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Idle,
    Running,
    Unread,
    Done,
}

impl Session {
    pub fn new(workspace: PathBuf) -> Self {
        let list = ListState::new(0, ListAlignment::Top, px(1200.));
        list.set_follow_mode(FollowMode::Tail);
        Self {
            view: SessionView::default(),
            list,
            workspace,
            touched: Instant::now(),
            unread: false,
            ops: None,
            pump: None,
        }
    }

    /// Keeps the virtual list in step with a view-model change.
    pub fn apply(&self, change: Change) {
        for ix in change.updated {
            self.list.remeasure_items(ix..ix + 1);
        }
        if !change.appended.is_empty() {
            let at = change.appended.start;
            self.list.splice(at..at, change.appended.len());
        }
    }

    pub fn title(&self) -> String {
        self.view
            .title
            .clone()
            .unwrap_or_else(|| "New session".to_string())
    }

    pub fn status(&self) -> Status {
        if self.view.running {
            Status::Running
        } else if self.unread {
            Status::Unread
        } else if self.view.turns.is_empty() {
            Status::Idle
        } else {
            Status::Done
        }
    }
}

pub fn folder_name(path: &std::path::Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string())
}
