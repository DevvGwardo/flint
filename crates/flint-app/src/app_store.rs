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
    pub(crate) fn refresh_archives(&mut self) {
        self.archives = store::load_archived(&self.home);
    }

    /// Recovery is available after restart, independent of process-local Undo.
    pub fn restore_archive(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some((dir, _)) = self.archives.get(ix) else {
            return;
        };
        let restored = match store::restore(&self.home, dir) {
            Ok(dir) => dir,
            Err(_) => {
                self.store_error =
                    Some("Couldn't restore archive. Existing history has been kept.".into());
                cx.notify();
                return;
            }
        };
        if let Some((meta, records)) = store::load(&restored) {
            self.restore_session(restored, meta, records);
        } else {
            self.store_error = Some(
                "Archive history was restored on disk, but its details couldn't be loaded.".into(),
            );
            self.refresh_archives();
            cx.notify();
            return;
        }
        self.refresh_archives();
        self.archived_session = None;
        self.store_error = None;
        if let Some(ix) = self.sessions.len().checked_sub(1) {
            self.select_session(ix, window, cx);
        }
        cx.notify();
    }
    /// Lists every saved session (most recent first) with its transcript.
    pub(crate) fn restore_sessions(&mut self, _cx: &mut Context<Self>) {
        for (dir, meta, records) in store::load_all(&self.home) {
            if self
                .sessions
                .iter()
                .any(|session| session.dir.as_ref() == Some(&dir))
            {
                continue;
            }
            self.restore_session(dir, meta, records);
        }
    }

    fn restore_session(
        &mut self,
        dir: std::path::PathBuf,
        meta: store::Meta,
        records: Vec<Logged>,
    ) {
        let mut session = self.new_session_value(meta.workspace.clone());
        session.dir = Some(dir);
        session.agent = meta.agent;
        session.created = from_secs(meta.created_at);
        let mut clock = Duration::ZERO;
        for record in records {
            let change = match record {
                Logged::User(text) => session.view.push_user(text),
                Logged::Event(event) => {
                    if let AgentEvent::SessionStopped {
                        history_saved: false,
                    } = &event
                    {
                        session.history_save_failed = true;
                    }
                    clock += Duration::from_millis(50);
                    if let AgentEvent::SessionOptions(options) = &event {
                        session.options = options.clone();
                    }
                    session.record_event_edit(&event);
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

    /// Shows an inline title editor for a session.
    pub fn start_rename(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session) = self.sessions.get(ix) else {
            return;
        };
        let title = session.title();
        let uid = session.uid;
        self.close_palette(window, cx);
        self.discard_rename();
        if !self.visible_sessions(cx).contains(&ix) {
            self.filter = crate::app::SessionFilter::All;
            self.search
                .update(cx, |state, cx| state.set_value("", window, cx));
        }
        if !self.sidebar_visible && !self.session_drawer {
            self.toggle_session_drawer(window, cx);
        }
        self.rename_needs_scroll.set(true);
        let input = cx.new(|cx| InputState::new(window, cx).default_value(title));
        input.update(cx, |state, cx| state.focus(window, cx));
        let subscription = cx.subscribe_in(
            &input,
            window,
            move |this, input, event: &InputEvent, window, cx| {
                if this
                    .renaming
                    .as_ref()
                    .is_none_or(|(_, current)| current != input)
                    || this
                        .sessions
                        .get(ix)
                        .is_none_or(|session| session.uid != uid)
                {
                    return;
                }
                match event {
                    InputEvent::PressEnter { .. } => this.commit_rename(window, cx),
                    InputEvent::Blur => {
                        this.discard_rename();
                        cx.notify();
                    }
                    _ => {}
                }
            },
        );
        self.rename_subscription = Some(subscription);
        self.renaming = Some((ix, input));
        self.session_menu = None;
        cx.notify();
    }

    pub fn commit_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((ix, input)) = self.renaming.take() else {
            return;
        };
        self.discard_rename();
        let title = input.read(cx).value().trim().to_string();
        if let Some(session) = self.sessions.get_mut(ix)
            && !title.is_empty()
        {
            session.view.title = Some(title);
            if let Err(err) = session.save_meta() {
                self.store_error = Some(format!("Couldn't save session title: {err}"));
            }
        }
        if self.session_drawer {
            self.search.update(cx, |state, cx| state.focus(window, cx));
        } else {
            self.composer
                .update(cx, |state, cx| state.focus(window, cx));
        }
        cx.notify();
    }

    pub fn cancel_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.discard_rename() {
            if self.session_drawer {
                self.search.update(cx, |state, cx| state.focus(window, cx));
            } else {
                self.composer
                    .update(cx, |state, cx| state.focus(window, cx));
            }
            cx.notify();
        }
    }

    pub(crate) fn discard_rename(&mut self) -> bool {
        let discarded = self.renaming.take().is_some();
        self.rename_subscription = None;
        self.rename_needs_scroll.set(false);
        discarded
    }

    /// Moves a session to the archive. Running work needs an extra click.
    pub fn delete_session(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if ix >= self.sessions.len() {
            return;
        }
        let uid = self.sessions[ix].uid;
        if self.sessions[ix].view.running
            && !self.sessions[ix].stopping
            && self.archive_confirm != Some(uid)
        {
            self.archive_confirm = Some(uid);
            self.archive_return_focus = window.focused(cx);
            self.archive_focus.focus(window, cx);
            cx.notify();
            return;
        }
        self.archive_confirm = None;
        if self.sessions[ix].ops.is_some() {
            if !self.sessions[ix].stopping {
                if let Some(ops) = &self.sessions[ix].ops
                    && let Err(err) = ops.try_send(Op::Shutdown)
                {
                    self.store_error = Some(format!("Couldn't stop session engine: {err}"));
                    cx.notify();
                    return;
                }
                self.sessions[ix].stopping = true;
            }
            self.store_error = Some(
                "Waiting for the engine to finish saving. Archive again when it stops.".into(),
            );
            cx.notify();
            return;
        }
        if self.sessions[ix].view.running {
            self.store_error = Some("Couldn't archive: work has not stopped.".into());
            cx.notify();
            return;
        }
        if self.sessions[ix].history_save_failed {
            self.store_error = Some(
                "Couldn't archive: engine history was not saved. Session and existing history have been kept.".into(),
            );
            cx.notify();
            return;
        }
        if self.sessions[ix]
            .flush_records()
            .and_then(|()| self.sessions[ix].save_meta())
            .is_err()
        {
            self.store_error = Some(
                "Couldn't archive: session events or details could not be saved. Session has been kept; fix storage and retry.".into(),
            );
            cx.notify();
            return;
        }
        // No writer may recreate the old directory after it moves to archive.
        self.sessions[ix].event_writer = None;
        self.sessions[ix].persistence_task = None;
        if let Some(dir) = self.sessions[ix].dir.as_ref() {
            match store::archive(&self.home, dir) {
                Ok(_) => {}
                Err(err) => {
                    self.store_error = Some(format!("Couldn't archive session: {err}"));
                    cx.notify();
                    return;
                }
            }
        }
        self.discard_rename();
        let session = self.sessions.remove(ix);
        // Its commands' read-only terminal tabs go with it.
        self.close_agent_terminals(session.uid, cx);
        self.archived_session = Some((session, ix));
        self.refresh_archives();
        self.store_error = None;
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

    pub fn undo_archive(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((mut session, ix)) = self.archived_session.take() else {
            return;
        };
        if let Some(dir) = session.dir.as_ref() {
            let archived = self
                .home
                .join("archive")
                .join(dir.file_name().unwrap_or_default());
            match store::restore(&self.home, &archived) {
                Ok(_) => {}
                Err(err) => {
                    self.store_error = Some(format!("Couldn't restore session: {err}"));
                    self.archived_session = Some((session, ix));
                    cx.notify();
                    return;
                }
            }
        }
        // An archived session's engine has shut down. The transcript remains,
        // and a later message starts a fresh engine from saved history.
        session.ops = None;
        session.pump = None;
        session.native_allow_all = false;
        if session.view.running {
            let change = session.view.fold(
                AgentEvent::TurnFinished {
                    turn_id: session.view.turn_id,
                    reason: TurnEndReason::Interrupted,
                },
                self.now(),
            );
            session.apply(change);
        }
        let ix = ix.min(self.sessions.len());
        self.discard_rename();
        self.sessions.insert(ix, session);
        self.refresh_archives();
        self.active = ix;
        self.store_error = None;
        self.composer
            .update(cx, |state, cx| state.focus(window, cx));
        cx.notify();
    }
}
