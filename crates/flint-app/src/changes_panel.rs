//! Right panel: files changed this session, and the full diff of the selected one.

use gpui_kit::assets::IconName;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::app::FlintApp;
use crate::diff;
use crate::theme::palette;
use crate::theme::size;
use crate::ui;

pub fn render(app: &FlintApp, cx: &mut Context<FlintApp>) -> impl IntoElement {
    let p = palette();
    let changes = &app.session().view.changes;
    let (added, removed) = changes
        .iter()
        .fold((0, 0), |(a, r), f| (a + f.added, r + f.removed));

    let header = div()
        .h(px(36.))
        .flex_shrink_0()
        .px(px(14.))
        .flex()
        .items_center()
        .justify_between()
        .border_b_1()
        .border_color(p.border)
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(ui::icon(IconName::FileDiff, 13., p.text_muted))
                .child(
                    div()
                        .text_size(px(size::BASE))
                        .font_weight(FontWeight::MEDIUM)
                        .child("Changes"),
                )
                .when(!changes.is_empty(), |row| {
                    row.child(ui::pill(changes.len().to_string(), p.text_muted, p.raised))
                }),
        )
        .when(!changes.is_empty(), |row| {
            row.child(
                div()
                    .flex()
                    .gap(px(6.))
                    .child(ui::mono(format!("+{added}"), size::SM, p.success))
                    .child(ui::mono(format!("−{removed}"), size::SM, p.danger)),
            )
        });

    let files = changes.iter().enumerate().map(|(ix, file)| {
        let selected = app.selected_change == Some(ix);
        let (dir, name) = split_path(&file.path);
        div()
            .id(("change", ix))
            .mx(px(6.))
            .px(px(8.))
            .h(px(30.))
            .rounded(px(6.))
            .flex()
            .items_center()
            .gap(px(8.))
            .cursor_pointer()
            .when(selected, |row| row.bg(p.raised))
            .when(!selected, |row| row.hover(|style| style.bg(p.surface)))
            .on_click(cx.listener(move |this, _, _, cx| this.select_change(ix, cx)))
            .child(ui::icon(
                if file.created {
                    IconName::FilePlus
                } else {
                    IconName::FileCode
                },
                13.,
                p.text_muted,
            ))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_baseline()
                    .gap(px(6.))
                    .overflow_hidden()
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_size(px(size::BASE))
                            .text_color(p.text)
                            .child(name.to_string()),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(px(size::XS))
                            .text_color(p.text_subtle)
                            .child(dir.to_string()),
                    ),
            )
            .child(ui::mono(format!("+{}", file.added), size::XS, p.success))
            .child(ui::mono(format!("−{}", file.removed), size::XS, p.danger))
    });

    let detail = app
        .selected_change
        .and_then(|ix| changes.get(ix))
        .map(|file| {
            let diffs = file.diffs.iter().enumerate().map(|(n, unified)| {
                let (body, _) = diff::render(unified, usize::MAX);
                div()
                    .when(n > 0, |d| d.border_t_1().border_color(p.border))
                    .child(body)
            });
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .border_t_1()
                .border_color(p.border)
                .child(
                    div()
                        .h(px(30.))
                        .px(px(14.))
                        .flex()
                        .items_center()
                        .bg(p.surface)
                        .border_b_1()
                        .border_color(p.border)
                        .child(ui::mono(file.path.clone(), size::SM, p.text_muted)),
                )
                .child(
                    div()
                        .id("change-diff")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .children(diffs),
                )
        });

    let empty = div()
        .flex_1()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(8.))
        .child(ui::icon(IconName::GitCompare, 20., p.text_subtle))
        .child(ui::label("No changes yet", size::BASE, p.text_muted))
        .child(ui::label(
            "Files the agent edits show up here.",
            size::SM,
            p.text_subtle,
        ));

    div()
        .size_full()
        .flex()
        .flex_col()
        .bg(p.chrome)
        .border_l_1()
        .border_color(p.border)
        .child(header)
        .when(changes.is_empty(), |panel| panel.child(empty))
        .when(!changes.is_empty(), |panel| {
            panel
                .child(
                    div()
                        .py(px(6.))
                        .flex()
                        .flex_col()
                        .gap(px(1.))
                        .children(files),
                )
                .children(detail)
        })
}

fn split_path(path: &str) -> (&str, &str) {
    match path.rsplit_once('/') {
        Some((dir, name)) => (dir, name),
        None => ("", path),
    }
}
