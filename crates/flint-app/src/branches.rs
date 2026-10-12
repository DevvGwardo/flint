//! Branch selection uses Git's configured remotes and credentials, including
//! private GitHub repositories. Never switch a checkout underneath an agent.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use anyhow::{Result, bail};

use crate::worktrees::{self, Worktree, git};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Branch {
    pub reference: String,
    pub name: String,
    pub remote: Option<String>,
    pub upstream: Option<String>,
    pub checkout: Option<PathBuf>,
}

#[derive(Debug)]
pub struct Repository {
    pub trees: Vec<Worktree>,
    pub branches: Vec<Branch>,
    pub remotes: Vec<String>,
}

pub fn list(cwd: &Path) -> Result<Repository> {
    let trees = worktrees::list(cwd)?;
    let remotes = String::from_utf8(git(cwd, ["remote"])?)?
        .lines()
        .map(str::to_string)
        .collect::<Vec<_>>();
    let bytes = git(
        cwd,
        [
            "for-each-ref",
            "--sort=refname",
            "--format=%(refname)%00%(symref)%00%(upstream)",
            "refs/heads/",
            "refs/remotes/",
        ],
    )?;
    let mut branches = Vec::new();
    for line in bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
    {
        let fields = line.split(|byte| *byte == 0).collect::<Vec<_>>();
        if fields.len() != 3 {
            bail!("Git returned an invalid branch record.");
        }
        // Remote HEAD aliases are navigation hints, not selectable branches.
        if !fields[1].is_empty() {
            continue;
        }
        let reference = std::str::from_utf8(fields[0])?;
        let upstream = std::str::from_utf8(fields[2])?;
        let (name, remote) = if let Some(name) = reference.strip_prefix("refs/heads/") {
            (name.to_string(), None)
        } else if let Some(name) = reference.strip_prefix("refs/remotes/") {
            let remote = remotes
                .iter()
                .filter(|remote| name.starts_with(&format!("{remote}/")))
                .max_by_key(|remote| remote.len());
            let Some(remote) = remote else {
                continue;
            };
            (name.to_string(), Some(remote.clone()))
        } else {
            continue;
        };
        let checkout = if remote.is_none() {
            trees
                .iter()
                .find(|tree| tree.branch.as_deref() == Some(&name))
                .map(|tree| tree.path.clone())
        } else {
            None
        };
        branches.push(Branch {
            reference: reference.into(),
            name,
            remote,
            upstream: (!upstream.is_empty()).then(|| upstream.to_string()),
            checkout,
        });
    }
    Ok(Repository {
        trees,
        branches,
        remotes,
    })
}

/// Explicitly requested, non-destructive refresh. Do not pull, prune, push,
/// modify credentials, or recurse into submodules.
pub fn fetch(cwd: &Path) -> Result<Repository> {
    if list(cwd)?.remotes.is_empty() {
        bail!("This repository has no remotes. Add a GitHub remote using Git first.");
    }
    git(
        cwd,
        [
            "fetch",
            "--all",
            "--no-tags",
            "--no-prune",
            "--no-prune-tags",
            "--no-recurse-submodules",
        ],
    )?;
    list(cwd)
}

pub fn open(cwd: &Path, home: &Path, reference: &str) -> Result<Worktree> {
    // Re-resolve the selection and checkout occupancy immediately before use.
    let repo = list(cwd)?;
    let Some(selected) = repo
        .branches
        .iter()
        .find(|branch| branch.reference == reference)
    else {
        bail!("This branch is no longer available. Refresh the branch list.");
    };
    let (local_name, remote_ref) = if let Some(remote) = &selected.remote {
        let name = selected.name.strip_prefix(&format!("{remote}/")).unwrap();
        if let Some(local) = repo
            .branches
            .iter()
            .find(|branch| branch.remote.is_none() && branch.name == name)
        {
            if local.upstream.as_deref() != Some(reference) {
                bail!(
                    "Local branch '{name}' already exists and does not track '{}'. Open the local branch, or create a new branch from '{}'.",
                    selected.name,
                    selected.name
                );
            }
            (name, None)
        } else {
            (name, Some(reference))
        }
    } else {
        (selected.name.as_str(), None)
    };
    if let Some(tree) = repo
        .trees
        .iter()
        .find(|tree| tree.branch.as_deref() == Some(local_name))
    {
        if tree.bare || tree.prunable || !tree.path.is_dir() {
            bail!("This branch's checkout is missing or unavailable. Repair it using Git first.");
        }
        return Ok(tree.clone());
    }
    git(cwd, ["check-ref-format", "--branch", local_name])?;
    let path = worktrees::checkout_path(cwd, home, local_name)?;
    let mut args = vec![OsStr::new("worktree"), OsStr::new("add")];
    if remote_ref.is_some() {
        args.extend([
            OsStr::new("--track"),
            OsStr::new("-b"),
            OsStr::new(local_name),
        ]);
    }
    args.extend([
        OsStr::new("--"),
        path.as_os_str(),
        OsStr::new(remote_ref.unwrap_or(local_name)),
    ]);
    git(cwd, args)?;
    worktrees::list(cwd)?
        .into_iter()
        .find(|tree| worktrees::same_path(&tree.path, &path))
        .ok_or_else(|| anyhow::anyhow!("Git did not register the new checkout. Refresh the list."))
}

#[cfg(test)]
#[path = "branches_tests.rs"]
mod tests;
