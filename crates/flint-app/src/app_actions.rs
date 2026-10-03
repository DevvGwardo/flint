//! Workspace and sidebar actions on the root view: opening folders, Finder
//! and Terminal, command-card actions, and the sidebar's search/filter.

use gpui_kit::*;

use crate::app::FlintApp;
use crate::app::SessionFilter;
use crate::session::Status;
use crate::view_model::Item;
use crate::view_model::ToolCall;

impl FlintApp {
    /// The tool call a transcript row shows, if it is one.
    pub(crate) fn tool_call(&self, ix: usize) -> Option<&ToolCall> {
        match self.session().view.items.get(ix) {
            Some(Item::Tool(call)) => Some(call),
            _ => None,
        }
    }

    /// A command card's "Open in terminal": the agent's own read-only
    /// terminal when it has one, else a shell in the workspace with the
    /// command on its prompt.
    pub(crate) fn open_in_terminal(
        &mut self,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(call) = self.tool_call(ix) else {
            return;
        };
        let terminal_id = call.terminal_id.clone();
        let command = command_text(call);
        if let Some(id) = terminal_id {
            let uid = self.session().uid;
            if self.show_agent_terminal(uid, &id, window, cx) {
                return;
            }
        }
        let cwd = self.session().workspace.clone();
        self.new_terminal(Some(cwd), Some(command), window, cx);
    }

    /// A command card's "Send to agent": the command's output goes back to
    /// the session's agent as a message.
    pub(crate) fn send_command_to_agent(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(call) = self.tool_call(ix).cloned() else {
            return;
        };
        let (shown, message) = crate::turns::command_message(&call);
        let active = self.active;
        self.send_message(active, shown, message, cx);
    }

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
            this.update(cx, |app, cx| app.set_project_folder(path, cx))
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

    pub fn cycle_filter(&mut self, cx: &mut Context<Self>) {
        self.filter = match self.filter {
            SessionFilter::All => SessionFilter::Running,
            SessionFilter::Running => SessionFilter::Unread,
            SessionFilter::Unread => SessionFilter::All,
        };
        self.sidebar_scroll.set_offset(Point::default());
        self.session_menu = None;
        cx.notify();
    }

    /// Sessions the sidebar shows, after search and filter.
    pub fn visible_sessions(&self, cx: &App) -> Vec<usize> {
        let query = self.search.read(cx).value().trim().to_lowercase();
        (0..self.sessions.len())
            .filter(|&ix| {
                let session = &self.sessions[ix];
                let status = session.status();
                let keep = match self.filter {
                    SessionFilter::All => true,
                    SessionFilter::Running => session.view.running,
                    SessionFilter::Unread => status == Status::Unread,
                };
                let listed = !session.view.items.is_empty() || ix == self.active;
                keep && listed
                    && (query.is_empty()
                        || session.title().to_lowercase().contains(&query)
                        || session
                            .workspace
                            .to_string_lossy()
                            .to_lowercase()
                            .contains(&query))
            })
            .collect()
    }
}

/// The shell command behind a command card: its arguments when they carry
/// one (flint's own agent and [CC] both do), else the summary shown.
fn command_text(call: &ToolCall) -> String {
    for key in ["command", "cmd", "script"] {
        if let Some(text) = call.args.get(key).and_then(|value| value.as_str())
            && !text.trim().is_empty()
        {
            return text.to_string();
        }
    }
    call.args
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| call.summary.clone())
}
