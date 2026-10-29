//! Native ownership of the Flutter view. Renderer actions only open existing
//! controls or insert a non-submitted, read-only follow-up into the composer.
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use flint_agent::genui::{
    Action, ActionName, CATALOG_ID, Component, SURFACE_ID, SurfaceSnapshot, TaskMetadata,
    TextStyle, TextValue,
};
use gpui_kit::*;

use crate::app::FlintApp;
use crate::genui_bridge::{Bridge, BridgeEvent, SnapshotInput};

pub(crate) struct GeneratedSurface {
    pub session_uid: u64,
    pub task: TaskMetadata,
    pub surface: SurfaceSnapshot,
}

pub(crate) struct InteractiveView {
    bridge: Bridge,
    runtime: Option<tokio::runtime::Runtime>,
    _reader: Task<()>,
    epoch: u64,
    pending: Vec<BridgeEvent>,
    published: Option<SnapshotKey>,
    disconnected: bool,
}

#[derive(Clone, PartialEq, Eq)]
struct SnapshotKey {
    session_uid: u64,
    task_projection: u64,
    generated_content: u64,
    task: Option<TaskMetadata>,
}

fn runtime_directory() -> Result<PathBuf, &'static str> {
    // The signed helper's home-relative entitlement is independent of
    // Options.home / FLINT_HOME, which select application data storage.
    flint_platform::home_dir()
        .filter(|home| home.is_absolute())
        .map(|home| home.join(".flint/genui-runtime"))
        .ok_or("Cannot resolve the sandbox-authorized GenUI runtime directory.")
}

impl Drop for InteractiveView {
    fn drop(&mut self) {
        self.bridge.shutdown();
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_timeout(Duration::from_secs(1));
        }
    }
}

pub(crate) fn companion_executable() -> Result<PathBuf, &'static str> {
    if let Some(path) = std::env::var_os("HOOTER_GENUI_EXECUTABLE") {
        let path = PathBuf::from(path);
        if path.is_absolute() && path.is_file() {
            return Ok(path);
        }
        return Err("The configured GenUI executable must be an absolute existing file.");
    }
    let exe = std::env::current_exe().map_err(|_| "Cannot resolve the Hooter executable.")?;
    let directory = exe.parent().ok_or("Cannot resolve the Hooter bundle.")?;
    let relative = "hooter_genui.app/Contents/MacOS/hooter_genui";
    let mut candidates = vec![directory.join(relative)];
    if let Some(contents) = directory.parent() {
        candidates.push(contents.join("Helpers").join(relative));
    }
    candidates.push(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../genui/build/macos/Build/Products/Release")
            .join(relative),
    );
    candidates
        .into_iter()
        .find(|path| path.is_file())
        .ok_or("GenUI companion is not installed. Build genui/ or package the helper with Hooter.")
}

fn bounded(value: &str) -> String {
    value.chars().take(1_000).collect()
}

#[cfg(test)]
#[path = "genui_ui_tests.rs"]
mod tests;

impl FlintApp {
    pub(crate) fn invalidate_genui_actions(&mut self) {
        if let Some(view) = &mut self.genui {
            view.published = None;
            if let Err(error) = view.bridge.invalidate_actions() {
                view.bridge.shutdown();
                view.disconnected = true;
                self.genui_error = Some(error);
            }
        }
    }

    fn genui_snapshot_key(&self) -> SnapshotKey {
        SnapshotKey {
            session_uid: self.session().uid,
            task_projection: self.session().view.task_projection_revision,
            generated_content: self.genui_content_revision,
            task: self.genui_task_metadata(),
        }
    }

    fn changed_genui_snapshot(
        &self,
        published: Option<&SnapshotKey>,
    ) -> Option<(SnapshotKey, SnapshotInput)> {
        let key = self.genui_snapshot_key();
        if published == Some(&key)
            && key.task_projection != u64::MAX
            && key.generated_content != u64::MAX
        {
            return None;
        }
        Some((key, self.genui_snapshot()))
    }

    pub(crate) fn genui_task_metadata(&self) -> Option<TaskMetadata> {
        let session = self.session();
        if session.selected_subagent.is_some() || (session.prompt_pending && !session.view.running)
        {
            return None;
        }
        session.view.task.as_ref().map(TaskMetadata::from_task)
    }

    fn genui_snapshot(&self) -> SnapshotInput {
        let session = self.session();
        let metadata = self.genui_task_metadata();
        if let Some(generated) = self.genui_generated.as_ref().filter(|generated| {
            generated.session_uid == session.uid
                && metadata.as_ref().is_some_and(|task| {
                    task.id == generated.task.id
                        && task.contract_revision == generated.task.contract_revision
                        && task.work_revision == generated.task.work_revision
                })
        }) {
            return SnapshotInput {
                session_uid: session.uid,
                task: metadata,
                surface: generated.surface.clone(),
            };
        }
        let task = session.view.task.as_ref();
        let mut components = Vec::new();
        let mut children = Vec::new();
        let mut text = |value: String, style: Option<TextStyle>| {
            let id = format!("text-{}", components.len());
            components.push(Component::PlainText {
                id: id.clone(),
                text: TextValue::Literal(value),
                style,
            });
            children.push(id);
        };
        text("Interactive task view".into(), Some(TextStyle::Heading));
        if let Some(task) = task.filter(|_| self.genui_task_metadata().is_some()) {
            text(bounded(&task.contract.objective), None);
            if let Some(assessment) = &task.assessment {
                text(assessment.outcome.label().into(), Some(TextStyle::Heading));
                for issue in assessment.issues.iter().take(8) {
                    text(bounded(issue), Some(TextStyle::Muted));
                }
                for requirement in assessment.requirements.iter().take(12) {
                    text(
                        format!(
                            "Requirement {}: {} · {}",
                            requirement.requirement_id,
                            requirement.state.label(),
                            bounded(&requirement.detail)
                        ),
                        None,
                    );
                }
            } else {
                text(
                    "Work is in progress. Verification remains host-owned.".into(),
                    Some(TextStyle::Muted),
                );
            }
            if let Some(review) = &task.automatic_review {
                text(
                    format!("Independent review: {}", bounded(&review.summary)),
                    None,
                );
            }
            for check in task.checks.iter().rev().take(5) {
                text(
                    format!(
                        "{}: {}",
                        if check.verification {
                            "Check"
                        } else {
                            "Observation"
                        },
                        bounded(&check.command)
                    ),
                    Some(TextStyle::Code),
                );
            }
        } else {
            text(
                "No active parent task. Native controls remain available in Hooter.".into(),
                Some(TextStyle::Muted),
            );
        }
        for (id, label, name) in [
            (
                "open-task",
                "Open native task controls",
                ActionName::OpenTask,
            ),
            (
                "stage-follow-up",
                "Draft a read-only follow-up",
                ActionName::StagePrompt,
            ),
        ] {
            if self.genui_task_metadata().is_none() {
                break;
            }
            let label_id = format!("{id}-label");
            components.push(Component::PlainText {
                id: label_id.clone(),
                text: TextValue::Literal(label.into()),
                style: None,
            });
            components.push(Component::Button {
                id: id.into(),
                child: label_id,
                action: Action { name },
            });
            children.push(id.into());
        }
        components.push(Component::Column {
            id: "root".into(),
            children,
        });
        SnapshotInput {
            session_uid: session.uid,
            task: self.genui_task_metadata(),
            surface: SurfaceSnapshot {
                surface_id: SURFACE_ID.into(),
                catalog_id: CATALOG_ID.into(),
                components,
                data: BTreeMap::new(),
            },
        }
    }

    pub(crate) fn open_genui(&mut self, cx: &mut Context<Self>) {
        if self.genui.is_some() {
            self.genui.take();
        }
        self.genui_error = None;
        let result = (|| {
            let executable = companion_executable()?;
            let published = self.genui_snapshot_key();
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(1)
                .enable_all()
                .build()
                .map_err(|_| "Cannot start the GenUI runtime.")?;
            let bridge = Bridge::launch(
                runtime.handle(),
                &runtime_directory()?,
                &executable,
                self.genui_snapshot(),
            )?;
            self.genui_epoch = self
                .genui_epoch
                .checked_add(1)
                .ok_or("The GenUI launch counter is exhausted.")?;
            let epoch = self.genui_epoch;
            let events = bridge.events.clone();
            let reader = cx.spawn(async move |app, cx| {
                while let Ok(event) = events.recv().await {
                    let keep = app
                        .update(cx, |app, cx| {
                            let Some(view) = app.genui.as_mut().filter(|view| view.epoch == epoch)
                            else {
                                return false;
                            };
                            if view.pending.len() >= 32 {
                                view.bridge.shutdown();
                                return false;
                            }
                            view.pending.push(event);
                            cx.notify();
                            true
                        })
                        .unwrap_or(false);
                    if !keep {
                        break;
                    }
                }
            });
            Ok(InteractiveView {
                bridge,
                runtime: Some(runtime),
                _reader: reader,
                epoch,
                pending: Vec::new(),
                published: Some(published),
                disconnected: false,
            })
        })();
        match result {
            Ok(view) => self.genui = Some(view),
            Err(error) => self.genui_error = Some(error),
        }
        cx.notify();
    }

    pub(crate) fn sync_genui(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let snapshot = self.genui.as_ref().and_then(|view| {
            (!view.disconnected)
                .then(|| self.changed_genui_snapshot(view.published.as_ref()))
                .flatten()
        });
        if let (Some(view), Some((key, snapshot))) = (&mut self.genui, snapshot) {
            match view.bridge.publish(snapshot) {
                Ok(_) => view.published = Some(key),
                Err(error) => {
                    view.disconnected = true;
                    self.genui_error = Some(error);
                }
            }
        }
        let pending = self
            .genui
            .as_mut()
            .map(|view| std::mem::take(&mut view.pending))
            .unwrap_or_default();
        for event in pending {
            match event {
                BridgeEvent::Connected => {
                    if let Some(view) = &mut self.genui {
                        view.disconnected = false;
                    }
                    self.genui_error = None;
                }
                BridgeEvent::Disconnected(error) => {
                    if let Some(view) = &mut self.genui {
                        view.disconnected = true;
                    }
                    self.genui_error = Some(error);
                }
                BridgeEvent::Action(action) => {
                    let task = self.genui_task_metadata();
                    let name = self.genui.as_ref().and_then(|view| {
                        view.bridge
                            .authorize(&action, self.session().uid, task.as_ref())
                            .ok()
                    });
                    if self.session().selected_subagent.is_some() {
                        continue;
                    }
                    match name {
                        Some(ActionName::OpenTask) => self.open_current_task(window, cx),
                        Some(ActionName::StagePrompt) => self.stage_genui_followup(window, cx),
                        None => {}
                    }
                }
            }
        }
    }

    pub(crate) fn stage_genui_followup(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.genui_task_metadata().is_none() {
            return;
        }
        const PROMPT: &str = "\n\nExplain the current task's remaining verification evidence and propose the next safe step. Do not make changes.\n\n";
        self.composer
            .update(cx, |input, cx| input.insert(PROMPT, window, cx));
        cx.notify();
    }

    pub(crate) fn genui_status(&self) -> &'static str {
        if self.genui_error.is_some() {
            "GenUI unavailable"
        } else if self
            .genui
            .as_ref()
            .is_some_and(|view| view.bridge.is_current_rendered())
        {
            "GenUI view ready"
        } else if self
            .genui
            .as_ref()
            .is_some_and(|view| view.bridge.is_connected())
        {
            "GenUI connected"
        } else if self.genui.is_some() {
            "GenUI connecting"
        } else {
            "Open interactive view"
        }
    }

    pub(crate) fn capture_genui_event(&mut self, uid: u64, event: &flint_agent::AgentEvent) {
        use flint_agent::AgentEvent;
        match event {
            AgentEvent::ToolCallStarted { call_id, name, .. }
                if name == flint_agent::tools::RENDER_UI =>
            {
                if uid != self.session().uid || self.genui_pending.len() >= 64 {
                    return;
                }
                let task = self.genui_task_metadata();
                self.genui_pending.insert((uid, call_id.clone()), task);
            }
            AgentEvent::ToolCallFinished {
                call_id,
                output,
                success,
                ..
            } => {
                let Some(Some(task)) = self.genui_pending.remove(&(uid, call_id.clone())) else {
                    return;
                };
                if !success
                    || uid != self.session().uid
                    || output.len() > flint_agent::genui::MAX_FRAME_BYTES
                {
                    return;
                }
                #[derive(serde::Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Prepared {
                    surface: SurfaceSnapshot,
                }
                if let Ok(prepared) = serde_json::from_str::<Prepared>(output)
                    && prepared.surface.validate().is_ok()
                {
                    self.genui_generated = Some(GeneratedSurface {
                        session_uid: uid,
                        task,
                        surface: prepared.surface,
                    });
                    self.genui_content_revision = self.genui_content_revision.saturating_add(1);
                }
            }
            AgentEvent::TurnFinished { .. } | AgentEvent::SessionStopped { .. } => {
                self.genui_pending
                    .retain(|(session_uid, _), _| *session_uid != uid);
            }
            _ => {}
        }
    }
}
