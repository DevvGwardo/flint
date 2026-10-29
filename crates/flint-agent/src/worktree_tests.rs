use super::*;
use std::io::Write as _;

pub(crate) fn run(cwd: &Path, args: &[&str]) -> Vec<u8> {
    let output = std::process::Command::new("git")
        .current_dir(cwd)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

pub(crate) fn repository(root: &Path) -> PathBuf {
    let repo = root.join("repo with spaces");
    std::fs::create_dir_all(&repo).unwrap();
    run(&repo, &["init", "--quiet", "--template="]);
    run(&repo, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    std::fs::write(repo.join("tracked.txt"), "base\n").unwrap();
    std::fs::write(repo.join(".gitignore"), "ignored\n").unwrap();
    run(&repo, &["add", "."]);
    let tree = String::from_utf8(run(&repo, &["write-tree"])).unwrap();
    // Static commit objects, without changing the configured Git identity.
    let object = format!(
        "tree {}\nauthor Fixture <fixture@example.invalid> 1 +0000\ncommitter Fixture <fixture@example.invalid> 1 +0000\n\nfixture\n",
        tree.trim()
    );
    let mut child = std::process::Command::new("git")
        .current_dir(&repo)
        .args(["hash-object", "-t", "commit", "-w", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(object.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let id = String::from_utf8(output.stdout).unwrap();
    run(&repo, &["update-ref", "refs/heads/main", id.trim()]);
    repo
}

fn request(branch: &str) -> Request {
    Request {
        branch: branch.into(),
        base: head(),
        repository: None,
    }
}

#[tokio::test]
async fn creates_real_checkout_without_copying_or_changing_source_edits() {
    let temp = tempfile::tempdir().unwrap();
    let repo = repository(temp.path());
    std::fs::write(repo.join("tracked.txt"), "user edits\n").unwrap();
    std::fs::write(repo.join("untracked.txt"), "preserve\n").unwrap();
    let tree = prepare(&request("feature/agent"), &repo, &temp.path().join("home"))
        .await
        .unwrap();
    assert!(tree.workspace.starts_with(flint_platform::without_verbatim(
        &temp.path().join("home/worktrees").canonicalize().unwrap()
    )));
    assert_eq!(
        std::fs::read_to_string(tree.workspace.join("tracked.txt")).unwrap(),
        "base\n"
    );
    assert_eq!(
        std::fs::read_to_string(repo.join("tracked.txt")).unwrap(),
        "user edits\n"
    );
    assert_eq!(
        std::fs::read_to_string(repo.join("untracked.txt")).unwrap(),
        "preserve\n"
    );
    assert_eq!(run(&repo, &["branch", "--show-current"]), b"main\n");
    assert_eq!(
        run(&tree.workspace, &["branch", "--show-current"]),
        b"feature/agent\n"
    );
    validate(&tree).await.unwrap();
}

#[tokio::test]
async fn reuses_clean_linked_checkout_and_preserves_unoccupied_branch_tip() {
    let temp = tempfile::tempdir().unwrap();
    let repo = repository(temp.path());
    let home = temp.path().join("home");
    run(&repo, &["branch", "existing"]);
    let tree = prepare(&request("existing"), &repo, &home).await.unwrap();
    let again = prepare(&request("existing"), &repo, &home).await.unwrap();
    assert_eq!(tree.workspace, again.workspace);
    let nested = tree.workspace.join("nested");
    std::fs::create_dir(&nested).unwrap();
    assert!(
        prepare(&request("existing"), &nested, &home)
            .await
            .unwrap_err()
            .contains("current workspace")
    );
    assert_eq!(
        run(&repo, &["rev-parse", "existing"]),
        run(&repo, &["rev-parse", "main"])
    );
    assert!(
        prepare(&request("main"), &repo, &home)
            .await
            .unwrap_err()
            .contains("primary")
    );
}

#[tokio::test]
async fn rejects_dirty_ignored_locked_missing_and_changed_checkouts() {
    let temp = tempfile::tempdir().unwrap();
    let repo = repository(temp.path());
    let home = temp.path().join("home");
    let tree = prepare(&request("task"), &repo, &home).await.unwrap();
    std::fs::write(tree.workspace.join("ignored"), "private\n").unwrap();
    assert!(prepare(&request("task"), &repo, &home).await.is_err());
    // Resume permits the child's own edits, unlike starting a new task.
    validate(&tree).await.unwrap();
    run(
        &repo,
        &["worktree", "lock", tree.workspace.to_str().unwrap()],
    );
    assert!(validate(&tree).await.is_err());
    run(
        &repo,
        &["worktree", "unlock", tree.workspace.to_str().unwrap()],
    );
    run(&tree.workspace, &["switch", "-c", "different"]);
    assert!(validate(&tree).await.is_err());
}

#[tokio::test]
async fn explicit_repository_can_be_reached_through_a_symlink() {
    let temp = tempfile::tempdir().unwrap();
    let repo = repository(temp.path());
    #[cfg(unix)]
    let path = {
        let path = temp.path().join("alias");
        std::os::unix::fs::symlink(&repo, &path).unwrap();
        path
    };
    #[cfg(not(unix))]
    let path = repo.clone();
    let mut request = request("external");
    request.repository = Some(path);
    let tree = prepare(&request, temp.path(), &temp.path().join("home"))
        .await
        .unwrap();
    assert_eq!(tree.repository, repo.canonicalize().unwrap());
}

#[test]
fn validates_worktree_arguments_and_parses_unusual_paths() {
    for value in [
        serde_json::json!({"branch":"-force"}),
        serde_json::json!({"branch":"task", "repository":"relative"}),
        serde_json::json!({"branch":"task", "base":"HEAD\n"}),
        serde_json::json!({"branch":"task", "force":true}),
        serde_json::json!({"branch":3}),
    ] {
        let args = serde_json::json!({"worktree":value});
        assert!(Request::parse(args.as_object().unwrap()).is_err());
    }
    let bytes = b"worktree /primary\0branch refs/heads/main\0\0worktree /linked\nwith \"quotes\"\0branch refs/heads/task\0\0";
    assert_eq!(
        checkout_for_branch(bytes, "task").unwrap(),
        Some("/linked\nwith \"quotes\"".into())
    );
}

#[cfg(unix)]
#[tokio::test]
async fn checkout_preparation_does_not_run_repository_hooks() {
    use std::os::unix::fs::PermissionsExt as _;
    let temp = tempfile::tempdir().unwrap();
    let repo = repository(temp.path());
    let hook_dir = repo.join(".git/hooks");
    std::fs::create_dir_all(&hook_dir).unwrap();
    let hook = hook_dir.join("post-checkout");
    let marker = temp.path().join("hook-ran");
    std::fs::write(
        &hook,
        format!("#!/bin/sh\nprintf ran > '{}'\n", marker.display()),
    )
    .unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o700)).unwrap();
    prepare(&request("no-hooks"), &repo, &temp.path().join("home"))
        .await
        .unwrap();
    assert!(!marker.exists());
}
