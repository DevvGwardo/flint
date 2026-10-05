//! Git worktree operations, adapted from T3 Code's GitVcsDriverCore:
//! NUL-delimited discovery and branch-based checkouts under the app home.
//! No operation forces removal, deletes branches, or changes the source tree.

use std::ffi::OsStr;
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, bail};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worktree {
    pub path: PathBuf,
    pub branch: Option<String>,
    pub detached: bool,
    pub locked: bool,
    pub bare: bool,
    pub prunable: bool,
    pub primary: bool,
}

impl Worktree {
    fn new(path: PathBuf, primary: bool) -> Self {
        Self {
            path,
            branch: None,
            detached: false,
            locked: false,
            bare: false,
            prunable: false,
            primary,
        }
    }
}

/// `-z` preserves paths containing spaces, newlines, quotes and backslashes.
fn parse_list(bytes: &[u8]) -> Result<Vec<Worktree>> {
    let mut trees = Vec::new();
    let mut current: Option<Worktree> = None;
    for field in bytes.split(|byte| *byte == 0) {
        if field.is_empty() {
            if let Some(tree) = current.take() {
                trees.push(tree);
            }
        } else if let Some(path) = field.strip_prefix(b"worktree ") {
            current = Some(Worktree::new(path_from_bytes(path)?, trees.is_empty()));
        } else if let Some(tree) = &mut current {
            if let Some(branch) = field.strip_prefix(b"branch refs/heads/") {
                tree.branch = Some(String::from_utf8(branch.to_vec())?);
            } else if field == b"detached" {
                tree.detached = true;
            } else if field == b"bare" {
                tree.bare = true;
            } else if field == b"locked" || field.starts_with(b"locked ") {
                tree.locked = true;
            } else if field == b"prunable" || field.starts_with(b"prunable ") {
                tree.prunable = true;
            }
        }
    }
    if let Some(tree) = current {
        trees.push(tree);
    }
    Ok(trees)
}

fn path_from_bytes(bytes: &[u8]) -> Result<PathBuf> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt as _;
        Ok(std::ffi::OsString::from_vec(bytes.to_vec()).into())
    }
    #[cfg(not(unix))]
    {
        Ok(std::str::from_utf8(bytes)?.into())
    }
}

pub fn list(cwd: &Path) -> Result<Vec<Worktree>> {
    parse_list(&git(cwd, ["worktree", "list", "--porcelain", "-z"])?)
}

/// New branches start from a resolved commit, never from uncommitted edits.
/// Slugs include a stable hash so `feature/a` and `feature-a` cannot collide.
pub fn create(cwd: &Path, home: &Path, branch: &str, base: &str) -> Result<Worktree> {
    validate_ref(branch)?;
    validate_ref(base)?;
    let checked = git(cwd, ["check-ref-format", "--branch", branch])?;
    if checked.strip_suffix(b"\n").unwrap_or(&checked) != branch.as_bytes() {
        bail!("Enter an explicit branch name, not a previous-checkout expression.");
    }
    let commit = git(
        cwd,
        [
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!("{base}^{{commit}}"),
        ],
    )?;
    let commit = std::str::from_utf8(&commit)?.trim();
    let common = git(
        cwd,
        ["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?;
    let common = path_from_bytes(common.strip_suffix(b"\n").unwrap_or(&common))?;
    let common = common.canonicalize()?;
    let repo = common.parent().unwrap_or(&common);
    let name = repo.file_name().unwrap_or_else(|| OsStr::new("repository"));
    let root = home.join("worktrees").join(format!(
        "{}-{:016x}",
        slug(&name.to_string_lossy()),
        hash(common.as_os_str().as_encoded_bytes())
    ));
    std::fs::create_dir_all(&root)?;
    let root = root.canonicalize()?;
    let path = root.join(format!("{}-{:016x}", slug(branch), hash(branch.as_bytes())));
    git(
        cwd,
        [
            OsStr::new("worktree"),
            OsStr::new("add"),
            OsStr::new("-b"),
            OsStr::new(branch),
            OsStr::new("--"),
            path.as_os_str(),
            OsStr::new(commit),
        ],
    )?;
    let mut tree = Worktree::new(path, false);
    tree.branch = Some(branch.to_string());
    Ok(tree)
}

/// Git refuses dirty and locked checkouts. Keep the branch and its commits.
pub fn remove(cwd: &Path, path: &Path) -> Result<()> {
    let trees = list(cwd)?;
    let tree = trees.iter().find(|tree| same_path(&tree.path, path));
    let Some(tree) = tree else {
        bail!("This checkout is no longer registered with the repository.");
    };
    if tree.primary || tree.bare || tree.locked || tree.prunable {
        bail!("The primary, bare, locked, or missing checkout cannot be removed here.");
    }
    // Git's default removal check can overlook ignored files (for example
    // local .env files). Keep those too rather than silently deleting them.
    if !git(
        &tree.path,
        [
            "status",
            "--porcelain",
            "--untracked-files=all",
            "--ignored",
            "-z",
        ],
    )?
    .is_empty()
    {
        bail!(
            "This checkout has modified, untracked, or ignored files. Keep or move them before removing it."
        );
    }
    git(
        cwd,
        [
            OsStr::new("worktree"),
            OsStr::new("remove"),
            OsStr::new("--"),
            tree.path.as_os_str(),
        ],
    )?;
    Ok(())
}

pub fn same_path(left: &Path, right: &Path) -> bool {
    left == right
        || left
            .canonicalize()
            .ok()
            .zip(right.canonicalize().ok())
            .is_some_and(|(a, b)| a == b)
}

/// Also covers a session rooted in a subfolder of a checkout.
pub fn contains_workspace(checkout: &Path, workspace: &Path) -> bool {
    workspace.starts_with(checkout)
        || checkout
            .canonicalize()
            .ok()
            .zip(workspace.canonicalize().ok())
            .is_some_and(|(a, b)| b.starts_with(a))
}

fn validate_ref(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 1024
        || value.starts_with('-')
        || value.chars().any(char::is_control)
    {
        bail!("Enter a branch or commit name without leading '-' or control characters.");
    }
    Ok(())
}

fn slug(value: &str) -> String {
    let slug: String = value
        .chars()
        .take(48)
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                ch
            } else {
                '-'
            }
        })
        .collect();
    if slug.is_empty() {
        "checkout".into()
    } else {
        slug
    }
}

// FNV-1a: deterministic across launches, unlike a process-seeded hasher.
fn hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}

/// Run off the UI thread. File-backed pipes avoid deadlocks on large output.
/// Bound both output and wall time, disable prompts, and preserve user config.
fn git(cwd: &Path, args: impl IntoIterator<Item = impl AsRef<OsStr>>) -> Result<Vec<u8>> {
    let mut stdout = tempfile::tempfile()?;
    let mut stderr = tempfile::tempfile()?;
    let mut command = Command::new("git");
    command
        .current_dir(cwd)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(stdout.try_clone()?)
        .stderr(stderr.try_clone()?);
    for key in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_INDEX_FILE",
    ] {
        command.env_remove(key);
    }
    let mut child = command.spawn().context("Couldn't start Git")?;
    let deadline = Instant::now() + Duration::from_secs(60);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            result => {
                child.kill().ok();
                child.wait().ok();
                match result {
                    Err(error) => return Err(error.into()),
                    _ => bail!("Git timed out. Check the repository before retrying."),
                }
            }
        }
    };
    if !status.success() {
        stderr.rewind()?;
        let mut message = Vec::new();
        stderr.take(16_384).read_to_end(&mut message)?;
        bail!("Git: {}", String::from_utf8_lossy(&message).trim());
    }
    if stdout.metadata()?.len() > 2 * 1024 * 1024 {
        bail!("Git output exceeded the worktree list limit.");
    }
    stdout.rewind()?;
    let mut bytes = Vec::new();
    stdout.read_to_end(&mut bytes)?;
    Ok(bytes)
}

#[cfg(test)]
#[path = "worktrees_tests.rs"]
mod tests;
