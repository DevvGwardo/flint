//! The empty state: the flint mark, a large two-line greeting, the composer
//! centered on the page, and suggestion chips, over a faint ember glow.

use gpui_kit::assets::IconName;
use gpui_kit::*;

use crate::app::FlintApp;
use crate::session::folder_name;
use crate::theme::palette;
use crate::theme::size;
use crate::ui;

const SUGGESTIONS: &[(&str, IconName)] = &[
    ("Fix the failing tests", IconName::FlaskConical),
    ("Explain this codebase", IconName::ListTree),
    ("Review my uncommitted changes", IconName::GitCompare),
    ("Add input validation", IconName::ShieldCheck),
];

pub fn render(app: &FlintApp, cx: &mut Context<FlintApp>) -> impl IntoElement {
    let p = palette();
    let workspace = folder_name(&app.session().workspace);
    let composer = crate::composer::render(app, cx);
    let chips = SUGGESTIONS.iter().enumerate().map(|(ix, (text, icon))| {
        let text = *text;
        div()
            .id(("suggestion", ix))
            .h(px(40.))
            .px(px(16.))
            .flex()
            .items_center()
            .gap(px(9.))
            .rounded_full()
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
            .child(ui::label(text, size::BASE - 1., p.text_muted))
    });

    let notice = (!app.settings.tip_dismissed).then(|| {
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
                        "Before it finishes, it re-runs your tests and stops itself from looping. \
                         Model, endpoint and safety options live in Settings.",
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
        .relative()
        .size_full()
        .bg(linear_gradient(
            180.,
            linear_color_stop(hsla(22. / 360., 0.5, 0.12, 1.), 0.),
            linear_color_stop(p.bg, 0.55),
        ))
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .px(px(32.))
        .pb(px(60.))
        .child(
            div()
                .w_full()
                .max_w(px(760.))
                .flex()
                .flex_col()
                .items_center()
                .gap(px(32.))
                .child(
                    div()
                        .size(px(48.))
                        .rounded(px(13.))
                        .bg(p.accent_soft)
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(ui::icon(IconName::Flame, 24., p.accent)),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(2.))
                        .child(
                            div()
                                .text_size(px(34.))
                                .line_height(px(44.))
                                .text_color(p.text_subtle)
                                .child("Ready when you are."),
                        )
                        .child(
                            div()
                                .text_size(px(34.))
                                .line_height(px(44.))
                                .font_weight(FontWeight::MEDIUM)
                                .child(format!("What should we build in {workspace}?")),
                        ),
                )
                .child(composer)
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .justify_center()
                        .gap(px(10.))
                        .children(chips),
                ),
        )
        .children(notice)
}
