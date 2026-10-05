//! Which project a workspace belongs to, so every worktree of one repository
//! groups together in the sidebar.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::session::folder_name;

/// The repository a workspace lives in, and where inside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectIdentity {
    /// The primary checkout's root; the workspace itself outside git.
    pub root: PathBuf,
    /// The linked worktree's folder name; `None` for the primary checkout.
    pub worktree: Option<String>,
    /// The workspace relative to its own worktree's root.
    pub subdir: PathBuf,
}

impl ProjectIdentity {
    /// What sessions group on: the repository and the folder within it.
    pub fn key(&self) -> (&Path, &Path) {
        (&self.root, &self.subdir)
    }

    /// [`Self::key`] as owned paths, for map keys.
    pub fn key_owned(&self) -> (PathBuf, PathBuf) {
        (self.root.clone(), self.subdir.clone())
    }

    /// `repo`, `repo › worktree`, with `/subdir` when below the checkout root.
    pub fn label(&self) -> String {
        let mut label = folder_name(&self.root);
        if let Some(worktree) = &self.worktree {
            label.push_str(" › ");
            label.push_str(worktree);
        }
        if !self.subdir.as_os_str().is_empty() {
            label.push('/');
            label.push_str(&self.subdir.to_string_lossy());
        }
        label
    }
}

/// Entries are re-read after this long, so a new worktree is picked up
/// without hitting the filesystem on every frame.
const CACHE_TTL: Duration = Duration::from_secs(10);

const BRANCH_TTL: Duration = Duration::from_secs(2);

type Cache<T> = Mutex<Option<HashMap<PathBuf, (Instant, T)>>>;

static BRANCHES: Cache<Option<String>> = Mutex::new(None);
static CACHE: Cache<ProjectIdentity> = Mutex::new(None);

/// The identity of `workspace`, cached briefly because the sidebar asks on
/// every frame.
pub fn identity(workspace: &Path) -> ProjectIdentity {
    let mut guard = CACHE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let cache = guard.get_or_insert_with(HashMap::new);
    if let Some((read, found)) = cache.get(workspace)
        && read.elapsed() < CACHE_TTL
    {
        return found.clone();
    }
    let found = read_identity(workspace);
    cache.insert(workspace.to_path_buf(), (Instant::now(), found.clone()));
    found
}

fn read_identity(workspace: &Path) -> ProjectIdentity {
    let Some(checkout) = checkout_of(workspace) else {
        return ProjectIdentity {
            root: workspace.to_path_buf(),
            worktree: None,
            subdir: PathBuf::new(),
        };
    };
    let subdir = workspace
        .strip_prefix(checkout)
        .map(Path::to_path_buf)
        .unwrap_or_default();
    let root = common_root(checkout).unwrap_or_else(|| checkout.to_path_buf());
    let worktree = (root != checkout).then(|| folder_name(checkout));
    ProjectIdentity {
        root,
        worktree,
        subdir,
    }
}

/// The nearest folder at or above `workspace` holding a `.git`.
fn checkout_of(workspace: &Path) -> Option<&Path> {
    workspace.ancestors().find(|dir| dir.join(".git").exists())
}

/// Where a linked worktree or submodule's `.git` file points; `None` when
/// `.git` is a directory.
fn pointed_gitdir(checkout: &Path) -> Option<PathBuf> {
    let link = std::fs::read_to_string(checkout.join(".git")).ok()?;
    let gitdir = Path::new(link.trim().strip_prefix("gitdir:")?.trim());
    Some(checkout.join(gitdir))
}

/// The directory holding this checkout's own `HEAD`.
fn git_dir(checkout: &Path) -> PathBuf {
    pointed_gitdir(checkout).unwrap_or_else(|| checkout.join(".git"))
}

/// The primary checkout's root for a worktree whose `.git` is a file pointing
/// at `<repo>/.git/worktrees/<name>`. `None` for an ordinary checkout, a
/// submodule or a bare layout, which are their own project.
fn common_root(checkout: &Path) -> Option<PathBuf> {
    let gitdir = pointed_gitdir(checkout)?;
    let common = std::fs::read_to_string(gitdir.join("commondir")).ok()?;
    let common = normalize(&gitdir.join(common.trim()));
    (common.file_name()? == ".git")
        .then(|| common.parent().map(Path::to_path_buf))
        .flatten()
}

/// Current branch name, or a short commit for a detached head. Works inside
/// linked worktrees, which keep their own `HEAD`.
pub fn read_branch(workspace: &Path) -> Option<String> {
    let head = std::fs::read_to_string(git_dir(checkout_of(workspace)?).join("HEAD")).ok()?;
    let head = head.trim();
    match head.strip_prefix("ref: refs/heads/") {
        Some(branch) => Some(branch.to_string()),
        None => Some(head.chars().take(7).collect()),
    }
}

/// [`read_branch`], cached briefly: a checkout can change branch at any time,
/// but the sidebar asks every frame.
pub fn branch(workspace: &Path) -> Option<String> {
    let mut guard = BRANCHES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let cache = guard.get_or_insert_with(HashMap::new);
    if let Some((read, found)) = cache.get(workspace)
        && read.elapsed() < BRANCH_TTL
    {
        return found.clone();
    }
    let found = read_branch(workspace);
    cache.insert(workspace.to_path_buf(), (Instant::now(), found.clone()));
    found
}

/// Resolves `.` and `..` lexically; the paths come from git's own files, so
/// symlinks are already what git resolved.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
#[path = "project_tests.rs"]
mod tests;
