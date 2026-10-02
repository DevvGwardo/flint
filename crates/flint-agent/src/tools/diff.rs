//! Unified diffs for file edits.

use similar::ChangeTag;
use similar::TextDiff;

use crate::protocol::FileDiff;

/// Diff between `old` (None when the file is new) and `new` for `path`.
pub fn file_diff(path: &str, old: Option<&str>, new: &str) -> FileDiff {
    let before = old.unwrap_or_default();
    let diff = TextDiff::from_lines(before, new);
    let (mut added, mut removed) = (0, 0);
    for change in diff.iter_all_changes() {
        match change.tag() {
            ChangeTag::Insert => added += 1,
            ChangeTag::Delete => removed += 1,
            ChangeTag::Equal => {}
        }
    }
    let from = if old.is_some() {
        format!("a/{path}")
    } else {
        "/dev/null".to_string()
    };
    let unified = diff
        .unified_diff()
        .context_radius(3)
        .header(&from, &format!("b/{path}"))
        .to_string();
    FileDiff {
        path: path.to_string(),
        unified,
        added,
        removed,
        created: old.is_none(),
    }
}
