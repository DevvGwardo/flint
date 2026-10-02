//! Left sidebar: workspace switcher, session list, new-session button.

use gpui_kit::assets::IconName;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::app::FlintApp;
use crate::app::NewSession;
use crate::app::OpenWorkspace;
use crate::engine;
use crate::theme::palette;
use crate::theme::size;
use crate::ui;

pub fn render(app: &FlintApp, cx: &mut Context<FlintApp>) -> impl IntoElement {
    let p = palette();

    let workspace = div()
        .id("workspace-switcher")
        .mx(px(8.))
        .mt(px(10.))
        .px(px(8.))
        .py(px(7.))
        .rounded(px(7.))
        .border_1()
        .border_color(p.border)
        .bg(p.surface)
        .cursor_pointer()
        .hover(|style| style.bg(p.raised))
        .on_click(cx.listener(|_, _, window, cx| {
            window.dispatch_action(Box::new(OpenWorkspace), cx);
        }))
        .flex()
        .items_center()
        .gap(px(8.))
        .child(
            div()
                .size(px(22.))
                .rounded(px(5.))
                .bg(p.accent_soft)
                .flex()
                .items_center()
                .justify_center()
                .child(ui::icon(IconName::FolderOpen, 12., p.accent)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(
                    div()
                        .text_size(px(size::BASE))
                        .font_weight(FontWeight::MEDIUM)
                        .truncate()
                        .child(
                            app.workspace
                                .file_name()
                                .map(|n| n.to_string_lossy().to_string())
                                .unwrap_or_default(),
                        ),
                )
                .child(
                    div()
                        .text_size(px(size::XS))
                        .text_color(p.text_subtle)
                        .truncate()
                        .child(engine::display_path(&app.workspace)),
                ),
        )
        .child(ui::icon(IconName::ChevronsUpDown, 12., p.text_subtle));

    let new_session = div()
        .id("new-session")
        .mx(px(8.))
        .mt(px(8.))
        .px(px(8.))
        .h(px(28.))
        .rounded(px(6.))
        .cursor_pointer()
        .hover(|style| style.bg(p.raised))
        .on_click(cx.listener(|_, _, window, cx| {
            window.dispatch_action(Box::new(NewSession), cx);
        }))
        .flex()
        .items_center()
        .gap(px(8.))
        .child(ui::icon(IconName::SquarePen, 13., p.text_muted))
        .child(
            div()
                .flex_1()
                .child(ui::label("New session", size::BASE, p.text_muted)),
        )
        .child(ui::key_hint("⌘N"));

    let now = std::time::Instant::now();
    let sessions = app
        .sessions
        .iter()
        .enumerate()
        .rev()
        .filter(|(ix, s)| !s.view.items.is_empty() || *ix == app.active)
        .map(|(ix, session)| {
            let active = ix == app.active;
            let running = session.view.running;
            div()
                .id(("session", ix))
                .mx(px(8.))
                .px(px(8.))
                .h(px(30.))
                .rounded(px(6.))
                .flex()
                .items_center()
                .gap(px(8.))
                .cursor_pointer()
                .when(active, |row| row.bg(p.raised))
                .when(!active, |row| row.hover(|style| style.bg(p.surface)))
                .on_click(cx.listener(move |this, _, _, cx| this.select_session(ix, cx)))
                .child(if running {
                    div()
                        .size(px(6.))
                        .rounded_full()
                        .bg(p.accent)
                        .into_any_element()
                } else {
                    ui::icon(
                        IconName::MessageSquare,
                        12.,
                        if active { p.text_muted } else { p.text_subtle },
                    )
                    .into_any_element()
                })
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px(size::BASE))
                        .text_color(if active { p.text } else { p.text_muted })
                        .child(session.title()),
                )
                .child(ui::label(
                    ui::ago(now.saturating_duration_since(session.created)),
                    size::XS,
                    p.text_subtle,
                ))
        });

    div()
        .w(px(252.))
        .h_full()
        .flex_shrink_0()
        .flex()
        .flex_col()
        .bg(p.chrome)
        .border_r_1()
        .border_color(p.border)
        .child(workspace)
        .child(new_session)
        .child(
            div()
                .px(px(16.))
                .pt(px(16.))
                .pb(px(6.))
                .text_size(px(size::XS))
                .font_weight(FontWeight::MEDIUM)
                .text_color(p.text_subtle)
                .child("SESSIONS"),
        )
        .child(
            div()
                .id("session-list")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap(px(1.))
                .children(sessions),
        )
        .child(
            div()
                .px(px(16.))
                .py(px(10.))
                .border_t_1()
                .border_color(p.border)
                .flex()
                .items_center()
                .gap(px(8.))
                .child(ui::icon(IconName::ShieldCheck, 12., p.text_subtle))
                .child(ui::label(
                    "flash harness · guard on",
                    size::XS,
                    p.text_subtle,
                )),
        )
}
