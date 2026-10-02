//! Saved sessions on the root view: restoring them on startup (replaying
//! their event logs through the view-model), and rename/delete.

use std::time::Duration;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use flint_agent::AgentEvent;
use flint_agent::Op;
use flint_agent::TurnEndReason;
use gpui_kit::component::input::InputEvent;
use gpui_kit::component::input::InputState;
use gpui_kit::*;

use crate::app::FlintApp;
use crate::store;
use crate::store::Logged;

fn from_secs(secs: i64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(secs.max(0) as u64)
}

impl FlintApp {
    /// Lists every saved session (most recent first) with its transcript.
    pub(crate) fn restore_sessions(&mut self, _cx: &mut Context<Self>) {
        for (dir, meta, records) in store::load_all(&self.home) {
            let mut session = self.new_session_value(meta.workspace.clone());
            session.dir = Some(dir);
            session.created = from_secs(meta.created_at);
            let mut clock = Duration::ZERO;
            for record in records {
                let change = match record {
                    Logged::User(text) => session.view.push_user(text),
                    Logged::Event(event) => {
                        clock += Duration::from_millis(50);
                        session.view.fold(event, clock)
                    }
                };
                session.apply(change);
            }
            // A turn cut off by quitting shows as stopped, not running forever.
            if session.view.running {
                let turn_id = session.view.turn_id;
                let change = session.view.fold(
                    AgentEvent::TurnFinished {
                        turn_id,
                        reason: TurnEndReason::Interrupted,
                    },
                    clock,
                );
                session.apply(change);
            }
            if meta.title.is_some() {
                session.view.title = meta.title;
            }
            session.touched = from_secs(meta.updated_at);
            self.sessions.push(session);
        }
    }

    /// Shows an inline title editor for a session.
    pub fn start_rename(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let title = self.sessions[ix].title();
        let input = cx.new(|cx| InputState::new(window, cx).default_value(title));
        input.update(cx, |state, cx| state.focus(window, cx));
        let subscription = cx.subscribe_in(
            &input,
            window,
            move |this, _, event: &InputEvent, window, cx| match event {
                InputEvent::PressEnter { .. } => this.commit_rename(window, cx),
                InputEvent::Blur => this.cancel_rename(window, cx),
                _ => {}
            },
        );
        self.subscriptions.push(subscription);
        self.renaming = Some((ix, input));
        self.session_menu = None;
        cx.notify();
    }

    pub fn commit_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((ix, input)) = self.renaming.take() else {
            return;
        };
        let title = input.read(cx).value().trim().to_string();
        if let Some(session) = self.sessions.get_mut(ix)
            && !title.is_empty()
        {
            session.view.title = Some(title);
            session.save_meta();
        }
        self.composer
            .update(cx, |state, cx| state.focus(window, cx));
        cx.notify();
    }

    pub fn cancel_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.renaming.take().is_some() {
            self.composer
                .update(cx, |state, cx| state.focus(window, cx));
            cx.notify();
        }
    }

    /// Stops a session's engine, removes it from the list and from disk.
    pub fn delete_session(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if ix >= self.sessions.len() {
            return;
        }
        let session = self.sessions.remove(ix);
        if let Some(ops) = &session.ops {
            ops.try_send(Op::Shutdown).ok();
        }
        if let Some(dir) = &session.dir {
            store::delete(dir).ok();
        }
        if self.sessions.is_empty() {
            let fresh = self.new_session_value(self.workspace.clone());
            self.sessions.push(fresh);
        }
        if self.active > ix || self.active >= self.sessions.len() {
            self.active = self.active.saturating_sub(1);
        }
        self.session_menu = None;
        self.selected_change = None;
        self.composer
            .update(cx, |state, cx| state.focus(window, cx));
        cx.notify();
    }
}
