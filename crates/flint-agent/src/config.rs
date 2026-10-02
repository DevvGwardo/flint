//! Default configurations.

use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;

use crate::protocol::AgentConfig;
use crate::protocol::ApprovalMode;
use crate::protocol::JevConfig;

/// Local Surplus Intelligence shim (Chat Completions).
pub const SURPLUS_BASE_URL: &str = "http://127.0.0.1:18433/v1";
pub const SURPLUS_MODEL: &str = "deepseek-v4.1-flash";
const DEFAULT_JEV_BASE_URL: &str = "https://api.typesafe.ai";
const DEFAULT_JEV_MODEL: &str = "jev-latest";

impl AgentConfig {
    /// deepseek-v4.1-flash on the local Surplus shim, key from
    /// `~/.fx/surplus.key`, no approvals, JEV when `TYPESAFE_API_KEY` is set.
    pub fn surplus_default(workspace: PathBuf) -> Result<Self> {
        let home = std::env::var_os("HOME").context("HOME is not set")?;
        let key_path = PathBuf::from(home).join(".fx").join("surplus.key");
        let api_key = std::fs::read_to_string(&key_path)
            .with_context(|| format!("cannot read the Surplus key at {}", key_path.display()))?
            .trim()
            .to_string();
        anyhow::ensure!(
            !api_key.is_empty(),
            "the Surplus key file {} is empty",
            key_path.display()
        );
        Ok(Self {
            base_url: SURPLUS_BASE_URL.to_string(),
            model: SURPLUS_MODEL.to_string(),
            api_key,
            workspace,
            approval: ApprovalMode::Auto,
            jev: jev_from_env(),
        })
    }
}

/// JEV settings from `TYPESAFE_API_KEY`, `TYPESAFE_BASE_URL` and `JEV_MODEL`.
pub fn jev_from_env() -> Option<JevConfig> {
    let env = |name: &str| {
        std::env::var(name)
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    };
    Some(JevConfig {
        api_key: env("TYPESAFE_API_KEY")?,
        base_url: env("TYPESAFE_BASE_URL").unwrap_or_else(|| DEFAULT_JEV_BASE_URL.to_string()),
        model: env("JEV_MODEL").unwrap_or_else(|| DEFAULT_JEV_MODEL.to_string()),
    })
}
