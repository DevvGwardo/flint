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
    if normalized.starts_with(workspace) {
        Ok(normalized)
    } else {
        Err(format!(
            "{} is outside the workspace {}",
            path.display(),
            workspace.display()
        ))
    }
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
