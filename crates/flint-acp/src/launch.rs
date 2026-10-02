//! Finding and describing ACP adapters: which binary to run, the environment
//! it needs, and what to tell the user when it can't start or isn't logged in.

use std::path::Path;
use std::path::PathBuf;

use flint_agent::AgentKind;

/// An agent reachable over ACP.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcpAgent {
    ClaudeCode,
    Codex,
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
            AcpAgent::Custom { name, .. } => name,
        }
    }

    /// Stable id stored in `acp.json`.
    pub fn id(&self) -> String {
        match self {
            AcpAgent::ClaudeCode => "claude_code".to_string(),
            AcpAgent::Codex => "codex".to_string(),
            AcpAgent::Custom { name, .. } => format!("custom:{name}"),
        }
    }

    /// The agent for a session kind (`None` for flint's own engine).
    pub fn for_kind(kind: AgentKind) -> Option<Self> {
        match kind {
            AgentKind::Flint => None,
            AgentKind::ClaudeCode => Some(AcpAgent::ClaudeCode),
            AgentKind::Codex => Some(AcpAgent::Codex),
        }
    }

    /// Adapter binary name and its npm package (for `npx` and install hints).
    fn adapter(&self) -> Option<(&'static str, &'static str)> {
        match self {
            AcpAgent::ClaudeCode => {
                Some(("claude-agent-acp", "@agentclientprotocol/claude-agent-acp"))
            }
            AcpAgent::Codex => Some(("codex-acp", "@agentclientprotocol/codex-acp")),
            AcpAgent::Custom { .. } => None,
        }
    }

    /// How to install the adapter.
    pub fn install_hint(&self) -> String {
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
            AcpAgent::Custom { name, .. } => format!("Log in to {name}, then retry."),
        }
    }

    /// The command to spawn: the adapter on PATH or in `~/.local/bin`, else
    /// `npx -y <package>`.
    pub fn command(&self, search_path: &str) -> Result<(PathBuf, Vec<String>), String> {
        match self {
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
}

/// PATH for the adapter. A GUI app launched from Finder gets a minimal PATH,
/// so the usual install locations are added; adapters spawn `claude` /
/// `codex` / `node` themselves.
pub fn search_path(home: Option<&Path>) -> String {
    let mut dirs: Vec<String> = std::env::var("PATH")
        .unwrap_or_default()
        .split(':')
        .filter(|d| !d.is_empty())
        .map(str::to_string)
        .collect();
    let mut extra = vec![
        "/opt/homebrew/bin".to_string(),
        "/usr/local/bin".to_string(),
        "/usr/bin".to_string(),
        "/bin".to_string(),
    ];
    if let Some(home) = home {
        extra.insert(0, home.join(".local/bin").display().to_string());
        extra.push(home.join(".npm-global/bin").display().to_string());
        extra.push(home.join(".bun/bin").display().to_string());
    }
    for dir in extra {
        if !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    dirs.join(":")
}

/// `name` itself if it is a path, else the first match on `search_path`.
pub fn find(name: &str, search_path: &str) -> Option<PathBuf> {
    if name.contains('/') {
        let path = PathBuf::from(name);
        return path.is_file().then_some(path);
    }
    search_path
        .split(':')
        .map(|dir| Path::new(dir).join(name))
        .find(|candidate| candidate.is_file())
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
    fn recognizes_auth_errors() {
        assert!(looks_like_auth_error(
            Some(-32000),
            "Authentication required"
        ));
        assert!(looks_like_auth_error(None, "Please run /login"));
        assert!(!looks_like_auth_error(Some(-32603), "internal error"));
    }
}
