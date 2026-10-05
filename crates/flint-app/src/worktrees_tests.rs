use std::io::Write as _;
use std::process::{Command, Stdio};

use pretty_assertions::assert_eq;

use super::*;

fn run(cwd: &Path, args: &[&str]) -> Vec<u8> {
    let output = Command::new("git")
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

/// Static commit objects avoid depending on, or changing, the user's author
/// identity. All Git writes target this disposable fixture.
fn commit(cwd: &Path) -> String {
    run(cwd, &["add", "."]);
    let tree = String::from_utf8(run(cwd, &["write-tree"])).unwrap();
    let object = format!(
        "tree {}\nauthor Fixture <fixture@example.invalid> 1 +0000\ncommitter Fixture <fixture@example.invalid> 1 +0000\n\nfixture\n",
        tree.trim()
    );
    let mut child = Command::new("git")
        .current_dir(cwd)
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
    let id = String::from_utf8(output.stdout).unwrap().trim().to_string();
    run(cwd, &["update-ref", "refs/heads/main", &id]);
    id
}

fn repository(root: &Path) -> PathBuf {
    let cwd = root.join("repo");
    std::fs::create_dir_all(&cwd).unwrap();
    run(&cwd, &["init", "--quiet", "--template="]);
    run(&cwd, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    std::fs::write(cwd.join("tracked.txt"), "base\n").unwrap();
    std::fs::write(cwd.join(".gitignore"), "ignored\n").unwrap();
    commit(&cwd);
    cwd
}

#[test]
fn worktree_porcelain_preserves_paths_and_checkout_flags() {
    let bytes = b"worktree /repo with spaces\nand \"quotes\"\\\0HEAD abc\0branch refs/heads/main\0\0worktree /linked\0HEAD def\0detached\0locked maintenance\0\0worktree /gone\0prunable missing gitdir\0\0worktree /bare\0bare\0";
    let mut primary = Worktree::new("/repo with spaces\nand \"quotes\"\\".into(), true);
    primary.branch = Some("main".into());
    let mut linked = Worktree::new("/linked".into(), false);
    linked.detached = true;
    linked.locked = true;
    let mut gone = Worktree::new("/gone".into(), false);
    gone.prunable = true;
    let mut bare = Worktree::new("/bare".into(), false);
    bare.bare = true;
    assert_eq!(
        parse_list(bytes).unwrap(),
        vec![primary, linked, gone, bare]
    );
}

#[cfg(unix)]
#[test]
fn worktree_porcelain_preserves_non_utf8_paths() {
    use std::os::unix::ffi::OsStrExt as _;
    let trees = parse_list(b"worktree /repo-\xff\0\0").unwrap();
    assert_eq!(trees[0].path.as_os_str().as_bytes(), b"/repo-\xff");
}

#[test]
fn worktree_creation_leaves_source_edits_and_uses_resolved_base() {
    let tmp = tempfile::tempdir().unwrap();
    let cwd = repository(tmp.path());
    let base = String::from_utf8(run(&cwd, &["rev-parse", "HEAD"])).unwrap();
    std::fs::write(cwd.join("tracked.txt"), "later\n").unwrap();
    commit(&cwd);
    std::fs::write(cwd.join("tracked.txt"), "unsaved original\n").unwrap();
    std::fs::write(cwd.join("untracked.txt"), "keep me").unwrap();
    let tree = create(
        &cwd,
        &tmp.path().join("home"),
        "feature/my-task",
        base.trim(),
    )
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(tree.path.join("tracked.txt")).unwrap(),
        "base\n"
    );
    assert_eq!(
        std::fs::read_to_string(cwd.join("tracked.txt")).unwrap(),
        "unsaved original\n"
    );
    assert_eq!(
        std::fs::read_to_string(cwd.join("untracked.txt")).unwrap(),
        "keep me"
    );
    assert_eq!(run(&cwd, &["branch", "--show-current"]), b"main\n");
    let trees = list(&cwd).unwrap();
    assert_eq!(trees.len(), 2);
    assert_eq!(trees[1], tree);
    assert_eq!(
        crate::project::identity(&tree.path).root,
        cwd.canonicalize().unwrap()
    );
}

#[test]
fn worktree_paths_separate_branch_slugs_and_same_named_repositories() {
    let tmp = tempfile::tempdir().unwrap();
    let cwd = repository(&tmp.path().join("first"));
    let other = repository(&tmp.path().join("second"));
    let home = tmp.path().join("home");
    let a = create(&cwd, &home, "feature/a", "HEAD").unwrap();
    let b = create(&cwd, &home, "feature-a", "HEAD").unwrap();
    let c = create(&other, &home, "feature/a", "HEAD").unwrap();
    assert_ne!(a.path, b.path);
    assert_ne!(a.path, c.path);
    std::fs::create_dir_all(a.path.join("nested")).unwrap();
    let d = create(&a.path.join("nested"), &home, "from-linked", "HEAD").unwrap();
    assert_eq!(a.path.parent(), d.path.parent());
}

#[test]
fn worktree_creation_rejects_bad_refs_and_preserves_existing_branches() {
    let tmp = tempfile::tempdir().unwrap();
    let cwd = repository(tmp.path());
    let home = tmp.path().join("home");
    for branch in ["", "-bad", "bad\nname", "bad name", "@{-1}"] {
        assert!(create(&cwd, &home, branch, "HEAD").is_err(), "{branch:?}");
    }
    for base in ["", "--orphan", "missing-ref"] {
        assert!(create(&cwd, &home, "new-branch", base).is_err(), "{base:?}");
    }
    let tree = create(&cwd, &home, "feature", "HEAD").unwrap();
    assert!(create(&cwd, &home, "feature", "HEAD").is_err());
    assert_eq!(list(&cwd).unwrap().len(), 2);
    assert_eq!(
        std::fs::read_to_string(tree.path.join("tracked.txt")).unwrap(),
        "base\n"
    );
}

#[test]
fn worktree_removal_refuses_primary_locked_dirty_untracked_and_ignored() {
    let tmp = tempfile::tempdir().unwrap();
    let cwd = repository(tmp.path());
    assert!(remove(&cwd, &cwd).is_err());
    let tree = create(&cwd, &tmp.path().join("home"), "feature", "HEAD").unwrap();
    run(&cwd, &["worktree", "lock", tree.path.to_str().unwrap()]);
    assert!(remove(&cwd, &tree.path).is_err());
    run(&cwd, &["worktree", "unlock", tree.path.to_str().unwrap()]);
    for (name, contents) in [
        ("tracked.txt", "changed\n"),
        ("untracked", "untracked"),
        ("ignored", "private local data"),
    ] {
        std::fs::write(tree.path.join(name), contents).unwrap();
        assert!(remove(&cwd, &tree.path).is_err(), "{name}");
        assert_eq!(
            std::fs::read_to_string(tree.path.join(name)).unwrap(),
            contents
        );
        if name == "tracked.txt" {
            std::fs::write(tree.path.join(name), "base\n").unwrap();
        } else {
            std::fs::remove_file(tree.path.join(name)).unwrap();
        }
    }
    remove(&cwd, &tree.path).unwrap();
    assert!(!tree.path.exists());
    assert_eq!(list(&cwd).unwrap().len(), 1);
    assert_eq!(
        run(
            &cwd,
            &["show-ref", "--verify", "--hash", "refs/heads/feature"]
        ),
        run(&cwd, &["rev-parse", "HEAD"])
    );
    assert!(remove(&cwd, &tree.path).is_err());
}

#[test]
fn worktree_lists_detached_checkouts_without_interpreting_path_newlines() {
    let tmp = tempfile::tempdir().unwrap();
    let cwd = repository(tmp.path());
    let path = tmp.path().join("linked\nwith spaces");
    run(
        &cwd,
        &[
            "worktree",
            "add",
            "--detach",
            path.to_str().unwrap(),
            "HEAD",
        ],
    );
    let trees = list(&cwd).unwrap();
    assert!(same_path(&trees[1].path, &path));
    assert!(trees[1].detached);
    assert_eq!(trees[1].branch, None);
    assert!(contains_workspace(&path, &path.join("nested")));
    assert!(!contains_workspace(&path, &tmp.path().join("linked")));
}

#[test]
fn worktree_operations_report_non_repository_errors() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(list(tmp.path()).is_err());
    assert!(create(tmp.path(), &tmp.path().join("home"), "feature", "HEAD").is_err());
    assert!(!tmp.path().join("home").exists());
}
