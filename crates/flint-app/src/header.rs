//! The main column's header: the session title (doubles as the window drag
//! area) and a few icon actions.

use gpui_kit::assets::IconName;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::app::FlintApp;
use crate::session::folder_name;
use crate::theme::palette;
use crate::theme::size;
use crate::ui;

pub const HEADER_HEIGHT: f32 = 54.;

pub fn render(app: &FlintApp, window: &mut Window, cx: &mut Context<FlintApp>) -> impl IntoElement {
    let p = palette();
    let session = app.session();
    let has_items = !session.view.items.is_empty();
    let changes = session.view.changes.len();
    let compact = app.chat_width(f32::from(window.viewport_size().width)) < 640.;

    let icon_button = |id: &'static str, icon: IconName, tip: &'static str, active: bool| {
        div()
            .id(id)
            .aria_label(tip)
            .tab_index(0)
            .focus_visible(|style| style.border_1().border_color(p.accent))
            .size(px(34.))
            .rounded(px(9.))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .when(active, |b| b.bg(p.raised))
            .hover(|style| style.bg(p.raised))
            .tooltip(move |window, cx| Tooltip::new(tip).build(window, cx))
            .child(ui::icon(
                icon,
                17.,
                if active { p.text } else { p.text_muted },
            ))
            .test_support()
            .when(id == "changes", |button| {
                button.track_focus(&app.changes_focus)
            })
    };

    // The title takes every pixel the actions leave and only then truncates;
    // the full title is in its tooltip.
    let full_title = session.title();
    let title = div()
        .flex_1()
        .min_w_0()
        .flex()
        .items_center()
        .gap(px(8.))
        .when(has_items, |row| {
            row.child(
                div()
                    .id("session-title")
                    .flex_shrink_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(size::BASE))
                    .font_weight(FontWeight::MEDIUM)
                    .tooltip(move |window, cx| Tooltip::new(full_title.clone()).build(window, cx))
                    .child(session.title())
                    .test_support(),
            )
        })
        .when(!compact, |row| {
            row.child(div().flex_shrink_0().child(ui::label(
                folder_name(&session.workspace),
                size::SM,
                p.text_subtle,
            )))
        })
        .when_some((!compact).then(|| app.branch()).flatten(), |row, branch| {
            row.child(
                div()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap(px(3.))
                    .child(ui::icon(IconName::GitBranch, 11., p.text_subtle))
                    .child(ui::mono(branch, size::XS, p.text_subtle)),
            )
        });

    let actions = div()
        .flex()
        .items_center()
        .gap(px(2.))
        .when(!compact, |actions| {
            actions.child(
                icon_button("reveal", IconName::FolderOpen, "Open in Finder  ⌘⇧R", false)
                    .on_click(cx.listener(|this, _, _, _| this.reveal_workspace())),
            )
        })
        .when(!compact, |actions| {
            actions.child(
                icon_button(
                    "header-terminal",
                    IconName::SquareTerminal,
                    "Terminal  ⌃`",
                    app.terminal.open,
                )
                .on_click(cx.listener(|this, _, window, cx| this.toggle_terminal(window, cx))),
            )
        })
        .child(
            div()
                .relative()
                .child(
                    icon_button(
                        "changes",
                        IconName::FileDiff,
                        "Changes panel  ⌘J",
                        app.changes_open,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.changes_open = !this.changes_open;
                        cx.notify();
                    })),
                )
                .when(changes > 0 && !app.changes_open, |button| {
                    button.child(
                        div()
                            .absolute()
                            .top(px(6.))
                            .right(px(6.))
                            .size(px(7.))
                            .rounded_full()
                            .bg(p.accent),
                    )
                }),
        );

    let bar = div()
        .id("header")
        .h(px(HEADER_HEIGHT))
        .flex_shrink_0()
        .px(px(20.))
        .flex()
        .items_center()
        .justify_between()
        .gap(px(12.))
        .child(crate::docking::handle(crate::docking::Panel::Chat, cx))
        .when(!app.sidebar_visible, |bar| {
            bar.child(
                div()
                    .id("sessions-control")
                    .aria_label("Open sessions")
                    .tab_index(0)
                    .focus_visible(|style| style.border_1().border_color(p.accent))
                    .px(px(9.))
                    .h(px(34.))
                    .rounded(px(8.))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .gap(px(5.))
                    .hover(|style| style.bg(p.raised))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.toggle_session_drawer(window, cx);
                    }))
                    .child(ui::icon(IconName::PanelLeft, 16., p.text_muted))
                    .child(ui::label("Sessions", size::SM, p.text))
                    .test_support(),
            )
        })
        .child(title)
        .child(actions);
    app.drag_region(bar, cx).test_support()
}
