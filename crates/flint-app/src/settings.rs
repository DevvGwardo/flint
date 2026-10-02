//! User settings in `~/.flint/config.toml` (or `$FLINT_HOME/config.toml`):
//! model, base URL, default approval mode, reasoning effort, theme, and
//! one-time tips. Read on startup, written by the Settings sheet.

use std::path::Path;
use std::path::PathBuf;

use flint_agent::ApprovalMode;
use flint_agent::ReasoningEffort;
use flint_agent::config::DEFAULT_BASE_URL;
use flint_agent::config::DEFAULT_MODEL;
use flint_agent::config::api_key_from_env;
use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub model: String,
    pub base_url: String,
    /// Name of an environment variable holding the API key. Empty falls back
    /// to `FLINT_API_KEY`, then `OPENAI_API_KEY`.
    pub api_key_env: String,
    /// File holding the API key (`~/` is expanded). Empty means none.
    pub api_key_file: String,
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
            model: DEFAULT_MODEL.to_string(),
            base_url: DEFAULT_BASE_URL.to_string(),
            api_key_env: String::new(),
            api_key_file: String::new(),
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
        let Ok(text) = std::fs::read_to_string(Self::path(home)) else {
            return Self::legacy_defaults().unwrap_or_default();
        };
        toml::from_str(&text).unwrap_or_default()
    }

    /// Backward compatibility for early installs that predate the provider
    /// settings: with no config file but a key at `~/.fx/surplus.key`, keep
    /// talking to the local shim those installs were written against.
    fn legacy_defaults() -> Option<Self> {
        legacy_key_path().is_file().then(|| Self {
            model: LEGACY_MODEL.to_string(),
            base_url: LEGACY_BASE_URL.to_string(),
            ..Self::default()
        })
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

const LEGACY_BASE_URL: &str = "http://127.0.0.1:18433/v1";
const LEGACY_MODEL: &str = "deepseek-v4.1-flash";

fn legacy_key_path() -> PathBuf {
    home().join(".fx").join("surplus.key")
}

/// Where the API key comes from, for display (never the key itself).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyStatus {
    /// Found; the string says where (an env var name or a file path).
    Found(String),
    /// Not found; the string says where flint looked first.
    Missing(String),
}

/// An API key and a description of where it came from.
pub struct ResolvedKey {
    pub key: String,
    pub source: String,
}

fn read_key_file(path: &Path) -> Option<String> {
    let key = std::fs::read_to_string(path).ok()?;
    Some(key.trim().to_string()).filter(|key| !key.is_empty())
}

/// Expands a leading `~/` to the home directory.
pub fn expand_home(path: &str) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => home().join(rest),
        None => PathBuf::from(path),
    }
}

impl Settings {
    /// Finds the API key. Order: `key_file` (an explicit override), the
    /// `api_key_file` setting, the env var named by `api_key_env`,
    /// `FLINT_API_KEY`, `OPENAI_API_KEY`. Keys from early installs
    /// (`~/.fx/surplus.key`) are used only for the endpoint they were issued
    /// for, so they are never sent to another provider.
    pub fn resolve_key(&self, key_file: Option<&Path>) -> Option<ResolvedKey> {
        let from_file = |path: PathBuf| {
            read_key_file(&path).map(|key| ResolvedKey {
                key,
                source: display_path(&path),
            })
        };
        if let Some(found) = key_file.and_then(|path| from_file(path.to_path_buf())) {
            return Some(found);
        }
        if !self.api_key_file.is_empty()
            && let Some(found) = from_file(expand_home(&self.api_key_file))
        {
            return Some(found);
        }
        let named = self.api_key_env.trim();
        if !named.is_empty()
            && let Some(key) = std::env::var(named)
                .ok()
                .map(|key| key.trim().to_string())
                .filter(|key| !key.is_empty())
        {
            return Some(ResolvedKey {
                key,
                source: format!("${named}"),
            });
        }
        if let Some(key) = api_key_from_env() {
            let source = if std::env::var("FLINT_API_KEY").is_ok_and(|v| !v.trim().is_empty()) {
                "$FLINT_API_KEY"
            } else {
                "$OPENAI_API_KEY"
            };
            return Some(ResolvedKey {
                key,
                source: source.to_string(),
            });
        }
        if self.base_url == LEGACY_BASE_URL {
            return from_file(legacy_key_path());
        }
        None
    }

    /// What Settings shows for the key: where it was found, or what to set.
    pub fn key_status(&self, key_file: Option<&Path>) -> KeyStatus {
        match self.resolve_key(key_file) {
            Some(found) => KeyStatus::Found(found.source),
            None if !self.api_key_file.is_empty() => {
                KeyStatus::Missing(display_path(&expand_home(&self.api_key_file)))
            }
            None if !self.api_key_env.trim().is_empty() => {
                KeyStatus::Missing(format!("${}", self.api_key_env.trim()))
            }
            None => KeyStatus::Missing("$FLINT_API_KEY".to_string()),
        }
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

#[cfg(test)]
#[path = "settings_tests.rs"]
mod tests;
