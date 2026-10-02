//! Workspace and sidebar actions on the root view: opening folders, Finder
//! and Terminal, the effort chip, and the sidebar's search/filter.

use gpui_kit::*;

use crate::app::Effort;
use crate::app::FlintApp;
use crate::app::SessionFilter;
use crate::session::Session;
use crate::session::Status;

impl FlintApp {
    pub(crate) fn open_workspace(&mut self, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Open workspace".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            this.update(cx, |app, cx| {
                app.workspace = path.clone();
                app.sessions.push(Session::new(path));
                app.active = app.sessions.len() - 1;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Opens the active session's workspace in Finder.
    pub(crate) fn reveal_workspace(&self) {
        std::process::Command::new("open")
            .arg(&self.session().workspace)
            .spawn()
            .ok();
    }

    /// Opens a Terminal window in the active session's workspace.
    pub(crate) fn open_terminal(&self) {
        std::process::Command::new("open")
            .args(["-a", "Terminal"])
            .arg(&self.session().workspace)
            .spawn()
            .ok();
    }

    pub fn cycle_effort(&mut self, cx: &mut Context<Self>) {
        self.effort = match self.effort {
            Effort::Low => Effort::Medium,
            Effort::Medium => Effort::High,
            Effort::High => Effort::Low,
        };
        cx.notify();
    }

    pub fn cycle_filter(&mut self, cx: &mut Context<Self>) {
        self.filter = match self.filter {
            SessionFilter::All => SessionFilter::Running,
            SessionFilter::Running => SessionFilter::Unread,
            SessionFilter::Unread => SessionFilter::All,
        };
        cx.notify();
    }

    /// Sessions the sidebar shows, after search and filter.
    pub fn visible_sessions(&self, cx: &App) -> Vec<usize> {
        let query = self.search.read(cx).value().to_lowercase();
        (0..self.sessions.len())
            .filter(|&ix| {
                let session = &self.sessions[ix];
                let status = session.status();
                let keep = match self.filter {
                    SessionFilter::All => true,
                    SessionFilter::Running => status == Status::Running,
                    SessionFilter::Unread => status == Status::Unread,
                };
                let listed = !session.view.items.is_empty() || ix == self.active;
                keep && listed
                    && (query.is_empty() || session.title().to_lowercase().contains(&query))
            })
            .collect()
    }
}
