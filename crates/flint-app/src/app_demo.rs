//! Demo playback on the root view (`--demo`): scripted sessions replayed with
//! scripted time, plus background sessions to show concurrency.

use gpui_kit::*;

use crate::app::FlintApp;
use crate::demo;

impl FlintApp {
    pub(crate) fn start_demo(&mut self, cx: &mut Context<Self>) {
        let uid = self.sessions[0].uid;
        let change = self.sessions[0].view.push_user(demo::PROMPT.to_string());
        self.sessions[0].apply(change);
        let mut beats = demo::script(self.options.demo_approval);
        if let Some(stop) = self.options.demo_stop {
            beats.truncate(stop);
        }
        self.play(uid, beats, self.options.demo_instant, cx);
        // Other sessions working in the background, to show concurrency.
        for extra in demo::extra_sessions(&self.workspace) {
            let mut session = self.new_session_value(extra.workspace);
            let change = session.view.push_user(extra.prompt.to_string());
            session.apply(change);
            let uid = session.uid;
            self.sessions.push(session);
            self.play(uid, extra.beats, extra.instant, cx);
        }
    }

    /// Plays scripted events into a session with scripted time, so instant
    /// playback still shows real durations.
    pub(crate) fn play(
        &mut self,
        uid: u64,
        beats: Vec<demo::Beat>,
        instant: bool,
        cx: &mut Context<Self>,
    ) {
        let start = self.now();
        let select_change =
            self.options.select_change && self.session_index(uid) == Some(self.active);
        let expand = self.options.demo_expand;
        let task = cx.spawn(async move |this, cx| {
            let mut clock = start;
            for beat in beats {
                clock += beat.delay;
                if !instant {
                    cx.background_executor().timer(beat.delay).await;
                }
                if this
                    .update(cx, |app, cx| {
                        app.apply_events(uid, vec![beat.event], clock, cx)
                    })
                    .is_err()
                {
                    return;
                }
            }
            this.update(cx, |app, cx| {
                if app.session_index(uid) == Some(app.active) {
                    if select_change {
                        app.changes_open = true;
                        app.selected_change = Some(0);
                    }
                    if expand {
                        app.toggle_work(1, cx);
                    }
                }
                cx.notify();
            })
            .ok();
        });
        if let Some(ix) = self.session_index(uid) {
            self.sessions[ix].pump = Some(task);
        }
    }
}
