//! `@`-mentions: a fuzzy file picker over the workspace (respecting
//! `.gitignore`) and the capped, labelled file attachments sent with a
//! message.

use std::io::Read as _;
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
        .take(MAX_INDEXED_FILES * 5)
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
    score_lowercase(&query.to_lowercase(), path)
}

fn score_lowercase(query: &str, path: &str) -> Option<i64> {
    if query.is_empty() {
        return Some(0);
    }
    let path_lower = path.to_lowercase();
    let name_start = path_lower.rfind('/').map_or(0, |i| i + 1);
    let mut chars = path_lower.char_indices();
    let mut previous_char = None;
    let mut score = 0i64;
    let mut previous_match_end = None;
    for q in query.chars() {
        let (found, ch, word_start) = chars.find_map(|(at, ch)| {
            let word_start =
                previous_char.is_none() || matches!(previous_char, Some('/' | '_' | '-' | '.'));
            previous_char = Some(ch);
            (ch == q).then_some((at, ch, word_start))
        })?;
        score += 1;
        // Match and filename positions use bytes from the same normalized path;
        // adjacency spans the previous scalar's complete UTF-8 encoding.
        if previous_match_end == Some(found) {
            score += 5;
        }
        if found >= name_start {
            score += 3;
        }
        if word_start {
            score += 4;
        }
        previous_match_end = Some(found + ch.len_utf8());
    }
    // Shorter paths win ties.
    Some(score * 100 - path.len() as i64)
}

pub fn search<'a>(files: &'a [String], query: &str) -> Vec<&'a String> {
    if files.is_empty() {
        return Vec::new();
    }
    // Reuse normalization across the workspace instead of per candidate.
    let query = query.to_lowercase();
    let hits = ranked_hits(files, &query);
    // Size the reference buffer separately instead of inheriting hit capacity.
    let mut results = Vec::with_capacity(hits.len());
    results.extend(hits.into_iter().map(|(_, path)| path));
    results
}

fn ranked_hits<'a>(files: &'a [String], query: &str) -> Vec<(i64, &'a String)> {
    let rank = |a: &(i64, &String), b: &(i64, &String)| b.0.cmp(&a.0).then_with(|| a.1.cmp(b.1));
    let mut hits = Vec::new();
    for path in files {
        let Some(score) = score_lowercase(query, path) else {
            continue;
        };
        let hit = (score, path);
        if hits.len() == MAX_RESULTS && hits.last().is_some_and(|worst| !rank(&hit, worst).is_lt())
        {
            continue;
        }
        let at = if hits.first().is_none_or(|best| rank(&hit, best).is_lt()) {
            0
        } else {
            hits.partition_point(|best| rank(best, &hit).is_lt())
        };
        // Remove before inserting so storage never grows beyond visible hits.
        if hits.len() == MAX_RESULTS {
            hits.pop();
        }
        hits.insert(at, hit);
    }
    hits
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
        let Ok((content, input_capped)) = attachment_text(workspace, rel) else {
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
        let mut truncated = input_capped || total_lines > MAX_ATTACH_LINES;
        if body.len() > MAX_ATTACH_BYTES {
            let mut cut = MAX_ATTACH_BYTES;
            while !body.is_char_boundary(cut) {
                cut -= 1;
            }
            body.truncate(cut);
            truncated = true;
        }
        let note = if input_capped {
            format!(
                ", truncated to the first {} lines (bounded text prefix)",
                body.lines().count()
            )
        } else if truncated {
            format!(
                ", truncated to the first {} of {total_lines} lines",
                body.lines().count()
            )
        } else {
            String::new()
        };
        let line_count = if input_capped {
            format!("at least {total_lines} lines")
        } else {
            format!("{total_lines} lines")
        };
        out.push_str(&format!(
            "\n\n[Attached file {rel} ({line_count}{note})]\n```\n{body}\n```"
        ));
    }
    out
}

/// Read only a bounded prefix of a regular file still inside the workspace.
/// Canonical checks refuse static escapes; they are not a race-proof sandbox.
fn attachment_text(workspace: &Path, relative: &str) -> std::io::Result<(String, bool)> {
    let relative = Path::new(relative);
    let invalid = || std::io::Error::other("not a readable workspace text file");
    if relative.is_absolute()
        || relative
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return Err(invalid());
    }
    let workspace = workspace.canonicalize()?;
    let path = workspace.join(relative).canonicalize()?;
    if !path.starts_with(&workspace) || !path.metadata()?.is_file() {
        return Err(invalid());
    }
    let file = std::fs::File::open(path)?;
    if !file.metadata()?.is_file() {
        return Err(invalid());
    }
    // Four extra bytes allow a Unicode scalar at the output boundary, plus
    // one sentinel to distinguish a capped prefix from a complete small file.
    let read_limit = MAX_ATTACH_BYTES + 5;
    let mut bytes = Vec::new();
    file.take(read_limit as u64).read_to_end(&mut bytes)?;
    let input_capped = bytes.len() == read_limit;
    if let Err(error) = std::str::from_utf8(&bytes) {
        if input_capped && error.error_len().is_none() {
            bytes.truncate(error.valid_up_to());
        } else {
            return Err(invalid());
        }
    }
    let content = String::from_utf8(bytes).map_err(|_| invalid())?;
    Ok((content, input_capped))
}

#[cfg(test)]
#[path = "mention_tests.rs"]
mod tests;
