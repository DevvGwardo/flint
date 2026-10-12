//! The `run_command` sandbox: on macOS, commands run under `sandbox-exec`
//! with a policy that allows everything except writing outside the
//! workspace, the temp directories, and the caches build tools need
//! (cargo, npm, pip, …). Reads and the network stay open, so builds, tests
//! and package installs work; a stray `rm -rf ~/…` or a write to dotfiles
//! fails with "Operation not permitted".

use std::path::Path;
use std::path::PathBuf;

/// The macOS sandbox launcher.
pub const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";

/// Caches under `$HOME` that build tools write to.
const HOME_CACHES: &[&str] = &[
    ".cache",
    ".cargo",
    ".rustup",
    ".npm",
    ".pnpm-store",
    ".yarn",
    ".bun",
    ".deno",
    ".gradle",
    ".m2",
    ".nuget",
    ".pub-cache",
    ".cocoapods",
    ".swiftpm",
    "go",
    ".local/share",
    ".local/state",
    "Library/Caches",
    "Library/Developer/Xcode/DerivedData",
    "Library/pnpm",
];

/// The `sandbox-exec` policy for commands in `workspace`, or `None` where
/// there is no sandbox (other platforms, or no `sandbox-exec`).
pub fn profile(workspace: &Path) -> Option<String> {
    if !cfg!(target_os = "macos") || !Path::new(SANDBOX_EXEC).is_file() {
        return None;
    }
    let home = std::env::var_os("HOME").map(PathBuf::from);
    Some(build_profile(workspace, home.as_deref()))
}

fn build_profile(workspace: &Path, home: Option<&Path>) -> String {
    let mut writable: Vec<PathBuf> = vec![
        real(workspace),
        PathBuf::from("/private/tmp"),
        PathBuf::from("/private/var/folders"),
        PathBuf::from("/dev"),
    ];
    if let Some(tmp) = std::env::var_os("TMPDIR") {
        writable.push(real(Path::new(&tmp)));
    }
    writable.extend(linked_git_dirs(workspace));
    if let Some(home) = home {
        writable.extend(HOME_CACHES.iter().map(|dir| real(&home.join(dir))));
    }
    writable.sort();
    writable.dedup();
    let mut out =
        String::from("(version 1)\n(allow default)\n(deny file-write*)\n(allow file-write*");
    for path in writable {
        out.push_str(&format!("\n  (subpath \"{}\")", escape(&path)));
    }
    out.push_str(")\n");
    out
}

/// The path with symlinks resolved (the sandbox sees real paths), or as-is
/// when it doesn't exist yet.
fn real(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn escape(path: &Path) -> String {
    path.display()
        .to_string()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
}

/// Resolve the nearest checkout's Git metadata, including nested workspaces,
/// relative gitdir/commondir files and repositories with a separate Git dir.
fn linked_git_dirs(workspace: &Path) -> Vec<PathBuf> {
    git_dirs(workspace).unwrap_or_default()
}

fn git_dirs(workspace: &Path) -> Option<Vec<PathBuf>> {
    let root = real(workspace);
    let marker = root
        .ancestors()
        .map(|path| path.join(".git"))
        .find(|path| path.exists())?;
    let git = if marker.is_dir() {
        marker
    } else {
        let text = std::fs::read_to_string(&marker).ok()?;
        let target = PathBuf::from(text.trim().strip_prefix("gitdir:")?.trim());
        if target.is_absolute() {
            target
        } else {
            marker.parent()?.join(target)
        }
    };
    let git = real(&git);
    let common = std::fs::read_to_string(git.join("commondir"))
        .ok()
        .map(|text| PathBuf::from(text.trim()))
        .map(|path| {
            if path.is_absolute() {
                path
            } else {
                git.join(path)
            }
        });
    let mut dirs = vec![git];
    if let Some(common) = common {
        dirs.push(real(&common));
    }
    dirs.sort();
    dirs.dedup();
    Some(dirs)
}

/// A note for the model when a command failed on a sandbox denial.
pub fn denial_hint(output: &str) -> Option<&'static str> {
    output.contains("Operation not permitted").then_some(
        "[flint sandbox: commands may only write inside the workspace, temp directories and \
         build caches. For isolated project work, use spawn_agent with worktree.branch \
         (and worktree.repository in general-agent mode) to request a scoped checkout. \
         Otherwise ask the user to attach the needed folder; do not disable the sandbox.]",
    )
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn profile_allows_the_workspace_temp_and_caches_only() {
        let profile = build_profile(Path::new("/w/my \"proj\""), Some(Path::new("/Users/me")));
        assert!(profile.starts_with("(version 1)\n(allow default)\n(deny file-write*)\n"));
        assert!(
            profile.contains(r#"(subpath "/w/my \"proj\"")"#),
            "{profile}"
        );
        assert!(profile.contains(r#"(subpath "/private/tmp")"#));
        assert!(profile.contains(r#"(subpath "/Users/me/.cargo")"#));
        assert!(!profile.contains(r#"(subpath "/Users/me")"#));
    }

    #[test]
    fn linked_worktrees_may_write_their_repository() {
        let dir = tempfile::tempdir().expect("tempdir");
        let ws = dir.path().join("wt");
        std::fs::create_dir_all(&ws).expect("mkdir");
        let git = dir.path().join("repo/.git/worktrees/wt");
        std::fs::create_dir_all(&git).unwrap();
        std::fs::write(git.join("commondir"), "../..\n").unwrap();
        std::fs::write(ws.join(".git"), format!("gitdir: {}\n", git.display())).expect("write");
        assert_eq!(
            linked_git_dirs(&ws),
            vec![
                dir.path().join("repo/.git").canonicalize().unwrap(),
                git.canonicalize().unwrap()
            ]
        );
        assert!(linked_git_dirs(dir.path()).is_empty());
    }

    #[test]
    fn nested_symlinked_worktrees_resolve_relative_metadata_without_granting_source_files() {
        let dir = tempfile::tempdir().unwrap();
        let shared = dir.path().join("source-metadata");
        let git = shared.join("worktrees/task");
        let ws = dir.path().join("checkout");
        std::fs::create_dir_all(&git).unwrap();
        std::fs::create_dir_all(ws.join("nested")).unwrap();
        std::fs::write(git.join("commondir"), "../..\n").unwrap();
        std::fs::write(
            ws.join(".git"),
            "gitdir: ../source-metadata/worktrees/task\n",
        )
        .unwrap();
        let alias = dir.path().join("alias");
        std::os::unix::fs::symlink(&ws, &alias).unwrap();
        let nested = alias.join("nested");
        let common = shared.canonicalize().unwrap();
        assert_eq!(
            linked_git_dirs(&nested),
            vec![common.clone(), git.canonicalize().unwrap()]
        );
        let profile = build_profile(&nested, None);
        assert!(profile.contains(&format!("(subpath \"{}\")", escape(&common))));
        assert!(!profile.contains(&format!(
            "(subpath \"{}\")",
            escape(&dir.path().canonicalize().unwrap())
        )));
    }

    #[test]
    fn separate_per_checkout_and_common_metadata_are_both_writable() {
        let dir = tempfile::tempdir().unwrap();
        let git = dir.path().join("private-checkout-metadata");
        let common = dir.path().join("shared-objects");
        let ws = dir.path().join("checkout");
        for path in [&git, &common, &ws] {
            std::fs::create_dir_all(path).unwrap();
        }
        std::fs::write(ws.join(".git"), "gitdir: ../private-checkout-metadata\n").unwrap();
        std::fs::write(git.join("commondir"), "../shared-objects\n").unwrap();
        let profile = build_profile(&ws, None);
        for path in [&git, &common] {
            assert!(profile.contains(&format!(
                "(subpath \"{}\")",
                escape(&path.canonicalize().unwrap())
            )));
        }
    }
}
