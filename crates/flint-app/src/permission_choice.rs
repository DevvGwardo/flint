//! An explicit permission choice only for installations without prior settings/history.
use crate::{
    app::FlintApp,
    theme::{palette, size},
    ui,
};
use flint_agent::ApprovalMode;
use gpui_kit::{base::FocusTrapElement as _, component::button::*, *};

impl FlintApp {
    pub fn choose_initial_permission(
        &mut self,
        mode: ApprovalMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut next = self.settings.clone();
        next.set_approval_mode(mode);
        next.permission_choice_pending = Some(false);
        if next.save(&self.home).is_err() {
            self.permission_choice_error = Some("Couldn't save your choice. Check storage and retry, or cancel to keep Ask for this run.".into());
            cx.notify();
            return;
        }
        self.settings = next;
        self.approval = mode;
        self.cancel_initial_permission(window, cx);
    }

    pub fn cancel_initial_permission(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings.permission_choice_pending == Some(true) {
            // This marker does not select Auto or persist a choice. It keeps a
            // cancelled fresh install distinct from history-only legacy installs.
            if std::fs::create_dir_all(&self.home)
                .and_then(|()| {
                    std::fs::write(self.home.join("permission-choice-pending"), b"pending\n")
                })
                .is_err()
            {
                self.permission_choice_error = Some(
                    "Couldn't remember the pending choice. Fix storage before continuing.".into(),
                );
                cx.notify();
                return;
            }
        }
        self.permission_choice_open = false;
        self.permission_choice_error = None;
        self.composer
            .update(cx, |state, cx| state.focus(window, cx));
        cx.notify();
    }
}

pub fn render(app: &FlintApp, cx: &mut Context<FlintApp>) -> Option<impl IntoElement> {
    if !app.permission_choice_open {
        return None;
    }
    let p = palette();
    Some(deferred(div().absolute().inset_0().bg(hsla(0., 0., 0., 0.6))
        .flex().items_center().justify_center()
        .child(div().w(px(540.)).p(px(24.)).rounded(px(14.)).bg(p.surface)
            .border_1().border_color(p.border_strong).flex().flex_col().gap(px(14.))
            .child(ui::label("How should new native engines act?", size::MD, p.text))
            .child(ui::label("Ask before changes is recommended. Flint asks before commands and file edits. Reading files does not need approval.", size::BASE, p.text_muted))
            .child(Button::new("first-run-ask").primary().label("Ask before changes (recommended)")
                .on_click(cx.listener(|this, _, window, cx| this.choose_initial_permission(ApprovalMode::AskForChanges, window, cx))))
            .child(ui::label("Auto-run lets new native engines execute commands and edit files in the workspace without asking, including later turns and subagent work. Commands can have effects beyond files. ACP agents use their own permission rules.", size::BASE, p.text_muted))
            .child(Button::new("first-run-auto").label("Choose Auto-run")
                .on_click(cx.listener(|this, _, window, cx| this.choose_initial_permission(ApprovalMode::Auto, window, cx))))
            .child(ui::label("This sets the default for new native engines only. Engines that are already running keep their permissions.", size::SM, p.text_muted))
            .children(app.permission_choice_error.as_ref().map(|error| ui::label(error.clone(), size::SM, p.danger)))
            .child(Button::new("first-run-cancel").label("Not now (keep Ask this run)")
                .on_click(cx.listener(|this, _, window, cx| this.cancel_initial_permission(window, cx))))
            .focus_trap("permission-focus-trap", &app.permission_focus))
    ).with_priority(40))
}
