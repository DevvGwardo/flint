//! Real-mode wiring: builds the engine config for a workspace from the user's
//! settings, and reads the current git branch for the header.

use std::path::Path;

use anyhow::bail;
use flint_agent::AgentConfig;
use flint_agent::ApprovalMode;

use crate::settings::Settings;

/// Prefix of the error shown when no API key is configured (the UI offers
/// Settings).
pub const NO_KEY: &str = "No API key found";

/// Settings decide the model and endpoint; `FLINT_MODEL` / `FLINT_BASE_URL`
/// override them for one run. The key comes from [`Settings::resolve_key`] and
/// is never logged or shown.
pub fn config_for(
    workspace: &Path,
    settings: &Settings,
    key_file: Option<&Path>,
    approval: ApprovalMode,
) -> anyhow::Result<AgentConfig> {
    let Some(found) = settings.resolve_key(key_file) else {
        bail!(
            "{NO_KEY}. Set FLINT_API_KEY (or OPENAI_API_KEY) in the environment, or choose a \
             key file in Settings, then retry."
        );
    };
    let mut config = AgentConfig::new(
        workspace.to_path_buf(),
        std::env::var("FLINT_BASE_URL").unwrap_or_else(|_| settings.base_url.clone()),
        std::env::var("FLINT_MODEL").unwrap_or_else(|_| settings.model.clone()),
        found.key,
    );
    config.approval = approval;
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
