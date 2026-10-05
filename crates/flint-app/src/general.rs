//! A private working folder for conversations that are not attached to a project.

use std::path::{Path, PathBuf};

pub fn workspace(home: &Path) -> PathBuf {
    home.join("agent-workspace")
}

pub fn prepare(home: &Path) -> std::io::Result<PathBuf> {
    let path = workspace(home);
    if path.is_symlink() {
        return Err(std::io::Error::other(
            "The general-agent working folder must not be a symlink.",
        ));
    }
    std::fs::create_dir_all(&path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_workspace_needs_no_repository_and_keeps_existing_files() {
        let home = tempfile::tempdir().expect("home");
        let path = prepare(home.path()).expect("prepare");
        std::fs::write(path.join("notes.txt"), "saved notes").expect("notes");
        assert_eq!(prepare(home.path()).expect("again"), path);
        assert_eq!(
            std::fs::read_to_string(path.join("notes.txt")).unwrap(),
            "saved notes"
        );
        assert!(!path.join(".git").exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o700
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn general_workspace_refuses_a_symlink_to_another_folder() {
        let home = tempfile::tempdir().expect("home");
        let other = tempfile::tempdir().expect("other");
        std::os::unix::fs::symlink(other.path(), workspace(home.path())).expect("symlink");
        assert!(prepare(home.path()).is_err());
    }
}
