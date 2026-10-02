//! Real-mode wiring: builds the engine config for a workspace from the user's
//! settings, and reads the current git branch for the header.

use std::path::Path;

use anyhow::bail;
use flint_agent::AgentConfig;
use flint_agent::ApprovalMode;
use flint_agent::DEFAULT_CONTEXT_BUDGET_TOKENS;
use flint_agent::config::jev_from_env;

use crate::settings;
use crate::settings::Settings;

/// Prefix of the error shown when no key file exists (the UI offers Settings).
pub const NO_KEY: &str = "No API key found";

/// Settings decide the model and endpoint; `FLINT_MODEL` / `FLINT_BASE_URL`
/// override them for one run. The key is read from the key file and never
/// logged or shown.
pub fn config_for(
    workspace: &Path,
    settings: &Settings,
    key_path: &Path,
    approval: ApprovalMode,
) -> anyhow::Result<AgentConfig> {
    let api_key = std::fs::read_to_string(key_path)
        .unwrap_or_default()
        .trim()
        .to_string();
    if api_key.is_empty() {
        bail!(
            "{NO_KEY} at {}. Add your Surplus key there, then retry.",
            settings::display_path(key_path)
        );
    }
    Ok(AgentConfig {
        base_url: std::env::var("FLINT_BASE_URL").unwrap_or_else(|_| settings.base_url.clone()),
        model: std::env::var("FLINT_MODEL").unwrap_or_else(|_| settings.model.clone()),
        api_key,
        workspace: workspace.to_path_buf(),
        approval,
        jev: jev_from_env(),
        session_dir: None,
        context_budget_tokens: DEFAULT_CONTEXT_BUDGET_TOKENS,
        reasoning_effort: None,
    })
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
