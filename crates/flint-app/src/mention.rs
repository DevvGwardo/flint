//! `@`-mentions: a fuzzy file picker over the workspace (respecting
//! `.gitignore`) and the capped, labelled file attachments sent with a
//! message.

use std::path::Path;

/// Files indexed per workspace, so huge trees stay responsive.
pub const MAX_INDEXED_FILES: usize = 20_000;
/// Rows the picker shows.
pub const MAX_RESULTS: usize = 8;
/// Per-file attachment caps.
pub const MAX_ATTACH_BYTES: usize = 24_000;
pub const MAX_ATTACH_LINES: usize = 400;

/// Workspace-relative file paths, honoring `.gitignore` and hidden files.
pub fn index_files(workspace: &Path) -> Vec<String> {
    let mut files = Vec::new();
    for entry in ignore::WalkBuilder::new(workspace)
        .hidden(true)
        .git_ignore(true)
        .require_git(false)
        .build()
        .flatten()
    {
        if entry.file_type().is_some_and(|t| t.is_file())
            && let Ok(rel) = entry.path().strip_prefix(workspace)
        {
            files.push(rel.to_string_lossy().to_string());
            if files.len() >= MAX_INDEXED_FILES {
                break;
            }
        }
    }
    files.sort();
    files
}

/// Subsequence match score (higher is better), or `None` when `query` isn't a
/// subsequence of `path`. Rewards matches in the file name, consecutive
/// characters and word starts.
pub fn score(query: &str, path: &str) -> Option<i64> {
    if query.is_empty() {
        return Some(0);
    }
    let path_lower = path.to_lowercase();
    let name_start = path_lower.rfind('/').map_or(0, |i| i + 1);
    let chars: Vec<char> = path_lower.chars().collect();
    let mut score = 0i64;
    let mut at = 0usize;
    let mut prev: Option<usize> = None;
    for q in query.to_lowercase().chars() {
        let found = (at..chars.len()).find(|&i| chars[i] == q)?;
        score += 1;
        if prev.is_some_and(|p| p + 1 == found) {
            score += 5;
        }
        if found >= name_start {
            score += 3;
        }
        if found == 0 || matches!(chars[found - 1], '/' | '_' | '-' | '.') {
            score += 4;
        }
        prev = Some(found);
        at = found + 1;
    }
    // Shorter paths win ties.
    Some(score * 100 - path.len() as i64)
}

pub fn search<'a>(files: &'a [String], query: &str) -> Vec<&'a String> {
    let mut hits: Vec<(i64, &String)> = files
        .iter()
        .filter_map(|path| score(query, path).map(|s| (s, path)))
        .collect();
    hits.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(b.1)));
    hits.into_iter().take(MAX_RESULTS).map(|(_, p)| p).collect()
}

/// The `@query` being typed at the end of `text`, if any.
pub fn active_query(text: &str) -> Option<&str> {
    let at = text.rfind('@')?;
    let query = &text[at + 1..];
    let starts_word = at == 0 || text[..at].ends_with(char::is_whitespace);
    (starts_word && !query.contains(char::is_whitespace)).then_some(query)
}

/// The message with each attached file appended, capped and labelled.
pub fn attach(message: &str, workspace: &Path, files: &[String]) -> String {
    let mut out = message.to_string();
    for rel in files {
        let Ok(content) = std::fs::read_to_string(workspace.join(rel)) else {
            out.push_str(&format!(
                "\n\n[Attached file {rel}: could not be read as text]"
            ));
            continue;
        };
        let total_lines = content.lines().count();
        let mut body: String = content
            .lines()
            .take(MAX_ATTACH_LINES)
            .collect::<Vec<_>>()
            .join("\n");
        let mut truncated = total_lines > MAX_ATTACH_LINES;
        if body.len() > MAX_ATTACH_BYTES {
            let mut cut = MAX_ATTACH_BYTES;
            while !body.is_char_boundary(cut) {
                cut -= 1;
            }
            body.truncate(cut);
            truncated = true;
        }
        let note = if truncated {
            format!(
                ", truncated to the first {} of {total_lines} lines",
                body.lines().count()
            )
        } else {
            String::new()
        };
        out.push_str(&format!(
            "\n\n[Attached file {rel} ({total_lines} lines{note})]\n```\n{body}\n```"
        ));
    }
    out
}

#[cfg(test)]
#[path = "mention_tests.rs"]
mod tests;
