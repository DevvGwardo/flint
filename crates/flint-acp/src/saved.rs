//! `session_dir/acp.json`: which agent session this is, its turn count,
//! and the options the user chose (re-applied when the agent can't reopen
//! the conversation).

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct Saved {
    pub agent: String,
    pub session_id: String,
    pub turn_id: u64,
    /// Option id -> value the user picked.
    #[serde(default)]
    pub options: BTreeMap<String, String>,
}

pub(crate) fn load_saved(dir: &Path) -> Option<Saved> {
    serde_json::from_slice(&std::fs::read(dir.join("acp.json")).ok()?).ok()
}

pub(crate) fn save_saved(dir: &Path, saved: &Saved) {
    if std::fs::create_dir_all(dir).is_ok()
        && let Ok(bytes) = serde_json::to_vec_pretty(saved)
    {
        let tmp = dir.join("acp.json.tmp");
        if std::fs::write(&tmp, bytes).is_ok() {
            let _ = std::fs::rename(&tmp, dir.join("acp.json"));
        }
    }
}
