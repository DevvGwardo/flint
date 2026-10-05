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
    if let Some(git) = linked_git_dir(workspace) {
        writable.push(real(&git));
    }
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

/// For a linked git worktree (`.git` is a file pointing elsewhere), the
/// repository's shared `.git` directory, which commits write to.
fn linked_git_dir(workspace: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(workspace.join(".git")).ok()?;
    let target = PathBuf::from(text.trim().strip_prefix("gitdir:")?.trim());
    let target = if target.is_absolute() {
        target
    } else {
        workspace.join(target)
    };
    // `<repo>/.git/worktrees/<name>` -> `<repo>/.git`.
    target
        .ancestors()
        .find(|p| p.file_name().is_some_and(|n| n == ".git"))
        .map(Path::to_path_buf)
        .or(Some(target))
}

/// A note for the model when a command failed on a sandbox denial.
pub fn denial_hint(output: &str) -> Option<&'static str> {
    output.contains("Operation not permitted").then_some(
        "[flint sandbox: commands may only write inside the workspace, temp directories and \
         build caches. If this write is needed, tell the user what to run themselves, or ask \
         them to turn off the sandbox in Settings.]",
    )
}

#[cfg(test)]
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
        std::fs::write(ws.join(".git"), "gitdir: /repo/.git/worktrees/wt\n").expect("write");
        assert_eq!(linked_git_dir(&ws), Some(PathBuf::from("/repo/.git")));
        assert_eq!(linked_git_dir(dir.path()), None);
    }
}
