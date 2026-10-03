//! The composer chips for an ACP agent's session options (Model, Reasoning,
//! Mode, Fast, More) and the menu each one opens. Logic lives in
//! `session_options.rs`.

use gpui_kit::assets::IconName;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::app::FlintApp;
use crate::session_options::MenuTarget;
use crate::session_options::is_permissive;
use crate::session_options::toggle_is_on;
use crate::session_options::toggled_value;
use crate::theme::palette;
use crate::theme::size;
use crate::ui;

fn chip(id: impl Into<ElementId>) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(34.))
        .px(px(11.))
        .rounded(px(17.))
        .flex()
        .flex_shrink_0()
        .items_center()
        .gap(px(6.))
        .cursor_pointer()
        .hover(|style| style.bg(hsla(0., 0., 1., 0.05)))
}

/// Chips for the active session's agent options; empty for flint's engine
/// or before the agent has reported any.
pub fn chips(app: &FlintApp, cx: &mut Context<FlintApp>) -> Vec<AnyElement> {
    let p = palette();
    if app.session().options.is_empty() && app.session().agent_starting() {
        // Greyed until the agent reports its options.
        return ["Model", "Reasoning", "Mode"]
            .into_iter()
            .enumerate()
            .map(|(n, name)| {
                div()
                    .id(("option-placeholder", n))
                    .h(px(34.))
                    .px(px(11.))
                    .flex()
                    .items_center()
                    .opacity(0.45)
                    .child(ui::label(name, size::BASE - 1., p.text_subtle))
                    .test_support()
                    .into_any_element()
            })
            .collect();
    }
    let slots = app.session_slots();
    let mut out = Vec::new();
    let label = |o: &flint_agent::SessionOption| {
        o.current_choice()
            .map_or_else(|| o.current.clone(), |c| c.name.clone())
    };
    let open = |target: MenuTarget| {
        cx.listener(move |this: &mut FlintApp, _: &ClickEvent, _, cx| {
            this.open_option_menu(target.clone(), cx)
        })
    };
    if let Some(model) = &slots.model {
        out.push(
            chip("option-model")
                .on_click(open(MenuTarget::Option(model.id.clone())))
                .child(ui::label(label(model), size::BASE - 1., p.text_muted))
                .child(ui::icon(IconName::ChevronDown, 11., p.text_subtle))
                .test_support()
                .into_any_element(),
        );
    }
    if let Some(reasoning) = &slots.reasoning {
        out.push(
            chip("option-reasoning")
                .on_click(open(MenuTarget::Option(reasoning.id.clone())))
                .child(ui::icon(IconName::Brain, 14., p.text_subtle))
                .child(ui::label(label(reasoning), size::BASE - 1., p.text_muted))
                .test_support()
                .into_any_element(),
        );
    }
    if let Some(mode) = &slots.mode {
        let plan = mode.current.to_lowercase().contains("plan");
        let permissive = is_permissive(&mode.current);
        let (icon, color) = if plan {
            (IconName::Map, p.accent)
        } else if permissive {
            (IconName::ShieldAlert, p.warning)
        } else {
            (IconName::Shield, p.text_muted)
        };
        out.push(
            chip("option-mode")
                .on_click(open(MenuTarget::Option(mode.id.clone())))
                .when(plan, |c| c.border_1().border_color(p.accent))
                .child(ui::icon(icon, 14., color))
                .child(ui::label(label(mode), size::BASE - 1., color))
                .test_support()
                .into_any_element(),
        );
    }
    if let Some(fast) = &slots.fast {
        let on = toggle_is_on(fast);
        let id = fast.id.clone();
        let next = toggled_value(fast);
        out.push(
            chip("option-fast")
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(value) = &next {
                        this.set_session_option(&id, value, cx);
                    }
                }))
                .child(ui::icon(
                    IconName::Zap,
                    14.,
                    if on { p.accent } else { p.text_subtle },
                ))
                .child(ui::label(
                    fast.name.clone(),
                    size::SM,
                    if on { p.text } else { p.text_subtle },
                ))
                .test_support()
                .into_any_element(),
        );
    }
    if !slots.more.is_empty() {
        out.push(
            chip("option-more")
                .px(px(9.))
                .on_click(open(MenuTarget::More))
                .child(ui::icon(IconName::Ellipsis, 15., p.text_subtle))
                .test_support()
                .into_any_element(),
        );
    }
    out
}

/// The open option menu, as a panel above the composer.
pub fn menu(
    app: &FlintApp,
    max_height: f32,
    window: &Window,
    cx: &mut Context<FlintApp>,
) -> Option<AnyElement> {
    let p = palette();
    let menu = app.option_menu.as_ref()?;
    if app.menu_needs_scroll.replace(false) {
        let view = cx.entity().downgrade();
        let target = menu.target.clone();
        let uid = app.session().uid;
        // GPUI initializes scroll bounds after the first scroll-to-item request.
        window.on_next_frame(move |_, cx| {
            view.update(cx, |app, cx| {
                if app.session().uid == uid
                    && let Some(menu) = &app.option_menu
                    && menu.target == target
                {
                    app.menu_scroll.scroll_to_item(menu.selected);
                    cx.notify();
                }
            })
            .ok();
        });
    }
    let title = match &menu.target {
        MenuTarget::Option(id) => app
            .session()
            .options
            .iter()
            .find(|o| &o.id == id)
            .map_or_else(|| id.clone(), |o| o.name.clone()),
        MenuTarget::More => "More options".to_string(),
    };
    let rows =
        app.option_menu_rows()
            .into_iter()
            .enumerate()
            .map(|(n, (_, name, detail, current))| {
                div()
                    .id(("option-item", n))
                    .mx(px(6.))
                    .px(px(10.))
                    .py(px(7.))
                    .flex_shrink_0()
                    .rounded(px(8.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .cursor_pointer()
                    .when(n == menu.selected, |row| row.bg(p.raised))
                    .on_click(cx.listener(move |this, _, _, cx| this.pick_option_row(n, cx)))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .child(ui::label(name, size::SM, p.text))
                            .children(detail.map(|d| ui::label(d, size::XS, p.text_muted))),
                    )
                    .when(current, |row| {
                        row.child(ui::icon(IconName::Check, 13., p.accent))
                    })
                    .test_support()
            });
    Some(
        div()
            .id("option-menu")
            .w_full()
            .rounded(px(14.))
            .border_1()
            .border_color(p.border_strong)
            .bg(p.surface)
            .shadow_lg()
            .py(px(8.))
            .flex()
            .flex_col()
            .child(
                div()
                    .px(px(16.))
                    .pb(px(6.))
                    .flex()
                    .justify_between()
                    .child(ui::label(title, size::XS, p.text_subtle))
                    .child(ui::label(
                        "↑↓ choose · ⏎ select · esc close",
                        size::XS,
                        p.text_subtle,
                    )),
            )
            .child(crate::menus::scroll_rows(
                app,
                "option-menu-rows",
                max_height,
                rows,
            ))
            .test_support()
            .into_any_element(),
    )
}
