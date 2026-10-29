use super::*;
use crate::worktrees::tests::{commit, repository, run};

fn remote(cwd: &Path, name: &str, url: &Path) {
    run(cwd, &["remote", "add", name, url.to_str().unwrap()]);
}

#[test]
fn branch_listing_includes_local_and_remote_branches_but_not_remote_head() {
    let tmp = tempfile::tempdir().unwrap();
    let cwd = repository(tmp.path());
    remote(&cwd, "origin", &cwd);
    remote(&cwd, "company/team", &cwd);
    let id = String::from_utf8(run(&cwd, &["rev-parse", "HEAD"])).unwrap();
    for reference in [
        "refs/heads/feature/日本語",
        "refs/remotes/origin/feature/api",
        "refs/remotes/company/team/review",
    ] {
        run(&cwd, &["update-ref", reference, id.trim()]);
    }
    run(
        &cwd,
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/feature/api",
        ],
    );
    let repo = list(&cwd).unwrap();
    assert_eq!(repo.branches.len(), 4);
    let main = repo
        .branches
        .iter()
        .find(|branch| branch.name == "main")
        .unwrap();
    assert!(worktrees::same_path(main.checkout.as_ref().unwrap(), &cwd));
    let nested = repo
        .branches
        .iter()
        .find(|branch| branch.name == "company/team/review")
        .unwrap();
    assert_eq!(nested.remote.as_deref(), Some("company/team"));
    assert!(
        repo.branches
            .iter()
            .all(|branch| !branch.name.ends_with("/HEAD"))
    );
}

#[test]
fn opening_local_branch_preserves_source_edits_and_reuses_existing_checkouts() {
    let tmp = tempfile::tempdir().unwrap();
    let cwd = repository(tmp.path());
    run(&cwd, &["branch", "feature/api"]);
    // A same-named tag must not turn a branch selection into detached HEAD.
    run(&cwd, &["tag", "feature/api"]);
    std::fs::write(cwd.join("tracked.txt"), "local edits\n").unwrap();
    std::fs::write(cwd.join("untracked"), "keep\n").unwrap();
    let home = tmp.path().join("home");
    let tree = open(&cwd, &home, "refs/heads/feature/api").unwrap();
    assert_eq!(tree.branch.as_deref(), Some("feature/api"));
    assert!(!tree.detached);
    assert_eq!(open(&cwd, &home, "refs/heads/feature/api").unwrap(), tree);
    assert!(worktrees::same_path(
        &open(&tree.path, &home, "refs/heads/main").unwrap().path,
        &cwd
    ));
    assert_eq!(run(&cwd, &["branch", "--show-current"]), b"main\n");
    assert_eq!(
        std::fs::read_to_string(cwd.join("tracked.txt")).unwrap(),
        "local edits\n"
    );
    assert_eq!(
        std::fs::read_to_string(cwd.join("untracked")).unwrap(),
        "keep\n"
    );
    assert_eq!(
        std::fs::read_to_string(tree.path.join("tracked.txt")).unwrap(),
        "base\n"
    );
    assert_eq!(worktrees::list(&cwd).unwrap().len(), 2);
}

#[test]
fn fetch_and_remote_selection_create_a_tracking_branch_without_pulling() {
    let tmp = tempfile::tempdir().unwrap();
    let cwd = repository(&tmp.path().join("local"));
    let upstream = repository(&tmp.path().join("remote"));
    run(&upstream, &["branch", "feature/api"]);
    remote(&cwd, "origin", &upstream);
    assert!(
        list(&cwd)
            .unwrap()
            .branches
            .iter()
            .all(|branch| branch.remote.is_none())
    );
    std::fs::write(cwd.join("tracked.txt"), "source edit\n").unwrap();
    let repo = fetch(&cwd).unwrap();
    assert!(
        repo.branches
            .iter()
            .any(|branch| branch.name == "origin/feature/api")
    );
    let home = tmp.path().join("home");
    let tree = open(&cwd, &home, "refs/remotes/origin/feature/api").unwrap();
    assert_eq!(tree.branch.as_deref(), Some("feature/api"));
    assert_eq!(
        run(
            &tree.path,
            &["rev-parse", "--symbolic-full-name", "@{upstream}"]
        ),
        b"refs/remotes/origin/feature/api\n"
    );
    std::fs::write(tree.path.join("tracked.txt"), "feature edits\n").unwrap();
    assert_eq!(
        open(&cwd, &home, "refs/remotes/origin/feature/api").unwrap(),
        tree
    );
    assert_eq!(
        std::fs::read_to_string(tree.path.join("tracked.txt")).unwrap(),
        "feature edits\n"
    );
    assert_eq!(
        std::fs::read_to_string(cwd.join("tracked.txt")).unwrap(),
        "source edit\n"
    );
}

#[test]
fn remote_branch_conflicts_and_stale_selections_never_reset_local_branches() {
    let tmp = tempfile::tempdir().unwrap();
    let cwd = repository(tmp.path());
    remote(&cwd, "origin", &cwd);
    run(&cwd, &["branch", "feature"]);
    let id = String::from_utf8(run(&cwd, &["rev-parse", "HEAD"])).unwrap();
    run(
        &cwd,
        &["update-ref", "refs/remotes/origin/feature", id.trim()],
    );
    let home = tmp.path().join("home");
    assert!(
        open(&cwd, &home, "refs/remotes/origin/feature")
            .unwrap_err()
            .to_string()
            .contains("already exists")
    );
    for reference in ["refs/heads/missing", "--force", "main", "refs/heads/main\n"] {
        assert!(open(&cwd, &home, reference).is_err(), "{reference:?}");
    }
    assert_eq!(run(&cwd, &["rev-parse", "feature"]), id.as_bytes());
    assert_eq!(worktrees::list(&cwd).unwrap().len(), 1);
    assert!(!home.exists());
}

#[test]
fn branch_selection_rechecks_deleted_and_missing_checkouts() {
    let tmp = tempfile::tempdir().unwrap();
    let cwd = repository(tmp.path());
    let home = tmp.path().join("home");
    let tree = worktrees::create(&cwd, &home, "missing-checkout", "HEAD").unwrap();
    let moved = tmp.path().join("moved-checkout");
    std::fs::rename(&tree.path, moved).unwrap();
    assert!(open(&cwd, &home, "refs/heads/missing-checkout").is_err());
    run(&cwd, &["branch", "deleted"]);
    assert!(
        list(&cwd)
            .unwrap()
            .branches
            .iter()
            .any(|branch| branch.name == "deleted")
    );
    run(&cwd, &["branch", "-d", "deleted"]);
    assert!(open(&cwd, &home, "refs/heads/deleted").is_err());
    assert_eq!(run(&cwd, &["branch", "--show-current"]), b"main\n");
}

#[test]
fn branch_operations_report_non_repository_missing_remote_and_fetch_errors() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(list(tmp.path()).is_err());
    assert!(open(tmp.path(), &tmp.path().join("home"), "refs/heads/main").is_err());
    let cwd = repository(tmp.path());
    assert!(fetch(&cwd).unwrap_err().to_string().contains("no remotes"));
    remote(&cwd, "origin", &tmp.path().join("not-a-repository"));
    assert!(fetch(&cwd).is_err());
    assert_eq!(run(&cwd, &["branch", "--show-current"]), b"main\n");
}

#[test]
fn unknown_remote_refs_are_skipped_and_open_fails_closed() {
    let tmp = tempfile::tempdir().unwrap();
    let cwd = repository(tmp.path());
    remote(&cwd, "origin", &cwd);
    let id = String::from_utf8(run(&cwd, &["rev-parse", "HEAD"])).unwrap();
    // A ref under an unconfigured remote must be skipped, never panic.
    run(&cwd, &["update-ref", "refs/remotes/stale/fork", id.trim()]);
    run(
        &cwd,
        &["update-ref", "refs/remotes/origin/valid", id.trim()],
    );
    let repo = list(&cwd).unwrap();
    assert!(
        repo.branches
            .iter()
            .all(|branch| branch.name != "stale/fork")
    );
    assert!(
        repo.branches
            .iter()
            .any(|branch| branch.name == "origin/valid")
    );
    // Opening an unknown reference fails with an error, not a panic.
    assert!(open(&cwd, &tmp.path().join("home"), "refs/remotes/stale/fork").is_err());
}

#[test]
fn fetch_never_prunes_or_advances_local_branches_even_when_git_config_enables_pruning() {
    let tmp = tempfile::tempdir().unwrap();
    let cwd = repository(&tmp.path().join("local"));
    let upstream = repository(&tmp.path().join("upstream"));
    run(&upstream, &["branch", "feature"]);
    run(&upstream, &["branch", "deleted-later"]);
    remote(&cwd, "origin", &upstream);
    fetch(&cwd).unwrap();
    let tree = open(
        &cwd,
        &tmp.path().join("home"),
        "refs/remotes/origin/feature",
    )
    .unwrap();
    let before = run(&tree.path, &["rev-parse", "HEAD"]);
    run(&cwd, &["config", "fetch.prune", "true"]);
    run(&cwd, &["config", "remote.origin.prune", "true"]);
    run(&cwd, &["config", "fetch.pruneTags", "true"]);
    std::fs::write(upstream.join("tracked.txt"), "updated remotely\n").unwrap();
    let updated = commit(&upstream);
    run(&upstream, &["update-ref", "refs/heads/feature", &updated]);
    run(&upstream, &["branch", "-D", "deleted-later"]);
    let repo = fetch(&cwd).unwrap();
    assert!(
        repo.branches
            .iter()
            .any(|branch| branch.name == "origin/deleted-later")
    );
    assert_eq!(run(&tree.path, &["rev-parse", "HEAD"]), before);
    assert_eq!(
        run(&cwd, &["rev-parse", "refs/remotes/origin/feature"]),
        format!("{updated}\n").as_bytes()
    );
    assert_eq!(
        std::fs::read_to_string(tree.path.join("tracked.txt")).unwrap(),
        "base\n"
    );
}
