//! The Settings sheet: model, endpoint, key status (never the key), JEV
//! status, default approval mode, reasoning effort and theme, saved to
//! `~/.flint/config.toml`.

use flint_agent::ApprovalMode;
use flint_agent::ReasoningEffort;
use gpui_kit::assets::IconName;
use gpui_kit::base::Disableable as _;
use gpui_kit::base::FocusTrapElement as _;
use gpui_kit::component::button::*;
use gpui_kit::component::input::Input;
use gpui_kit::component::input::InputEvent;
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
    pub subagent_model: Entity<InputState>,
    pub base_url: Entity<InputState>,
    pub api_key_file: Entity<InputState>,
    pub api_key_env: Entity<InputState>,
    pub approval: ApprovalMode,
    pub effort: Option<ReasoningEffort>,
    pub error: Option<String>,
    pub subscriptions: Vec<Subscription>,
    pub focus: FocusHandle,
    pub return_focus: Option<FocusHandle>,
    pub probe: Option<crate::connection_test::Probe>,
    pub probe_task: Option<Task<()>>,
    pub probe_draft: Option<Settings>,
    pub probe_result: Option<crate::connection_test::ConnectionResult>,
}

impl SettingsForm {
    pub fn draft(&self, saved: &Settings, cx: &App) -> Settings {
        let mut draft = saved.clone();
        draft.model = self.model.read(cx).value().trim().to_string();
        draft.subagent_model = self.subagent_model.read(cx).value().trim().to_string();
        draft.base_url = self.base_url.read(cx).value().trim().to_string();
        draft.api_key_file = self.api_key_file.read(cx).value().trim().to_string();
        draft.api_key_env = self.api_key_env.read(cx).value().trim().to_string();
        draft
    }

    fn invalidate_probe(&mut self) {
        self.probe = None;
        self.probe_task = None;
        self.probe_draft = None;
        self.probe_result = None;
    }
}

impl FlintApp {
    pub fn test_provider_connection(&mut self, cx: &mut Context<Self>) {
        let Some(form) = &mut self.settings_form else {
            return;
        };
        form.invalidate_probe();
        let draft = form.draft(&self.settings, cx);
        if draft.model.is_empty() || !valid_env_name(&draft.api_key_env) {
            form.error =
                Some("Enter a model and a valid credential environment variable name.".into());
            cx.notify();
            return;
        }
        let key = draft
            .resolve_key(self.key_path.as_deref(), &self.key_sources)
            .map(|key| key.key);
        let probe =
            match crate::connection_test::Probe::start(&draft.base_url, draft.model.clone(), key) {
                Ok(probe) => probe,
                Err(message) => {
                    form.error = Some(message.into());
                    cx.notify();
                    return;
                }
            };
        let results = probe.result.clone();
        let form_id = form.model.entity_id();
        form.probe_draft = Some(draft.clone());
        form.error = None;
        form.probe = Some(probe);
        form.probe_task = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(30))
                    .await;
                match results.try_recv() {
                    Ok(result) => {
                        this.update(cx, |app, cx| {
                            if let Some(form) = &mut app.settings_form
                                && form.model.entity_id() == form_id
                                && form.probe_draft.as_ref() == Some(&draft)
                                && form.draft(&app.settings, cx) == draft
                            {
                                form.probe_result = Some(result);
                                form.probe = None;
                                form.probe_task = None;
                                cx.notify();
                            }
                        })
                        .ok();
                        break;
                    }
                    Err(async_channel::TryRecvError::Closed) => break,
                    Err(async_channel::TryRecvError::Empty) => {}
                }
            }
        }));
        cx.notify();
    }

    pub fn cancel_provider_test(&mut self, cx: &mut Context<Self>) {
        if let Some(form) = &mut self.settings_form {
            form.invalidate_probe();
        }
        cx.notify();
    }
    pub fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let model = self.settings.model.clone();
        let base_url = self.settings.base_url.clone();
        let model = cx.new(|cx| InputState::new(window, cx).default_value(model));
        let subagent_model = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Inherit parent model")
                .default_value(self.settings.subagent_model.clone())
        });
        let base_url = cx.new(|cx| InputState::new(window, cx).default_value(base_url));
        let key_file = self.settings.api_key_file.clone();
        let api_key_file = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Optional, e.g. ~/.config/flint/key")
                .default_value(key_file)
        });
        let api_key_env = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("FLINT_API_KEY, then OPENAI_API_KEY")
                .default_value(self.settings.api_key_env.clone())
        });
        let mut subscriptions: Vec<_> = [
            &model,
            &subagent_model,
            &base_url,
            &api_key_file,
            &api_key_env,
        ]
        .into_iter()
        .map(|input| {
            cx.subscribe(input, |this, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    if let Some(form) = &mut this.settings_form {
                        form.error = None;
                        form.invalidate_probe();
                    }
                    cx.notify();
                }
            })
        })
        .collect();
        // Presets and programmatic edits notify the input without emitting
        // InputEvent::Change. They must invalidate checks just like typing.
        subscriptions.extend(
            [
                &model,
                &subagent_model,
                &base_url,
                &api_key_file,
                &api_key_env,
            ]
            .into_iter()
            .map(|input| {
                cx.observe(input, |this, _, cx| {
                    if let Some(form) = &mut this.settings_form
                        && form
                            .probe_draft
                            .as_ref()
                            .is_some_and(|draft| *draft != form.draft(&this.settings, cx))
                    {
                        form.invalidate_probe();
                        cx.notify();
                    }
                })
            }),
        );
        self.settings_form = Some(SettingsForm {
            model,
            subagent_model,
            base_url,
            api_key_file,
            api_key_env,
            approval: self.approval,
            effort: self.effort,
            error: None,
            subscriptions,
            focus: cx.focus_handle(),
            return_focus: if self.palette.is_some() {
                self.palette_return_focus.take()
            } else {
                window.focused(cx)
            },
            probe: None,
            probe_task: None,
            probe_draft: None,
            probe_result: None,
        });
        self.palette = None;
        let form = self.settings_form.as_ref().unwrap();
        if crate::automation::background_launch() && !window.is_window_active() {
            // Like the composer, a never-key automation window must not start
            // an input's caret timer. Keep keyboard focus in the sheet.
            form.focus.focus(window, cx);
        } else {
            form.model.update(cx, |state, cx| state.focus(window, cx));
        }
        cx.notify();
    }

    pub fn close_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let return_focus = self.settings_form.take().and_then(|form| form.return_focus);
        if let Some(focus) = return_focus {
            focus.focus(window, cx);
        } else {
            self.composer
                .update(cx, |state, cx| state.focus(window, cx));
        }
        cx.notify();
    }

    pub fn save_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = &mut self.settings_form else {
            return;
        };
        let mut next = form.draft(&self.settings, cx);
        if next.model.is_empty() || crate::connection_test::models_url(&next.base_url).is_err() {
            form.error = Some("Enter a model name and an http(s) URL with a host.".into());
            cx.notify();
            return;
        }
        if !valid_env_name(&next.api_key_env) {
            form.error = Some("Enter an environment variable name, not a key value.".into());
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

fn valid_env_name(name: &str) -> bool {
    name.chars()
        .enumerate()
        .all(|(i, c)| c == '_' || c.is_ascii_alphabetic() || (i > 0 && c.is_ascii_digit()))
}

fn segment(id: impl Into<ElementId>, label: &str, selected: bool) -> Stateful<Div> {
    let p = palette();
    div()
        .id(id)
        .aria_label(label.to_string())
        .tab_index(0)
        .focus_visible(|style| style.border_2().border_color(p.accent))
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

pub fn render(
    app: &FlintApp,
    window: &mut Window,
    cx: &mut Context<FlintApp>,
) -> Option<impl IntoElement> {
    let p = palette();
    let form = app.settings_form.as_ref()?;
    let draft = form.draft(&app.settings, cx);
    let (key_icon, key_text, key_color) =
        match draft.key_status(app.key_path.as_deref(), &app.key_sources) {
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

    let width = (f32::from(window.viewport_size().width) - 32.).clamp(280., 640.);
    let height = (f32::from(window.viewport_size().height) - 32.).max(180.);
    let content = div()
        .id("settings-content")
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .gap(px(20.))
        .child(row(
            "Provider",
            Some("Presets fill the endpoint; confirm the model before saving"),
            div()
                .flex()
                .gap(px(8.))
                .child(
                    Button::new("preset-openai")
                        .label("OpenAI")
                        .on_click(cx.listener(|this, _, window, cx| {
                            if let Some(form) = &this.settings_form {
                                form.base_url.update(cx, |input, cx| {
                                    input.set_value(
                                        flint_agent::config::DEFAULT_BASE_URL,
                                        window,
                                        cx,
                                    )
                                });
                                form.model.update(cx, |input, cx| {
                                    input.set_value(flint_agent::config::DEFAULT_MODEL, window, cx)
                                });
                            }
                        })),
                )
                .child(
                    Button::new("preset-ollama")
                        .label("Local (Ollama)")
                        .on_click(cx.listener(|this, _, window, cx| {
                            if let Some(form) = &this.settings_form {
                                form.base_url.update(cx, |input, cx| {
                                    input.set_value("http://127.0.0.1:11434/v1", window, cx)
                                });
                            }
                        })),
                ),
        ))
        .child(row(
            "Model",
            Some("New native engines; running sessions keep their model"),
            Input::new(&form.model)
                .id("settings-model")
                .aria_label("Model"),
        ))
        .child(row(
            "Subagent model",
            Some("New Flint sessions; same endpoint"),
            Input::new(&form.subagent_model)
                .id("settings-subagent-model")
                .aria_label("Subagent model"),
        ))
        .child(row(
            "Base URL",
            Some("OpenAI-compatible endpoint; new native engines"),
            Input::new(&form.base_url)
                .id("settings-base-url")
                .aria_label("Base URL"),
        ))
        .child(row(
            "API key env",
            Some("Name only; FLINT_API_KEY then OPENAI_API_KEY if blank"),
            Input::new(&form.api_key_env)
                .id("settings-key-env")
                .aria_label("API key environment variable"),
        ))
        .child(row(
            "API key file",
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
                .child(
                    Input::new(&form.api_key_file)
                        .id("settings-key-file")
                        .aria_label("API key file"),
                ),
        ))
        .child(row(
            "JEV judge",
            Some("Smarter guard checks"),
            div()
                .pt(px(7.))
                .child(ui::label(jev_text, size::BASE - 1., jev_color)),
        ))
        .child(row(
            "Approval",
            Some("Default for new native engines; ACP modes are separate"),
            approval_row,
        ))
        .child(row("Reasoning effort", None, effort_row))
        .child(row(
            "Theme",
            None,
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .child(segment("settings-theme", "Dark", true).tab_stop(false))
                .child(ui::label(
                    "Light theme isn't available yet",
                    size::SM,
                    p.text_subtle,
                )),
        ))
        .test_support();
    let sheet = div()
        .id("settings-sheet")
        .w(px(width))
        .max_h(px(height))
        .rounded(px(16.))
        .border_1()
        .border_color(p.border_strong)
        .bg(p.surface)
        .shadow_lg()
        .p(px(20.))
        .flex()
        .flex_col()
        .gap(px(14.))
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
        .child(content)
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .child(
                    Button::new("test-provider-connection")
                        .label(if form.probe.is_some() {
                            "Testing GET /models…"
                        } else {
                            "Test connection (GET /models)"
                        })
                        .disabled(form.probe.is_some())
                        .on_click(cx.listener(|this, _, _, cx| this.test_provider_connection(cx))),
                )
                .when(form.probe.is_some(), |row| {
                    row.child(
                        Button::new("cancel-provider-test")
                            .label("Cancel test")
                            .on_click(cx.listener(|this, _, _, cx| this.cancel_provider_test(cx))),
                    )
                }),
        )
        .when_some(
            form.probe_result
                .as_ref()
                .filter(|_| form.probe_draft.as_ref() == Some(&draft)),
            |sheet, result| {
                sheet.child(
                    div()
                        .id("connection-result")
                        .child(ui::label(result.label(), size::SM, p.text_muted))
                        .test_support(),
                )
            },
        )
        .when_some(form.error.clone(), |sheet, error| {
            sheet.child(
                div()
                    .id("settings-error")
                    .child(ui::label(error, size::SM, p.danger))
                    .test_support(),
            )
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
                .items_center()
                .justify_center()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| this.close_settings(window, cx)),
                )
                .child(sheet.focus_trap("settings-focus-trap", &form.focus)),
        )
        .with_priority(20),
    )
}
