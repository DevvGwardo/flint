//! Finding and describing ACP adapters: which binary to run, the environment
//! it needs, and what to tell the user when it can't start or isn't logged in.

use std::ffi::{OsStr, OsString};
use std::path::Path;
use std::path::PathBuf;

use flint_agent::AgentKind;

/// An agent reachable over ACP.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcpAgent {
    ClaudeCode,
    Codex,
    /// Factory Droid speaks ACP directly; it does not need an adapter.
    Droid,
    /// OpenCode speaks ACP directly via `opencode acp`.
    OpenCode,
    /// Any other ACP agent: `command args…`.
    Custom {
        name: String,
        command: String,
        args: Vec<String>,
    },
}

impl AcpAgent {
    /// Display name.
    pub fn name(&self) -> &str {
        match self {
            AcpAgent::ClaudeCode => "Claude Code",
            AcpAgent::Codex => "Codex",
            AcpAgent::Droid => "Droid",
            AcpAgent::OpenCode => "OpenCode",
            AcpAgent::Custom { name, .. } => name,
        }
    }

    /// Short lowercase name for terminal tab labels ("claude: npm test").
    pub fn label_prefix(&self) -> String {
        match self {
            AcpAgent::ClaudeCode => "claude".to_string(),
            AcpAgent::Codex => "codex".to_string(),
            AcpAgent::Droid => "droid".to_string(),
            AcpAgent::OpenCode => "opencode".to_string(),
            AcpAgent::Custom { name, .. } => name.to_lowercase(),
        }
    }

    /// Stable id stored in `acp.json`.
    pub fn id(&self) -> String {
        match self {
            AcpAgent::ClaudeCode => "claude_code".to_string(),
            AcpAgent::Codex => "codex".to_string(),
            AcpAgent::Droid => "droid".to_string(),
            AcpAgent::OpenCode => "opencode".to_string(),
            AcpAgent::Custom { name, .. } => format!("custom:{name}"),
        }
    }

    /// The agent for a session kind (`None` for flint's own engine).
    pub fn for_kind(kind: AgentKind) -> Option<Self> {
        match kind {
            AgentKind::Flint => None,
            AgentKind::ClaudeCode => Some(AcpAgent::ClaudeCode),
            AgentKind::Codex => Some(AcpAgent::Codex),
            AgentKind::Droid => Some(AcpAgent::Droid),
            AgentKind::OpenCode => Some(AcpAgent::OpenCode),
        }
    }

    /// Adapter binary name and its npm package (for `npx` and install hints).
    fn adapter(&self) -> Option<(&'static str, &'static str)> {
        match self {
            AcpAgent::ClaudeCode => {
                Some(("claude-agent-acp", "@agentclientprotocol/claude-agent-acp"))
            }
            AcpAgent::Codex => Some(("codex-acp", "@agentclientprotocol/codex-acp")),
            AcpAgent::Droid | AcpAgent::OpenCode | AcpAgent::Custom { .. } => None,
        }
    }

    /// How to install the adapter.
    pub fn install_hint(&self) -> String {
        if *self == AcpAgent::OpenCode {
            return "Install OpenCode with `npm i -g opencode-ai`, ensure `opencode` is on PATH, then retry.".to_string();
        }
        if *self == AcpAgent::Droid {
            if cfg!(windows) {
                return "Install a Windows-compatible Factory Droid build, add it to PATH, then retry.".to_string();
            }
            return "Install Droid with `npm i -g droid` or `brew install --cask droid`, then retry.".to_string();
        }
        match self.adapter() {
            Some((_, package)) => format!("Install it with `npm i -g {package}`, then retry."),
            None => "Check the command in your settings, then retry.".to_string(),
        }
    }

    /// How to log the underlying agent in.
    pub fn login_hint(&self) -> String {
        match self {
            AcpAgent::ClaudeCode => {
                "Run `claude` once in a terminal to log in, then retry.".to_string()
            }
            AcpAgent::Codex => "Run `codex login` in a terminal, then retry.".to_string(),
            AcpAgent::Droid => "Run `droid` once in a terminal to log in, then retry.".to_string(),
            AcpAgent::OpenCode => {
                "Run `opencode auth login` in a terminal, then retry.".to_string()
            }
            AcpAgent::Custom { name, .. } => format!("Log in to {name}, then retry."),
        }
    }

    /// The command to spawn: the adapter on PATH or in `~/.local/bin`, else
    /// `npx -y <package>`. Droid and OpenCode run their native ACP servers directly.
    pub fn command(
        &self,
        search_path: impl AsRef<OsStr>,
    ) -> Result<(PathBuf, Vec<String>), String> {
        let search_path = search_path.as_ref();
        match self {
            AcpAgent::OpenCode => find("opencode", search_path)
                .map(|program| (program, vec!["acp".to_string()]))
                .ok_or_else(|| {
                    format!(
                        "OpenCode (`opencode`) isn't installed or isn't on PATH. {}",
                        self.install_hint()
                    )
                }),
            AcpAgent::Droid => find("droid", search_path)
                .map(|program| {
                    (
                        program,
                        vec![
                            "exec".to_string(),
                            "--output-format".to_string(),
                            "acp".to_string(),
                        ],
                    )
                })
                .ok_or_else(|| {
                    format!(
                        "Droid (`droid`) isn't installed or isn't on PATH. {}",
                        self.install_hint()
                    )
                }),
            AcpAgent::Custom { command, args, .. } => {
                let program = find(command, search_path).unwrap_or_else(|| PathBuf::from(command));
                Ok((program, args.clone()))
            }
            AcpAgent::ClaudeCode | AcpAgent::Codex => {
                let Some((binary, package)) = self.adapter() else {
                    return Err(self.install_hint());
                };
                if let Some(path) = find(binary, search_path) {
                    return Ok((path, Vec::new()));
                }
                match find("npx", search_path) {
                    Some(npx) => Ok((npx, vec!["-y".to_string(), package.to_string()])),
                    None => Err(format!(
                        "{}'s ACP adapter (`{binary}`) isn't installed and `npx` isn't available. {}",
                        self.name(),
                        self.install_hint()
                    )),
                }
            }
        }
    }

    /// Whether the adapter must not inherit the environment variable `name`.
    ///
    /// - Every agent drops the markers of an enclosing Claude Code session
    ///   (`CLAUDECODE`, `CLAUDE_CODE_*`, `CLAUDE_PID`, ...): flint itself
    ///   may have been started from one, and the child would think it runs
    ///   nested inside it.
    /// - Claude Code also drops `ANTHROPIC_API_KEY` / `ANTHROPIC_AUTH_TOKEN`,
    ///   which Claude Code prefers over the user's subscription login, so a
    ///   stray key in the shell profile would otherwise bill (or break) API
    ///   access instead of using the plan. Set `FLINT_CLAUDE_USE_API_KEY=1`
    ///   to keep them.
    pub fn drops_env(&self, name: &str, keep_api_key: bool) -> bool {
        let normalized;
        let name = if cfg!(windows) {
            normalized = name.to_ascii_uppercase();
            normalized.as_str()
        } else {
            name
        };
        let session_marker = name == "CLAUDECODE"
            || name.starts_with("CLAUDE_CODE_")
            || matches!(name, "CLAUDE_PID" | "CLAUDE_EFFORT");
        let api_key = matches!(name, "ANTHROPIC_API_KEY" | "ANTHROPIC_AUTH_TOKEN");
        match self {
            AcpAgent::ClaudeCode => session_marker || (api_key && !keep_api_key),
            AcpAgent::Codex | AcpAgent::Droid | AcpAgent::OpenCode | AcpAgent::Custom { .. } => {
                session_marker
            }
        }
    }
}

/// PATH for GUI-launched adapters, preserving the platform's path separator.
pub fn search_path(home: Option<&Path>) -> OsString {
    flint_platform::search_path(home)
}

/// `name` itself if it is a path, else the first match on `search_path`.
pub fn find(name: &str, search_path: impl AsRef<OsStr>) -> Option<PathBuf> {
    flint_platform::find_executable(name, search_path.as_ref())
}

/// A user-facing explanation with the fix, from an agent error and the
/// adapter's recent stderr.
pub fn describe(agent: &AcpAgent, message: &str, code: Option<i64>, stderr: &str) -> String {
    let combined = format!("{message}\n{stderr}");
    let detail = message.trim();
    if looks_like_auth_error(code, &combined) {
        return format!(
            "{} isn't logged in ({detail}). {}",
            agent.name(),
            agent.login_hint()
        );
    }
    let lines: Vec<&str> = stderr.lines().filter(|l| !l.trim().is_empty()).collect();
    let tail = lines[lines.len().saturating_sub(4)..].join(" · ");
    if tail.is_empty() {
        format!("{} stopped responding: {detail}", agent.name())
    } else {
        format!(
            "{} stopped responding: {detail}. Adapter output: {tail}",
            agent.name()
        )
    }
}

/// Whether an error from the agent means it isn't logged in.
pub fn looks_like_auth_error(code: Option<i64>, message: &str) -> bool {
    // ACP's `auth_required` error code.
    if code == Some(-32000) {
        return true;
    }
    let lower = message.to_lowercase();
    [
        "auth",
        "log in",
        "login",
        "logged in",
        "not logged",
        "unauthorized",
        "api key",
        "credential",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[cfg(windows)]
    #[test]
    fn windows_environment_filtering_is_case_insensitive() {
        assert!(AcpAgent::ClaudeCode.drops_env("anthropic_api_key", false));
        assert!(!AcpAgent::ClaudeCode.drops_env("anthropic_api_key", true));
        for agent in [
            AcpAgent::ClaudeCode,
            AcpAgent::Codex,
            AcpAgent::Droid,
            AcpAgent::OpenCode,
        ] {
            assert!(agent.drops_env("claudecode", false));
            assert!(agent.drops_env("claude_code_session_id", false));
        }
    }

    #[test]
    fn finds_adapters_and_falls_back_to_npx() {
        let dir = tempfile::tempdir().expect("tempdir");
        let bin = dir.path().join("bin");
        std::fs::create_dir_all(&bin).expect("mkdir");
        std::fs::write(bin.join("npx"), "").expect("write");
        let path = bin.display().to_string();
        assert_eq!(
            AcpAgent::ClaudeCode.command(&path),
            Ok((
                bin.join("npx"),
                vec![
                    "-y".to_string(),
                    "@agentclientprotocol/claude-agent-acp".to_string()
                ]
            ))
        );
        std::fs::write(bin.join("codex-acp"), "").expect("write");
        assert_eq!(
            AcpAgent::Codex.command(&path),
            Ok((bin.join("codex-acp"), Vec::new()))
        );
        let missing = AcpAgent::ClaudeCode
            .command("/nonexistent")
            .expect_err("missing");
        assert!(
            missing.contains("npm i -g @agentclientprotocol/claude-agent-acp"),
            "{missing}"
        );
    }

    #[test]
    fn opencode_uses_its_native_acp_server_without_an_adapter_or_npx() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = std::env::join_paths([dir.path()]).unwrap();
        std::fs::write(dir.path().join("npx"), "").unwrap();
        let missing = AcpAgent::OpenCode
            .command(&path)
            .expect_err("missing OpenCode");
        assert!(missing.contains("npm i -g opencode-ai"), "{missing}");
        assert!(missing.contains("isn't installed"), "{missing}");
        std::fs::write(dir.path().join("opencode"), "").unwrap();
        assert_eq!(
            AcpAgent::OpenCode.command(&path),
            Ok((dir.path().join("opencode"), vec!["acp".into()]))
        );
        assert_eq!(
            AcpAgent::for_kind(AgentKind::OpenCode),
            Some(AcpAgent::OpenCode)
        );
        assert_eq!(AcpAgent::OpenCode.name(), "OpenCode");
        assert_eq!(AcpAgent::OpenCode.id(), "opencode");
        assert_eq!(AcpAgent::OpenCode.label_prefix(), "opencode");
        assert_eq!(
            describe(
                &AcpAgent::OpenCode,
                "Authentication required",
                Some(-32000),
                ""
            ),
            "OpenCode isn't logged in (Authentication required). Run `opencode auth login` in a terminal, then retry."
        );
        assert!(AcpAgent::OpenCode.drops_env("CLAUDECODE", false));
        for key in ["OPENCODE_API_KEY", "ANTHROPIC_API_KEY", "OPENAI_API_KEY"] {
            assert!(!AcpAgent::OpenCode.drops_env(key, false));
        }
    }

    #[test]
    fn droid_uses_its_native_acp_server_without_an_adapter_or_npx() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().display().to_string();
        std::fs::write(dir.path().join("npx"), "").expect("npx");
        let missing = AcpAgent::Droid.command(&path).expect_err("missing Droid");
        assert!(missing.contains("npm i -g droid"));
        assert!(missing.contains("isn't installed"));
        std::fs::write(dir.path().join("droid"), "").expect("Droid");
        assert_eq!(
            AcpAgent::Droid.command(&path),
            Ok((
                dir.path().join("droid"),
                vec![
                    "exec".to_string(),
                    "--output-format".to_string(),
                    "acp".to_string()
                ],
            ))
        );
        assert_eq!(AcpAgent::for_kind(AgentKind::Droid), Some(AcpAgent::Droid));
        assert_eq!(AcpAgent::Droid.id(), "droid");
        assert_eq!(AcpAgent::Droid.label_prefix(), "droid");
        assert_eq!(
            describe(
                &AcpAgent::Droid,
                "Authentication required",
                Some(-32000),
                ""
            ),
            "Droid isn't logged in (Authentication required). Run `droid` once in a terminal to log in, then retry."
        );
        assert!(AcpAgent::Droid.drops_env("CLAUDECODE", false));
        assert!(!AcpAgent::Droid.drops_env("FACTORY_API_KEY", false));
        assert!(!AcpAgent::Droid.drops_env("ANTHROPIC_API_KEY", false));
    }

    #[test]
    fn claude_uses_the_subscription_login_not_a_stray_api_key() {
        let claude = AcpAgent::ClaudeCode;
        assert!(claude.drops_env("ANTHROPIC_API_KEY", false));
        assert!(!claude.drops_env("ANTHROPIC_API_KEY", true));
        assert!(claude.drops_env("CLAUDECODE", true));
        assert!(claude.drops_env("CLAUDE_CODE_SESSION_ID", true));
        assert!(!claude.drops_env("PATH", false));
        assert!(!claude.drops_env("CLAUDE_CONFIG_DIR", false));
        let codex = AcpAgent::Codex;
        assert!(!codex.drops_env("ANTHROPIC_API_KEY", false));
        assert!(!codex.drops_env("OPENAI_API_KEY", false));
        assert!(codex.drops_env("CLAUDE_CODE_ENTRYPOINT", false));
    }

    #[test]
    fn recognizes_auth_errors() {
        assert!(looks_like_auth_error(
            Some(-32000),
            "Authentication required"
        ));
        assert!(looks_like_auth_error(None, "Please run /login"));
        assert!(!looks_like_auth_error(Some(-32603), "internal error"));
    }

    #[cfg(windows)]
    #[test]
    fn windows_adapters_find_npm_cmd_shims_and_native_executables() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("folder with spaces");
        std::fs::create_dir(&bin).unwrap();
        let path = std::env::join_paths([&bin]).unwrap();
        std::fs::write(bin.join("npx.cmd"), "").unwrap();
        assert_eq!(
            AcpAgent::ClaudeCode.command(&path).unwrap().0,
            bin.join("npx.cmd")
        );
        std::fs::write(bin.join("codex-acp.cmd"), "").unwrap();
        assert_eq!(
            AcpAgent::Codex.command(&path).unwrap().0,
            bin.join("codex-acp.cmd")
        );
        std::fs::write(bin.join("droid.exe"), "").unwrap();
        assert_eq!(
            AcpAgent::Droid.command(&path).unwrap().0,
            bin.join("droid.exe")
        );
        std::fs::write(bin.join("opencode.cmd"), "").unwrap();
        assert_eq!(
            AcpAgent::OpenCode.command(&path).unwrap(),
            (bin.join("opencode.cmd"), vec!["acp".into()])
        );
        assert_eq!(
            find(bin.join("droid.exe").to_str().unwrap(), &path),
            Some(bin.join("droid.exe"))
        );
    }
}
