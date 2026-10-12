use std::io::Write as _;
use std::path::Path;
use std::process::{Command, Stdio};

use flint_agent::AgentKind;
use flint_app::worktrees;

use super::*;

fn git(cwd: &Path, args: &[&str]) -> String {
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
    String::from_utf8(output.stdout).unwrap().trim().into()
}

fn repo(root: &Path) -> std::path::PathBuf {
    let cwd = root.join("repo");
    std::fs::create_dir_all(&cwd).unwrap();
    git(&cwd, &["init", "--quiet", "--template="]);
    git(&cwd, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    std::fs::write(cwd.join("tracked.txt"), "original\n").unwrap();
    git(&cwd, &["add", "."]);
    let tree = git(&cwd, &["write-tree"]);
    let object = format!(
        "tree {tree}\nauthor Fixture <fixture@example.invalid> 1 +0000\ncommitter Fixture <fixture@example.invalid> 1 +0000\n\nfixture\n"
    );
    let mut child = Command::new("git")
        .current_dir(&cwd)
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
    git(
        &cwd,
        &[
            "update-ref",
            "refs/heads/main",
            String::from_utf8(output.stdout).unwrap().trim(),
        ],
    );
    cwd
}

fn show_picker(ui: &Ui, cx: &mut TestAppContext) {
    ui.with(cx, |window, cx| {
        window.dispatch_action(Box::new(flint_app::app::OpenWorktrees), cx);
    });
    wait_for(ui, cx, |app| {
        app.worktree_form.as_ref().is_some_and(|form| !form.busy)
    });
    assert!(has(ui, cx, "worktree-picker"));
}

#[gpui_kit::test]
fn worktree_picker_non_repository_error_and_escape_preserve_the_draft(cx: &mut TestAppContext) {
    let ui = open(cx);
    ui.input(cx, "original draft");
    let count = ui.read(cx, |app, _| app.sessions.len());
    ui.click(cx, "attach");
    let ix = ui.read(cx, |app, _| {
        app.project_items()
            .iter()
            .position(|item| matches!(item, flint_app::project_menu::ProjectItem::Worktrees))
            .unwrap()
    });
    ui.click(cx, ("project-item", ix));
    wait_for(&ui, cx, |app| {
        app.worktree_form.as_ref().is_some_and(|form| !form.busy)
    });
    assert!(ui.read(cx, |app, _| {
        app.worktree_form.as_ref().unwrap().error.is_some()
    }));
    ui.press(cx, "cmd-n");
    assert_eq!(ui.read(cx, |app, _| app.sessions.len()), count);
    ui.press(cx, "escape");
    assert!(!has(&ui, cx, "worktree-picker"));
    assert_eq!(
        ui.read(cx, |app, cx| app.composer.read(cx).value().to_string()),
        "original draft"
    );
}

#[gpui_kit::test]
fn worktree_creation_inherits_agent_and_preserves_source_session_and_draft(
    cx: &mut TestAppContext,
) {
    let tmp = tempfile::tempdir().unwrap();
    let cwd = repo(tmp.path());
    let ui = open_with(
        cx,
        Options {
            workspace: Some(cwd.clone()),
            ..test_options()
        },
    );
    ui.app.update(cx, |app, _| {
        app.sessions[app.active].agent = AgentKind::Droid
    });
    ui.input(cx, "unsent source draft");
    let original_uid = ui.read(cx, |app, _| app.session().uid);
    std::fs::write(cwd.join("tracked.txt"), "unsaved source edit\n").unwrap();
    show_picker(&ui, cx);
    ui.click(cx, "worktree-branch");
    ui.input(cx, "replace-this");
    ui.press(cx, "cmd-a");
    ui.input(cx, "feature/isolated");
    ui.click(cx, "worktree-create");
    wait_for(&ui, cx, |app| {
        app.worktree_form.is_none() || app.worktree_form.as_ref().is_some_and(|form| !form.busy)
    });
    assert_eq!(
        ui.read(cx, |app, _| app
            .worktree_form
            .as_ref()
            .and_then(|form| form.error.clone())),
        None
    );
    let (path, agent, count, new_uid) = ui.read(cx, |app, _| {
        (
            app.session().workspace.clone(),
            app.session().agent,
            app.sessions.len(),
            app.session().uid,
        )
    });
    assert_eq!(agent, AgentKind::Droid);
    assert_eq!(count, 2);
    assert_ne!(new_uid, original_uid);
    assert!(path.join(".git").is_file());
    assert_eq!(
        std::fs::read_to_string(path.join("tracked.txt")).unwrap(),
        "original\n"
    );
    assert_eq!(
        std::fs::read_to_string(cwd.join("tracked.txt")).unwrap(),
        "unsaved source edit\n"
    );
    assert_eq!(
        ui.read(cx, |app, cx| app.composer.read(cx).value().to_string()),
        ""
    );
    ui.with(cx, |window, cx| {
        ui.app
            .update(cx, |app, cx| app.select_session(0, window, cx))
    });
    assert_eq!(
        ui.read(cx, |app, cx| app.composer.read(cx).value().to_string()),
        "unsent source draft"
    );
    assert_eq!(ui.read(cx, |app, _| app.sessions[0].workspace.clone()), cwd);
}

#[gpui_kit::test]
fn worktree_open_reuses_checkout_sessions_and_keeps_separate_drafts(cx: &mut TestAppContext) {
    let tmp = tempfile::tempdir().unwrap();
    let cwd = repo(tmp.path());
    let options = Options {
        workspace: Some(cwd.clone()),
        ..test_options()
    };
    let tree = worktrees::create(&cwd, options.home.as_ref().unwrap(), "existing", "HEAD").unwrap();
    let ui = open_with(cx, options);
    ui.input(cx, "main draft");
    show_picker(&ui, cx);
    ui.click(cx, ("worktree-open", 1usize));
    assert_eq!(
        ui.read(cx, |app, _| app.session().workspace.clone()),
        tree.path
    );
    ui.input(cx, "linked draft");
    show_picker(&ui, cx);
    ui.click(cx, ("worktree-open", 0usize));
    assert_eq!(
        ui.read(cx, |app, cx| app.composer.read(cx).value().to_string()),
        "main draft"
    );
    show_picker(&ui, cx);
    ui.click(cx, ("worktree-open", 1usize));
    assert_eq!(ui.read(cx, |app, _| app.sessions.len()), 2);
    assert_eq!(
        ui.read(cx, |app, cx| app.composer.read(cx).value().to_string()),
        "linked draft"
    );
    let group = ui.read(cx, |app, _| {
        flint_app::project::identity(&app.session().workspace).root
    });
    assert_eq!(group, cwd.canonicalize().unwrap());
}

#[gpui_kit::test]
fn worktree_removal_requires_confirmation_and_preserves_dirty_checkout(cx: &mut TestAppContext) {
    let tmp = tempfile::tempdir().unwrap();
    let cwd = repo(tmp.path());
    let options = Options {
        workspace: Some(cwd.clone()),
        ..test_options()
    };
    let tree = worktrees::create(&cwd, options.home.as_ref().unwrap(), "unused", "HEAD").unwrap();
    let ui = open_with(cx, options);
    show_picker(&ui, cx);
    ui.click(cx, ("worktree-remove", 1usize));
    assert!(tree.path.exists());
    assert!(has(&ui, cx, "worktree-remove-confirm"));
    ui.click(cx, "worktree-remove-cancel");
    assert!(tree.path.exists());
    std::fs::write(tree.path.join("untracked"), "keep me").unwrap();
    ui.click(cx, ("worktree-remove", 1usize));
    ui.click(cx, "worktree-remove-confirm");
    wait_for(&ui, cx, |app| {
        app.worktree_form.as_ref().is_some_and(|form| !form.busy)
    });
    assert!(ui.read(cx, |app, _| {
        app.worktree_form.as_ref().unwrap().error.is_some()
    }));
    assert_eq!(
        std::fs::read_to_string(tree.path.join("untracked")).unwrap(),
        "keep me"
    );
    std::fs::remove_file(tree.path.join("untracked")).unwrap();
    ui.click(cx, "worktree-remove-confirm");
    wait_for(&ui, cx, |app| {
        app.worktree_form.as_ref().is_some_and(|form| !form.busy)
    });
    assert!(!tree.path.exists());
    assert_eq!(
        ui.read(cx, |app, _| app.worktree_form.as_ref().unwrap().trees.len()),
        1
    );
    assert!(!git(&cwd, &["show-ref", "--verify", "refs/heads/unused"]).is_empty());
}

#[gpui_kit::test]
fn worktree_removal_rechecks_sessions_created_after_confirmation(cx: &mut TestAppContext) {
    let tmp = tempfile::tempdir().unwrap();
    let cwd = repo(tmp.path());
    let options = Options {
        workspace: Some(cwd.clone()),
        ..test_options()
    };
    let tree = worktrees::create(&cwd, options.home.as_ref().unwrap(), "in-use", "HEAD").unwrap();
    let ui = open_with(cx, options);
    show_picker(&ui, cx);
    ui.click(cx, ("worktree-remove", 1usize));
    std::fs::create_dir(tree.path.join("nested")).unwrap();
    ui.app.update(cx, |app, _| {
        app.sessions.push(flint_app::session::Session::new(
            9_999,
            tree.path.join("nested"),
        ))
    });
    ui.click(cx, "worktree-remove-confirm");
    assert!(tree.path.exists());
    assert!(ui.read(cx, |app, _| {
        app.worktree_form.as_ref().unwrap().remove_confirm.is_none()
    }));
    assert!(ui.read(cx, |app, _| {
        app.worktree_form
            .as_ref()
            .unwrap()
            .error
            .as_ref()
            .unwrap()
            .contains("session")
    }));
}

#[gpui_kit::test]
fn worktree_picker_fits_a_short_window_and_does_not_leak_clicks(cx: &mut TestAppContext) {
    let tmp = tempfile::tempdir().unwrap();
    let cwd = repo(tmp.path());
    for ix in 0..40 {
        git(&cwd, &["branch", &format!("feature/branch-{ix:02}")]);
    }
    let ui = open_with(
        cx,
        Options {
            workspace: Some(cwd),
            window_size: Some((900., 560.)),
            ..test_options()
        },
    );
    show_picker(&ui, cx);
    ui.with(cx, |window, cx| {
        window.render_frame(cx);
        for id in [
            "worktree-picker",
            "worktree-create",
            "worktree-close",
            "branch-fetch",
            "branch-search",
        ] {
            let bounds = window.find(ElementId::Name(id.into())).bounds();
            assert!(
                bounds.left() >= px(0.) && bounds.right() <= px(900.),
                "{id}: {bounds:?}"
            );
            assert!(
                bounds.top() >= px(0.) && bounds.bottom() <= px(560.),
                "{id}: {bounds:?}"
            );
        }
    });
    ui.click(cx, "worktree-picker");
    assert!(!ui.read(cx, |app, _| app.session().view.running));
    assert!(has(&ui, cx, "worktree-picker"));
    ui.press(cx, "escape");
    ui.input(cx, "back in the composer");
    assert_eq!(
        ui.read(cx, |app, cx| app.composer.read(cx).value().to_string()),
        "back in the composer"
    );
}

fn branch_index(ui: &Ui, cx: &mut TestAppContext, name: &str) -> usize {
    ui.read(cx, |app, _| {
        app.worktree_form
            .as_ref()
            .unwrap()
            .branches
            .iter()
            .position(|branch| branch.name == name)
            .unwrap()
    })
}

#[gpui_kit::test]
fn branch_header_opens_picker_and_current_branch_preserves_subfolder_draft(
    cx: &mut TestAppContext,
) {
    for (window_size, control) in [
        ((1440., 900.), "branch-select"),
        ((600., 560.), "header-branches"),
    ] {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = repo(tmp.path());
        let nested = cwd.join("nested");
        std::fs::create_dir(&nested).unwrap();
        let ui = open_with(
            cx,
            Options {
                workspace: Some(nested.clone()),
                window_size: Some(window_size),
                ..test_options()
            },
        );
        ui.input(cx, "keep this subfolder draft");
        let uid = ui.read(cx, |app, _| app.session().uid);
        ui.click(cx, control);
        wait_for(&ui, cx, |app| {
            app.worktree_form.as_ref().is_some_and(|form| !form.busy)
        });
        ui.click(cx, "branch-search");
        ui.input(cx, "MAIN");
        ui.press(cx, "enter");
        wait_for(&ui, cx, |app| app.worktree_form.is_none());
        ui.read(cx, |app, cx| {
            assert_eq!(app.session().uid, uid);
            assert_eq!(app.sessions.len(), 1);
            assert_eq!(app.session().workspace, nested);
            assert_eq!(
                app.composer.read(cx).value().as_ref(),
                "keep this subfolder draft"
            );
        });
    }
}

#[gpui_kit::test]
fn local_branch_selection_keeps_the_running_source_session_and_its_draft(cx: &mut TestAppContext) {
    let tmp = tempfile::tempdir().unwrap();
    let cwd = repo(tmp.path());
    git(&cwd, &["branch", "feature/api"]);
    let ui = open_with(
        cx,
        Options {
            workspace: Some(cwd.clone()),
            ..test_options()
        },
    );
    let engine = ui.engine(cx);
    ui.input(cx, "source request");
    ui.press(cx, "enter");
    engine.sent();
    engine.send(cx, AgentEvent::TurnStarted { turn_id: 1 });
    ui.input(cx, "source draft");
    let source_uid = ui.read(cx, |app, _| app.session().uid);
    std::fs::write(cwd.join("tracked.txt"), "source changes\n").unwrap();
    show_picker(&ui, cx);
    let ix = branch_index(&ui, cx, "feature/api");
    ui.click(cx, ("branch-open", ix));
    wait_for(&ui, cx, |app| app.worktree_form.is_none());
    ui.read(cx, |app, _| {
        assert_ne!(app.session().uid, source_uid);
        assert_eq!(app.branch().as_deref(), Some("feature/api"));
        assert!(app.sessions[0].view.running);
        assert!(app.sessions[0].ops.is_some());
        assert_eq!(app.sessions[0].workspace, cwd);
    });
    assert_eq!(
        std::fs::read_to_string(cwd.join("tracked.txt")).unwrap(),
        "source changes\n"
    );
    assert_eq!(git(&cwd, &["branch", "--show-current"]), "main");
    show_picker(&ui, cx);
    let ix = branch_index(&ui, cx, "main");
    ui.click(cx, ("branch-open", ix));
    wait_for(&ui, cx, |app| app.worktree_form.is_none());
    assert_eq!(ui.read(cx, |app, _| app.sessions.len()), 2);
    assert_eq!(
        ui.read(cx, |app, cx| app.composer.read(cx).value().to_string()),
        "source draft"
    );
}

#[gpui_kit::test]
fn remote_branch_fetch_search_and_tracking_selection_use_configured_git_remote(
    cx: &mut TestAppContext,
) {
    let tmp = tempfile::tempdir().unwrap();
    let cwd = repo(&tmp.path().join("local"));
    let upstream = repo(&tmp.path().join("upstream"));
    git(&upstream, &["branch", "feature/github"]);
    git(
        &cwd,
        &["remote", "add", "origin", upstream.to_str().unwrap()],
    );
    let ui = open_with(
        cx,
        Options {
            workspace: Some(cwd.clone()),
            ..test_options()
        },
    );
    show_picker(&ui, cx);
    assert!(ui.read(cx, |app, _| {
        app.worktree_form
            .as_ref()
            .unwrap()
            .branches
            .iter()
            .all(|branch| branch.remote.is_none())
    }));
    ui.click(cx, "branch-fetch");
    wait_for(&ui, cx, |app| {
        app.worktree_form.as_ref().is_some_and(|form| !form.busy)
    });
    assert_eq!(
        ui.read(cx, |app, _| app
            .worktree_form
            .as_ref()
            .unwrap()
            .status
            .clone()),
        Some("Remote branches updated.".into())
    );
    ui.click(cx, "branch-search");
    ui.input(cx, "ORIGIN/FEATURE/GITHUB");
    ui.read(cx, |app, cx| {
        let form = app.worktree_form.as_ref().unwrap();
        let indices = form.branch_indices(cx);
        assert_eq!(indices.len(), 1);
        assert_eq!(form.branches[indices[0]].name, "origin/feature/github");
    });
    ui.press(cx, "enter");
    wait_for(&ui, cx, |app| app.worktree_form.is_none());
    let checkout = ui.read(cx, |app, _| app.session().workspace.clone());
    assert_eq!(
        git(&checkout, &["branch", "--show-current"]),
        "feature/github"
    );
    assert_eq!(
        git(
            &checkout,
            &["rev-parse", "--symbolic-full-name", "@{upstream}"]
        ),
        "refs/remotes/origin/feature/github"
    );
    assert_eq!(git(&cwd, &["branch", "--show-current"]), "main");
}

#[gpui_kit::test]
fn branch_conflicts_and_empty_search_keep_the_original_session(cx: &mut TestAppContext) {
    let tmp = tempfile::tempdir().unwrap();
    let cwd = repo(tmp.path());
    git(&cwd, &["branch", "feature/conflict"]);
    git(&cwd, &["remote", "add", "origin", cwd.to_str().unwrap()]);
    let commit = git(&cwd, &["rev-parse", "HEAD"]);
    git(
        &cwd,
        &[
            "update-ref",
            "refs/remotes/origin/feature/conflict",
            &commit,
        ],
    );
    let ui = open_with(
        cx,
        Options {
            workspace: Some(cwd.clone()),
            ..test_options()
        },
    );
    ui.input(cx, "keep source");
    show_picker(&ui, cx);
    ui.click(cx, "branch-search");
    ui.input(cx, "not-a-branch");
    ui.press(cx, "enter");
    assert!(has(&ui, cx, "worktree-picker"));
    assert_eq!(ui.read(cx, |app, _| app.sessions.len()), 1);
    ui.press(cx, "cmd-a");
    ui.input(cx, "origin/feature/conflict");
    ui.press(cx, "enter");
    wait_for(&ui, cx, |app| {
        app.worktree_form.as_ref().is_some_and(|form| !form.busy)
    });
    ui.read(cx, |app, _| {
        assert!(
            app.worktree_form
                .as_ref()
                .unwrap()
                .error
                .as_ref()
                .unwrap()
                .contains("already exists")
        );
        assert_eq!(app.session().workspace, cwd);
        assert_eq!(app.sessions.len(), 1);
    });
    ui.press(cx, "escape");
    assert_eq!(
        ui.read(cx, |app, cx| app.composer.read(cx).value().to_string()),
        "keep source"
    );
}

#[gpui_kit::test]
fn branch_refresh_reads_external_changes_without_fetching(cx: &mut TestAppContext) {
    let tmp = tempfile::tempdir().unwrap();
    let cwd = repo(tmp.path());
    let ui = open_with(
        cx,
        Options {
            workspace: Some(cwd.clone()),
            ..test_options()
        },
    );
    show_picker(&ui, cx);
    git(&cwd, &["branch", "created-externally"]);
    ui.click(cx, "branch-refresh");
    wait_for(&ui, cx, |app| {
        app.worktree_form.as_ref().is_some_and(|form| !form.busy)
    });
    branch_index(&ui, cx, "created-externally");
    ui.click(cx, "branch-fetch");
    ui.read(cx, |app, _| {
        let form = app.worktree_form.as_ref().unwrap();
        assert!(form.remotes.is_empty());
        assert!(!form.busy);
        assert!(form.error.is_none());
    });
}
