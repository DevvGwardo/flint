//! Session-local worktree picker. Git runs off-thread; the source checkout,
//! agent and dialog identity stay fixed while an operation is in flight.

use std::path::{Path, PathBuf};
use std::time::Duration;

use flint_agent::AgentKind;
use gpui_kit::base::{Disableable as _, FocusTrapElement as _};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::app::FlintApp;
use crate::theme::{palette, size};
use crate::{ui, worktrees};

pub struct WorktreeForm {
    pub branch: Entity<InputState>,
    pub base: Entity<InputState>,
    pub trees: Vec<worktrees::Worktree>,
    pub busy: bool,
    pub error: Option<String>,
    pub remove_confirm: Option<PathBuf>,
    cwd: PathBuf,
    agent: AgentKind,
    focus: FocusHandle,
    return_focus: Option<FocusHandle>,
    _subscriptions: Vec<Subscription>,
    task: Option<Task<()>>,
}

enum Job {
    List,
    Create { branch: String, base: String },
    Remove(PathBuf),
}

enum Outcome {
    Listed(Vec<worktrees::Worktree>),
    Created(worktrees::Worktree),
    Removed(PathBuf),
}

impl FlintApp {
    pub fn open_worktrees(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.worktree_form.is_some()
            || self.settings_form.is_some()
            || self.permission_choice_open
            || self.archive_confirm.is_some()
        {
            return;
        }
        self.close_palette(window, cx);
        self.discard_rename();
        self.mention = None;
        self.slash = None;
        self.option_menu = None;
        self.project_menu = None;
        self.agent_menu = false;
        self.session_menu = None;
        let branch = cx.new(|cx| InputState::new(window, cx).placeholder("feature/my-task"));
        let base = cx.new(|cx| InputState::new(window, cx).default_value("HEAD"));
        let subscriptions = [&branch, &base]
            .into_iter()
            .map(|input| {
                cx.subscribe_in(input, window, |app, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::PressEnter { .. }) {
                        app.create_worktree(window, cx);
                    }
                })
            })
            .collect();
        let focus = cx.focus_handle();
        let return_focus = window.focused(cx);
        self.worktree_form = Some(WorktreeForm {
            branch: branch.clone(),
            base,
            trees: Vec::new(),
            busy: false,
            error: None,
            remove_confirm: None,
            cwd: self.session().workspace.clone(),
            agent: self.session().agent,
            focus,
            return_focus,
            _subscriptions: subscriptions,
            task: None,
        });
        branch.update(cx, |input, cx| input.focus(window, cx));
        self.worktree_job(Job::List, window, cx);
    }

    pub fn close_worktrees(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.worktree_form.as_ref().is_some_and(|form| form.busy) {
            return;
        }
        if let Some(form) = self.worktree_form.take() {
            if let Some(focus) = form.return_focus {
                focus.focus(window, cx);
            } else {
                self.composer
                    .update(cx, |input, cx| input.focus(window, cx));
            }
        }
        cx.notify();
    }

    pub fn create_worktree(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = &self.worktree_form else {
            return;
        };
        if form.busy || form.remove_confirm.is_some() {
            return;
        }
        let branch = form.branch.read(cx).value().trim().to_string();
        let base = form.base.read(cx).value().trim().to_string();
        self.worktree_job(Job::Create { branch, base }, window, cx);
    }

    pub fn open_worktree(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = &self.worktree_form else {
            return;
        };
        if form.busy {
            return;
        }
        let Some(tree) = form.trees.get(ix) else {
            return;
        };
        if tree.bare || tree.prunable || !tree.path.is_dir() {
            return;
        }
        let path = tree.path.clone();
        let agent = form.agent;
        self.worktree_form = None;
        self.activate_worktree(path, agent, window, cx);
    }

    fn activate_worktree(
        &mut self,
        path: PathBuf,
        agent: AgentKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ix = self
            .sessions
            .iter()
            .position(|session| worktrees::same_path(&session.workspace, &path))
            .unwrap_or_else(|| {
                let mut session = self.new_session_value(path.clone());
                session.agent = agent;
                self.sessions.push(session);
                self.sessions.len() - 1
            });
        self.workspace = path;
        self.select_session(ix, window, cx);
        self.start_agent_early(ix, cx);
    }

    pub fn worktree_in_use(&self, path: &Path, cx: &App) -> bool {
        self.sessions
            .iter()
            .any(|session| worktrees::contains_workspace(path, &session.workspace))
            || self
                .archives
                .iter()
                .any(|(_, meta)| worktrees::contains_workspace(path, &meta.workspace))
            || self
                .archived_session
                .as_ref()
                .is_some_and(|(session, _)| worktrees::contains_workspace(path, &session.workspace))
            || self
                .terminal
                .tabs
                .iter()
                .any(|view| worktrees::contains_workspace(path, &view.read(cx).cwd))
    }

    pub fn request_remove_worktree(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(form) = &self.worktree_form else {
            return;
        };
        if form.busy {
            return;
        }
        let Some(tree) = form.trees.get(ix) else {
            return;
        };
        if tree.primary || tree.bare || tree.locked || tree.prunable {
            return;
        }
        let path = tree.path.clone();
        let in_use = self.worktree_in_use(&path, cx);
        if let Some(form) = &mut self.worktree_form {
            if in_use {
                form.error =
                    Some("A session, archive, or terminal still uses this checkout.".into());
            } else {
                form.remove_confirm = Some(path);
                form.error = None;
            }
        }
        cx.notify();
    }

    pub fn confirm_remove_worktree(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = &self.worktree_form else {
            return;
        };
        if form.busy {
            return;
        }
        let Some(path) = form.remove_confirm.clone() else {
            return;
        };
        // Recheck after the confirmation, not only when the list was rendered.
        if self.worktree_in_use(&path, cx) {
            if let Some(form) = &mut self.worktree_form {
                form.error =
                    Some("A session, archive, or terminal still uses this checkout.".into());
                form.remove_confirm = None;
            }
            cx.notify();
            return;
        }
        self.worktree_job(Job::Remove(path), window, cx);
    }

    fn worktree_job(&mut self, job: Job, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = &mut self.worktree_form else {
            return;
        };
        if form.busy {
            return;
        }
        form.busy = true;
        form.error = None;
        let id = form.branch.entity_id();
        let cwd = form.cwd.clone();
        let home = self.home.clone();
        let (tx, rx) = async_channel::bounded(1);
        let thread = std::thread::Builder::new()
            .name("flint-worktrees".into())
            .spawn(move || {
                let result = match job {
                    Job::List => worktrees::list(&cwd).map(Outcome::Listed),
                    Job::Create { branch, base } => {
                        worktrees::create(&cwd, &home, &branch, &base).map(Outcome::Created)
                    }
                    Job::Remove(path) => {
                        worktrees::remove(&cwd, &path).map(|()| Outcome::Removed(path))
                    }
                }
                .map_err(|error| error.to_string());
                tx.send_blocking(result).ok();
            });
        if let Err(error) = thread {
            form.busy = false;
            form.error = Some(format!("Couldn't start worktree operation: {error}"));
            cx.notify();
            return;
        }
        form.task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = loop {
                match rx.try_recv() {
                    Ok(result) => break result,
                    Err(async_channel::TryRecvError::Closed) => {
                        break Err("Worktree worker stopped unexpectedly.".into());
                    }
                    Err(async_channel::TryRecvError::Empty) => {
                        cx.background_executor()
                            .timer(Duration::from_millis(20))
                            .await
                    }
                }
            };
            this.update_in(cx, |app, window, cx| {
                let Some(form) = &mut app.worktree_form else {
                    return;
                };
                if form.branch.entity_id() != id {
                    return;
                }
                form.busy = false;
                form.task = None;
                match result {
                    Ok(Outcome::Listed(trees)) => form.trees = trees,
                    Ok(Outcome::Removed(path)) => {
                        form.trees
                            .retain(|tree| !worktrees::same_path(&tree.path, &path));
                        form.remove_confirm = None;
                    }
                    Ok(Outcome::Created(tree)) => {
                        let agent = form.agent;
                        app.worktree_form = None;
                        app.activate_worktree(tree.path, agent, window, cx);
                    }
                    Err(error) => form.error = Some(error),
                }
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }
}

pub fn render(app: &FlintApp, window: &Window, cx: &mut Context<FlintApp>) -> Option<AnyElement> {
    let form = app.worktree_form.as_ref()?;
    let p = palette();
    let rows = form.trees.iter().enumerate().map(|(ix, tree)| {
        let unavailable = tree.bare || tree.prunable || !tree.path.is_dir();
        let label = tree.branch.clone().unwrap_or_else(|| {
            if tree.bare {
                "Bare repository".into()
            } else {
                "Detached HEAD".into()
            }
        });
        let suffix = if tree.primary {
            " · primary"
        } else if tree.locked {
            " · locked"
        } else if tree.prunable {
            " · missing"
        } else {
            ""
        };
        div()
            .id(("worktree-row", ix))
            .flex()
            .items_center()
            .gap(px(8.))
            .py(px(6.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(ui::label(format!("{label}{suffix}"), size::SM, p.text))
                    .child(div().text_ellipsis().child(ui::label(
                        tree.path.display().to_string(),
                        size::XS,
                        p.text_muted,
                    ))),
            )
            .child(
                Button::new(("worktree-open", ix))
                    .label("Open")
                    .disabled(form.busy || unavailable)
                    .on_click(
                        cx.listener(move |app, _, window, cx| app.open_worktree(ix, window, cx)),
                    ),
            )
            .child(
                Button::new(("worktree-remove", ix))
                    .label("Remove")
                    .disabled(
                        form.busy
                            || tree.primary
                            || tree.locked
                            || unavailable
                            || app.worktree_in_use(&tree.path, cx),
                    )
                    .on_click(
                        cx.listener(move |app, _, _, cx| app.request_remove_worktree(ix, cx)),
                    ),
            )
    });
    let sheet = div().id("worktree-picker").w(px(620.))
        .max_h(window.viewport_size().height - px(40.)).p(px(20.)).rounded(px(14.))
        .border_1().border_color(p.border_strong).bg(p.surface).flex().flex_col().gap(px(12.))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(|_, _, cx| cx.stop_propagation())
        .child(ui::label("Git worktrees", size::PROSE, p.text))
        .child(ui::label("Open an isolated checkout, or create a new branch from a commit.", size::SM, p.text_muted))
        .child(
            div().id("worktree-list").max_h((window.viewport_size().height - px(350.)).max(px(80.)))
                .overflow_y_scroll().children(rows).test_support(),
        )
        .child(ui::label("New branch", size::SM, p.text))
        .child(Input::new(&form.branch).id("worktree-branch").aria_label("New worktree branch").disabled(form.busy))
        .child(ui::label("Start from branch or commit", size::SM, p.text))
        .child(Input::new(&form.base).id("worktree-base").aria_label("Worktree base branch or commit").disabled(form.busy))
        .children(form.error.as_ref().map(|error| ui::label(error.clone(), size::SM, p.danger)))
        .when_some(form.remove_confirm.clone(), |sheet, path| {
            sheet.child(ui::label(format!("Remove {}? Only a clean checkout can be removed. Its branch and commits are kept.", path.display()), size::SM, p.text))
                .child(
                    div().flex().gap(px(8.))
                        .child(Button::new("worktree-remove-cancel").label("Keep checkout").disabled(form.busy).on_click(cx.listener(|app, _, _, cx| {
                            if let Some(form) = &mut app.worktree_form { form.remove_confirm = None; }
                            cx.notify();
                        })))
                        .child(Button::new("worktree-remove-confirm").label("Remove checkout").disabled(form.busy).on_click(cx.listener(|app, _, window, cx| app.confirm_remove_worktree(window, cx)))),
                )
        })
        .child(
            div().flex().justify_end().gap(px(8.))
                .when(form.busy, |row| row.child(ui::label("Working…", size::SM, p.text_muted)))
                .child(Button::new("worktree-close").label("Close").disabled(form.busy).on_click(cx.listener(|app, _, window, cx| app.close_worktrees(window, cx))))
                .child(Button::new("worktree-create").primary().label("Create worktree").disabled(form.busy || form.remove_confirm.is_some()).on_click(cx.listener(|app, _, window, cx| app.create_worktree(window, cx)))),
        )
        .focus_trap("worktree-picker", &form.focus).test_support();
    Some(
        deferred(
            div()
                .absolute()
                .inset_0()
                .bg(hsla(0., 0., 0., 0.5))
                .flex()
                .items_center()
                .justify_center()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|app, _, window, cx| app.close_worktrees(window, cx)),
                )
                .child(sheet),
        )
        .with_priority(25)
        .into_any_element(),
    )
}
