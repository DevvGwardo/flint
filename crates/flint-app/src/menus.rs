//! Popovers that sit just above the composer: the `@` file picker, the `/`
//! command menu, and the help card.

use gpui_kit::assets::IconName;
use gpui_kit::component::scroll::{Scrollbar, ScrollbarMode};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::app::FlintApp;
use crate::slash;
use crate::theme::MONO_FONT;
use crate::theme::palette;
use crate::theme::size;
use crate::transcript::file_icon;
use crate::ui;

fn panel() -> Div {
    let p = palette();
    div()
        .block_mouse_except_scroll()
        .w_full()
        .rounded(px(14.))
        .border_1()
        .border_color(p.border_strong)
        .bg(p.surface)
        .shadow_lg()
        .py(px(8.))
        .flex()
        .flex_col()
}

fn header(title: &str, hint: &str) -> Div {
    let p = palette();
    let full_hint = hint.to_string();
    div()
        .px(px(16.))
        .pb(px(6.))
        .min_w_0()
        .flex()
        .items_center()
        .gap(px(12.))
        .justify_between()
        .child(
            ui::label(title.to_string(), size::XS, p.text_subtle)
                .id("menu-title")
                .flex_1()
                .min_w_0()
                .truncate()
                .test_support(),
        )
        .child(
            ui::label(hint.to_string(), size::XS, p.text_subtle)
                .id("menu-hint")
                .flex_shrink_0()
                .max_w(relative(0.65))
                .truncate()
                .tooltip(move |window, cx| Tooltip::new(full_hint.clone()).build(window, cx))
                .test_support(),
        )
}

fn item_row(id: impl Into<ElementId>, selected: bool) -> Stateful<Div> {
    let p = palette();
    div()
        .id(id)
        .mx(px(6.))
        .px(px(10.))
        .h(px(36.))
        .flex_shrink_0()
        .rounded(px(8.))
        .flex()
        .items_center()
        .gap(px(10.))
        .cursor_pointer()
        .when(selected, |row| row.bg(p.raised))
        .when(!selected, |row| row.hover(|s| s.bg(hsla(0., 0., 1., 0.04))))
}

/// What a list menu's panel adds around its rows: padding, border and header.
pub const MENU_CHROME: f32 = 44.;

/// A list menu's rows in a viewport at most `max_height` tall (the whole
/// popover, chrome included) that scrolls with an always-visible scrollbar.
/// Every list menu shares `app.menu_scroll`; the rows are the viewport's
/// direct children, so `menu_scroll.scroll_to_item(n)` reaches row `n`.
pub fn scroll_rows<E: IntoElement>(
    app: &FlintApp,
    id: &'static str,
    max_height: f32,
    rows: impl IntoIterator<Item = E>,
) -> Div {
    div()
        .relative()
        .child(
            div()
                .id(id)
                .max_h(px((max_height - MENU_CHROME).max(36.)))
                .overflow_y_scroll()
                .track_scroll(&app.menu_scroll)
                .pr(px(12.))
                .flex()
                .flex_col()
                .children(rows)
                .test_support(),
        )
        .child(Scrollbar::vertical(&app.menu_scroll).mode(ScrollbarMode::Always))
}

pub fn render(
    app: &FlintApp,
    max_height: f32,
    window: &Window,
    cx: &mut Context<FlintApp>,
) -> Option<AnyElement> {
    let p = palette();
    if let Some(menu) = &app.mention {
        let rows = menu.results.iter().enumerate().map(|(n, path)| {
            let (dir, name) = path.rsplit_once('/').unwrap_or(("", path));
            let pick = path.clone();
            let tip = path.clone();
            item_row(("mention-item", n), n == menu.selected)
                .role(gpui_kit::Role::Button)
                .aria_label(format!("Attach file {path}"))
                .min_w_0()
                .overflow_hidden()
                .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.pick_mention(pick.clone(), window, cx)
                }))
                .child(ui::icon(file_icon(path), 15., p.text_muted).flex_shrink_0())
                .child(
                    ui::mono(name.to_string(), size::SM, p.text)
                        .id(("mention-name", n))
                        .min_w_0()
                        .max_w(relative(0.7))
                        .truncate()
                        .test_support(),
                )
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .font_family(MONO_FONT)
                        .text_size(px(size::XS))
                        .text_color(p.text_subtle)
                        .child(dir.to_string()),
                )
                .test_support()
        });
        let empty = menu.results.is_empty();
        let indexing = !app.file_index.contains_key(&app.session().workspace);
        let title = if menu.query.is_empty() {
            "Attach a file".to_string()
        } else {
            format!("Files matching “{}”", menu.query)
        };
        return Some(
            panel()
                .id("mention-menu")
                .child(header(&title, "↑↓ choose · ⏎ attach · esc close"))
                .child(scroll_rows(app, "mention-menu-rows", max_height, rows))
                .when(empty, |panel| {
                    panel.child(div().px(px(16.)).py(px(8.)).child(ui::label(
                        if indexing {
                            "Indexing workspace files..."
                        } else {
                            "No files match. Keep typing or press esc."
                        },
                        size::BASE - 1.,
                        p.text_muted,
                    )))
                })
                .test_support()
                .into_any_element(),
        );
    }
    if let Some(menu) = crate::option_chips::menu(app, max_height, window, cx) {
        return Some(menu);
    }
    if app.agent_menu {
        let current = app.session().agent;
        let rows = crate::agents::AGENTS
            .into_iter()
            .enumerate()
            .map(|(n, kind)| {
                item_row(("agent-item", n), kind == current)
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.choose_agent(kind, window, cx)),
                    )
                    .child(ui::label(app.agent_label(kind), size::SM, p.text).w(px(220.)))
                    .child(ui::label(
                        crate::agents::about(kind),
                        size::BASE - 1.,
                        p.text_muted,
                    ))
                    .when(kind == current, |row| {
                        row.child(div().flex_1())
                            .child(ui::icon(IconName::Check, 13., p.accent))
                    })
                    .test_support()
            });
        let hint = if app.session().view.items.is_empty() {
            "for this session"
        } else {
            "opens a new session"
        };
        return Some(
            panel()
                .id("agent-menu")
                .child(header("Agent", hint))
                .child(scroll_rows(app, "agent-menu-rows", max_height, rows))
                .test_support()
                .into_any_element(),
        );
    }
    if let Some(menu) = &app.slash {
        let text = app.composer.read(cx).value().to_string();
        let commands = slash::active_query(&text)
            .map(slash::matches)
            .unwrap_or_default();
        let rows = commands
            .into_iter()
            .enumerate()
            .map(|(n, (command, name, about))| {
                item_row(("slash-item", n), n == menu.selected)
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.run_slash(command, window, cx)),
                    )
                    .child(ui::mono(name, size::SM, p.text).w(px(96.)))
                    .child(ui::label(about, size::BASE - 1., p.text_muted))
                    .test_support()
            });
        return Some(
            panel()
                .id("slash-menu")
                .child(header("Commands", "↑↓ choose · ⏎ run · esc close"))
                .child(scroll_rows(app, "slash-menu-rows", max_height, rows))
                .test_support()
                .into_any_element(),
        );
    }
    if app.help_open {
        let line = |keys: &str, what: &str| {
            div()
                .px(px(16.))
                .h(px(28.))
                .flex()
                .items_center()
                .gap(px(14.))
                .child(ui::mono(keys.to_string(), size::SM, p.text).w(px(110.)))
                .child(ui::label(what.to_string(), size::BASE - 1., p.text_muted))
        };
        return Some(
            panel()
                .id("help-card")
                .child(
                    div()
                        .px(px(16.))
                        .pb(px(6.))
                        .flex()
                        .justify_between()
                        .child(ui::label("Shortcuts", size::XS, p.text_subtle))
                        .child(
                            div()
                                .id("close-help")
                                .cursor_pointer()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.help_open = false;
                                    cx.notify();
                                }))
                                .child(ui::icon(IconName::X, 13., p.text_subtle)),
                        ),
                )
                .child(line("⏎ / ⇧⏎", "Send / new line"))
                .child(line("@", "Attach a workspace file"))
                .child(line("+", "Attach an image from any folder"))
                .child(line(
                    "/",
                    "Commands: new, clear, agent, model, effort, mode, approval, review",
                ))
                .child(line("⇧⇥", "Switch auto-run / ask before changes"))
                .child(line(
                    "Y · A · N",
                    "Approve · always · deny (empty composer)",
                ))
                .child(line("⌘K · ⌘J · ⌘N", "Commands · changes · new agent"))
                .child(line("esc · ⌘.", "Stop the running turn"))
                .into_any_element(),
        );
    }
    None
}
