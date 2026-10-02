//! Default configurations.

use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;

use crate::protocol::AgentConfig;
use crate::protocol::ApprovalMode;
use crate::protocol::DEFAULT_CONTEXT_BUDGET_TOKENS;
use crate::protocol::JevConfig;

/// Default endpoint: any OpenAI-compatible Chat Completions API works.
pub const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";
/// Default model for [`DEFAULT_BASE_URL`]; change it in Settings for other providers.
pub const DEFAULT_MODEL: &str = "gpt-4.1-mini";
const DEFAULT_JEV_BASE_URL: &str = "https://api.typesafe.ai";
const DEFAULT_JEV_MODEL: &str = "jev-latest";

impl AgentConfig {
    /// A config with no approvals, no saved session and JEV when
    /// `TYPESAFE_API_KEY` is set.
    pub fn new(workspace: PathBuf, base_url: String, model: String, api_key: String) -> Self {
        Self {
            base_url,
            model,
            api_key,
            workspace,
            approval: ApprovalMode::Auto,
            jev: jev_from_env(),
            session_dir: None,
            context_budget_tokens: DEFAULT_CONTEXT_BUDGET_TOKENS,
            reasoning_effort: None,
        }
    }

    /// Endpoint, model and key from the environment: `FLINT_BASE_URL`,
    /// `FLINT_MODEL` and [`api_key_from_env`], falling back to
    /// [`DEFAULT_BASE_URL`] / [`DEFAULT_MODEL`]. Fails when no key is set.
    pub fn from_env(workspace: PathBuf) -> Result<Self> {
        let api_key = api_key_from_env()
            .context("no API key: set FLINT_API_KEY (or OPENAI_API_KEY) in the environment")?;
        Ok(Self::new(
            workspace,
            env_var("FLINT_BASE_URL").unwrap_or_else(|| DEFAULT_BASE_URL.to_string()),
            env_var("FLINT_MODEL").unwrap_or_else(|| DEFAULT_MODEL.to_string()),
            api_key,
        ))
    }
}

/// The API key from `FLINT_API_KEY`, else `OPENAI_API_KEY`.
pub fn api_key_from_env() -> Option<String> {
    env_var("FLINT_API_KEY").or_else(|| env_var("OPENAI_API_KEY"))
}

fn env_var(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// JEV settings from `TYPESAFE_API_KEY`, `TYPESAFE_BASE_URL` and `JEV_MODEL`.
pub fn jev_from_env() -> Option<JevConfig> {
    Some(JevConfig {
        api_key: env_var("TYPESAFE_API_KEY")?,
        base_url: env_var("TYPESAFE_BASE_URL").unwrap_or_else(|| DEFAULT_JEV_BASE_URL.to_string()),
        model: env_var("JEV_MODEL").unwrap_or_else(|| DEFAULT_JEV_MODEL.to_string()),
    })
}
