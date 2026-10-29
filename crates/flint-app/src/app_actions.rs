//! Workspace and sidebar actions on the root view: opening folders, a file manager
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
        let command = terminal_command_text(call);
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

    /// Opens the active session's workspace in the platform's file manager.
    pub(crate) fn reveal_workspace(&self) {
        flint_platform::desktop::open_directory(&self.session().workspace).ok();
    }

    /// Opens a Terminal window in the active session's workspace.
    pub(crate) fn open_terminal(&self) {
        flint_platform::desktop::open_terminal(&self.session().workspace).ok();
    }

    pub fn set_filter(&mut self, filter: SessionFilter, cx: &mut Context<Self>) {
        self.filter = filter;
        self.session_limit = SESSION_PAGE;
        self.sidebar_scroll.set_offset(Point::default());
        self.session_menu = None;
        self.sidebar_menu = None;
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
        self.sidebar_menu = None;
        cx.notify();
    }

    /// Collapse or expand one heading. The state outlives a relaunch.
    pub fn toggle_group_collapsed(
        &mut self,
        key: crate::session_groups::GroupKey,
        cx: &mut Context<Self>,
    ) {
        if !self.collapsed_groups.remove(&key) {
            self.collapsed_groups.insert(key);
        }
        self.save_collapsed_groups();
        self.session_menu = None;
        self.sidebar_menu = None;
        cx.notify();
    }

    /// Persist the collapsed headings, pruning any that no longer exist so the
    /// settings file does not collect stale keys.
    fn save_collapsed_groups(&mut self) {
        let listed = self.listed_group_keys();
        self.collapsed_groups.retain(|key| listed.contains(key));
        let mut keys: Vec<String> = self
            .collapsed_groups
            .iter()
            .map(|key| key.setting_key())
            .collect();
        keys.sort();
        if self.settings.collapsed_groups != keys {
            self.settings.collapsed_groups = keys;
            if !self.options.ephemeral() {
                self.settings.save(&self.home).ok();
            }
        }
    }

    /// Open or close the archived-sessions disclosure. Remembered across launches.
    pub fn toggle_archived(&mut self, cx: &mut Context<Self>) {
        self.archived_open = !self.archived_open;
        if self.settings.archived_open != self.archived_open {
            self.settings.archived_open = self.archived_open;
            if !self.options.ephemeral() {
                self.settings.save(&self.home).ok();
            }
        }
        self.session_menu = None;
        self.sidebar_menu = None;
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
    let command = raw_command(call);
    if let Some(args) = call.args.get("args").and_then(serde_json::Value::as_array)
        && args.iter().all(|arg| arg.is_string())
    {
        #[cfg(windows)]
        {
            return std::iter::once(command)
                .chain(args.iter().filter_map(serde_json::Value::as_str))
                .map(flint_platform::quote_windows_arg)
                .collect::<Vec<_>>()
                .join(" ");
        }
        #[cfg(not(windows))]
        return std::iter::once(command)
            .chain(args.iter().filter_map(serde_json::Value::as_str))
            .map(shell_word)
            .collect::<Vec<_>>()
            .join(" ");
    }
    command.to_string()
}

fn raw_command(call: &ToolCall) -> &str {
    ["command", "cmd", "script"]
        .iter()
        .find_map(|key| {
            call.args
                .get(*key)
                .and_then(|value| value.as_str())
                .filter(|text| !text.trim().is_empty())
        })
        .or_else(|| call.args.as_str())
        .unwrap_or(&call.summary)
}

fn terminal_command_text(call: &ToolCall) -> String {
    #[cfg(windows)]
    if let Some(args) = call.args.get("args").and_then(serde_json::Value::as_array)
        && args.iter().all(|arg| arg.is_string())
    {
        // Do not run literal argv through cmd expansion or PowerShell 5's
        // lossy native-argument binder. Keep expanded cards human-readable;
        // only the editable terminal prompt gets the encoded invocation.
        use base64::Engine as _;
        let program = raw_command(call);
        let quote = |word: &str| format!("'{}'", word.replace('\'', "''"));
        let batch = std::path::Path::new(program)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("cmd") || ext.eq_ignore_ascii_case("bat"));
        let script = if batch {
            // Batch programs necessarily retain their own shell semantics.
            let words = std::iter::once(program)
                .chain(args.iter().filter_map(serde_json::Value::as_str))
                .map(quote)
                .collect::<Vec<_>>()
                .join(" ");
            format!("$ErrorActionPreference = 'Stop'; & {words}; exit $LASTEXITCODE")
        } else {
            let arguments = args
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(flint_platform::quote_windows_arg)
                .collect::<Vec<_>>()
                .join(" ");
            format!(
                "$ErrorActionPreference = 'Stop'; $info = New-Object System.Diagnostics.ProcessStartInfo; $info.FileName = {}; $info.Arguments = {}; $info.UseShellExecute = $false; $child = [System.Diagnostics.Process]::Start($info); $child.WaitForExit(); exit $child.ExitCode",
                quote(program),
                quote(&arguments)
            )
        };
        let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
        return format!(
            "powershell.exe -NoLogo -NoProfile -EncodedCommand {}",
            base64::engine::general_purpose::STANDARD.encode(bytes)
        );
    }
    command_text(call)
}

#[cfg(not(windows))]
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
    use super::{command_text, terminal_command_text};
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
            #[cfg(not(windows))]
            assert_eq!(command_text(call), expected);
            #[cfg(windows)]
            if call.args.get("args").is_some() {
                use base64::Engine as _;
                assert_eq!(
                    command_text(call),
                    "\"printf\" \"%s\" \"hi; echo wrong\" \"\" \"a'b\" \"$HOME\""
                );
                let typed = terminal_command_text(call);
                let encoded = typed
                    .strip_prefix("powershell.exe -NoLogo -NoProfile -EncodedCommand ")
                    .unwrap();
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(encoded)
                    .unwrap();
                let units: Vec<u16> = bytes
                    .chunks_exact(2)
                    .map(|b| u16::from_le_bytes([b[0], b[1]]))
                    .collect();
                assert_eq!(
                    String::from_utf16(&units).unwrap(),
                    "$ErrorActionPreference = 'Stop'; $info = New-Object System.Diagnostics.ProcessStartInfo; $info.FileName = 'printf'; $info.Arguments = '\"%s\" \"hi; echo wrong\" \"\" \"a''b\" \"$HOME\"'; $info.UseShellExecute = $false; $child = [System.Diagnostics.Process]::Start($info); $child.WaitForExit(); exit $child.ExitCode"
                );
            } else {
                assert_eq!(command_text(call), expected);
            }
            #[cfg(not(windows))]
            assert_eq!(terminal_command_text(call), expected);
        }
    }

    #[test]
    fn empty_argument_vectors_still_quote_the_executable() {
        let mut view = SessionView::default();
        view.fold(
            flint_agent::AgentEvent::ToolCallStarted {
                call_id: "empty".into(),
                name: "execute".into(),
                kind: flint_agent::ToolKind::Command,
                summary: "literal executable".into(),
                args: serde_json::json!({"command":"program with spaces.exe","args":[]}),
            },
            std::time::Duration::ZERO,
        );
        let Item::Tool(call) = &view.items[0] else {
            panic!("tool row")
        };
        assert_eq!(
            command_text(call),
            if cfg!(windows) {
                "\"program with spaces.exe\""
            } else {
                "'program with spaces.exe'"
            }
        );
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn windows_terminal_replay_preserves_native_argv_without_expansion() {
        let arguments = [
            "",
            "%PATH%",
            "a& echo injected",
            "a\"b",
            r"a\",
            "a'b",
            "$HOME",
        ];
        let mut args = vec![
            "-c".to_string(),
            "import json, sys; print(json.dumps(sys.argv[1:]))".into(),
        ];
        args.extend(arguments.map(str::to_string));
        let mut view = SessionView::default();
        view.fold(
            flint_agent::AgentEvent::ToolCallStarted {
                call_id: "replay".into(),
                name: "execute".into(),
                kind: flint_agent::ToolKind::Command,
                summary: "literal argv".into(),
                args: serde_json::json!({"command":"python","args":args}),
            },
            std::time::Duration::ZERO,
        );
        let Item::Tool(call) = &view.items[0] else {
            panic!("tool row")
        };
        let dir = tempfile::tempdir().unwrap();
        let output = flint_platform::shell_command(&terminal_command_text(call))
            .current_dir(dir.path())
            .output()
            .await
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            serde_json::from_slice::<Vec<String>>(&output.stdout).unwrap(),
            arguments
        );
    }
}
