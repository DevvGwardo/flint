//! Native delegation into real Git checkouts. Never switch the source tree,
//! overwrite a branch, fetch, or remove a checkout.

use std::ffi::OsStr;
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tokio::process::Command;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Request {
    pub branch: String,
    pub repository: Option<PathBuf>,
    #[serde(default = "head")]
    pub base: String,
}

fn head() -> String {
    "HEAD".into()
}

impl Request {
    pub fn parse(args: &Map<String, Value>) -> Result<Option<Self>, String> {
        let Some(value) = args.get("worktree").filter(|value| !value.is_null()) else {
            return Ok(None);
        };
        let request: Self = serde_json::from_value(value.clone())
            .map_err(|error| format!("Invalid `worktree`: {error}"))?;
        for value in [&request.branch, &request.base] {
            if value.is_empty()
                || value.len() > 1024
                || value.starts_with('-')
                || value.chars().any(char::is_control)
            {
                return Err("Worktree branch/base must be explicit Git references.".into());
            }
        }
        if request
            .repository
            .as_ref()
            .is_some_and(|path| !path.is_absolute())
        {
            return Err("`worktree.repository` must be an absolute repository path.".into());
        }
        Ok(Some(request))
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct Checkout {
    pub workspace: PathBuf,
    pub repository: PathBuf,
    pub branch: String,
}

/// Use the same managed root as the desktop picker.
pub(crate) fn home() -> Result<PathBuf, String> {
    std::env::var_os("FLINT_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| flint_platform::home_dir().map(|home| home.join(".flint")))
        .ok_or_else(|| "Cannot locate Hooter's worktree directory.".into())
}

pub(crate) async fn prepare(
    request: &Request,
    workspace: &Path,
    home: &Path,
) -> Result<Checkout, String> {
    let repository = request
        .repository
        .as_deref()
        .unwrap_or(workspace)
        .canonicalize()
        .map_err(|error| format!("Cannot open worktree repository: {error}"))?;
    let checked = git(
        &repository,
        ["check-ref-format", "--branch", &request.branch],
    )
    .await?;
    if checked.strip_suffix(b"\n").unwrap_or(&checked) != request.branch.as_bytes() {
        return Err("Use an explicit branch name, not a previous-checkout expression.".into());
    }
    let listing = git(&repository, ["worktree", "list", "--porcelain", "-z"]).await?;
    if let Some(path) = checkout_for_branch(&listing, &request.branch)? {
        let path = path.canonicalize().map_err(|error| error.to_string())?;
        if workspace
            .canonicalize()
            .unwrap_or_else(|_| workspace.into())
            .starts_with(&path)
        {
            return Err(
                "This branch is the current workspace. Omit worktree or choose another branch."
                    .into(),
            );
        }
        if !git(
            &path,
            [
                "status",
                "--porcelain",
                "--untracked-files=all",
                "--ignored",
                "-z",
            ],
        )
        .await?
        .is_empty()
        {
            return Err("This worktree has modified, untracked, or ignored files. Choose a new branch to preserve existing work.".into());
        }
        return Ok(Checkout {
            workspace: path,
            repository,
            branch: request.branch.clone(),
        });
    }
    let root = home.join("worktrees");
    tokio::fs::create_dir_all(&root)
        .await
        .map_err(|error| format!("Cannot create worktree directory: {error}"))?;
    let path = root
        .canonicalize()
        .map_err(|error| error.to_string())?
        .join(format!("agent-{}", uuid::Uuid::new_v4()));
    // Existing, unoccupied local branches are opened without resetting them.
    let reference = format!("refs/heads/{}", request.branch);
    let existing = git(&repository, ["show-ref", "--verify", "--quiet", &reference])
        .await
        .is_ok();
    if existing {
        git(
            &repository,
            [
                OsStr::new("worktree"),
                OsStr::new("add"),
                OsStr::new("--"),
                path.as_os_str(),
                OsStr::new(&request.branch),
            ],
        )
        .await?;
    } else {
        let base = format!("{}^{{commit}}", request.base);
        let commit = git(
            &repository,
            ["rev-parse", "--verify", "--end-of-options", &base],
        )
        .await?;
        let commit = std::str::from_utf8(&commit)
            .map_err(|error| error.to_string())?
            .trim();
        git(
            &repository,
            [
                OsStr::new("worktree"),
                OsStr::new("add"),
                OsStr::new("-b"),
                OsStr::new(&request.branch),
                OsStr::new("--"),
                path.as_os_str(),
                OsStr::new(commit),
            ],
        )
        .await?;
    }
    Ok(Checkout {
        workspace: path,
        repository,
        branch: request.branch.clone(),
    })
}

pub(crate) async fn validate(checkout: &Checkout) -> Result<(), String> {
    let listing = git(
        &checkout.repository,
        ["worktree", "list", "--porcelain", "-z"],
    )
    .await?;
    let path = checkout_for_branch(&listing, &checkout.branch)?
        .ok_or("The saved subagent worktree is no longer registered.")?;
    if path.canonicalize().ok()
        != Some(
            checkout
                .workspace
                .canonicalize()
                .map_err(|error| error.to_string())?,
        )
    {
        return Err("The saved subagent branch now belongs to a different checkout.".into());
    }
    Ok(())
}

fn checkout_for_branch(bytes: &[u8], branch: &str) -> Result<Option<PathBuf>, String> {
    for (index, record) in bytes
        .split(|byte| *byte == 0)
        .collect::<Vec<_>>()
        .split(|field| field.is_empty())
        .enumerate()
    {
        let wanted = format!("branch refs/heads/{branch}");
        if !record.contains(&wanted.as_bytes()) {
            continue;
        }
        if index == 0
            || record.iter().any(|field| {
                *field == b"bare" || field.starts_with(b"locked") || field.starts_with(b"prunable")
            })
        {
            return Err("This branch's checkout is primary, bare, locked, or unavailable. Choose a new branch for isolated work.".into());
        }
        let path = record
            .iter()
            .find_map(|field| field.strip_prefix(b"worktree "))
            .ok_or("Git returned an invalid checkout record.")?;
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt as _;
            return Ok(Some(std::ffi::OsString::from_vec(path.to_vec()).into()));
        }
        #[cfg(not(unix))]
        return Ok(Some(
            std::str::from_utf8(path)
                .map_err(|error| error.to_string())?
                .into(),
        ));
    }
    Ok(None)
}

/// Direct argv, no shell, hooks, credential prompts, or inherited Git overrides.
async fn git(
    cwd: &Path,
    args: impl IntoIterator<Item = impl AsRef<OsStr>>,
) -> Result<Vec<u8>, String> {
    let mut stdout = tempfile::tempfile().map_err(|error| error.to_string())?;
    let mut stderr = tempfile::tempfile().map_err(|error| error.to_string())?;
    let hooks = tempfile::tempdir().map_err(|error| error.to_string())?;
    let mut command = Command::new("git");
    command
        .current_dir(cwd)
        .args([
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.askPass=",
            "-c",
            "credential.interactive=false",
        ])
        .arg("-c")
        .arg(format!("core.hooksPath={}", hooks.path().display()))
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(stdout.try_clone().map_err(|error| error.to_string())?)
        .stderr(stderr.try_clone().map_err(|error| error.to_string())?)
        .kill_on_drop(true);
    for key in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_INDEX_FILE",
    ] {
        command.env_remove(key);
    }
    let mut child = command
        .spawn()
        .map_err(|error| format!("Cannot start Git: {error}"))?;
    let status = tokio::time::timeout(Duration::from_secs(60), child.wait())
        .await
        .map_err(|_| {
            "Worktree Git operation timed out; inspect the repository before retrying.".to_string()
        })?
        .map_err(|error| format!("Cannot start Git: {error}"))?;
    if !status.success() {
        stderr.rewind().map_err(|error| error.to_string())?;
        let mut message = Vec::new();
        stderr
            .take(16_384)
            .read_to_end(&mut message)
            .map_err(|error| error.to_string())?;
        return Err(format!("Git: {}", String::from_utf8_lossy(&message).trim()));
    }
    if stdout.metadata().map_err(|error| error.to_string())?.len() > 2 * 1024 * 1024 {
        return Err("Git worktree listing exceeded the size limit.".into());
    }
    stdout.rewind().map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    stdout
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    Ok(bytes)
}

#[cfg(test)]
#[path = "worktree_tests.rs"]
pub(crate) mod tests;
