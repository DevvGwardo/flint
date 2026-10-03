//! The floating sidebar: new agent, search, sessions grouped by workspace with
//! live status glyphs, and filter/settings at the bottom.

use std::time::SystemTime;

use gpui_kit::assets::IconName;
use gpui_kit::base::ElementExt as _;
use gpui_kit::component::input::Input;
use gpui_kit::component::scroll::{Scrollbar, ScrollbarMode};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::app::FlintApp;
use crate::app::NewSession;
use crate::app::OpenWorkspace;
use crate::app::SessionFilter;
use crate::app::ToggleSidebar;
use crate::app::ToggleTerminal;
use crate::session::Status;
use crate::session::folder_name;
use crate::theme::palette;
use crate::theme::size;
use crate::ui;

fn white(alpha: f32) -> Hsla {
    hsla(0., 0., 1., alpha)
}

struct RowFocus {
    handle: FocusHandle,
    last: Option<FocusHandle>,
}

#[derive(IntoElement)]
struct ScrollableRow {
    id: ElementId,
    row: Stateful<Div>,
    scroll: ScrollHandle,
}

impl RenderOnce for ScrollableRow {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = window.use_keyed_state(self.id, cx, |_, cx| RowFocus {
            handle: cx.focus_handle().tab_stop(false),
            last: None,
        });
        let handle = state.read(cx).handle.clone();
        self.row
            .track_focus(&handle)
            .on_prepaint(move |bounds, window, cx| {
                let focused = window
                    .focused(cx)
                    .filter(|_| handle.contains_focused(window, cx));
                let changed = state.update(cx, |state, _| {
                    let changed = state.last != focused;
                    state.last = focused.clone();
                    changed
                });
                if changed && let Some(focused) = focused {
                    reveal_row(self.scroll, bounds, focused, window);
                }
            })
    }
}

fn reveal_row(
    scroll: ScrollHandle,
    bounds: Bounds<Pixels>,
    focus: FocusHandle,
    window: &mut Window,
) {
    let viewport = scroll.bounds();
    let delta = if bounds.top() < viewport.top() {
        viewport.top() - bounds.top()
    } else if bounds.bottom() > viewport.bottom() {
        viewport.bottom() - bounds.bottom()
    } else {
        return;
    };
    let offset = scroll.offset();
    window.on_next_frame(move |window, _| {
        if focus.is_focused(window) && scroll.offset() == offset {
            scroll.set_offset(point(
                offset.x,
                (offset.y + delta).clamp(-scroll.max_offset().y, px(0.)),
            ));
            window.refresh();
        }
    });
}

pub fn render(app: &FlintApp, cx: &mut Context<FlintApp>) -> impl IntoElement {
    let p = palette();

    // The dedicated grip moves the panel; the rest of the row moves the window.
    let top = app.drag_region(
        div()
            .id("sidebar-top")
            .h(px(34.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap(px(4.))
            .px(px(8.))
            .child(crate::docking::handle(crate::docking::Panel::Sidebar, cx))
            .child(ui::label("Sessions", size::SM, p.text_subtle))
            .child(div().flex_1())
            .child(
                div()
                    .id("hide-sidebar")
                    .aria_label("Close sessions panel")
                    .tab_index(0)
                    .focus_visible(|style| style.border_1().border_color(p.accent))
                    .size(px(32.))
                    .rounded(px(8.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .hover(|style| style.bg(white(0.06)))
                    .on_click(cx.listener(|this, _, window, cx| {
                        if this.session_drawer {
                            this.close_session_drawer(window, cx);
                        } else {
                            window.dispatch_action(Box::new(ToggleSidebar), cx);
                        }
                    }))
                    .child(ui::icon(IconName::PanelLeft, 16., p.text_subtle))
                    .test_support(),
            ),
        cx,
    );

    let new_agent = nav_row("new-agent", IconName::SquarePen, "New agent", Some("⌘N")).on_click(
        cx.listener(|_, _, window, cx| {
            window.dispatch_action(Box::new(NewSession), cx);
        }),
    );
    let open_folder = nav_row(
        "open-folder",
        IconName::FolderPlus,
        "Open folder",
        Some("⌘O"),
    )
    .on_click(cx.listener(|_, _, window, cx| {
        window.dispatch_action(Box::new(OpenWorkspace), cx);
    }));

    let terminal = nav_row("terminal", IconName::SquareTerminal, "Terminal", Some("⌃`"))
        .when(app.terminal.open, |row| row.bg(white(0.075)))
        .on_click(cx.listener(|_, _, window, cx| {
            window.dispatch_action(Box::new(ToggleTerminal), cx);
        }))
        .test_support();

    let search = div()
        .flex_shrink_0()
        .mx(px(10.))
        .mt(px(12.))
        .mb(px(4.))
        .child(
            Input::new(&app.search)
                .id("session-search")
                .prefix(ui::icon(IconName::Search, 14., p.text_subtle))
                .cleanable(true),
        );

    // Group by workspace, most recently active first.
    let mut ordered = app.visible_sessions(cx);
    ordered.sort_by_key(|&ix| std::cmp::Reverse(app.sessions[ix].touched));
    let mut groups: Vec<(std::path::PathBuf, Vec<usize>)> = Vec::new();
    for ix in ordered {
        let workspace = &app.sessions[ix].workspace;
        match groups.iter_mut().find(|(w, _)| w == workspace) {
            Some((_, rows)) => rows.push(ix),
            None => groups.push((workspace.clone(), vec![ix])),
        }
    }
    let mut list = Vec::new();
    for (g, (workspace, rows)) in groups.into_iter().enumerate() {
        let full_path = workspace.to_string_lossy().to_string();
        let rows: Vec<_> = rows
            .into_iter()
            .map(|ix| session_row(app, ix, cx))
            .collect();
        list.push(
            div()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(1.))
                .child(
                    div()
                        .px(px(20.))
                        .pt(px(if g == 0 { 16. } else { 24. }))
                        .pb(px(6.))
                        .flex()
                        .items_center()
                        .gap(px(7.))
                        .child(ui::icon(IconName::Folder, 13., p.text_subtle))
                        .child(
                            div()
                                .id(("workspace-label", g))
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(px(size::SM))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(p.text_subtle)
                                .tooltip(move |window, cx| {
                                    Tooltip::new(full_path.clone()).build(window, cx)
                                })
                                .child(folder_name(&workspace))
                                .test_support(),
                        ),
                )
                .children(rows),
        );
    }
    let empty = list.is_empty();
    let searching = !app.search.read(cx).value().trim().is_empty();
    let empty_label = if searching {
        "No matching sessions"
    } else {
        match app.filter {
            SessionFilter::All => "No sessions",
            SessionFilter::Running => "No running sessions",
            SessionFilter::Unread => "No unread sessions",
        }
    };

    let filter_label = match app.filter {
        SessionFilter::All => "All",
        SessionFilter::Running => "Running",
        SessionFilter::Unread => "Unread",
    };
    let bottom = div()
        .flex_shrink_0()
        .py(px(8.))
        .border_t_1()
        .border_color(white(0.05))
        .child(
            nav_row("filter", IconName::ListFilter, "Filter sessions", None)
                .on_click(cx.listener(|this, _, _, cx| this.cycle_filter(cx)))
                .child(ui::label(filter_label, size::XS, p.text_subtle))
                .test_support(),
        )
        .child(
            nav_row("settings", IconName::Settings, "Settings", Some("⌘,"))
                .on_click(cx.listener(|this, _, window, cx| this.open_settings(window, cx)))
                .test_support(),
        );

    div()
        .id("sidebar")
        .w_full()
        .h_full()
        .flex()
        .flex_col()
        .rounded(px(14.))
        .bg(p.sidebar_fill)
        .border_1()
        .border_color(white(0.06))
        .overflow_hidden()
        .child(top)
        .child(new_agent)
        .child(open_folder)
        .child(terminal)
        .child(search)
        .child(
            div()
                .relative()
                .flex_1()
                .min_h_0()
                .child(
                    div()
                        .id("session-list")
                        .h_full()
                        .overflow_y_scroll()
                        .track_scroll(&app.sidebar_scroll)
                        .pr(px(12.))
                        .pb(px(8.))
                        .children(list)
                        .when(empty, |list| {
                            list.child(
                                div()
                                    .id("session-list-empty")
                                    .p(px(20.))
                                    .flex()
                                    .flex_col()
                                    .gap(px(12.))
                                    .child(ui::label(empty_label, size::SM, p.text_muted))
                                    .child(
                                        gpui_kit::component::button::Button::new(
                                            "reset-session-filter",
                                        )
                                        .label("Show all sessions")
                                        .on_click(
                                            cx.listener(|this, _, window, cx| {
                                                this.filter = SessionFilter::All;
                                                this.session_menu = None;
                                                this.sidebar_scroll.set_offset(Point::default());
                                                this.search.update(cx, |state, cx| {
                                                    state.set_value("", window, cx);
                                                    state.focus(window, cx);
                                                });
                                                cx.notify();
                                            }),
                                        ),
                                    )
                                    .test_support(),
                            )
                        })
                        .test_support(),
                )
                .child(Scrollbar::vertical(&app.sidebar_scroll).mode(ScrollbarMode::Always)),
        )
        .when(!app.archives.is_empty(), |sidebar| {
            sidebar.child(
                div()
                    .id("archived-sessions")
                    .flex_shrink_0()
                    .border_t_1()
                    .border_color(white(0.05))
                    .child(div().px(px(20.)).py(px(6.)).child(ui::label(
                        "Archived",
                        size::SM,
                        p.text_muted,
                    )))
                    .child(
                        div()
                            .relative()
                            .child(
                                div()
                                    .id("archive-list")
                                    .max_h(px(112.))
                                    .overflow_y_scroll()
                                    .track_scroll(&app.archive_scroll)
                                    .pr(px(12.))
                                    .children(app.archives.iter().enumerate().map(
                                        |(ix, (_, meta))| {
                                            let title = meta
                                                .title
                                                .as_deref()
                                                .unwrap_or("session")
                                                .to_string();
                                            let tip = format!(
                                                "Restore {title}\n{}",
                                                meta.workspace.display()
                                            );
                                            let row =
                                                div()
                                                    .id(("restore-archive", ix))
                                                    .aria_label(format!("Restore {title}"))
                                                    .tab_index(0)
                                                    .focus_visible(|style| {
                                                        style.border_2().border_color(p.accent)
                                                    })
                                                    .mx(px(8.))
                                                    .px(px(12.))
                                                    .h(px(34.))
                                                    .rounded(px(7.))
                                                    .flex()
                                                    .items_center()
                                                    .gap(px(8.))
                                                    .cursor_pointer()
                                                    .hover(|style| style.bg(white(0.05)))
                                                    .tooltip(move |window, cx| {
                                                        Tooltip::new(tip.clone()).build(window, cx)
                                                    })
                                                    .on_click(cx.listener(
                                                        move |this, _, window, cx| {
                                                            this.restore_archive(ix, window, cx)
                                                        },
                                                    ))
                                                    .child(ui::icon(
                                                        IconName::ArchiveRestore,
                                                        14.,
                                                        p.text_muted,
                                                    ))
                                                    .child(
                                                        div().flex_1().min_w_0().truncate().child(
                                                            ui::label(title, size::SM, p.text),
                                                        ),
                                                    )
                                                    .test_support();
                                            ScrollableRow {
                                                id: ("archive-scroll-row", ix).into(),
                                                row: div().id(("archive-container", ix)).child(row),
                                                scroll: app.archive_scroll.clone(),
                                            }
                                        },
                                    ))
                                    .test_support(),
                            )
                            .child(
                                Scrollbar::vertical(&app.archive_scroll)
                                    .mode(ScrollbarMode::Always),
                            ),
                    )
                    .test_support(),
            )
        })
        .child(bottom)
        .test_support()
}

fn nav_row(
    id: &'static str,
    icon: IconName,
    label: &'static str,
    hint: Option<&str>,
) -> Stateful<Div> {
    let p = palette();
    div()
        .id(id)
        .aria_label(label)
        .tab_index(0)
        .focus_visible(|style| style.border_2().border_color(p.accent))
        .mx(px(8.))
        .px(px(12.))
        .h(px(38.))
        .flex_shrink_0()
        .rounded(px(9.))
        .flex()
        .items_center()
        .gap(px(11.))
        .cursor_pointer()
        .hover(|style| style.bg(white(0.05)))
        .child(ui::icon(icon, 16., p.text_muted))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .child(ui::label(label, size::BASE, p.text)),
        )
        .when_some(hint.map(str::to_string), |row, hint| {
            row.child(ui::label(hint, size::XS, p.text_subtle))
        })
}

fn session_row(app: &FlintApp, ix: usize, cx: &mut Context<FlintApp>) -> AnyElement {
    let p = palette();
    let session = &app.sessions[ix];
    let active = ix == app.active;
    let status = session.status();
    let glyph: AnyElement = match status {
        Status::Running => ui::spinner(app.now(), 13., p.accent).into_any_element(),
        Status::NeedsApproval => ui::icon(IconName::ShieldAlert, 13., p.warning).into_any_element(),
        Status::Unread => div()
            .size(px(7.))
            .rounded_full()
            .bg(p.accent)
            .into_any_element(),
        Status::Done => ui::icon(IconName::Check, 12., p.text_subtle).into_any_element(),
        Status::Idle => div()
            .size(px(7.))
            .rounded_full()
            .border_1()
            .border_color(p.text_subtle)
            .into_any_element(),
    };
    let (subtitle, subtitle_color) = match status {
        Status::NeedsApproval => ("Needs approval".to_string(), p.warning),
        Status::Running => (
            match session.view.running_commands().last() {
                Some(call) => format!("Running {}", call.summary),
                None => "Working…".to_string(),
            },
            p.text_subtle,
        ),
        Status::Unread | Status::Done => (
            match session.view.turns.last() {
                Some(turn) if !turn.files.is_empty() => {
                    let n = turn.files.len();
                    format!(
                        "{n} file{} changed · +{} −{}",
                        if n == 1 { "" } else { "s" },
                        turn.added,
                        turn.removed
                    )
                }
                _ => "Answered".to_string(),
            },
            p.text_subtle,
        ),
        Status::Idle => ("Idle".to_string(), p.text_subtle),
    };
    // ACP sessions carry their agent's name, e.g. "Claude Code · Answered".
    let subtitle = match session.agent {
        flint_agent::AgentKind::Flint => subtitle,
        agent => format!("{} · {subtitle}", agent.label()),
    };
    let emphasized = active || matches!(status, Status::Unread | Status::NeedsApproval);
    let ago = SystemTime::now()
        .duration_since(session.touched)
        .unwrap_or_default();
    let renaming = app
        .renaming
        .as_ref()
        .filter(|(row, _)| *row == ix)
        .map(|(_, input)| input.clone());
    let is_renaming = renaming.is_some();
    let full_title = session.title();
    let full_subtitle = subtitle.clone();
    let title: AnyElement = match renaming {
        Some(input) => {
            let focus = input.focus_handle(cx);
            let scroll = app.sidebar_scroll.clone();
            let reveal = app.rename_needs_scroll.replace(false);
            div()
                .id(("rename-container", ix))
                .on_prepaint(move |bounds, window, _| {
                    if reveal {
                        reveal_row(scroll, bounds, focus, window);
                    }
                })
                .flex_1()
                .min_w_0()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(|_, _, cx| cx.stop_propagation())
                .child(Input::new(&input).id(("rename-input", ix)).small())
                .into_any_element()
        }
        None => div()
            .id(("sidebar-title", ix))
            .flex_1()
            .min_w_0()
            .truncate()
            .text_size(px(size::BASE))
            .text_color(if emphasized { p.text } else { p.text_muted })
            .when(status == Status::Unread, |t| {
                t.font_weight(FontWeight::MEDIUM)
            })
            .tooltip(move |window, cx| Tooltip::new(full_title.clone()).build(window, cx))
            .child(session.title())
            .test_support()
            .into_any_element(),
    };
    let row = div()
        .id(("session", ix))
        .aria_label(format!("Session: {}", session.title()))
        .tab_index(if is_renaming { -1 } else { 0 })
        .focus_visible(|style| style.border_2().border_color(p.accent))
        .mx(px(8.))
        .px(px(12.))
        .py(px(9.))
        .rounded(px(9.))
        .flex()
        .gap(px(11.))
        .cursor_pointer()
        .when(active, |row| row.bg(white(0.075)))
        .when(!active, |row| row.hover(|style| style.bg(white(0.04))))
        .on_click(cx.listener(move |this, _, window, cx| {
            if !is_renaming {
                this.select_session(ix, window, cx);
            }
        }))
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |this, _, _, cx| {
                this.session_menu = Some(ix);
                cx.notify();
            }),
        )
        .child(
            div()
                .w(px(16.))
                .h(px(22.))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .child(glyph),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(
                    div()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(title)
                        .child(div().flex_shrink_0().child(ui::label(
                            ui::ago(ago),
                            size::XS,
                            p.text_subtle,
                        ))),
                )
                .child(
                    div()
                        .id(("sidebar-subtitle", ix))
                        .min_w_0()
                        .truncate()
                        .text_size(px(size::XS))
                        .text_color(subtitle_color)
                        .tooltip(move |window, cx| {
                            Tooltip::new(full_subtitle.clone()).build(window, cx)
                        })
                        .child(subtitle)
                        .test_support(),
                ),
        )
        .child(
            div()
                .id(("session-actions", ix))
                .aria_label(format!("Actions for {}", session.title()))
                .tab_index(0)
                .focus_visible(|style| style.border_1().border_color(p.accent))
                .size(px(24.))
                .flex_shrink_0()
                .rounded(px(5.))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .hover(|style| style.bg(white(0.08)))
                .tooltip(|window, cx| Tooltip::new("Session actions").build(window, cx))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.session_menu = if this.session_menu == Some(ix) {
                        None
                    } else {
                        Some(ix)
                    };
                    cx.stop_propagation();
                    cx.notify();
                }))
                .child(ui::icon(IconName::Ellipsis, 14., p.text_muted))
                .test_support(),
        )
        .test_support();
    let item = |id: &'static str, icon: IconName, label: &'static str, color: Hsla| {
        div()
            .id((id, ix))
            .aria_label(label)
            .tab_index(0)
            .focus_visible(|style| style.border_2().border_color(p.accent))
            .h(px(34.))
            .px(px(12.))
            .flex()
            .items_center()
            .gap(px(10.))
            .rounded(px(7.))
            .cursor_pointer()
            .hover(|s| s.bg(white(0.06)))
            .child(ui::icon(icon, 14., color))
            .child(div().flex_1().min_w_0().truncate().child(ui::label(
                label,
                size::BASE - 1.,
                color,
            )))
    };
    let container = div()
        .id(("session-container", session.uid))
        .flex()
        .flex_col()
        .child(row)
        .when(app.session_menu == Some(ix), |container| {
            container.child(
                div()
                    .mx(px(14.))
                    .my(px(4.))
                    .p(px(4.))
                    .rounded(px(10.))
                    .border_1()
                    .border_color(p.border_strong)
                    .bg(p.surface)
                    .shadow_lg()
                    .child(
                        item("rename-session", IconName::Pencil, "Rename", p.text)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.start_rename(ix, window, cx)
                            }))
                            .test_support(),
                    )
                    .child(
                        item(
                            "delete-session",
                            IconName::Archive,
                            if session.view.running && app.archive_confirm == Some(session.uid) {
                                "Stop work first?"
                            } else if session.stopping {
                                "Archive after engine saves"
                            } else {
                                "Archive (Undo this run)"
                            },
                            p.danger,
                        )
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.delete_session(ix, window, cx)
                        }))
                        .test_support(),
                    ),
            )
        });
    ScrollableRow {
        id: ("session-scroll-row", session.uid).into(),
        row: container,
        scroll: app.sidebar_scroll.clone(),
    }
    .into_any_element()
}
