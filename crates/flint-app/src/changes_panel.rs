//! Right panel: the files changed this session. With several files, a list
//! with a selection; for the selected file, its path, net stats, an "Open in
//! editor" action, and one combined diff (original -> current) that scrolls
//! sideways instead of clipping long lines.

use gpui_kit::assets::IconName;
use gpui_kit::component::button::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::app::FlintApp;
use crate::diff;
use crate::header::HEADER_HEIGHT;
use crate::theme::palette;
use crate::theme::size;
use crate::transcript::file_icon;
use crate::ui;

pub fn render(app: &FlintApp, cx: &mut Context<FlintApp>) -> impl IntoElement {
    let p = palette();
    let changes = &app.session().view.changes;
    let (added, removed) = changes
        .iter()
        .fold((0, 0), |(a, r), f| (a + f.added, r + f.removed));
    let selected = app
        .selected_change
        .filter(|&ix| ix < changes.len())
        .or(if changes.is_empty() { None } else { Some(0) });

    let header = div()
        .h(px(HEADER_HEIGHT))
        .flex_shrink_0()
        .px(px(18.))
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
                .child(crate::docking::handle(crate::docking::Panel::Changes, cx))
                .child(ui::icon(IconName::FileDiff, 15., p.text_muted))
                .child(
                    div()
                        .text_size(px(size::BASE))
                        .font_weight(FontWeight::MEDIUM)
                        .child("Changes"),
                )
                .when(!changes.is_empty(), |row| {
                    row.child(ui::label(
                        format!(
                            "{} file{}",
                            changes.len(),
                            if changes.len() == 1 { "" } else { "s" }
                        ),
                        size::SM,
                        p.text_subtle,
                    ))
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

    if changes.is_empty() {
        return div()
            .size_full()
            .flex()
            .flex_col()
            .bg(p.chrome)
            .child(header)
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(8.))
                    .child(ui::icon(IconName::GitCompare, 22., p.text_subtle))
                    .child(ui::label("No changes yet", size::BASE, p.text_muted))
                    .child(ui::label(
                        "Files the agent edits show up here.",
                        size::SM,
                        p.text_subtle,
                    )),
            );
    }

    // A file list only when there is something to choose between.
    let list = (changes.len() > 1).then(|| {
        div()
            .id("changed-files")
            .max_h(px(160.))
            .flex_shrink_0()
            .overflow_y_scroll()
            .py(px(6.))
            .flex()
            .flex_col()
            .gap(px(1.))
            .border_b_1()
            .border_color(p.border)
            .children(changes.iter().enumerate().map(|(ix, file)| {
                let is_selected = selected == Some(ix);
                let (dir, name) = file.path.rsplit_once('/').unwrap_or(("", &file.path));
                div()
                    .id(("change", ix))
                    .aria_label(format!("Review {}", file.path))
                    .tab_index(0)
                    .focus_visible(|style| style.border_1().border_color(p.accent))
                    .mx(px(8.))
                    .px(px(12.))
                    .h(px(38.))
                    .flex_shrink_0()
                    .rounded(px(9.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .cursor_pointer()
                    .when(is_selected, |row| row.bg(p.raised))
                    .when(!is_selected, |row| row.hover(|style| style.bg(p.surface)))
                    .on_click(cx.listener(move |this, _, _, cx| this.select_change(ix, cx)))
                    .child(ui::icon(file_icon(&file.path), 15., p.text_muted))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(ui::mono(name.to_string(), size::SM, p.text).truncate())
                            .when(!dir.is_empty(), |col| {
                                col.child(
                                    ui::label(dir.to_string(), size::XS, p.text_subtle).truncate(),
                                )
                            }),
                    )
                    .child(ui::mono(format!("+{}", file.added), size::XS, p.success))
                    .child(ui::mono(format!("−{}", file.removed), size::XS, p.danger))
                    .test_support()
            }))
            .test_support()
    });

    let detail = selected.and_then(|ix| changes.get(ix)).map(|file| {
        let path = app.session().workspace.join(&file.path);
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(48.))
                    .flex_shrink_0()
                    .px(px(18.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .border_b_1()
                    .border_color(p.border)
                    .child(ui::icon(file_icon(&file.path), 15., p.text_muted))
                    .child(
                        ui::mono(file.path.clone(), size::SM, p.text)
                            .flex_1()
                            .min_w_0()
                            .truncate(),
                    )
                    .child(ui::mono(format!("+{}", file.added), size::XS, p.success))
                    .child(ui::mono(format!("−{}", file.removed), size::XS, p.danger))
                    .child(
                        div()
                            .id("open-in-editor")
                            .child(
                                Button::new("open-editor")
                                    .outline()
                                    .icon(IconName::ExternalLink)
                                    .tooltip("Open in editor")
                                    .on_click(move |_, _, _| open_in_editor(&path)),
                            )
                            .test_support(),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .child(diff::render_virtual(app, file)),
            )
    });

    div()
        .size_full()
        .flex()
        .flex_col()
        .bg(p.chrome)
        .child(header)
        .children(list)
        .children(detail)
}

/// `$EDITOR <path>` when set (a GUI editor command such as `code` or `zed`),
/// otherwise the system default app via `open`.
pub fn open_in_editor(path: &std::path::Path) {
    let editor = std::env::var("EDITOR")
        .ok()
        .filter(|e| !e.trim().is_empty());
    let spawned = editor.and_then(|editor| {
        let mut parts = editor.split_whitespace();
        let program = parts.next()?;
        std::process::Command::new(program)
            .args(parts)
            .arg(path)
            .spawn()
            .ok()
    });
    if spawned.is_none() {
        std::process::Command::new("open").arg(path).spawn().ok();
    }
}
