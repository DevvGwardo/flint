//! Workspace and sidebar actions on the root view: opening folders, Finder
//! and Terminal, command-card actions, and the sidebar's search/filter.

use gpui_kit::*;

use crate::app::FlintApp;
use crate::app::SESSION_PAGE;
use crate::app::SessionFilter;
use crate::app::SessionGrouping;
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

    pub fn set_filter(&mut self, filter: SessionFilter, cx: &mut Context<Self>) {
        self.filter = filter;
        self.session_limit = SESSION_PAGE;
        self.sidebar_scroll.set_offset(Point::default());
        self.session_menu = None;
        cx.notify();
    }

    pub fn set_grouping(&mut self, grouping: SessionGrouping, cx: &mut Context<Self>) {
        self.grouping = grouping;
        // Remembered across launches (not in tests and demos).
        if self.settings.session_grouping != grouping.setting() {
            self.settings.session_grouping = grouping.setting().to_string();
            if !self.options.ephemeral() {
                self.settings.save(&self.home).ok();
            }
        }
        self.session_limit = SESSION_PAGE;
        self.sidebar_scroll.set_offset(Point::default());
        self.session_menu = None;
        cx.notify();
    }

    /// Whether the sidebar lists the session at all, before search and filter.
    pub fn is_listed(&self, ix: usize) -> bool {
        !self.sessions[ix].view.items.is_empty()
            || !self.sessions[ix].prompt_queue.items.is_empty()
            || ix == self.active
    }

    /// The search box's query, trimmed and lowercased.
    pub(crate) fn search_query(&self, cx: &App) -> String {
        self.search.read(cx).value().trim().to_lowercase()
    }

    /// Matches the title, workspace and last message; titles are generated, so
    /// what the session last said is often the better handle.
    pub(crate) fn parent_matches_query(&self, ix: usize, query: &str) -> bool {
        let session = &self.sessions[ix];
        query.is_empty()
            || session.title().to_lowercase().contains(query)
            || session
                .workspace
                .to_string_lossy()
                .to_lowercase()
                .contains(query)
            || session
                .last_message_text()
                .is_some_and(|text| text.to_lowercase().contains(query))
            || session.agent.label().to_lowercase().contains(query)
            || crate::project::branch(&session.workspace)
                .is_some_and(|branch| branch.to_lowercase().contains(query))
    }

    fn matches_query(&self, ix: usize, query: &str) -> bool {
        self.parent_matches_query(ix, query)
            || self.sessions[ix]
                .view
                .subagents
                .iter()
                .any(|child| child.matches_query(query))
    }

    fn matches_bucket(&self, ix: usize, bucket: crate::session::Bucket) -> bool {
        let session = &self.sessions[ix];
        session.status().bucket() == bucket
            || session
                .view
                .subagents
                .iter()
                .any(|child| child.status(&session.view).bucket() == bucket)
    }

    /// Sessions the sidebar shows, after search and filter.
    pub fn visible_sessions(&self, cx: &App) -> Vec<usize> {
        let query = self.search_query(cx);
        (0..self.sessions.len())
            .filter(|&ix| {
                self.is_listed(ix)
                    && match self.filter {
                        SessionFilter::All => true,
                        SessionFilter::Only(bucket) => self.matches_bucket(ix, bucket),
                    }
                    && self.matches_query(ix, &query)
            })
            .collect()
    }

    /// How many listed sessions match the search, in total and per bucket, so
    /// the filter strip says what each choice would show.
    pub fn bucket_counts(&self, cx: &App) -> BucketCounts {
        let query = self.search_query(cx);
        let mut counts = BucketCounts::default();
        for ix in (0..self.sessions.len()).filter(|&ix| self.is_listed(ix)) {
            if self.matches_query(ix, &query) {
                counts.all += 1;
                for bucket in crate::session::Bucket::ALL {
                    if self.matches_bucket(ix, bucket) {
                        counts.per_bucket[bucket as usize] += 1;
                    }
                }
            }
        }
        counts
    }
}

/// Session counts behind the sidebar's filter strip.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct BucketCounts {
    pub all: usize,
    per_bucket: [usize; 4],
}

impl BucketCounts {
    pub fn of(&self, filter: SessionFilter) -> usize {
        match filter {
            SessionFilter::All => self.all,
            SessionFilter::Only(bucket) => self.per_bucket[bucket as usize],
        }
    }
}

/// The shell command behind a command card: its arguments when they carry
/// one (flint's own agent and [CC] both do), else the summary shown.
pub(crate) fn command_text(call: &ToolCall) -> String {
    let command = ["command", "cmd", "script"]
        .iter()
        .find_map(|key| {
            call.args
                .get(*key)
                .and_then(|value| value.as_str())
                .filter(|text| !text.trim().is_empty())
        })
        .or_else(|| call.args.as_str())
        .unwrap_or(&call.summary);
    if let Some(args) = call.args.get("args").and_then(serde_json::Value::as_array)
        && !args.is_empty()
        && args.iter().all(|arg| arg.is_string())
    {
        return std::iter::once(command)
            .chain(args.iter().filter_map(serde_json::Value::as_str))
            .map(shell_word)
            .collect::<Vec<_>>()
            .join(" ");
    }
    command.to_string()
}

fn shell_word(word: &str) -> String {
    if !word.is_empty()
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_-./:%+,=@".contains(c))
    {
        word.to_string()
    } else {
        format!("'{}'", word.replace('\'', "'\\''"))
    }
}

#[cfg(test)]
mod tests {
    use super::command_text;
    use crate::view_model::{Item, SessionView};

    #[test]
    fn expanded_commands_and_terminal_actions_keep_literal_argv() {
        let mut view = SessionView::default();
        for (args, expected) in [
            (
                serde_json::json!({"command": "cat <<'END'\na\nEND"}),
                "cat <<'END'\na\nEND",
            ),
            (
                serde_json::json!({"command": "printf", "args": ["%s", "hi; echo wrong", "", "a'b", "$HOME"]}),
                "printf %s 'hi; echo wrong' '' 'a'\\''b' '$HOME'",
            ),
        ] {
            view.fold(
                flint_agent::AgentEvent::ToolCallStarted {
                    call_id: "call".into(),
                    name: "execute".into(),
                    kind: flint_agent::ToolKind::Command,
                    summary: "short summary".into(),
                    args,
                },
                std::time::Duration::ZERO,
            );
            let Item::Tool(call) = view.items.last().unwrap() else {
                panic!("tool row")
            };
            assert_eq!(command_text(call), expected);
        }
    }
}
