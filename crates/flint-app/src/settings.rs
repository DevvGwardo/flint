//! User settings in `~/.flint/config.toml` (or `$FLINT_HOME/config.toml`):
//! model, base URL, default approval mode, reasoning effort, theme, and
//! one-time tips. Read on startup, written by the Settings sheet.

use std::path::Path;
use std::path::PathBuf;

use flint_agent::ApprovalMode;
use flint_agent::ReasoningEffort;
use flint_agent::config::SURPLUS_BASE_URL;
use flint_agent::config::SURPLUS_MODEL;
use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub model: String,
    pub base_url: String,
    /// `auto` or `ask`.
    pub approval: String,
    /// `low`, `medium`, `high`, or empty for the provider default.
    pub effort: String,
    pub theme: String,
    /// The welcome tip about the guard was dismissed.
    pub tip_dismissed: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            model: SURPLUS_MODEL.to_string(),
            base_url: SURPLUS_BASE_URL.to_string(),
            approval: "auto".to_string(),
            effort: "medium".to_string(),
            theme: "dark".to_string(),
            tip_dismissed: false,
        }
    }
}

/// `$FLINT_HOME`, or `~/.flint`.
pub fn flint_home() -> PathBuf {
    match std::env::var_os("FLINT_HOME").filter(|v| !v.is_empty()) {
        Some(dir) => PathBuf::from(dir),
        None => home().join(".flint"),
    }
}

pub fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default()
}

impl Settings {
    pub fn path(home: &Path) -> PathBuf {
        home.join("config.toml")
    }

    /// Reads the settings file; a missing or unreadable file gives defaults.
    pub fn load(home: &Path) -> Self {
        std::fs::read_to_string(Self::path(home))
            .ok()
            .and_then(|text| toml::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, home: &Path) -> anyhow::Result<()> {
        let path = Self::path(home);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&path, toml::to_string_pretty(self)?)?;
        Ok(())
    }

    pub fn approval_mode(&self) -> ApprovalMode {
        match self.approval.as_str() {
            "ask" => ApprovalMode::AskForChanges,
            _ => ApprovalMode::Auto,
        }
    }

    pub fn set_approval_mode(&mut self, mode: ApprovalMode) {
        self.approval = match mode {
            ApprovalMode::Auto => "auto",
            ApprovalMode::AskForChanges => "ask",
        }
        .to_string();
    }

    pub fn reasoning_effort(&self) -> Option<ReasoningEffort> {
        match self.effort.as_str() {
            "low" => Some(ReasoningEffort::Low),
            "medium" => Some(ReasoningEffort::Medium),
            "high" => Some(ReasoningEffort::High),
            _ => None,
        }
    }

    pub fn set_reasoning_effort(&mut self, effort: Option<ReasoningEffort>) {
        self.effort = effort
            .map(ReasoningEffort::as_str)
            .unwrap_or("")
            .to_string();
    }
}

/// Where the API key comes from, for display (never the key itself).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyStatus {
    Found(String),
    Missing(String),
}

/// The default Surplus key file.
pub fn default_key_path() -> PathBuf {
    home().join(".fx").join("surplus.key")
}

pub fn key_status(path: &Path) -> KeyStatus {
    let shown = display_path(path);
    match std::fs::read_to_string(path) {
        Ok(key) if !key.trim().is_empty() => KeyStatus::Found(shown),
        _ => KeyStatus::Missing(shown),
    }
}

pub fn jev_key_set() -> bool {
    std::env::var("TYPESAFE_API_KEY").is_ok_and(|key| !key.trim().is_empty())
}

/// `~/…` for paths under the home directory.
pub fn display_path(path: &Path) -> String {
    let home = home();
    match path.strip_prefix(&home) {
        Ok(rest) if !home.as_os_str().is_empty() => format!("~/{}", rest.display()),
        _ => path.display().to_string(),
    }
}
