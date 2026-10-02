//! The Settings sheet: model, endpoint, key status (never the key), JEV
//! status, default approval mode, reasoning effort and theme, saved to
//! `~/.flint/config.toml`.

use flint_agent::ApprovalMode;
use flint_agent::ReasoningEffort;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::*;
use gpui_kit::component::input::Input;
use gpui_kit::component::input::InputState;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::app::FlintApp;
use crate::settings;
use crate::settings::KeyStatus;
use crate::settings::Settings;
use crate::theme::palette;
use crate::theme::size;
use crate::ui;

pub struct SettingsForm {
    pub model: Entity<InputState>,
    pub base_url: Entity<InputState>,
    pub api_key_file: Entity<InputState>,
    pub approval: ApprovalMode,
    pub effort: Option<ReasoningEffort>,
    pub error: Option<String>,
}

impl FlintApp {
    pub fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let model = self.settings.model.clone();
        let base_url = self.settings.base_url.clone();
        let model = cx.new(|cx| InputState::new(window, cx).default_value(model));
        let base_url = cx.new(|cx| InputState::new(window, cx).default_value(base_url));
        let key_file = self.settings.api_key_file.clone();
        let api_key_file = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Optional, e.g. ~/.config/flint/key")
                .default_value(key_file)
        });
        self.settings_form = Some(SettingsForm {
            model,
            base_url,
            api_key_file,
            approval: self.approval,
            effort: self.effort,
            error: None,
        });
        self.palette = None;
        cx.notify();
    }

    pub fn close_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings_form = None;
        self.composer
            .update(cx, |state, cx| state.focus(window, cx));
        cx.notify();
    }

    pub fn save_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = &mut self.settings_form else {
            return;
        };
        let mut next = self.settings.clone();
        next.model = form.model.read(cx).value().trim().to_string();
        next.base_url = form.base_url.read(cx).value().trim().to_string();
        next.api_key_file = form.api_key_file.read(cx).value().trim().to_string();
        if next.model.is_empty() || !next.base_url.starts_with("http") {
            form.error = Some("Enter a model name and an http(s) base URL.".into());
            cx.notify();
            return;
        }
        next.set_approval_mode(form.approval);
        next.set_reasoning_effort(form.effort);
        if let Err(err) = next.save(&self.home) {
            form.error = Some(format!("Couldn't save settings: {err}"));
            cx.notify();
            return;
        }
        self.model = next.model.clone();
        self.approval = form.approval;
        self.effort = form.effort;
        self.settings = next;
        self.broadcast_effort();
        self.close_settings(window, cx);
    }

    pub fn dismiss_tip(&mut self, cx: &mut Context<Self>) {
        self.settings.tip_dismissed = true;
        self.settings.save(&self.home).ok();
        cx.notify();
    }
}

fn segment(id: impl Into<ElementId>, label: &str, selected: bool) -> Stateful<Div> {
    let p = palette();
    div()
        .id(id)
        .h(px(34.))
        .px(px(14.))
        .flex()
        .items_center()
        .rounded(px(8.))
        .cursor_pointer()
        .text_size(px(size::BASE - 1.))
        .when(selected, |s| {
            s.bg(p.raised)
                .text_color(p.text)
                .border_1()
                .border_color(p.border_strong)
        })
        .when(!selected, |s| {
            s.text_color(p.text_muted).hover(|h| h.bg(p.surface))
        })
        .child(label.to_string())
}

fn row(label: &str, hint: Option<&str>, control: impl IntoElement) -> Div {
    let p = palette();
    div()
        .flex()
        .items_start()
        .gap(px(20.))
        .child(
            div()
                .w(px(150.))
                .pt(px(7.))
                .flex_shrink_0()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(ui::label(label.to_string(), size::BASE, p.text))
                .when_some(hint.map(str::to_string), |col, hint| {
                    col.child(ui::label(hint, size::XS, p.text_subtle))
                }),
        )
        .child(div().flex_1().min_w_0().child(control))
}

pub fn render(app: &FlintApp, cx: &mut Context<FlintApp>) -> Option<impl IntoElement> {
    let p = palette();
    let form = app.settings_form.as_ref()?;
    let (key_icon, key_text, key_color) = match app
        .settings
        .key_status(app.key_path.as_deref(), &app.key_sources)
    {
        KeyStatus::Found(source) => (
            IconName::CircleCheck,
            format!("Found in {source}"),
            p.success,
        ),
        KeyStatus::Missing(source) => (
            IconName::CircleAlert,
            format!("Not found — set {source}, or choose a key file below"),
            p.danger,
        ),
    };
    let (jev_text, jev_color) = if settings::jev_key_set() {
        ("On — TYPESAFE_API_KEY is set", p.success)
    } else {
        (
            "Off — optional; set TYPESAFE_API_KEY to enable",
            p.text_subtle,
        )
    };
    let approvals = [
        (ApprovalMode::Auto, "Auto-run"),
        (ApprovalMode::AskForChanges, "Ask before changes"),
    ];
    let efforts = [
        (Some(ReasoningEffort::Low), "Low"),
        (Some(ReasoningEffort::Medium), "Medium"),
        (Some(ReasoningEffort::High), "High"),
        (None, "Default"),
    ];
    let approval_row = div()
        .flex()
        .gap(px(6.))
        .children(approvals.into_iter().enumerate().map(|(n, (mode, label))| {
            segment(("settings-approval", n), label, form.approval == mode)
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(form) = &mut this.settings_form {
                        form.approval = mode;
                    }
                    cx.notify();
                }))
                .test_support()
        }));
    let effort_row = div()
        .flex()
        .gap(px(6.))
        .children(efforts.into_iter().enumerate().map(|(n, (effort, label))| {
            segment(("settings-effort", n), label, form.effort == effort)
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(form) = &mut this.settings_form {
                        form.effort = effort;
                    }
                    cx.notify();
                }))
                .test_support()
        }));

    let sheet = div()
        .id("settings-sheet")
        .w(px(640.))
        .rounded(px(16.))
        .border_1()
        .border_color(p.border_strong)
        .bg(p.surface)
        .shadow_lg()
        .p(px(28.))
        .flex()
        .flex_col()
        .gap(px(22.))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(4.))
                .child(
                    div()
                        .text_size(px(20.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("Settings"),
                )
                .child(
                    ui::label(
                        format!(
                            "Saved to {}",
                            settings::display_path(&Settings::path(&app.home))
                        ),
                        size::SM,
                        p.text_subtle,
                    )
                    .truncate(),
                ),
        )
        .child(row("Model", None, Input::new(&form.model)))
        .child(row(
            "Base URL",
            Some("OpenAI-compatible endpoint"),
            Input::new(&form.base_url),
        ))
        .child(row(
            "API key",
            Some("Never shown or logged"),
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(
                    div()
                        .pt(px(7.))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(ui::icon(key_icon, 15., key_color))
                        .child(ui::label(key_text, size::BASE - 1., key_color)),
                )
                .child(Input::new(&form.api_key_file)),
        ))
        .child(row(
            "JEV judge",
            Some("Smarter guard checks"),
            div()
                .pt(px(7.))
                .child(ui::label(jev_text, size::BASE - 1., jev_color)),
        ))
        .child(row("Approval", Some("Default for new turns"), approval_row))
        .child(row("Reasoning effort", None, effort_row))
        .child(row(
            "Theme",
            None,
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .child(segment("settings-theme", "Dark", true))
                .child(ui::label(
                    "Light theme isn't available yet",
                    size::SM,
                    p.text_subtle,
                )),
        ))
        .when_some(form.error.clone(), |sheet, error| {
            sheet.child(ui::label(error, size::SM, p.danger))
        })
        .child(
            div()
                .flex()
                .justify_end()
                .gap(px(10.))
                .child(
                    div()
                        .id("settings-cancel")
                        .child(Button::new("cancel").label("Cancel").on_click(
                            cx.listener(|this, _, window, cx| this.close_settings(window, cx)),
                        ))
                        .test_support(),
                )
                .child(
                    div()
                        .id("settings-save")
                        .child(Button::new("save").primary().label("Save").on_click(
                            cx.listener(|this, _, window, cx| this.save_settings(window, cx)),
                        ))
                        .test_support(),
                ),
        );

    Some(
        deferred(
            div()
                .absolute()
                .inset_0()
                .bg(hsla(0., 0., 0., 0.5))
                .flex()
                .items_start()
                .justify_center()
                .pt(px(90.))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| this.close_settings(window, cx)),
                )
                .child(sheet),
        )
        .with_priority(20),
    )
}
