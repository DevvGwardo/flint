//! Project-first welcome workspace with a composer and suggested tasks.

use gpui_kit::assets::IconName;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::app::FlintApp;
use crate::session::folder_name;
use crate::settings::KeyStatus;
use crate::theme::palette;
use crate::theme::size;
use crate::ui;

const SUGGESTIONS: &[(&str, IconName)] = &[
    ("Fix the failing tests", IconName::FlaskConical),
    ("Explain this codebase", IconName::ListTree),
    ("Review my uncommitted changes", IconName::GitCompare),
    ("Add input validation", IconName::ShieldCheck),
];

pub fn render(app: &FlintApp, window: &mut Window, cx: &mut Context<FlintApp>) -> impl IntoElement {
    let p = palette();
    let workspace = folder_name(&app.session().workspace);
    let readiness = {
        let agent = app.session().agent;
        let credential = if agent == flint_agent::AgentKind::Flint {
            match app
                .settings
                .key_status(app.key_path.as_deref(), &app.key_sources)
            {
                KeyStatus::Found(source) => format!("Credential: available ({source})"),
                KeyStatus::Missing(source) => {
                    format!("Credential: missing; set {source} or open Settings")
                }
            }
        } else {
            "Credential: managed by the ACP agent; checked on start".to_string()
        };
        let permission = if agent == flint_agent::AgentKind::Flint {
            match app.approval {
                flint_agent::ApprovalMode::Auto => "Native auto-run: selected for new engines",
                flint_agent::ApprovalMode::AskForChanges => {
                    "Native permission: ask before commands and edits"
                }
            }
        } else {
            "Permission: controlled by the ACP agent's mode"
        };
        (credential, permission)
    };
    let composer = crate::composer::render(app, window, cx);
    let chips = SUGGESTIONS.iter().enumerate().map(|(ix, (text, icon))| {
        let text = *text;
        div()
            .id(("suggestion", ix))
            .aria_label(text)
            .tab_index(0)
            .focus_visible(|style| style.border_color(p.accent))
            .max_w_full()
            .min_w_0()
            .h(px(40.))
            .px(px(16.))
            .flex()
            .items_center()
            .gap(px(9.))
            .rounded(px(8.))
            .border_1()
            .border_color(p.border)
            .cursor_pointer()
            .hover(|style| style.bg(p.surface).border_color(p.border_strong))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.composer.update(cx, |state, cx| {
                    state.set_value(text, window, cx);
                    state.focus(window, cx);
                });
            }))
            .child(ui::icon(*icon, 15., p.text_subtle))
            .child(ui::label(text, size::BASE - 1., p.text_muted).truncate())
    });

    // The tip floats over the top-right corner, clear of the centered hero
    // only on wide windows; on narrow ones it would sit on the headline.
    let roomy = window.viewport_size().width >= px(1200.);
    // On short windows the mark goes, so the greeting never crowds the header.
    let show_logo = window.viewport_size().height >= px(720.);
    let folder_chip = div()
        .id("welcome-folder")
        .aria_label("Choose workspace")
        .tab_index(0)
        .focus_visible(|style| style.border_color(p.accent))
        .max_w_full()
        .min_w_0()
        .px(px(12.))
        .h(px(38.))
        .rounded(px(8.))
        .flex()
        .items_center()
        .gap(px(6.))
        .cursor_pointer()
        .border_1()
        .border_color(p.border_strong)
        .hover(|style| style.bg(p.surface))
        .on_click(cx.listener(|this, _, _, cx| this.toggle_project_menu(cx)))
        .child(ui::icon(IconName::Folder, 16., p.accent))
        .child(
            ui::label(workspace.clone(), size::BASE, p.text)
                .min_w_0()
                .truncate(),
        )
        .child(ui::icon(IconName::ChevronDown, 14., p.text_subtle))
        .test_support();
    let notice = (!app.settings.tip_dismissed && roomy).then(|| {
        div()
            .id("welcome-tip")
            .absolute()
            .top(px(16.))
            .right(px(20.))
            .w(px(340.))
            .p(px(16.))
            .rounded(px(14.))
            .border_1()
            .border_color(p.border_strong)
            .bg(p.surface)
            .shadow_lg()
            .flex()
            .gap(px(10.))
            .child(ui::icon(IconName::Lightbulb, 17., p.accent))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(ui::label("flint checks its own work", size::BASE, p.text))
                    .child(ui::label(
                        "Flint's native agent can nudge unverified edits. A check is only reported \
                         when the agent actually runs it. Other agents use their own workflows.",
                        size::SM,
                        p.text_subtle,
                    ))
                    .child(
                        div()
                            .id("tip-settings")
                            .cursor_pointer()
                            .text_size(px(size::SM))
                            .text_color(p.accent)
                            .hover(|s| s.underline())
                            .on_click(
                                cx.listener(|this, _, window, cx| this.open_settings(window, cx)),
                            )
                            .child("Open settings"),
                    ),
            )
            .child(
                div()
                    .id("dismiss-tip")
                    .size(px(28.))
                    .flex_shrink_0()
                    .rounded(px(5.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .hover(|style| style.bg(p.raised))
                    .on_click(cx.listener(|this, _, _, cx| this.dismiss_tip(cx)))
                    .child(ui::icon(IconName::X, 14., p.text_subtle))
                    .test_support(),
            )
            .test_support()
    });

    div()
        .id("welcome-content")
        .relative()
        .size_full()
        .min_h_0()
        .overflow_y_scroll()
        .bg(p.bg)
        .flex()
        .flex_col()
        .items_center()
        .when(window.viewport_size().height >= px(700.), |col| {
            col.justify_center()
        })
        .when(window.viewport_size().height < px(700.), |col| {
            col.justify_start()
        })
        .px(px(if window.viewport_size().width < px(600.) {
            16.
        } else {
            32.
        }))
        .py(px(24.))
        .child(
            div()
                .w_full()
                .max_w(px(760.))
                .flex()
                .flex_col()
                .items_start()
                .gap(px(16.))
                .when(show_logo, |hero| {
                    hero.child(
                        div()
                            .id("welcome-logo")
                            .size(px(48.))
                            .rounded(px(13.))
                            .bg(p.accent_soft)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(ui::icon(IconName::Flame, 24., p.accent))
                            .test_support(),
                    )
                })
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .items_start()
                        .w_full()
                        .gap(px(12.))
                        .child(
                            div()
                                .text_size(px(24.))
                                .line_height(px(30.))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(p.text)
                                .child("New session"),
                        )
                        // The folder is a chip: click it to pick another project.
                        .child(div().w_full().min_w_0().child(folder_chip)),
                )
                .child(composer)
                .child(
                    div()
                        .id("welcome-readiness")
                        .flex()
                        .flex_col()
                        .w_full()
                        .items_start()
                        .gap(px(3.))
                        .child(ui::label(
                            format!(
                                "Workspace: {} · Agent: {}",
                                if app.session().workspace.is_dir() {
                                    workspace.clone()
                                } else {
                                    "folder unavailable; choose a folder".into()
                                },
                                app.session().agent.label(),
                            ),
                            size::SM,
                            p.text_muted,
                        ))
                        .child(ui::label(readiness.0, size::SM, p.text_muted))
                        .child(ui::label(readiness.1, size::SM, p.text_muted))
                        .test_support(),
                )
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .w_full()
                        .justify_start()
                        .gap(px(10.))
                        .children(chips),
                ),
        )
        .children(notice)
}
