//! Real-mode wiring: builds the engine config for a workspace and reads the
//! current git branch for the title bar.

use std::path::Path;
use std::path::PathBuf;

use flint_agent::AgentConfig;
use flint_agent::ApprovalMode;
use flint_agent::config::SURPLUS_MODEL;

/// The model the app will use, for display before a session starts.
pub fn model_name() -> String {
    std::env::var("FLINT_MODEL").unwrap_or_else(|_| SURPLUS_MODEL.to_string())
}

/// The engine's Surplus defaults (key from `~/.fx/surplus.key`), with the
/// app's approval mode and optional FLINT_BASE_URL / FLINT_MODEL overrides.
pub fn config_for(workspace: &Path, approval: ApprovalMode) -> anyhow::Result<AgentConfig> {
    let mut config = AgentConfig::surplus_default(workspace.to_path_buf())?;
    config.approval = approval;
    config.model = model_name();
    if let Ok(base_url) = std::env::var("FLINT_BASE_URL") {
        config.base_url = base_url;
    }
    Ok(config)
}

/// Current branch name from `.git/HEAD`, or a short commit for a detached head.
pub fn git_branch(workspace: &Path) -> Option<String> {
    let head = std::fs::read_to_string(workspace.join(".git/HEAD")).ok()?;
    let head = head.trim();
    match head.strip_prefix("ref: refs/heads/") {
        Some(branch) => Some(branch.to_string()),
        None => Some(head.chars().take(7).collect()),
    }
}

/// `~/Projects/foo` style display path.
pub fn display_path(path: &Path) -> String {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    match path.strip_prefix(&home) {
        Ok(rest) if !home.as_os_str().is_empty() => format!("~/{}", rest.display()),
        _ => path.display().to_string(),
    }
}
