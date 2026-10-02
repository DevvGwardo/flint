//! One conversation: its view-model, its virtual-list state, and its link to
//! the engine (or the demo player).

use std::time::Instant;

use flint_agent::Op;
use gpui_kit::*;

use crate::view_model::Change;
use crate::view_model::SessionView;

pub struct Session {
    pub view: SessionView,
    pub list: ListState,
    pub created: Instant,
    /// Engine ops channel, once the session has started an engine.
    pub ops: Option<async_channel::Sender<Op>>,
    /// Events pump (engine) or demo playback; dropping it stops either.
    pub pump: Option<Task<()>>,
    pub demo: bool,
}

impl Session {
    pub fn new() -> Self {
        let list = ListState::new(0, ListAlignment::Top, px(1200.));
        list.set_follow_mode(FollowMode::Tail);
        Self {
            view: SessionView::default(),
            list,
            created: Instant::now(),
            ops: None,
            pump: None,
            demo: false,
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
}
