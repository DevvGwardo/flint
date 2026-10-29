//! Platform behavior shared by the desktop app, agent, adapters and terminal.

mod argv;
pub mod desktop;
mod process;
pub use argv::quote_windows_arg;
pub use process::{ProcessTree, spawn};

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use tokio::process::Command;

/// The user's home, without treating an empty environment variable as a path.
pub fn home_dir() -> Option<PathBuf> {
    home_from(cfg!(windows), |name| std::env::var_os(name))
}

fn home_from(windows: bool, env: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let nonempty = |name| env(name).filter(|value| !value.is_empty());
    if windows {
        nonempty("USERPROFILE")
            .or_else(|| nonempty("HOME"))
            .or_else(|| {
                let mut drive = nonempty("HOMEDRIVE")?;
                drive.push(nonempty("HOMEPATH")?);
                Some(drive)
            })
            .map(PathBuf::from)
    } else {
        nonempty("HOME").map(PathBuf::from)
    }
}

/// PATH inherited by GUI-launched agents, including common per-user installs.
pub fn search_path(home: Option<&Path>) -> OsString {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|path| {
            std::env::split_paths(&path)
                .filter(|p| !p.as_os_str().is_empty())
                .collect()
        })
        .unwrap_or_default();
    let mut extra = Vec::new();
    if cfg!(windows) {
        if let Some(appdata) = std::env::var_os("APPDATA").filter(|v| !v.is_empty()) {
            extra.push(PathBuf::from(appdata).join("npm"));
        }
        if let Some(local) = std::env::var_os("LOCALAPPDATA").filter(|v| !v.is_empty()) {
            extra.push(PathBuf::from(local).join("Microsoft/WindowsApps"));
        }
        if let Some(root) = std::env::var_os("SystemRoot").filter(|v| !v.is_empty()) {
            let root = PathBuf::from(root);
            extra.push(root.join("System32"));
            extra.push(root.join("System32/WindowsPowerShell/v1.0"));
        }
    } else {
        extra
            .extend(["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin"].map(PathBuf::from));
    }
    if let Some(home) = home {
        extra.extend(
            [
                ".local/bin",
                ".npm-global/bin",
                ".bun/bin",
                ".cargo/bin",
                ".opencode/bin",
            ]
            .map(|p| home.join(p)),
        );
    }
    for dir in extra {
        if !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    // Invalid entries are not a reason to discard the inherited PATH.
    std::env::join_paths(&dirs).unwrap_or_else(|_| std::env::var_os("PATH").unwrap_or_default())
}

/// Finds a literal executable path or an executable on PATH. On Windows this
/// also finds npm's `.cmd` shims and native `.exe` / `.com` programs.
pub fn find_executable(name: impl AsRef<OsStr>, search_path: &OsStr) -> Option<PathBuf> {
    let name = Path::new(name.as_ref());
    let extensions = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into())
            .split(';')
            .filter(|ext| {
                [".com", ".exe", ".bat", ".cmd"]
                    .iter()
                    .any(|allowed| ext.eq_ignore_ascii_case(allowed))
            })
            .map(str::to_string)
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    if name.components().count() > 1 || name.is_absolute() {
        return executable_candidate(name, &extensions);
    }
    std::env::split_paths(search_path)
        .filter(|dir| !dir.as_os_str().is_empty())
        .find_map(|dir| executable_candidate(&dir.join(name), &extensions))
}

fn executable_candidate(path: &Path, extensions: &[String]) -> Option<PathBuf> {
    if path.is_file() {
        return Some(on_disk(path).unwrap_or_else(|| path.to_path_buf()));
    }
    if path.extension().is_some() {
        return None;
    }
    extensions.iter().find_map(|extension| {
        let mut candidate = path.as_os_str().to_os_string();
        candidate.push(extension);
        let candidate = PathBuf::from(candidate);
        candidate
            .is_file()
            .then(|| on_disk(&candidate).unwrap_or(candidate))
    })
}

/// The on-disk spelling of `candidate`. `PATHEXT` entries are usually
/// uppercase, but the file may be `npx.cmd`: a case-insensitive filesystem
/// matches either way, so resolve the real name for a stable path.
fn on_disk(candidate: &Path) -> Option<PathBuf> {
    let parent = candidate.parent()?;
    let name = candidate.file_name()?;
    std::fs::read_dir(parent).ok()?.flatten().find_map(|entry| {
        entry
            .file_name()
            .as_encoded_bytes()
            .eq_ignore_ascii_case(name.as_encoded_bytes())
            .then(|| entry.path())
    })
}

/// Windows always has `cmd.exe`; do not depend on Git Bash or PowerShell 7.
#[cfg(windows)]
fn command_shell() -> OsString {
    std::env::var_os("COMSPEC")
        .filter(|value| {
            !value.is_empty() && Path::new(value).is_absolute() && Path::new(value).is_file()
        })
        .unwrap_or_else(|| {
            std::env::var_os("SystemRoot")
                .map(|root| {
                    PathBuf::from(root)
                        .join("System32/cmd.exe")
                        .into_os_string()
                })
                .unwrap_or_else(|| "cmd.exe".into())
        })
}

/// A shell for the interactive terminal, with platform-appropriate arguments.
pub fn interactive_shell() -> (String, Vec<String>) {
    #[cfg(windows)]
    {
        (
            command_shell().to_string_lossy().into_owned(),
            vec!["/D".into()],
        )
    }
    #[cfg(not(windows))]
    {
        let fallback = if cfg!(target_os = "macos") {
            "/bin/zsh"
        } else {
            "/bin/sh"
        };
        let shell = std::env::var("SHELL")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| fallback.into());
        (shell, vec!["-l".into()])
    }
}

pub const SHELL_DESCRIPTION: &str = if cfg!(windows) {
    "cmd.exe /D /S /C (Windows command syntax; use powershell.exe explicitly for PowerShell scripts)"
} else {
    "sh -c"
};

/// A script is intentionally interpreted by a shell, unlike executable/argv
/// requests. `/D` disables cmd AutoRun; `/S` strips only our outer quote pair.
pub fn shell_command(script: &str) -> Command {
    #[cfg(windows)]
    {
        let mut command = Command::new(command_shell());
        command.args(["/D", "/S", "/C"]);
        command.raw_arg(format!("\"{script}\""));
        command
    }
    #[cfg(not(windows))]
    {
        let mut command = Command::new("sh");
        command.args(["-c", script]);
        command
    }
}

/// Resolve a program without ever interpreting its argument vector as a script.
pub fn executable_command(program: impl AsRef<OsStr>, path: &OsStr, cwd: &Path) -> Command {
    let program = Path::new(program.as_ref());
    // Resolve explicit relative paths before CreateProcess on Windows, where
    // executable lookup does not use the child's working-directory parameter.
    let specified = if !program.is_absolute() && program.components().count() > 1 {
        cwd.join(program)
    } else {
        program.to_path_buf()
    };
    let resolved = find_executable(&specified, path).unwrap_or(specified);
    Command::new(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn homes_use_native_environment_and_ignore_empty_values() {
        let env = |name: &str| match name {
            "HOME" => Some(OsString::from("/unix-home")),
            "USERPROFILE" => Some(OsString::from(r"C:\Users\Alice")),
            _ => None,
        };
        assert_eq!(home_from(false, env), Some(PathBuf::from("/unix-home")));
        assert_eq!(home_from(true, env), Some(PathBuf::from(r"C:\Users\Alice")));
        assert_eq!(home_from(true, |_| Some(OsString::new())), None);
        assert_eq!(
            home_from(true, |name| match name {
                "HOMEDRIVE" => Some("D:".into()),
                "HOMEPATH" => Some(r"\Users\Alice".into()),
                _ => None,
            }),
            Some(PathBuf::from(r"D:\Users\Alice"))
        );
    }

    #[test]
    fn executable_extensions_preserve_spaces_and_explicit_suffixes() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().join("adapter with spaces");
        let exe = base.with_extension("exe");
        let cmd = dir.path().join("npx.cmd");
        std::fs::write(&exe, "").unwrap();
        std::fs::write(&cmd, "").unwrap();
        let extensions = vec![".exe".into(), ".cmd".into()];
        assert_eq!(executable_candidate(&base, &extensions), Some(exe.clone()));
        assert_eq!(executable_candidate(&exe, &extensions), Some(exe));
        assert_eq!(
            executable_candidate(&dir.path().join("npx"), &extensions),
            Some(cmd)
        );
        assert_eq!(
            executable_candidate(&dir.path().join("npx.ps1"), &extensions),
            None
        );
    }

    #[test]
    fn executable_lookup_returns_the_on_disk_casing() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("tool.CMD"), "").unwrap();
        // A differently-cased query still resolves to the real file name,
        // so PATHEXT case never leaks into spawned commands.
        assert_eq!(
            on_disk(&dir.path().join("tool.cmd")),
            Some(dir.path().join("tool.CMD"))
        );
        assert_eq!(on_disk(&dir.path().join("missing")), None);
    }

    #[test]
    fn search_paths_roundtrip_without_splitting_space_containing_paths() {
        let home = tempfile::tempdir().unwrap();
        let path = search_path(Some(home.path()));
        let dirs: Vec<_> = std::env::split_paths(&path).collect();
        assert!(dirs.contains(&home.path().join(".local/bin")));
        assert!(dirs.contains(&home.path().join(".cargo/bin")));
        assert!(dirs.contains(&home.path().join(".opencode/bin")));
        assert_eq!(std::env::join_paths(dirs).unwrap(), path);
    }

    #[test]
    fn explicit_relative_programs_resolve_against_the_requested_workspace() {
        let dir = tempfile::tempdir().unwrap();
        let program = dir.path().join("tool with spaces.exe");
        std::fs::write(&program, "").unwrap();
        let command = executable_command("./tool with spaces.exe", OsStr::new(""), dir.path());
        assert_eq!(Path::new(command.as_std().get_program()), program.as_path());
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn windows_shell_preserves_quoted_paths_and_reports_failure() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("folder with spaces");
        std::fs::create_dir(&folder).unwrap();
        let mut command = shell_command(r#"echo literal>"result with spaces.txt" & exit /b 7"#);
        command.current_dir(&folder);
        let (mut child, _tree) = spawn(&mut command).unwrap();
        assert_eq!(child.wait().await.unwrap().code(), Some(7));
        assert_eq!(
            std::fs::read_to_string(folder.join("result with spaces.txt"))
                .unwrap()
                .trim(),
            "literal"
        );
    }
}
