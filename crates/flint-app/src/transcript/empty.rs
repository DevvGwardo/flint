//! A coastal home dashboard, sized to its chat panel rather than the screen.

use std::sync::{Arc, LazyLock};

use flint_agent::{AgentKind, ApprovalMode};
use gpui_kit::assets::IconName;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::app::FlintApp;
use crate::settings::KeyStatus;
use crate::theme::{DISPLAY_FONT, palette, size};
use crate::ui;

static COAST: LazyLock<Arc<Image>> = LazyLock::new(|| {
    Arc::new(Image::from_bytes(
        ImageFormat::Jpeg,
        include_bytes!("../../../../assets/welcome-coast.jpg").to_vec(),
    ))
});

fn coast(height: f32) -> impl IntoElement {
    let p = palette();
    div()
        .id("welcome-coast")
        .absolute()
        .top_0()
        .left_0()
        .w_full()
        .h(px(height))
        .overflow_hidden()
        .child(
            img(ImageSource::Image(COAST.clone()))
                .size_full()
                .object_fit(ObjectFit::Cover),
        )
        .child(div().absolute().inset_0().bg(linear_gradient(
            180.,
            linear_color_stop(p.bg.opacity(0.), 0.),
            linear_color_stop(p.bg, 1.),
        )))
        .test_support()
}

struct Suggestion {
    title: &'static str,
    detail: &'static str,
    prompt: &'static str,
    icon: IconName,
}

const SUGGESTIONS: &[Suggestion] = &[
    Suggestion {
        title: "Fix the failing tests",
        detail: "Find the cause. Get back to green.",
        prompt: "Fix the failing tests",
        icon: IconName::FlaskConical,
    },
    Suggestion {
        title: "Explore this codebase",
        detail: "Understand how the pieces fit together.",
        prompt: "Explain this codebase",
        icon: IconName::ListTree,
    },
    Suggestion {
        title: "Review my changes",
        detail: "A fresh pair of eyes before you ship.",
        prompt: "Review my uncommitted changes",
        icon: IconName::GitCompare,
    },
    Suggestion {
        title: "Make it more resilient",
        detail: "Add validation where it matters.",
        prompt: "Add input validation",
        icon: IconName::ShieldCheck,
    },
];

const GENERAL_SUGGESTIONS: &[Suggestion] = &[
    Suggestion {
        title: "Plan my day",
        detail: "Turn a busy day into a clear plan.",
        prompt: "Help me plan my day",
        icon: IconName::ListTree,
    },
    Suggestion {
        title: "Make something click",
        detail: "Break down a topic, one idea at a time.",
        prompt: "Explain a topic simply",
        icon: IconName::Lightbulb,
    },
    Suggestion {
        title: "Find the right words",
        detail: "A first draft, a rewrite, a new angle.",
        prompt: "Help me write something",
        icon: IconName::SquarePen,
    },
    Suggestion {
        title: "Think it through",
        detail: "Weigh the options and find a next step.",
        prompt: "Think through a decision with me",
        icon: IconName::GitCompare,
    },
];

pub fn render(app: &FlintApp, window: &mut Window, cx: &mut Context<FlintApp>) -> impl IntoElement {
    let p = palette();
    let session = app.session();
    let workspace = session.workspace_label();
    let general = session.general;
    let native = session.agent == AgentKind::Flint;
    let panel = app.popover_room.area.get();
    let width = panel.map_or_else(
        || app.chat_width(f32::from(window.viewport_size().width)),
        |area| f32::from(area.size.width),
    );
    let height = panel.map_or(
        f32::from(window.viewport_size().height) - crate::header::HEADER_HEIGHT - 36.,
        |area| f32::from(area.size.height),
    );
    let compact = height < 600.;
    let columns = if width >= 640. { 2 } else { 1 };
    let roomy = width >= 900. && height >= 700.;
    let hero_height = if compact {
        154.
    } else {
        (height * 0.38).clamp(220., 310.)
    };

    let (credential, credential_detail, missing) = if native {
        match app.key_status_cache.borrow_mut().get(
            &app.settings,
            app.key_path.as_deref(),
            &app.key_sources,
            std::time::Instant::now(),
        ) {
            KeyStatus::Found(source) => (
                "Credential available".to_string(),
                format!("Credential available from {source}"),
                false,
            ),
            KeyStatus::Missing(source) => (
                "Add an API key to get started".to_string(),
                format!("Credential missing; set {source} or open Settings"),
                true,
            ),
        }
    } else {
        (
            format!("Credentials managed by {}", session.agent.label()),
            "Credentials are checked when the ACP agent starts.".to_string(),
            false,
        )
    };
    let permission = if native {
        match session.native_approval.unwrap_or(app.approval) {
            ApprovalMode::Auto => "Native auto-run",
            ApprovalMode::AskForChanges => "Ask before changes",
        }
    } else {
        "Agent-managed permissions"
    };

    let folder_chip = div()
        .id("welcome-folder")
        .role(gpui_kit::Role::Button)
        .aria_label("Choose workspace")
        .tab_index(0)
        .focus_visible(|style| style.border_color(p.accent))
        .max_w_full()
        .min_w_0()
        .flex_shrink_1()
        .px(px(9.))
        .h(px(30.))
        .rounded(px(9.))
        .bg(p.surface)
        .flex()
        .items_center()
        .gap(px(8.))
        .cursor_pointer()
        .border_1()
        .border_color(p.border)
        .hover(|style| style.bg(p.raised).border_color(p.border_strong))
        .on_click(cx.listener(|this, _, _, cx| this.toggle_project_menu(cx)))
        .child(
            ui::icon(
                if general {
                    IconName::Sparkles
                } else {
                    IconName::Folder
                },
                16.,
                p.accent,
            )
            .flex_shrink_0(),
        )
        .child(
            ui::label(workspace.clone(), size::SM, p.text)
                .min_w_0()
                .truncate(),
        )
        .child(ui::icon(IconName::ChevronDown, 13., p.text_subtle).flex_shrink_0())
        .test_support();

    let suggestions = if general {
        GENERAL_SUGGESTIONS
    } else {
        SUGGESTIONS
    };
    let card_cx = &*cx;
    let cards: Vec<_> = suggestions
        .chunks(columns)
        .enumerate()
        .map(|(row, entries)| {
            div()
                .w_full()
                .flex()
                .gap(px(10.))
                .children(entries.iter().enumerate().map(move |(col, suggestion)| {
                    let prompt = suggestion.prompt;
                    div()
                        .id(("suggestion", row * columns + col))
                        .role(gpui_kit::Role::Button)
                        .aria_label(prompt)
                        .tab_index(0)
                        .focus_visible(|style| style.border_color(p.accent))
                        .flex_1()
                        .min_w_0()
                        .h(px(if compact { 40. } else { 48. }))
                        .px(px(12.))
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .rounded(px(9.))
                        .border_1()
                        .border_color(p.border)
                        .bg(p.surface.opacity(0.6))
                        .cursor_pointer()
                        .hover(|style| style.bg(p.surface).border_color(p.accent.opacity(0.4)))
                        .tooltip(move |window, cx| {
                            Tooltip::new(suggestion.detail).build(window, cx)
                        })
                        .on_click(card_cx.listener(move |this, _, window, cx| {
                            this.welcome_scroll.set_offset(Point::default());
                            this.composer.update(cx, |state, cx| {
                                state.set_value(prompt, window, cx);
                                state.focus(window, cx);
                            });
                        }))
                        .child(
                            div()
                                .size(px(24.))
                                .flex_shrink_0()
                                .rounded(px(9.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(ui::icon(suggestion.icon, 18., p.accent)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .gap(px(4.))
                                .child(
                                    ui::label(suggestion.title, size::SM, p.text_muted)
                                        .font_weight(FontWeight::MEDIUM)
                                        .truncate(),
                                ),
                        )
                        .child(ui::icon(IconName::ArrowUpRight, 15., p.text_subtle).flex_shrink_0())
                        .test_support()
                }))
        })
        .collect();

    let notice = (!app.settings.tip_dismissed && roomy).then(|| {
        div()
            .id("welcome-tip")
            .w_full()
            .px(px(6.))
            .flex()
            .items_center()
            .gap(px(10.))
            .child(ui::icon(IconName::ShieldCheck, 17., p.accent).flex_shrink_0())
            .child(
                ui::label("Hooter's native agent can check its work.", size::SM, p.text_muted)
                    .id("welcome-tip-copy")
                    .flex_1()
                    .min_w_0()
                    .tooltip(|window, cx| {
                        Tooltip::new(
                            "The native agent can nudge unverified edits. A check is only reported \
                             when the agent actually runs it. Other agents use their own workflows.",
                        ).build(window, cx)
                    }),
            )
            .child(
                div()
                    .id("tip-settings")
                    .role(gpui_kit::Role::Button)
                    .aria_label("Open settings")
                    .tab_index(0)
                    .focus_visible(|style| style.border_1().border_color(p.accent))
                    .flex_shrink_0()
                    .cursor_pointer()
                    .text_size(px(size::SM))
                    .text_color(p.accent)
                    .hover(|style| style.underline())
                    .on_click(cx.listener(|this, _, window, cx| this.open_settings(window, cx)))
                    .child("Settings")
                    .test_support(),
            )
            .child(
                div()
                    .id("dismiss-tip")
                    .role(gpui_kit::Role::Button)
                    .aria_label("Dismiss tip")
                    .tab_index(0)
                    .focus_visible(|style| style.border_1().border_color(p.accent))
                    .size(px(28.))
                    .flex_shrink_0()
                    .rounded(px(6.))
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
        .track_scroll(&app.welcome_scroll)
        .bg(p.bg)
        .child(coast(hero_height))
        .child(
            div()
                .relative()
                .w_full()
                .px(px(if width < 600. { 20. } else { 40. }))
                .pt(px(if compact { 24. } else { hero_height * 0.24 }))
                .pb(px(32.))
                .flex()
                .flex_col()
                .items_center()
                .child(
                    div()
                        .id("welcome-workbench")
                        .w_full()
                        .max_w(px(crate::transcript::WELCOME_WIDTH))
                        .flex_shrink_0()
                        .flex()
                        .flex_col()
                        .gap(px(if compact { 8. } else { 12. }))
                        .child(
                            div()
                                .w_full()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .items_center()
                                .gap(px(9.))
                                .when(!compact, |hero| {
                                    hero.child(
                                        div()
                                            .id("welcome-logo")
                                            .flex()
                                            .items_center()
                                            .gap(px(7.))
                                            .child(crate::brand::mark(20.))
                                            .child(ui::label("HOOTER / HOME", 11., p.text_muted))
                                            .test_support(),
                                    )
                                })
                                .child(
                                    div()
                                        .id("welcome-heading")
                                        .font_family(DISPLAY_FONT)
                                        .text_size(px(if compact { 24. } else { 30. }))
                                        .line_height(px(if compact { 32. } else { 40. }))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(p.text)
                                        .text_center()
                                        .child(if general {
                                            "What would you like to do?"
                                        } else {
                                            "What should your agents work on?"
                                        })
                                        .test_support(),
                                ),
                        )
                        .child(
                            div()
                                .id("welcome-task-tabs")
                                .w_full()
                                .min_w_0()
                                .flex()
                                .items_center()
                                .gap(px(12.))
                                .child(
                                    ui::label("New task", size::XS, p.text)
                                        .pb(px(6.))
                                        .border_b_1()
                                        .border_color(p.info)
                                        .flex_shrink_0(),
                                )
                                .child(
                                    div()
                                        .id("welcome-running-agents")
                                        .role(gpui_kit::Role::Button)
                                        .aria_label("Show working sessions")
                                        .tab_index(0)
                                        .focus_visible(|style| {
                                            style.border_1().border_color(p.info)
                                        })
                                        .cursor_pointer()
                                        .text_size(px(size::XS))
                                        .text_color(p.text_subtle)
                                        .hover(|style| style.text_color(p.text))
                                        .on_click(cx.listener(|app, _, window, cx| {
                                            app.open_sessions(window, cx);
                                            app.set_filter(
                                                crate::app::SessionFilter::Only(
                                                    crate::session::Bucket::Working,
                                                ),
                                                cx,
                                            );
                                        }))
                                        .child(format!(
                                            "Working sessions  {}",
                                            app.sessions
                                                .iter()
                                                .filter(|session| session.status().bucket()
                                                    == crate::session::Bucket::Working)
                                                .count()
                                        ))
                                        .test_support(),
                                )
                                .child(div().flex_1())
                                .child(folder_chip)
                                .test_support(),
                        )
                        .child(crate::composer::render(app, window, cx))
                        .child(
                            div()
                                .id("welcome-readiness")
                                .w_full()
                                .flex()
                                .flex_wrap()
                                .items_center()
                                .gap(px(8.))
                                .child(ui::icon(
                                    if missing {
                                        IconName::CircleAlert
                                    } else {
                                        IconName::ShieldCheck
                                    },
                                    14.,
                                    if missing { p.warning } else { p.text_subtle },
                                ))
                                .child(
                                    ui::label(
                                        credential,
                                        size::SM,
                                        if missing { p.warning } else { p.text_subtle },
                                    )
                                    .id("welcome-credential")
                                    .tooltip(
                                        move |window, cx| {
                                            Tooltip::new(credential_detail.clone())
                                                .build(window, cx)
                                        },
                                    ),
                                )
                                .child(ui::label("·", size::SM, p.text_subtle))
                                .child(ui::label(permission, size::SM, p.text_subtle))
                                .when(missing, |row| {
                                    row.child(
                                        div()
                                            .id("welcome-credential-settings")
                                            .role(gpui_kit::Role::Button)
                                            .aria_label("Configure API key in Settings")
                                            .tab_index(0)
                                            .focus_visible(|style| {
                                                style.border_1().border_color(p.accent)
                                            })
                                            .text_size(px(size::SM))
                                            .text_color(p.accent)
                                            .cursor_pointer()
                                            .hover(|style| style.underline())
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.open_settings(window, cx)
                                            }))
                                            .child("Open settings")
                                            .test_support(),
                                    )
                                })
                                .test_support(),
                        )
                        .children(notice)
                        .when(!compact, |workbench| {
                            workbench.child(crate::welcome_dashboard::render(app, columns, cx))
                        })
                        .child(
                            div()
                                .id("welcome-suggestions")
                                .w_full()
                                .flex()
                                .flex_col()
                                .gap(px(if compact { 8. } else { 10. }))
                                .children(cards)
                                .test_support(),
                        )
                        .when(general && compact, |hero| {
                            hero.child(ui::label(
                                "No project needed. Files stay in your private working folder.",
                                size::XS,
                                p.text_subtle,
                            ))
                        })
                        .when(!general && !session.workspace.is_dir(), |hero| {
                            hero.child(ui::label(
                                format!(
                                    "{workspace} is unavailable. Choose another folder to continue."
                                ),
                                size::SM,
                                p.warning,
                            ))
                        })
                        .test_support(),
                ),
        )
        .test_support()
}
