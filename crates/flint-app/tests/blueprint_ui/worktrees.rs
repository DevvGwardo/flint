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
        for id in ["worktree-picker", "worktree-create", "worktree-close"] {
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
