//! The agent's `fs/*` requests: reads and writes confined to the workspace.

use std::path::Path;
use std::path::PathBuf;

use agent_client_protocol::schema::v1::ReadTextFileRequest;

pub(crate) fn rpc_error(message: String) -> agent_client_protocol::Error {
    let mut error = agent_client_protocol::Error::invalid_params();
    error.message = message;
    error
}

/// Resolves an agent-supplied path, refusing anything outside the workspace.
pub(crate) fn inside_workspace(workspace: &Path, path: &Path) -> Result<PathBuf, String> {
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        workspace.join(path)
    };
    let mut normalized = PathBuf::new();
    for component in joined.components() {
        match component {
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            std::path::Component::CurDir => {}
            other => normalized.push(other.as_os_str()),
        }
    }
    let outside = || {
        format!(
            "{} is outside the workspace {}",
            path.display(),
            workspace.display()
        )
    };
    let root = workspace
        .canonicalize()
        .map_err(|err| format!("cannot resolve workspace {}: {err}", workspace.display()))?;
    // A symlinked workspace may be addressed by its real path: agents often
    // resolve the cwd they were given.
    let base = if normalized.starts_with(workspace) {
        workspace
    } else if normalized.starts_with(&root) {
        root.as_path()
    } else {
        return Err(outside());
    };
    // New files may not exist yet. Check the deepest existing ancestor,
    // including dangling symlinks, before reading, writing or changing cwd.
    for part in normalized
        .ancestors()
        .take_while(|part| part.starts_with(base))
    {
        match part.symlink_metadata() {
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => return Err(format!("cannot inspect {}: {err}", part.display())),
            Ok(_) => match part.canonicalize() {
                Ok(real) if real.starts_with(&root) => break,
                Ok(_) | Err(_) => return Err(outside()),
            },
        }
    }
    Ok(normalized)
}

pub(crate) fn read_text(workspace: &Path, request: &ReadTextFileRequest) -> Result<String, String> {
    let path = inside_workspace(workspace, &request.path)?;
    let text = std::fs::read_to_string(&path)
        .map_err(|err| format!("cannot read {}: {err}", path.display()))?;
    let start = request
        .line
        .map_or(0, |line| line.saturating_sub(1) as usize);
    match (start, request.limit) {
        (0, None) => Ok(text),
        (start, limit) => {
            let lines = text.lines().skip(start);
            let picked: Vec<&str> = match limit {
                Some(limit) => lines.take(limit as usize).collect(),
                None => lines.collect(),
            };
            Ok(picked.join("\n"))
        }
    }
}

pub(crate) fn write_text(
    workspace: &Path,
    path: &Path,
    content: &str,
) -> Result<(PathBuf, Option<String>), String> {
    let path = inside_workspace(workspace, path)?;
    let old = std::fs::read_to_string(&path).ok();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| format!("cannot create {}: {err}", parent.display()))?;
    }
    std::fs::write(&path, content)
        .map_err(|err| format!("cannot write {}: {err}", path.display()))?;
    Ok((path, old))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn paths_refuse_outside_and_dangling_symlinks() {
        let dir = tempfile::tempdir().expect("tempdir");
        let workspace = dir.path().canonicalize().expect("canonical");
        let outside = tempfile::tempdir().expect("outside");
        std::fs::write(outside.path().join("existing.txt"), "keep").expect("write");
        std::os::unix::fs::symlink(outside.path(), workspace.join("out")).expect("link");
        std::os::unix::fs::symlink(outside.path().join("new.txt"), workspace.join("dangling"))
            .expect("link");
        for path in ["out/existing.txt", "out/new/deep.txt", "dangling"] {
            assert!(
                inside_workspace(&workspace, Path::new(path)).is_err(),
                "{path} must be refused"
            );
            assert!(write_text(&workspace, Path::new(path), "overwrite").is_err());
            let request = serde_json::from_value::<ReadTextFileRequest>(serde_json::json!({
                "sessionId": "test",
                "path": workspace.join(path),
            }))
            .expect("request");
            assert!(read_text(&workspace, &request).is_err());
        }
        assert_eq!(
            std::fs::read_to_string(outside.path().join("existing.txt")).expect("read"),
            "keep"
        );
        assert!(!outside.path().join("new").exists());
        assert!(!outside.path().join("new.txt").exists());
    }

    #[test]
    fn paths_allow_internal_symlinks_and_new_descendants() {
        let dir = tempfile::tempdir().expect("tempdir");
        let workspace = dir.path().canonicalize().expect("canonical");
        std::fs::create_dir(workspace.join("src")).expect("mkdir");
        std::os::unix::fs::symlink(workspace.join("src"), workspace.join("inside")).expect("link");
        assert_eq!(
            inside_workspace(&workspace, Path::new("inside/new/file.txt")).expect("inside"),
            workspace.join("inside/new/file.txt")
        );
        write_text(&workspace, Path::new("inside/new/file.txt"), "kept").expect("write");
        assert_eq!(
            std::fs::read_to_string(workspace.join("src/new/file.txt")).expect("read"),
            "kept"
        );
    }

    #[test]
    fn symlinked_workspace_accepts_its_real_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let real = dir.path().canonicalize().expect("canonical").join("real");
        std::fs::create_dir_all(real.join("src")).expect("mkdir");
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&real, &link).expect("link");
        assert_eq!(inside_workspace(&link, &real).expect("root"), real);
        assert_eq!(
            inside_workspace(&link, &real.join("src/new.txt")).expect("descendant"),
            real.join("src/new.txt")
        );
        assert!(inside_workspace(&link, dir.path()).is_err());
        assert!(inside_workspace(&link, &real.join("../elsewhere")).is_err());
    }
}
