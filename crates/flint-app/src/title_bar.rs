//! Title bar: workspace + branch on the left, model and panel toggles on the right.

use gpui_kit::assets::IconName;
use gpui_kit::component::TitleBar;
use gpui_kit::component::button::*;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::app::FlintApp;
use crate::app::TogglePalette;
use crate::theme::palette;
use crate::theme::size;
use crate::ui;

pub fn render(app: &FlintApp, cx: &mut Context<FlintApp>) -> impl IntoElement {
    let p = palette();
    let workspace_name = app
        .workspace
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| app.workspace.display().to_string());

    let left = div()
        .flex()
        .items_center()
        .gap(px(10.))
        .child(div().flex().items_center().gap(px(6.)).child(ui::icon(
            IconName::Flame,
            14.,
            p.accent,
        )))
        .child(
            div().flex().items_center().gap(px(6.)).child(
                div()
                    .text_size(px(size::BASE))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(p.text)
                    .child(workspace_name),
            ),
        )
        .when_some(app.branch.clone(), |row, branch| {
            row.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .px(px(6.))
                    .h(px(20.))
                    .rounded(px(5.))
                    .bg(p.surface)
                    .border_1()
                    .border_color(p.border)
                    .child(ui::icon(IconName::GitBranch, 12., p.text_muted))
                    .child(ui::mono(branch, size::XS, p.text_muted)),
            )
        });

    let toggle = |id: &'static str, icon: IconName, active: bool, tip: &'static str| {
        Button::new(id)
            .ghost()
            .xsmall()
            .icon(ui::icon(
                icon,
                14.,
                if active { p.text } else { p.text_subtle },
            ))
            .tooltip(tip)
    };

    let right = div()
        .flex()
        .items_center()
        .gap(px(4.))
        .child(
            div()
                .id("palette-hint")
                .flex()
                .items_center()
                .gap(px(8.))
                .px(px(8.))
                .h(px(22.))
                .mr(px(6.))
                .rounded(px(6.))
                .border_1()
                .border_color(p.border)
                .bg(p.surface)
                .cursor_pointer()
                .hover(|style| style.bg(p.raised))
                .on_click(cx.listener(|_, _, window, cx| {
                    window.dispatch_action(Box::new(TogglePalette), cx);
                }))
                .child(ui::icon(IconName::Search, 12., p.text_subtle))
                .child(ui::label("Search commands", size::SM, p.text_subtle))
                .child(ui::key_hint("⌘K")),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .px(px(8.))
                .h(px(22.))
                .rounded(px(6.))
                .bg(p.accent_soft)
                .child(div().size(px(6.)).rounded_full().bg(p.accent))
                .child(ui::mono(app.model.clone(), size::XS, p.accent)),
        )
        .child(div().w(px(6.)))
        .child(
            toggle(
                "toggle-sidebar",
                IconName::PanelLeft,
                app.sidebar_open,
                "Toggle sidebar  ⌘B",
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.sidebar_open = !this.sidebar_open;
                cx.notify();
            })),
        )
        .child(
            toggle(
                "toggle-changes",
                IconName::PanelRight,
                app.changes_open,
                "Toggle changes  ⌘J",
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.changes_open = !this.changes_open;
                cx.notify();
            })),
        );

    TitleBar::new()
        .bg(p.chrome)
        .border_b_1()
        .border_color(p.border)
        .child(
            div()
                .flex()
                .flex_1()
                .items_center()
                .justify_between()
                .pr(px(10.))
                .child(left)
                .child(right),
        )
}
