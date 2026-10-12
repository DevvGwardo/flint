//! Inspectable task scope, evidence and explicit user review. Native scope
//! controls do not pretend to govern an independent ACP loop.

use flint_agent::{
    AgentKind, EvidenceState, FileState, Op, RequirementSpec, ReviewDecision, TaskEdit, TaskMode,
    TaskOutcome, TaskRecord, TaskReview, TaskSource,
};
use gpui_kit::assets::IconName;
use gpui_kit::base::{Disableable as _, FocusTrapElement as _};
use gpui_kit::component::button::*;
use gpui_kit::component::input::{Textarea, TextareaState};
use gpui_kit::component::scroll::{Scrollbar, ScrollbarMode};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::app::FlintApp;
use crate::session::Session;
use crate::store::Logged;
use crate::subagent_ui::Conversation;
use crate::theme::{palette, size};
use crate::ui;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewKey {
    pub uid: u64,
    pub turn_id: u64,
    pub task_id: u64,
    pub contract_revision: u64,
    pub work_revision: u64,
}

pub fn review_key(session: &Session) -> ReviewKey {
    let task = session.view.task.as_ref();
    ReviewKey {
        uid: session.uid,
        turn_id: session.view.turn_id,
        task_id: task.map_or(0, |task| task.contract.id),
        contract_revision: task.map_or(0, |task| task.contract.revision),
        work_revision: task.map_or(0, |task| task.work_revision),
    }
}

pub struct QueueConfirmation {
    pub key: ReviewKey,
    pub prompt_id: Option<u64>,
}

pub struct TaskDraft {
    pub objective: Entity<TextareaState>,
    pub requirements: Entity<TextareaState>,
    pub restrictions: Entity<TextareaState>,
    pub mode: TaskMode,
    pub clear_blockers: bool,
    pub revision: u64,
}

pub struct TaskPanel {
    pub target: Conversation,
    pub turn_id: u64,
    pub focus: FocusHandle,
    pub return_focus: Option<FocusHandle>,
    pub scroll: ScrollHandle,
    pub draft: Option<TaskDraft>,
    pub confirm_accept: Option<ReviewKey>,
    pub pending_review: Option<ReviewKey>,
    pub pending_scope: Option<ReviewKey>,
    pub error: Option<String>,
    pub saving: bool,
}

impl FlintApp {
    pub fn task_for_panel(&self) -> Option<&TaskRecord> {
        let panel = self.task_panel.as_ref()?;
        self.conversation_view(&panel.target)?
            .turns
            .iter()
            .find(|turn| turn.turn_id == panel.turn_id)?
            .task
            .as_ref()
    }

    pub fn open_task_details(
        &mut self,
        target: Conversation,
        turn_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.conversation_view(&target).is_none_or(|view| {
            !view
                .turns
                .iter()
                .any(|turn| turn.turn_id == turn_id && turn.task.is_some())
        }) {
            return;
        }
        self.close_palette(window, cx);
        self.slash = None;
        self.mention = None;
        self.option_menu = None;
        self.project_menu = None;
        self.agent_menu = false;
        self.help_open = false;
        self.task_panel = Some(TaskPanel {
            target,
            turn_id,
            focus: cx.focus_handle(),
            return_focus: window.focused(cx),
            scroll: ScrollHandle::new(),
            draft: None,
            confirm_accept: None,
            pending_review: None,
            pending_scope: None,
            error: None,
            saving: false,
        });
        self.task_panel.as_ref().unwrap().focus.focus(window, cx);
        self.queue_resume_confirm = None;
        cx.notify();
    }

    pub fn open_current_task(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let target = Conversation {
            parent: self.session().uid,
            child: self.session().selected_subagent.clone(),
        };
        let turn_id = self
            .conversation_view(&target)
            .map_or(0, |view| view.turn_id);
        self.open_task_details(target, turn_id, window, cx);
    }

    pub fn close_task_details(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let return_focus = self.task_panel.take().and_then(|panel| panel.return_focus);
        if let Some(focus) = return_focus {
            focus.focus(window, cx);
        } else {
            self.composer
                .update(cx, |input, cx| input.focus(window, cx));
        }
        cx.notify();
    }

    pub(crate) fn task_panel_updated(&mut self, cx: &mut Context<Self>) {
        let Some(panel) = &self.task_panel else {
            return;
        };
        let target = panel.target.clone();
        let Some(ix) = self.session_index(target.parent) else {
            self.task_panel = None;
            return;
        };
        let current_key = review_key(&self.sessions[ix]);
        let accepted = self.sessions[ix].view.outcome() == TaskOutcome::Accepted;
        let updating = self.sessions[ix].task_update_pending.is_some();
        let panel = self.task_panel.as_mut().unwrap();
        if panel
            .confirm_accept
            .as_ref()
            .is_some_and(|key| key != &current_key)
        {
            panel.confirm_accept = None;
            panel.error = Some("The task or evidence changed. Review the current result.".into());
        }
        if accepted {
            panel.pending_review = None;
        }
        if !updating {
            if panel.pending_scope.as_ref().is_some_and(|key| {
                key.uid == current_key.uid
                    && key.turn_id == current_key.turn_id
                    && key.task_id == current_key.task_id
                    && key.contract_revision.checked_add(1) == Some(current_key.contract_revision)
            }) {
                panel.draft = None;
            }
            panel.pending_scope = None;
            panel.saving = false;
        }
        cx.notify();
    }

    fn task_controls_available(&self) -> bool {
        let Some(panel) = &self.task_panel else {
            return false;
        };
        let Some(ix) = self.session_index(panel.target.parent) else {
            return false;
        };
        let session = &self.sessions[ix];
        let native = session.agent == AgentKind::Flint
            && session
                .view
                .task
                .as_ref()
                .is_some_and(|task| task.source == TaskSource::Native);
        let writer_available = !native
            || match &session.ops {
                Some(ops) => !ops.is_closed(),
                None => session.pump.is_none() && session.engine_shutdown_verified,
            };
        panel.target.child.is_none()
            && panel.turn_id == session.view.turn_id
            && !session.view.running
            && !session.prompt_pending
            && !session.stopping
            && !session.history_save_failed
            && !session.storage_failed()
            && session.task_update_pending.is_none()
            && !panel.saving
            && panel.pending_review.is_none()
            && writer_available
    }

    pub fn request_task_acceptance(&mut self, cx: &mut Context<Self>) {
        if !self.task_controls_available() {
            return;
        }
        let uid = self.task_panel.as_ref().unwrap().target.parent;
        let Some(ix) = self.session_index(uid) else {
            return;
        };
        let key = review_key(&self.sessions[ix]);
        let panel = self.task_panel.as_mut().unwrap();
        panel.confirm_accept = Some(key);
        panel.error = None;
        cx.notify();
    }

    pub fn confirm_task_acceptance(&mut self, cx: &mut Context<Self>) {
        if !self.task_controls_available() {
            return;
        }
        let Some(key) = self
            .task_panel
            .as_mut()
            .and_then(|panel| panel.confirm_accept.take())
        else {
            return;
        };
        let Some(ix) = self.session_index(key.uid) else {
            return;
        };
        if key != review_key(&self.sessions[ix]) {
            return;
        }
        let review = TaskReview {
            turn_id: key.turn_id,
            task_id: key.task_id,
            contract_revision: key.contract_revision,
            work_revision: key.work_revision,
            decision: ReviewDecision::Accept,
        };
        let native = self.sessions[ix].agent == AgentKind::Flint
            && self.sessions[ix]
                .view
                .task
                .as_ref()
                .is_some_and(|task| task.source == TaskSource::Native);
        if native
            && let Some(ops) = self.sessions[ix]
                .ops
                .as_ref()
                .filter(|ops| !ops.is_closed())
        {
            if ops.try_send(Op::ReviewTask(review)).is_ok() {
                self.task_panel.as_mut().unwrap().pending_review = Some(key);
                self.sessions[ix].task_update_pending = Some(crate::session::TaskWrite::Engine);
            } else {
                self.task_panel.as_mut().unwrap().error =
                    Some("Couldn't request the task review.".into());
            }
        } else if native && let Some(dir) = self.sessions[ix].dir.clone() {
            let workspace = self.sessions[ix].workspace.clone();
            self.task_panel.as_mut().unwrap().saving = true;
            self.sessions[ix].task_update_pending = Some(crate::session::TaskWrite::Stopped);
            let update = cx.background_executor().spawn(async move {
                let result = flint_agent::review_saved_task(&dir, &workspace, &review);
                let refreshed = if result.is_err() {
                    flint_agent::saved_task(&dir).ok().flatten()
                } else {
                    None
                };
                (result, refreshed)
            });
            cx.spawn(async move |this, cx| {
                let (result, refreshed) = update.await;
                this.update(cx, |app, cx| {
                    if let Some(ix) = app.session_index(key.uid) {
                        app.sessions[ix].task_update_pending = None;
                        if let Some((turn_id, task)) = refreshed {
                            app.apply_event(
                                ix,
                                flint_agent::AgentEvent::TaskUpdated {
                                    turn_id,
                                    task: Box::new(task),
                                },
                                cx,
                            );
                        }
                        match result {
                            Ok(task) => app.apply_event(
                                ix,
                                flint_agent::AgentEvent::TaskAssessed {
                                    turn_id: key.turn_id,
                                    task: Box::new(task),
                                },
                                cx,
                            ),
                            Err(error) => {
                                if let Some(panel) = &mut app.task_panel
                                    && panel.target.parent == key.uid
                                    && panel.turn_id == key.turn_id
                                {
                                    panel.error = Some(error);
                                }
                            }
                        }
                    }
                    if let Some(panel) = &mut app.task_panel
                        && panel.target.parent == key.uid
                        && panel.turn_id == key.turn_id
                    {
                        panel.saving = false;
                    }
                    cx.notify();
                })
                .ok();
            })
            .detach();
        } else {
            // External/legacy results can only be accepted by the user. Keep
            // the review record distinct from an engine-issued assessment.
            if self.storage_ready(ix, cx) {
                let task = self.sessions[ix].view.task.as_ref().cloned();
                if let Some(mut task) = task {
                    match task.review(&review) {
                        Ok(()) => {
                            let saved = self.sessions[ix]
                                .log(Logged::TaskReviewed(review.clone()))
                                .and_then(|()| self.sessions[ix].flush_records());
                            match saved {
                                Ok(()) => {
                                    if let Ok(change) =
                                        self.sessions[ix].view.review_local_task(&review)
                                    {
                                        self.sessions[ix].apply(change);
                                        self.queue_task_reviewed(ix, cx);
                                    }
                                }
                                Err(_) => {
                                    self.task_panel.as_mut().unwrap().error = Some(
                                        "Couldn't save the review. Queued work remains paused."
                                            .into(),
                                    )
                                }
                            }
                        }
                        Err(error) => self.task_panel.as_mut().unwrap().error = Some(error),
                    }
                }
            }
        }
        cx.notify();
    }

    pub fn continue_task(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.task_controls_available() {
            return;
        }
        let uid = self.task_panel.as_ref().unwrap().target.parent;
        let Some(ix) = self.session_index(uid) else {
            return;
        };
        let Some(task) = self.sessions[ix].view.task.clone() else {
            return;
        };
        if self.sessions[ix].agent == AgentKind::Flint && task.source == TaskSource::Native {
            self.dispatch_task_continuation(ix, task.contract.id, task.contract.revision, cx);
        } else {
            // This is an explicit user follow-up, not a native restriction or
            // acceptance command sent to an external agent.
            let message = format!(
                "Continue this task:\n{}\n\nRequirements:\n{}\n\nRestrictions:\n{}\n\
                 Address the outstanding work and report remaining blockers.",
                task.contract.objective,
                task.contract
                    .requirements
                    .iter()
                    .map(|requirement| format!(
                        "{}: {}",
                        requirement.spec.kind_label(),
                        requirement.spec.text()
                    ))
                    .collect::<Vec<_>>()
                    .join("\n"),
                task.contract.restrictions.join("\n"),
            );
            self.dispatch_prompt(
                ix,
                crate::prompt_queue::Prompt::new(
                    "Continue the current task".into(),
                    message,
                    Vec::new(),
                ),
                cx,
            );
        }
        // Setup/storage errors include recovery actions in the transcript.
        // Do not leave those actions hidden behind a modal that did nothing.
        self.close_task_details(window, cx);
        cx.notify();
    }

    pub fn edit_task_scope(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.task_controls_available() {
            return;
        }
        let Some(task) = self.task_for_panel().cloned() else {
            return;
        };
        if task.source != TaskSource::Native {
            return;
        }
        let input = |text: String, window: &mut Window, cx: &mut Context<Self>| {
            let entity = cx.new(|cx| {
                TextareaState::new(window, cx)
                    .auto_grow(2, 5)
                    .submit_on_enter(false)
            });
            entity.update(cx, |input, cx| input.set_value(text, window, cx));
            entity
        };
        let objective = input(task.contract.objective.clone(), window, cx);
        let requirements = input(
            task.contract
                .requirements
                .iter()
                .map(|requirement| {
                    format!(
                        "{}: {}",
                        requirement.spec.kind_label().to_ascii_lowercase(),
                        requirement.spec.text()
                    )
                })
                .collect::<Vec<_>>()
                .join("\n"),
            window,
            cx,
        );
        let restrictions = input(task.contract.restrictions.join("\n"), window, cx);
        self.task_panel.as_mut().unwrap().draft = Some(TaskDraft {
            objective: objective.clone(),
            requirements,
            restrictions,
            mode: task.contract.mode,
            clear_blockers: false,
            revision: task.contract.revision,
        });
        self.task_panel.as_mut().unwrap().confirm_accept = None;
        objective.update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }

    pub fn save_task_scope(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.task_controls_available() {
            return;
        }
        let Some(panel) = &self.task_panel else {
            return;
        };
        let Some(draft) = &panel.draft else { return };
        let Some(ix) = self.session_index(panel.target.parent) else {
            return;
        };
        let Some(task) = self.sessions[ix].view.task.as_ref() else {
            return;
        };
        let requirements = match parse_requirements(&draft.requirements.read(cx).value()) {
            Ok(requirements) => requirements,
            Err(error) => {
                self.task_panel.as_mut().unwrap().error = Some(error);
                cx.notify();
                return;
            }
        };
        let edit = TaskEdit {
            task_id: task.contract.id,
            expected_revision: draft.revision,
            objective: draft.objective.read(cx).value().trim().into(),
            mode: draft.mode,
            requirements,
            restrictions: draft
                .restrictions
                .read(cx)
                .value()
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(str::to_string)
                .collect(),
            clear_reported_blockers: draft.clear_blockers,
        };
        // Validate on a copy before sending or touching storage.
        let mut validated = task.clone();
        if let Err(error) = validated.edit(edit.clone()) {
            self.task_panel.as_mut().unwrap().error = Some(error);
            cx.notify();
            return;
        }
        let uid = self.sessions[ix].uid;
        let turn_id = self.sessions[ix].view.turn_id;
        let key = review_key(&self.sessions[ix]);
        if let Some(ops) = self.sessions[ix]
            .ops
            .as_ref()
            .filter(|ops| !ops.is_closed())
        {
            if ops.try_send(Op::EditTask(edit)).is_err() {
                self.task_panel.as_mut().unwrap().error =
                    Some("Couldn't send the scope revision.".into());
                cx.notify();
                return;
            }
            self.task_panel.as_mut().unwrap().pending_scope = Some(key);
            self.task_panel.as_mut().unwrap().saving = true;
            self.sessions[ix].task_update_pending = Some(crate::session::TaskWrite::Engine);
        } else if let Some(dir) = self.sessions[ix].dir.clone() {
            self.task_panel.as_mut().unwrap().pending_scope = Some(key);
            self.task_panel.as_mut().unwrap().saving = true;
            self.sessions[ix].task_update_pending = Some(crate::session::TaskWrite::Stopped);
            let update = cx
                .background_executor()
                .spawn(async move { flint_agent::edit_saved_task(&dir, turn_id, edit) });
            cx.spawn(async move |this, cx| {
                let result = update.await;
                this.update(cx, |app, cx| {
                    if let Some(ix) = app.session_index(uid) {
                        app.sessions[ix].task_update_pending = None;
                        match result {
                            Ok((turn_id, task)) => app.apply_event(
                                ix,
                                flint_agent::AgentEvent::TaskUpdated {
                                    turn_id,
                                    task: Box::new(task),
                                },
                                cx,
                            ),
                            Err(error) => {
                                if let Some(panel) = &mut app.task_panel
                                    && panel.target.parent == uid
                                    && panel.turn_id == turn_id
                                {
                                    panel.error = Some(error);
                                }
                            }
                        }
                    }
                    if let Some(panel) = &mut app.task_panel
                        && panel.target.parent == uid
                        && panel.turn_id == turn_id
                    {
                        panel.saving = false;
                    }
                    cx.notify();
                })
                .ok();
            })
            .detach();
        } else {
            self.task_panel.as_mut().unwrap().pending_scope = Some(key);
            self.apply_event(
                ix,
                flint_agent::AgentEvent::TaskUpdated {
                    turn_id,
                    task: Box::new(validated),
                },
                cx,
            );
        }
        if let Some(panel) = &mut self.task_panel {
            panel.error = None;
            panel.focus.focus(window, cx);
        }
        cx.notify();
    }
}

pub fn parse_requirements(text: &str) -> Result<Vec<RequirementSpec>, String> {
    let mut requirements = Vec::new();
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let (kind, text) = line
            .split_once(':')
            .ok_or("Prefix each requirement with review:, file:, or check:.")?;
        let text = text.trim();
        if text.is_empty() {
            return Err("Each requirement needs text after its prefix.".into());
        }
        let spec = match kind.trim().to_ascii_lowercase().as_str() {
            "review" => RequirementSpec::Review(text.into()),
            "file" => RequirementSpec::File(text.into()),
            "check" => RequirementSpec::Command(text.into()),
            _ => return Err("Use only review:, file:, and check: requirement prefixes.".into()),
        };
        requirements.push(spec);
    }
    if requirements.is_empty() {
        return Err("Keep at least one acceptance requirement.".into());
    }
    Ok(requirements)
}

pub fn outcome_style(outcome: TaskOutcome) -> (IconName, Hsla) {
    let p = palette();
    match outcome {
        TaskOutcome::Accepted => (IconName::Check, p.success),
        TaskOutcome::Answered => (IconName::MessageCircle, p.text_muted),
        TaskOutcome::Blocked => (IconName::CircleAlert, p.warning),
        TaskOutcome::Partial | TaskOutcome::NeedsReview | TaskOutcome::Unknown => {
            (IconName::CircleAlert, p.warning)
        }
    }
}

fn section(title: &'static str, child: impl IntoElement) -> Div {
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(8.))
        .child(ui::label(title, size::SM, palette().text_muted))
        .child(child)
}

fn evidence_row(
    id: String,
    title: String,
    detail: String,
    state: EvidenceState,
) -> impl IntoElement {
    let p = palette();
    let color = match state {
        EvidenceState::Passed => p.success,
        EvidenceState::Failed | EvidenceState::Missing => p.danger,
        EvidenceState::Stale | EvidenceState::NeedsReview | EvidenceState::Unknown => p.warning,
        EvidenceState::Observed => p.text_muted,
    };
    div()
        .id(SharedString::from(id))
        .w_full()
        .py(px(8.))
        .flex()
        .flex_col()
        .gap(px(4.))
        .child(
            div()
                .flex()
                .items_start()
                .gap(px(10.))
                .child(ui::label(title, size::SM, p.text).flex_1().min_w_0())
                .child(ui::label(state.label(), size::XS, color).flex_shrink_0()),
        )
        .child(ui::label(detail, size::XS, p.text_muted))
        .test_support()
}

pub fn render(
    app: &FlintApp,
    window: &mut Window,
    cx: &mut Context<FlintApp>,
) -> Option<AnyElement> {
    let panel = app.task_panel.as_ref()?;
    let task = app.task_for_panel()?;
    let p = palette();
    let controls = app.task_controls_available();
    let session = &app.sessions[app.session_index(panel.target.parent)?];
    let saving =
        session.task_update_pending.is_some() || panel.pending_review.is_some() || panel.saving;
    let latest = app
        .conversation_view(&panel.target)
        .is_some_and(|view| view.turn_id == panel.turn_id);
    let outcome = task
        .assessment
        .as_ref()
        .map_or(TaskOutcome::Unknown, |assessment| assessment.outcome);
    let (icon, color) = outcome_style(outcome);
    let width = (f32::from(window.viewport_size().width) - 24.).clamp(260., 680.);
    let height = (f32::from(window.viewport_size().height) - 24.).max(180.);
    let header = div()
        .id("task-sheet-header")
        .relative()
        .px(px(20.))
        .py(px(16.))
        .flex()
        .items_center()
        .gap(px(10.))
        .child(ui::icon(icon, 18., color))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(3.))
                .child(ui::label("Task details", size::BASE, p.text))
                .child(ui::label(outcome.label(), size::SM, color)),
        )
        .child(
            Button::new("task-close")
                .ghost()
                .icon(IconName::X)
                .accessibility_label("Close task details")
                .on_click(cx.listener(|app, _, window, cx| app.close_task_details(window, cx))),
        )
        .child(crate::automation::task_control_probe("header"))
        .test_support();

    let mut content = div()
        .id("task-sheet-content")
        .px(px(20.))
        .pb(px(16.))
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .track_scroll(&panel.scroll)
        .flex()
        .flex_col()
        .gap(px(20.));
    if let Some(draft) = &panel.draft {
        content = content
            .child(section("Goal", div().id("task-goal-input").child(Textarea::new(&draft.objective).disabled(saving).accessibility_id("task-goal-input").aria_label("Task goal")).test_support()))
            .child(section("Mode", div().flex().flex_wrap().gap(px(6.)).children([
                TaskMode::Conversation, TaskMode::Work, TaskMode::ReadOnly, TaskMode::Unspecified,
            ].into_iter().enumerate().map(|(n, mode)| {
                Button::new(("task-mode", n)).outline().label(mode.label())
                    .disabled(saving)
                    .when(draft.mode == mode, |button| button.primary())
                    .on_click(cx.listener(move |app, _, _, cx| {
                        if let Some(draft) = app.task_panel.as_mut().and_then(|panel| panel.draft.as_mut()) {
                            draft.mode = mode;
                        }
                        cx.notify();
                    }))
            }))))
            .child(section("Acceptance requirements", div().flex().flex_col().gap(px(7.))
                .child(ui::label("One per line: review: meaning, file: path, or check: exact command. Files and checks do not certify meaning.", size::XS, p.text_muted))
                .child(div().id("task-requirements-input").child(Textarea::new(&draft.requirements).disabled(saving).accessibility_id("task-requirements-input").aria_label("Acceptance requirements")).test_support())))
            .child(section("Restrictions", div().flex().flex_col().gap(px(7.))
                .child(ui::label("One instruction per line. Read-only mode also refuses native writes and complex shell/MCP calls.", size::XS, p.text_muted))
                .child(div().id("task-restrictions-input").child(Textarea::new(&draft.restrictions).disabled(saving).accessibility_id("task-restrictions-input").aria_label("Task restrictions")).test_support())))
            .child(Button::new("task-clear-blockers").outline()
                .disabled(saving)
                .label(if draft.clear_blockers { "Blockers will be cleared" } else { "Keep reported blockers" })
                .on_click(cx.listener(|app, _, _, cx| {
                    if let Some(draft) = app.task_panel.as_mut().and_then(|panel| panel.draft.as_mut()) {
                        draft.clear_blockers = !draft.clear_blockers;
                    }
                    cx.notify();
                })));
    } else {
        content = content
            .child(section(
                "Goal",
                div()
                    .id("task-goal")
                    .text_size(px(size::BASE))
                    .child(task.contract.objective.clone())
                    .test_support(),
            ))
            .child(ui::label(
                format!(
                    "{} mode, scope revision {}",
                    task.contract.mode.label(),
                    task.contract.revision
                ),
                size::XS,
                p.text_muted,
            ))
            .child(section(
                "Requirements",
                div()
                    .flex()
                    .flex_col()
                    .children(task.contract.requirements.iter().map(|requirement| {
                        let status = task.assessment.as_ref().and_then(|assessment| {
                            assessment
                                .requirements
                                .iter()
                                .find(|a| a.requirement_id == requirement.id)
                        });
                        evidence_row(
                            format!("task-requirement-{}", requirement.id),
                            format!(
                                "{}: {}",
                                requirement.spec.kind_label(),
                                requirement.spec.text()
                            ),
                            status.map_or_else(
                                || "Not assessed yet.".into(),
                                |status| status.detail.clone(),
                            ),
                            status.map_or(EvidenceState::Unknown, |status| status.state),
                        )
                    })),
            ))
            .child(section(
                "Checks",
                div()
                    .id("task-checks")
                    .flex()
                    .flex_col()
                    .when(task.checks.is_empty(), |rows| {
                        rows.child(ui::label(
                            "No completed check was recorded.",
                            size::SM,
                            p.text_muted,
                        ))
                    })
                    .children(task.checks.iter().map(|check| {
                        evidence_row(
                            format!("task-check-{}", check.id),
                            check.command.clone(),
                            format!(
                                "{} (observed revision {})",
                                check.detail, check.work_revision
                            ),
                            check.state(task.contract.revision, task.work_revision),
                        )
                    }))
                    .test_support(),
            ))
            .child(section(
                "Deliverables",
                div()
                    .id("task-deliverables")
                    .flex()
                    .flex_col()
                    .when(task.files.is_empty(), |rows| {
                        rows.child(ui::label(
                            "No output file was recorded.",
                            size::SM,
                            p.text_muted,
                        ))
                    })
                    .children(task.files.iter().map(|file| {
                        let (state, detail) = match &file.state {
                            FileState::Present { bytes, .. } => (
                                EvidenceState::Observed,
                                format!("{bytes} bytes. Content is not certified."),
                            ),
                            FileState::Missing => (EvidenceState::Missing, "Not found.".into()),
                            FileState::Unknown(why) => (EvidenceState::Unknown, why.clone()),
                        };
                        evidence_row(
                            format!("task-file-{}", file.path),
                            file.path.clone(),
                            detail,
                            state,
                        )
                    }))
                    .test_support(),
            ));
        if !task.proposed_actions.is_empty() {
            content = content.child(section(
                "Suggested next steps (not applied)",
                div()
                    .id("task-proposed-actions")
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .children(
                        task.proposed_actions
                            .iter()
                            .map(|action| ui::label(action.clone(), size::SM, p.text_muted)),
                    )
                    .test_support(),
            ));
        }
        if !task.blockers.is_empty() {
            content = content.child(section(
                "Blockers",
                div()
                    .id("task-blockers")
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .children(task.blockers.iter().map(|blocker| {
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(3.))
                            .child(ui::label(blocker.text.clone(), size::SM, p.warning))
                            .child(ui::label(
                                format!("Reported by {}", blocker.source),
                                size::XS,
                                p.text_muted,
                            ))
                    }))
                    .test_support(),
            ));
        }
        if !task.children.is_empty() {
            content = content.child(section(
                "Child results",
                div()
                    .id("task-children")
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .children(task.children.iter().map(|child| {
                        ui::label(
                            format!("{}: {}", child.session_id, child.outcome.label()),
                            size::SM,
                            p.text,
                        )
                    }))
                    .test_support(),
            ));
        }
        if let Some(assessment) = &task.assessment {
            content = content.child(section(
                "Execution",
                ui::label(
                    task.continuation_stop
                        .as_ref()
                        .filter(|_| {
                            assessment.reason == flint_agent::TurnEndReason::ContinuationLimit
                        })
                        .map_or_else(|| stop_label(&assessment.reason), |stop| stop.detail()),
                    size::SM,
                    p.text_muted,
                ),
            ));
            if !assessment.issues.is_empty() {
                content = content.child(section(
                    "Outstanding work",
                    div()
                        .id("task-issues")
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .children(
                            assessment
                                .issues
                                .iter()
                                .map(|issue| ui::label(issue.clone(), size::SM, p.warning)),
                        )
                        .test_support(),
                ));
            }
        }
        if !task.contract.restrictions.is_empty() || !task.contract.instructions.is_empty() {
            content = content.child(section(
                "Restrictions and later instructions",
                ui::label(
                    task.contract
                        .restrictions
                        .iter()
                        .chain(&task.contract.instructions)
                        .cloned()
                        .collect::<Vec<_>>()
                        .join("\n"),
                    size::SM,
                    p.text,
                ),
            ));
        }
        content = content.child(section(
            "Original request",
            div()
                .id("task-original-request")
                .text_size(px(size::SM))
                .text_color(p.text_muted)
                .child(task.contract.original_request.clone())
                .test_support(),
        ));
        if let Some(report) = &task.report {
            content = content.child(section(
                "Agent report",
                ui::label(report.clone(), size::SM, p.text_muted),
            ));
        }
        content = content.child(ui::label(
            if task.source == TaskSource::External {
                "External agent: its own loop controls execution. Native scope, checks and read-only policy do not govern it. Acceptance here records your review only."
            } else if task.source == TaskSource::Legacy {
                "Older turn: no independent task assessment was saved. A normal stop is not evidence of acceptance."
            } else {
                "Evidence covers observed native tool activity and bounded file fingerprints, not every external change. Passing checks and existing files do not prove the requested meaning."
            }, size::XS, p.text_muted,
        ));
    }
    if let Some(error) = &panel.error {
        content = content.child(
            div()
                .id("task-sheet-error")
                .child(ui::label(error.clone(), size::SM, p.danger))
                .test_support(),
        );
    }
    let confirmation = panel.confirm_accept.is_some();
    let footer = div().id("task-sheet-footer").relative().flex_shrink_0().px(px(20.)).py(px(14.))
        .border_t_1().border_color(p.border).flex().flex_col().gap(px(10.))
        .when(confirmation, |footer| footer.child(ui::label(
            "Accept this result despite any listed gaps? This records your judgment, not independent certification. A queue held for this review may resume.",
            size::SM, p.warning,
        )))
        .when(saving, |footer| footer.child(ui::label(
            if session.task_update_pending==Some(crate::session::TaskWrite::Restore) {
                "Restoring saved task details…"
            } else {
                "Saving the review or scope revision…"
            }, size::SM, p.text_muted)))
        .when(!controls && !saving && latest && panel.target.child.is_none(), |footer| footer.child(ui::label(
            if session.history_save_failed || session.storage_failed() {
                "History could not be confirmed. Existing data has been kept; repair storage before changing this task."
            } else if session.view.running || session.prompt_pending {
                "Wait for this turn to stop before revising or accepting its result."
            } else {
                "The engine is finishing its history write. Wait before changing this task."
            },size::XS,p.warning,
        )))
        .child(div().flex().flex_wrap().items_center().gap(px(7.))
            .when(panel.draft.is_some(), |row| row
                .child(Button::new("task-edit-cancel").label("Cancel").on_click(cx.listener(|app, _, window, cx| {
                    if let Some(panel) = &mut app.task_panel { panel.draft = None; panel.focus.focus(window, cx); }
                    cx.notify();
                })))
                .child(Button::new("task-edit-save").primary().label("Save scope").disabled(!controls)
                    .on_click(cx.listener(|app, _, window, cx| app.save_task_scope(window, cx)))))
            .when(panel.draft.is_none() && confirmation, |row| row
                .child(Button::new("task-accept-cancel").label("Cancel").on_click(cx.listener(|app, _, _, cx| {
                    if let Some(panel) = &mut app.task_panel { panel.confirm_accept = None; }
                    cx.notify();
                })))
                .child(Button::new("task-accept-confirm").primary().label("Accept result").disabled(!controls)
                    .on_click(cx.listener(|app, _, _, cx| app.confirm_task_acceptance(cx)))))
            .when(panel.draft.is_none() && !confirmation, |row| row
                .when(latest && panel.target.child.is_none(), |row| row
                    .child(Button::new("task-continue").label("Continue task").disabled(!controls)
                        .on_click(cx.listener(|app, _, window, cx| app.continue_task(window, cx))))
                    .when(task.source == TaskSource::Native, |row| row
                        .child(Button::new("task-edit").label("Revise scope").disabled(!controls)
                            .on_click(cx.listener(|app, _, window, cx| app.edit_task_scope(window, cx)))))
                    .when(outcome != TaskOutcome::Accepted, |row| row
                        .child(Button::new("task-accept").primary().label("Review acceptance").disabled(!controls || task.assessment.is_none())
                            .on_click(cx.listener(|app, _, _, cx| app.request_task_acceptance(cx))))))
                .when(!latest || panel.target.child.is_some(), |row| row.child(ui::label(
                    "Read-only details. Continue or accept from the latest parent task.", size::XS, p.text_muted,
                )))))
        .child(crate::automation::task_control_probe("footer")).test_support();
    let sheet = div()
        .id("task-sheet")
        .role(gpui_kit::Role::Dialog)
        .aria_label("Task details")
        .w(px(width))
        .h(px(height.min(760.)))
        .bg(p.surface)
        .rounded(px(14.))
        .border_1()
        .border_color(p.border_strong)
        .overflow_hidden()
        .flex()
        .flex_col()
        .child(header)
        .child(
            div()
                .relative()
                .flex_1()
                .min_h_0()
                .overflow_hidden()
                .flex()
                .flex_col()
                .child(content.test_support())
                .child(Scrollbar::vertical(&panel.scroll).mode(ScrollbarMode::Always)),
        )
        .child(footer)
        .relative()
        .child(crate::automation::task_control_probe("sheet"))
        .focus_trap("task-sheet", &panel.focus)
        .test_support();
    Some(
        deferred(
            div()
                .absolute()
                .inset_0()
                .bg(hsla(0., 0., 0., 0.6))
                .flex()
                .items_center()
                .justify_center()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|app, _, window, cx| app.close_task_details(window, cx)),
                )
                .child(sheet.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())),
        )
        .with_priority(25)
        .into_any_element(),
    )
}

pub fn stop_label(reason: &flint_agent::TurnEndReason) -> String {
    match reason {
        flint_agent::TurnEndReason::Completed => {
            "Loop ended normally. Task acceptance is separate.".into()
        }
        flint_agent::TurnEndReason::Interrupted => "Execution interrupted.".into(),
        flint_agent::TurnEndReason::StepLimit => "Execution step limit reached.".into(),
        flint_agent::TurnEndReason::ContinuationLimit => {
            "Automatic follow-ups stopped with unresolved finish conditions.".into()
        }
        flint_agent::TurnEndReason::Failed(error) => format!("Execution failed: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::parse_requirements;
    #[test]
    fn requirement_editor_is_explicit_and_rejects_empty_or_unknown_prefixes() {
        assert_eq!(
            parse_requirements(
                "review: actual methods\nfile: methods.c\ncheck: python3 validate.py"
            )
            .unwrap()
            .len(),
            3
        );
        for input in ["", "dropped: methods", "methods.c", "file:"] {
            assert!(parse_requirements(input).is_err(), "{input}");
        }
    }
}
