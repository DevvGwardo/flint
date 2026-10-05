use super::{normalize, read_identity};
use std::fs;
use std::path::Path;

/// A repo at `<tmp>/repo` with a linked worktree at `<tmp>/repo-feature`,
/// laid out the way `git worktree add` writes them.
fn repo_with_worktree(tmp: &Path) -> (std::path::PathBuf, std::path::PathBuf) {
    let repo = tmp.join("repo");
    let linked = tmp.join("repo-feature");
    let admin = repo.join(".git/worktrees/repo-feature");
    fs::create_dir_all(&admin).unwrap();
    fs::create_dir_all(&linked).unwrap();
    fs::write(admin.join("commondir"), "../..\n").unwrap();
    fs::write(
        linked.join(".git"),
        format!("gitdir: {}\n", admin.display()),
    )
    .unwrap();
    (repo, linked)
}

#[test]
fn outside_git_the_workspace_is_its_own_project() {
    let tmp = tempfile::tempdir().unwrap();
    let id = read_identity(tmp.path());
    assert_eq!(id.root, tmp.path());
    assert_eq!(id.worktree, None);
    assert_eq!(
        id.label(),
        tmp.path().file_name().unwrap().to_string_lossy()
    );
}

#[test]
fn a_linked_worktree_groups_under_the_primary_checkout() {
    let tmp = tempfile::tempdir().unwrap();
    let (repo, linked) = repo_with_worktree(tmp.path());
    let main = read_identity(&repo);
    let feature = read_identity(&linked);
    assert_eq!(main.root, repo);
    assert_eq!(feature.root, repo);
    assert_eq!(main.key(), feature.key());
    assert_eq!(main.worktree, None);
    assert_eq!(feature.worktree.as_deref(), Some("repo-feature"));
    assert_eq!(main.label(), "repo");
    assert_eq!(feature.label(), "repo › repo-feature");
}

#[test]
fn a_relative_gitdir_pointer_resolves_against_the_worktree() {
    let tmp = tempfile::tempdir().unwrap();
    let (repo, linked) = repo_with_worktree(tmp.path());
    fs::write(
        linked.join(".git"),
        "gitdir: ../repo/.git/worktrees/repo-feature\n",
    )
    .unwrap();
    assert_eq!(read_identity(&linked).root, repo);
}

#[test]
fn subfolders_stay_distinct_within_a_repository() {
    let tmp = tempfile::tempdir().unwrap();
    let (repo, linked) = repo_with_worktree(tmp.path());
    fs::create_dir_all(repo.join("crates/app")).unwrap();
    fs::create_dir_all(linked.join("crates/app")).unwrap();
    let main = read_identity(&repo.join("crates/app"));
    let feature = read_identity(&linked.join("crates/app"));
    assert_eq!(main.key(), feature.key());
    assert_ne!(main.key(), read_identity(&repo).key());
    assert_eq!(feature.label(), "repo › repo-feature/crates/app");
}

#[test]
fn a_submodule_is_its_own_project() {
    let tmp = tempfile::tempdir().unwrap();
    let sub = tmp.path().join("super/vendor/lib");
    let modules = tmp.path().join("super/.git/modules/lib");
    fs::create_dir_all(&sub).unwrap();
    fs::create_dir_all(&modules).unwrap();
    fs::write(sub.join(".git"), format!("gitdir: {}\n", modules.display())).unwrap();
    let id = read_identity(&sub);
    assert_eq!(id.root, sub);
    assert_eq!(id.worktree, None);
}

#[test]
fn normalize_resolves_dot_dot() {
    assert_eq!(
        normalize(Path::new("/a/b/c/../../d/./e")),
        Path::new("/a/d/e")
    );
}

#[test]
fn branch_is_read_inside_a_linked_worktree_and_from_subfolders() {
    let tmp = tempfile::tempdir().unwrap();
    let (repo, linked) = repo_with_worktree(tmp.path());
    fs::write(repo.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
    fs::write(
        repo.join(".git/worktrees/repo-feature/HEAD"),
        "ref: refs/heads/feature/x\n",
    )
    .unwrap();
    fs::create_dir_all(repo.join("crates/app")).unwrap();
    assert_eq!(super::read_branch(&repo).as_deref(), Some("main"));
    assert_eq!(
        super::read_branch(&repo.join("crates/app")).as_deref(),
        Some("main")
    );
    assert_eq!(super::read_branch(&linked).as_deref(), Some("feature/x"));
    fs::write(repo.join(".git/HEAD"), "0123456789abcdef\n").unwrap();
    assert_eq!(super::read_branch(&repo).as_deref(), Some("0123456"));
    assert_eq!(super::read_branch(tmp.path()), None);
}
