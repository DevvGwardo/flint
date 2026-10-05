//! Compact session frames, edge drop previews and nested resizable tiles.

use gpui_kit::assets::IconName;
use gpui_kit::component::resizable::*;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::app::FlintApp;
use crate::docking::{Edge, SplitAxis};
use crate::theme::{palette, size};
use crate::ui;

use super::{Node, PaneMode};

#[derive(Clone)]
pub struct SessionDrag {
    pub uid: u64,
    owner: EntityId,
    title: String,
}

struct Preview {
    title: String,
}

impl Render for Preview {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let p = palette();
        div()
            .max_w(px(280.))
            .px(px(14.))
            .py(px(10.))
            .rounded(px(9.))
            .bg(p.surface)
            .border_1()
            .border_color(p.accent)
            .shadow_lg()
            .flex()
            .items_center()
            .gap(px(9.))
            .child(ui::icon(IconName::PanelsTopLeft, 16., p.accent))
            .child(
                div()
                    .truncate()
                    .child(ui::label(self.title.clone(), size::SM, p.text)),
            )
    }
}

/// The entire sidebar row is draggable; rename inputs retain normal selection.
pub fn drag_row<E: StatefulInteractiveElement>(
    row: E,
    uid: u64,
    title: String,
    cx: &mut Context<FlintApp>,
) -> E {
    let owner = cx.entity().entity_id();
    let weak = cx.weak_entity();
    row.on_drag(SessionDrag { uid, owner, title }, move |drag, _, _, cx| {
        cx.stop_propagation();
        let title = drag.title.clone();
        let uid = drag.uid;
        let finish = weak.clone();
        weak.update(cx, |app, cx| {
            app.session_workspace.dragging = Some(uid);
            app.drag_armed = false;
            app.session_menu = None;
            cx.notify();
        })
        .ok();
        cx.new(|cx| {
            cx.on_release(move |_, cx| {
                finish
                    .update(cx, |app, cx| {
                        app.session_workspace.dragging = None;
                        cx.notify();
                    })
                    .ok();
            })
            .detach();
            Preview { title }
        })
    })
}

pub fn targets(uid: u64, cx: &mut Context<FlintApp>) -> AnyElement {
    let p = palette();
    let owner = cx.entity().entity_id();
    div()
        .absolute()
        .inset_0()
        .children(
            [
                (Edge::Left, "left", "Split left"),
                (Edge::Right, "right", "Split right"),
                (Edge::Top, "top", "Split above"),
                (Edge::Bottom, "bottom", "Split below"),
            ]
            .map(|(edge, name, label)| {
                let zone = div()
                    .id(SharedString::from(format!("session-drop-{uid}-{name}")))
                    .absolute()
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(p.accent.opacity(0.10))
                    .border_1()
                    .border_color(p.accent.opacity(0.35))
                    .drag_over::<SessionDrag>(|style, _, _, _| {
                        style.bg(p.accent.opacity(0.28)).border_color(p.accent)
                    })
                    .can_drop(move |drag, _, _| {
                        drag.downcast_ref::<SessionDrag>()
                            .is_some_and(|drag| drag.owner == owner && drag.uid != uid)
                    })
                    .on_drop(cx.listener(move |app, drag: &SessionDrag, window, cx| {
                        if drag.owner == owner && drag.uid != uid {
                            app.split_session_pane(drag.uid, uid, edge, window, cx);
                        }
                        cx.stop_propagation();
                    }))
                    .child(
                        div()
                            .px(px(10.))
                            .py(px(6.))
                            .rounded(px(6.))
                            .bg(p.surface)
                            .child(ui::label(label, size::XS, p.text)),
                    )
                    .test_support();
                match edge {
                    Edge::Left => zone
                        .left_0()
                        .top(relative(0.25))
                        .bottom(relative(0.25))
                        .w(relative(0.25)),
                    Edge::Right => zone
                        .right_0()
                        .top(relative(0.25))
                        .bottom(relative(0.25))
                        .w(relative(0.25)),
                    Edge::Top => zone.top_0().left_0().right_0().h(relative(0.25)),
                    Edge::Bottom => zone.bottom_0().left_0().right_0().h(relative(0.25)),
                }
            }),
        )
        .into_any_element()
}

fn button(
    id: SharedString,
    icon: IconName,
    tip: &'static str,
    selected: bool,
) -> impl IntoElement + StatefulInteractiveElement + ParentElement + Styled {
    let p = palette();
    div()
        .id(id)
        .role(gpui_kit::Role::Button)
        .aria_label(tip)
        .tab_index(0)
        .size(px(26.))
        .flex_shrink_0()
        .rounded(px(5.))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .bg(if selected { p.raised } else { p.bg })
        .hover(|style| style.bg(p.raised))
        .focus_visible(|style| style.border_1().border_color(p.accent))
        .tooltip(move |window, cx| Tooltip::new(tip).build(window, cx))
        .child(ui::icon(
            icon,
            14.,
            if selected { p.accent } else { p.text_muted },
        ))
        .test_support()
}

fn header(uid: u64, app: &FlintApp, cx: &mut Context<FlintApp>) -> AnyElement {
    let Some(ix) = app.session_index(uid) else {
        return div().into_any_element();
    };
    let session = &app.sessions[ix];
    let mode = app
        .session_workspace
        .panes
        .get(&uid)
        .map_or(PaneMode::Both, |pane| pane.mode);
    let p = palette();
    let title = session.title();
    let handle = drag_row(
        div()
            .id(SharedString::from(format!("session-pane-drag-{uid}")))
            .size(px(22.))
            .flex_shrink_0()
            .rounded(px(5.))
            .cursor(CursorStyle::OpenHand)
            .flex()
            .items_center()
            .justify_center()
            .hover(|style| style.bg(p.raised))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(ui::icon(IconName::GripVertical, 14., p.text_subtle))
            .test_support(),
        uid,
        title.clone(),
        cx,
    );
    div()
        .id(SharedString::from(format!("session-pane-header-{uid}")))
        .h(px(40.))
        .flex_shrink_0()
        .px(px(7.))
        .flex()
        .items_center()
        .gap(px(6.))
        .bg(p.surface)
        .border_b_1()
        .border_color(p.border)
        .child(handle)
        .child(if session.view.running {
            ui::spinner(app.now(), 12., p.accent).into_any_element()
        } else {
            ui::icon(
                if session.agent == flint_agent::AgentKind::Flint {
                    IconName::SquareTerminal
                } else {
                    IconName::Bot
                },
                12.,
                p.text_subtle,
            )
            .into_any_element()
        })
        .child(
            div()
                .id(SharedString::from(format!("session-pane-title-{uid}")))
                .flex_1()
                .min_w_0()
                .truncate()
                .font_weight(FontWeight::MEDIUM)
                .text_size(px(size::SM))
                .text_color(ui::tinted(p.text, session.color_seed()))
                .tooltip(move |window, cx| Tooltip::new(title.clone()).build(window, cx))
                .child(session.title()),
        )
        .children(
            [
                (
                    PaneMode::Chat,
                    "chat",
                    IconName::MessageSquare,
                    "Show conversation",
                ),
                (
                    PaneMode::Terminal,
                    "terminal",
                    IconName::SquareTerminal,
                    "Show terminal",
                ),
                (
                    PaneMode::Both,
                    "both",
                    IconName::PanelsTopLeft,
                    "Show conversation and terminal",
                ),
            ]
            .map(|(choice, name, icon, tip)| {
                button(
                    format!("session-pane-mode-{uid}-{name}").into(),
                    icon,
                    tip,
                    mode == choice,
                )
                .on_click(cx.listener(move |app, _, window, cx| {
                    app.set_session_pane_mode(uid, choice, window, cx)
                }))
            }),
        )
        .child(
            button(
                format!("session-pane-close-{uid}").into(),
                IconName::X,
                "Close pane, keep session running",
                false,
            )
            .on_click(
                cx.listener(move |app, _, window, cx| app.close_session_pane(uid, window, cx)),
            ),
        )
        .into_any_element()
}

fn terminal(uid: u64, app: &FlintApp, cx: &mut Context<FlintApp>) -> AnyElement {
    let p = palette();
    let current = app.pane_terminal(uid).map(Entity::entity_id);
    let tabs = app
        .terminal
        .tabs
        .iter()
        .enumerate()
        .filter(|(_, view)| app.terminal_owner(view, cx) == Some(uid))
        .map(|(ix, view)| {
            let selected = current == Some(view.entity_id());
            div()
                .id(SharedString::from(format!(
                    "session-terminal-tab-{uid}-{ix}"
                )))
                .h(px(26.))
                .max_w(px(180.))
                .min_w_0()
                .px(px(8.))
                .rounded(px(5.))
                .flex()
                .items_center()
                .gap(px(4.))
                .cursor_pointer()
                .when(selected, |tab| tab.bg(p.raised))
                .hover(|style| style.bg(p.raised))
                .on_click(
                    cx.listener(move |app, _, window, cx| app.select_terminal_tab(ix, window, cx)),
                )
                .child(div().min_w_0().truncate().child(ui::label(
                    view.read(cx).label(),
                    size::XS,
                    p.text_muted,
                )))
                .child(
                    div()
                        .id(SharedString::from(format!(
                            "session-terminal-close-{uid}-{ix}"
                        )))
                        .size(px(16.))
                        .flex_shrink_0()
                        .on_click(cx.listener(move |app, _, window, cx| {
                            cx.stop_propagation();
                            app.close_terminal_tab(ix, window, cx);
                        }))
                        .child(ui::icon(IconName::X, 10., p.text_subtle))
                        .test_support(),
                )
                .test_support()
        })
        .collect::<Vec<_>>();
    div()
        .id(SharedString::from(format!("session-terminal-{uid}")))
        .capture_any_mouse_down(cx.listener(move |app, event: &MouseDownEvent, _, _| {
            if event.button == MouseButton::Left
                && let Some(pane) = app.session_workspace.panes.get_mut(&uid)
            {
                pane.follow_commands = false;
            }
        }))
        .size_full()
        .min_h_0()
        .flex()
        .flex_col()
        .bg(p.bg)
        .child(
            div()
                .h(px(32.))
                .flex_shrink_0()
                .px(px(6.))
                .flex()
                .items_center()
                .gap(px(4.))
                .border_b_1()
                .border_color(p.border)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .gap(px(3.))
                        .overflow_hidden()
                        .children(tabs),
                )
                .child(
                    button(
                        format!("session-terminal-new-{uid}").into(),
                        IconName::Plus,
                        "New shell in this session's workspace",
                        false,
                    )
                    .on_click(cx.listener(move |app, _, window, cx| {
                        app.focus_session_pane(uid, false, window, cx);
                        app.new_terminal_for_session(uid, None, None, window, cx);
                    })),
                ),
        )
        .child(
            div()
                .flex_1()
                .min_h_0()
                .relative()
                .px(px(6.))
                .pt(px(4.))
                .children(app.pane_terminal(uid).cloned())
                .when(current.is_none(), |content| {
                    content.child(
                        div()
                            .size_full()
                            .flex()
                            .flex_col()
                            .items_center()
                            .justify_center()
                            .gap(px(8.))
                            .child(ui::label("No terminal open", size::SM, p.text_subtle))
                            .child(
                                div()
                                    .id(SharedString::from(format!("session-terminal-open-{uid}")))
                                    .role(gpui_kit::Role::Button)
                                    .aria_label("Open terminal")
                                    .tab_index(0)
                                    .px(px(12.))
                                    .py(px(6.))
                                    .rounded(px(6.))
                                    .border_1()
                                    .border_color(p.border_strong)
                                    .cursor_pointer()
                                    .hover(|style| style.bg(p.raised))
                                    .on_click(cx.listener(move |app, _, window, cx| {
                                        app.focus_session_pane(uid, false, window, cx);
                                        app.ensure_pane_terminal(uid, window, cx);
                                    }))
                                    .child(ui::label("Open terminal", size::SM, p.text))
                                    .test_support(),
                            ),
                    )
                }),
        )
        .test_support()
        .into_any_element()
}

fn collapsed_terminal(uid: u64, cx: &mut Context<FlintApp>) -> AnyElement {
    let p = palette();
    div()
        .id(SharedString::from(format!(
            "session-terminal-collapsed-{uid}"
        )))
        .h(px(32.))
        .flex_shrink_0()
        .px(px(10.))
        .flex()
        .items_center()
        .gap(px(6.))
        .border_t_1()
        .border_color(p.border)
        .cursor_pointer()
        .hover(|style| style.bg(p.raised))
        .on_click(cx.listener(move |app, _, window, cx| {
            app.focus_session_pane(uid, false, window, cx);
            app.ensure_pane_terminal(uid, window, cx);
        }))
        .child(ui::icon(IconName::SquareTerminal, 12., p.text_subtle))
        .child(ui::label("Open terminal", size::XS, p.text_muted))
        .test_support()
        .into_any_element()
}

fn pane(uid: u64, app: &FlintApp, window: &mut Window, cx: &mut Context<FlintApp>) -> AnyElement {
    let Some(state) = app.session_workspace.panes.get(&uid) else {
        return div().into_any_element();
    };
    let p = palette();
    let active = app.session().uid == uid;
    let bounds = state.bounds.clone();
    let content = match state.mode {
        PaneMode::Chat => crate::transcript::render_session_pane(app, uid, window, cx),
        PaneMode::Terminal => terminal(uid, app, cx),
        // Shells don't survive restarts or splits; an empty half pane is wasted
        // room. Agent command mirrors alone don't expand it either.
        PaneMode::Both if !state.terminal_shown || app.pane_terminal(uid).is_none() => div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .child(crate::transcript::render_session_pane(app, uid, window, cx)),
            )
            .child(collapsed_terminal(uid, cx))
            .into_any_element(),
        PaneMode::Both => v_resizable(SharedString::from(format!("session-pane-content-{uid}")))
            .child(
                resizable_panel()
                    .size_range(px(80.)..Pixels::MAX)
                    .child(crate::transcript::render_session_pane(app, uid, window, cx)),
            )
            .child(
                resizable_panel()
                    .size_range(px(80.)..Pixels::MAX)
                    .child(terminal(uid, app, cx)),
            )
            .into_any_element(),
    };
    div()
        .id(SharedString::from(format!("session-pane-{uid}")))
        .size_full()
        .min_w_0()
        .min_h_0()
        .relative()
        .flex()
        .flex_col()
        .overflow_hidden()
        .rounded(px(8.))
        .border_1()
        .border_color(if active {
            p.accent.opacity(0.7)
        } else {
            p.border_strong
        })
        .track_focus(&state.focus)
        .capture_any_mouse_down(cx.listener(move |app, event: &MouseDownEvent, window, cx| {
            if matches!(event.button, MouseButton::Left | MouseButton::Right)
                && app.session().uid != uid
            {
                app.focus_session_pane(uid, false, window, cx);
            }
        }))
        .child(header(uid, app, cx))
        .child(div().flex_1().min_h_0().child(content))
        .child(
            canvas(
                move |at, window, _| {
                    if bounds.get() != Some(at) {
                        bounds.set(Some(at));
                        window.refresh();
                    }
                },
                |_, _, _, _| {},
            )
            .absolute()
            .inset_0(),
        )
        .when(
            app.session_workspace
                .dragging
                .is_some_and(|drag| drag != uid)
                && cx.has_active_drag(),
            |pane| pane.child(deferred(targets(uid, cx)).with_priority(5)),
        )
        .test_support()
        .into_any_element()
}

fn node(
    tree: &Node<u64>,
    available: Size<Pixels>,
    app: &FlintApp,
    window: &mut Window,
    cx: &mut Context<FlintApp>,
) -> AnyElement {
    match tree {
        Node::Session { session } => pane(*session, app, window, cx),
        Node::Split {
            axis,
            sizes,
            first,
            second,
        } => {
            let key = tree.key();
            let total = match axis {
                SplitAxis::Horizontal => available.width,
                SplitAxis::Vertical => available.height,
            };
            let fraction = match sizes {
                [Some(a), Some(b)] => a / (a + b),
                _ => 0.5,
            };
            let a = total * fraction;
            let b = total - a;
            let dimensions = |amount| match axis {
                SplitAxis::Horizontal => Size {
                    width: amount,
                    ..available
                },
                SplitAxis::Vertical => Size {
                    height: amount,
                    ..available
                },
            };
            let first = node(first, dimensions(a), app, window, cx);
            let second = node(second, dimensions(b), app, window, cx);
            let group_id = SharedString::from(format!(
                "session-split-{}-{key}",
                app.session_workspace.revision
            ));
            let group = match axis {
                SplitAxis::Horizontal => h_resizable(group_id),
                SplitAxis::Vertical => v_resizable(group_id),
            };
            let weak = cx.weak_entity();
            let revision = app.session_workspace.revision;
            group
                .child(
                    resizable_panel()
                        .size_range(px(100.)..Pixels::MAX)
                        .size(a.max(px(100.)))
                        .flex_none()
                        .child(first),
                )
                .child(
                    resizable_panel()
                        .size_range(px(100.)..Pixels::MAX)
                        .size(b.max(px(100.)))
                        .flex_none()
                        .child(second),
                )
                .on_resize(move |state, _, cx| {
                    if let [a, b] = state.read(cx).sizes().as_slice() {
                        let measured = [f32::from(*a), f32::from(*b)];
                        weak.update(cx, |app, _| {
                            if app.session_workspace.revision == revision
                                && let Some(root) = &mut app.session_workspace.layout.root
                                && root.set_sizes(&key, measured)
                            {
                                app.save_session_layout();
                            }
                        })
                        .ok();
                    }
                })
                .into_any_element()
        }
    }
}

pub fn render(app: &FlintApp, window: &mut Window, cx: &mut Context<FlintApp>) -> AnyElement {
    let p = palette();
    let bounds = app.session_workspace.bounds.clone();
    let available = bounds.get().map_or_else(
        || Size {
            width: px(app.chat_panel_width(f32::from(window.viewport_size().width)) - 16.),
            height: (window.viewport_size().height - px(88.)).max(px(100.)),
        },
        |at| Size {
            width: (at.size.width - px(16.)).max(px(100.)),
            height: (at.size.height - px(16.)).max(px(100.)),
        },
    );
    div()
        .id("session-workspace")
        .size_full()
        .min_w_0()
        .min_h_0()
        .flex()
        .flex_col()
        .bg(p.bg)
        .child(
            div()
                .id("session-workspace-toolbar")
                .h(px(36.))
                .flex_shrink_0()
                .px(px(10.))
                .flex()
                .items_center()
                .gap(px(7.))
                .child(crate::docking::handle(crate::docking::Panel::Chat, cx))
                .child(ui::label(
                    format!("{} sessions", app.session_workspace.layout.sessions().len()),
                    size::SM,
                    p.text_muted,
                ))
                .child(div().flex_1())
                .child(
                    button(
                        "session-grid".into(),
                        IconName::Grid2x2,
                        "Arrange sessions in a balanced grid",
                        false,
                    )
                    .on_click(cx.listener(|app, _, _, cx| app.arrange_session_grid(cx))),
                )
                .child(
                    button(
                        "session-single".into(),
                        IconName::Maximize2,
                        "Keep only the focused session visible",
                        false,
                    )
                    .on_click(
                        cx.listener(|app, _, window, cx| app.reset_session_panes(window, cx)),
                    ),
                )
                .when(!app.sidebar_visible, |bar| {
                    bar.child(
                        button(
                            "session-workspace-sidebar".into(),
                            IconName::PanelLeft,
                            "Open sessions",
                            false,
                        )
                        .on_click(cx.listener(|app, _, window, cx| app.open_sessions(window, cx))),
                    )
                })
                .test_support(),
        )
        .child(
            div()
                .flex_1()
                .min_h_0()
                .relative()
                .p(px(8.))
                .children(
                    app.session_workspace
                        .layout
                        .root
                        .as_ref()
                        .map(|root| node(root, available, app, window, cx)),
                )
                .child(
                    canvas(
                        move |at, window, _| {
                            if bounds.get() != Some(at) {
                                bounds.set(Some(at));
                                window.refresh();
                            }
                        },
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .inset_0(),
                ),
        )
        .test_support()
        .into_any_element()
}
