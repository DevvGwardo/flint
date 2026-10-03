//! Real-mode wiring: builds the engine config for a workspace from the user's
//! settings, and reads the current git branch for the header.

use std::path::Path;

use anyhow::bail;
use flint_agent::AgentConfig;
use flint_agent::ApprovalMode;

use crate::settings::KeySources;
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
    sources: &KeySources,
    approval: ApprovalMode,
) -> anyhow::Result<AgentConfig> {
    let mut effective = settings.clone();
    effective.base_url =
        (sources.env)("FLINT_BASE_URL").unwrap_or_else(|| settings.base_url.clone());
    let Some(found) = effective.resolve_key(key_file, sources) else {
        bail!(
            "{NO_KEY}. Set FLINT_API_KEY (or OPENAI_API_KEY) in the environment, or choose a \
             key file in Settings, then retry."
        );
    };
    let mut config = AgentConfig {
        workspace: workspace.to_path_buf(),
        base_url: effective.base_url,
        model: (sources.env)("FLINT_MODEL").unwrap_or_else(|| settings.model.clone()),
        api_key: found.key,
        approval,
        jev: (sources.env)("TYPESAFE_API_KEY").map(|api_key| flint_agent::JevConfig {
            api_key,
            base_url: (sources.env)("TYPESAFE_BASE_URL")
                .unwrap_or_else(|| "https://api.typesafe.ai".into()),
            model: (sources.env)("JEV_MODEL").unwrap_or_else(|| "jev-latest".into()),
        }),
        subagent_model: None,
        session_dir: None,
        context_budget_tokens: flint_agent::DEFAULT_CONTEXT_BUDGET_TOKENS,
        reasoning_effort: None,
    };
    config.subagent_model = (sources.env)("FLINT_SUBAGENT_MODEL")
        .or_else(|| Some(settings.subagent_model.trim().to_string()))
        .map(|model| model.trim().to_string())
        .filter(|model| !model.is_empty());
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
