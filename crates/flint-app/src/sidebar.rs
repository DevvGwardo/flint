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

use crate::app::ARCHIVE_PAGE;
use crate::app::FlintApp;
use crate::app::NewSession;
use crate::app::OpenWorkspace;
use crate::app::SESSION_PAGE;
use crate::app::SessionFilter;
use crate::app::SessionGrouping;
use crate::app::ToggleSidebar;
use crate::app::ToggleTerminal;
use crate::project;
use crate::session::Bucket;
use crate::session::Status;
use crate::session::Tone;
use crate::session_groups::GroupKey;
use crate::session_groups::SessionGroups;
use crate::theme::palette;
use crate::theme::size;
use crate::ui;

/// Minimum content width for full filter names and footer controls.
pub const MIN_WIDTH: f32 = 264.;

fn white(alpha: f32) -> Hsla {
    hsla(0., 0., 1., alpha)
}

struct RowFocus {
    handle: FocusHandle,
    last: Option<FocusHandle>,
}

#[derive(IntoElement)]
pub(crate) struct ScrollableRow {
    pub(crate) id: ElementId,
    pub(crate) row: Stateful<Div>,
    pub(crate) scroll: ScrollHandle,
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
            .test_support()
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
                    .role(gpui_kit::Role::Button)
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

    // Headings per the chosen grouping. A heading counts "n of N" when
    // search, the filter or paging hides some of its sessions.
    let SessionGroups { groups, more } = app.session_groups(cx);
    let mut list: Vec<AnyElement> = Vec::new();
    for (g, group) in groups.into_iter().enumerate() {
        let icon = match group.key {
            GroupKey::Project(..) => ui::icon(IconName::Folder, 13., p.text_subtle),
            GroupKey::Status(Bucket::NeedsYou) => ui::icon(IconName::ShieldAlert, 13., p.warning),
            GroupKey::Status(Bucket::Working) => ui::icon(IconName::Loader, 13., p.accent),
            GroupKey::Status(Bucket::Ready) => ui::icon(IconName::Check, 13., p.text_subtle),
            GroupKey::Status(Bucket::Inactive) => ui::icon(IconName::Circle, 13., p.text_subtle),
            GroupKey::Agent(_) => ui::icon(IconName::Bot, 13., p.text_subtle),
        };
        let shown = group.rows.len();
        let total = group.total;
        let tooltip = group.tooltip.clone();
        // Project and agent headings say how many of theirs are working or
        // waiting, so a busy group stands out even when scrolled past.
        let (working, waiting) = if matches!(group.key, GroupKey::Status(_)) {
            (0, 0)
        } else {
            group.rows.iter().fold((0, 0), |(w, n), &ix| {
                match app.sessions[ix].status().bucket() {
                    Bucket::Working => (w + 1, n),
                    Bucket::NeedsYou => (w, n + 1),
                    Bucket::Ready | Bucket::Inactive => (w, n),
                }
            })
        };
        let rows: Vec<_> = group
            .rows
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
                        .child(icon)
                        .child(
                            div()
                                .id(("workspace-label", g))
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(px(size::SM))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(p.text_subtle)
                                .when_some(tooltip, |label, tip| {
                                    label.tooltip(move |window, cx| {
                                        Tooltip::new(tip.clone()).build(window, cx)
                                    })
                                })
                                .child(group.label)
                                .test_support(),
                        )
                        .when(waiting > 0, |header| {
                            header.child(
                                div()
                                    .id(("group-waiting", g))
                                    .flex_shrink_0()
                                    .flex()
                                    .items_center()
                                    .gap(px(3.))
                                    .tooltip(move |window, cx| {
                                        Tooltip::new(format!("{waiting} need you"))
                                            .build(window, cx)
                                    })
                                    .child(ui::icon(IconName::CircleAlert, 11., p.warning))
                                    .child(ui::label(waiting.to_string(), size::XS, p.warning))
                                    .test_support(),
                            )
                        })
                        .when(working > 0, |header| {
                            header.child(
                                div()
                                    .id(("group-working", g))
                                    .flex_shrink_0()
                                    .flex()
                                    .items_center()
                                    .gap(px(3.))
                                    .tooltip(move |window, cx| {
                                        Tooltip::new(format!("{working} working")).build(window, cx)
                                    })
                                    .child(ui::spinner(app.now(), 11., p.accent))
                                    .child(ui::label(working.to_string(), size::XS, p.accent))
                                    .test_support(),
                            )
                        })
                        .when(shown < total, |header| {
                            header.child(div().flex_shrink_0().child(ui::label(
                                format!("{shown} of {total}"),
                                size::XS,
                                p.text_subtle,
                            )))
                        }),
                )
                .children(rows)
                .into_any_element(),
        );
    }
    if more > 0 {
        list.push(
            show_more(
                "show-more-sessions",
                more.min(SESSION_PAGE),
                more,
                cx.listener(|this, _, _, cx| {
                    this.session_limit += SESSION_PAGE;
                    cx.notify();
                }),
            )
            .into_any_element(),
        );
    }
    let empty = list.is_empty();
    let searching = !app.search.read(cx).value().trim().is_empty();
    let empty_label = if searching {
        "No matching sessions"
    } else {
        match app.filter {
            SessionFilter::All => "No sessions",
            SessionFilter::Only(Bucket::NeedsYou) => "Nothing needs you",
            SessionFilter::Only(Bucket::Working) => "No sessions working",
            SessionFilter::Only(Bucket::Ready) => "No sessions ready",
            SessionFilter::Only(Bucket::Inactive) => "No inactive sessions",
        }
    };

    // Equal-width columns and bounded count slots keep live updates from
    // moving filter targets or changing the list's available height.
    let counts = app.bucket_counts(cx);
    let chips: Vec<_> = [
        SessionFilter::All,
        SessionFilter::Only(Bucket::NeedsYou),
        SessionFilter::Only(Bucket::Working),
        SessionFilter::Only(Bucket::Ready),
    ]
    .into_iter()
    .enumerate()
    .map(|(ix, filter)| {
        let label = match filter {
            SessionFilter::All => "All",
            SessionFilter::Only(bucket) => bucket.label(),
        };
        let count_color = match filter {
            SessionFilter::Only(Bucket::NeedsYou) if counts.of(filter) > 0 => p.warning,
            SessionFilter::Only(Bucket::Working) if counts.of(filter) > 0 => p.accent,
            _ => p.text_subtle,
        };
        filter_chip(
            ix,
            label,
            counts.of(filter),
            filter == app.filter,
            count_color,
        )
        .on_click(cx.listener(move |this, _, _, cx| this.set_filter(filter, cx)))
        .test_support()
    })
    .collect();
    let grouping = app.grouping;
    let group_toggle = div()
        .id("session-grouping")
        .role(gpui_kit::Role::Button)
        .aria_label(format!("Group by {}", grouping.label()))
        .tab_index(0)
        .focus_visible(|style| style.border_1().border_color(p.accent))
        .tooltip(move |window, cx| {
            Tooltip::new(format!(
                "Grouped by {}  ⇧⌘G\nClick to group by {}",
                grouping.label(),
                grouping.next().label()
            ))
            .build(window, cx)
        })
        .w(px(98.))
        .flex_shrink_0()
        .h(px(24.))
        .px(px(8.))
        .rounded(px(6.))
        .flex()
        .items_center()
        .gap(px(5.))
        .cursor_pointer()
        .hover(|style| style.bg(white(0.06)))
        .on_click(cx.listener(|this, _, _, cx| this.set_grouping(this.grouping.next(), cx)))
        .child(ui::icon(IconName::Layers, 13., p.text_subtle))
        .child(div().min_w_0().truncate().child(ui::label(
            grouping.label(),
            size::XS,
            p.text_subtle,
        )))
        .test_support();
    let mut chips = chips.into_iter();
    let first_filters = div().flex().gap(px(4.)).children(chips.by_ref().take(2));
    let remaining_filters = div().flex().gap(px(4.)).children(chips);
    let filters = div()
        .id("session-filters")
        .flex_shrink_0()
        .mx(px(10.))
        .mb(px(2.))
        .flex()
        .flex_col()
        .gap(px(4.))
        .child(first_filters)
        .child(remaining_filters);
    let bottom = div()
        .flex_shrink_0()
        .py(px(8.))
        .border_t_1()
        .border_color(white(0.05))
        .child(
            div()
                .flex()
                .items_center()
                .pr(px(8.))
                .child(
                    nav_row("settings", IconName::Settings, "Settings", Some("⌘,"))
                        .flex_1()
                        .min_w_0()
                        .on_click(cx.listener(|this, _, window, cx| this.open_settings(window, cx)))
                        .test_support(),
                )
                .child(group_toggle),
        );

    div()
        .id("sidebar")
        .w_full()
        .h_full()
        .flex()
        .flex_col()
        .rounded(px(14.))
        .bg(if app.session_drawer && !app.sidebar_visible {
            p.chrome
        } else {
            p.sidebar_fill
        })
        .border_1()
        .border_color(white(0.06))
        .overflow_hidden()
        .child(top)
        .child(new_agent)
        .child(open_folder)
        .child(terminal)
        .child(search)
        .child(filters)
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
                        format!("Archived · {}", app.archives.len()),
                        size::SM,
                        p.text_muted,
                    )))
                    .child(
                        div()
                            .relative()
                            .child(
                                div()
                                    .id("archive-list")
                                    // The compact drawer loses the titlebar's height;
                                    // keep that space available to current sessions.
                                    .max_h(px(if app.session_drawer && !app.sidebar_visible {
                                        76.
                                    } else {
                                        112.
                                    }))
                                    .overflow_y_scroll()
                                    .track_scroll(&app.archive_scroll)
                                    .pr(px(12.))
                                    .children(
                                        app.archives
                                            .iter()
                                            .enumerate()
                                            .take(app.archive_limit)
                                            .map(|(ix, (_, meta))| {
                                                let title = meta
                                                    .title
                                                    .as_deref()
                                                    .unwrap_or("session")
                                                    .to_string();
                                                let tip = format!(
                                                    "Restore {title}\n{}",
                                                    meta.workspace.display()
                                                );
                                                let row = div()
                                                    .id(("restore-archive", ix))
                                                    .role(gpui_kit::Role::Button)
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
                                                    row: div()
                                                        .id(("archive-container", ix))
                                                        .child(row),
                                                    scroll: app.archive_scroll.clone(),
                                                }
                                            }),
                                    )
                                    .when(app.archives.len() > app.archive_limit, |list| {
                                        let hidden = app.archives.len() - app.archive_limit;
                                        list.child(show_more(
                                            "show-more-archives",
                                            hidden.min(ARCHIVE_PAGE),
                                            hidden,
                                            cx.listener(|this, _, _, cx| {
                                                this.archive_limit += ARCHIVE_PAGE;
                                                cx.notify();
                                            }),
                                        ))
                                    })
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

/// The row that reveals the next page of a long list.
fn show_more(
    id: &'static str,
    next: usize,
    hidden: usize,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let p = palette();
    div()
        .id(id)
        .role(gpui_kit::Role::Button)
        .aria_label(format!("Show {next} more"))
        .tab_index(0)
        .focus_visible(|style| style.border_2().border_color(p.accent))
        .mx(px(8.))
        .mt(px(8.))
        .px(px(12.))
        .h(px(32.))
        .rounded(px(8.))
        .flex()
        .items_center()
        .justify_between()
        .cursor_pointer()
        .hover(|style| style.bg(white(0.05)))
        .on_click(on_click)
        .child(ui::label(
            format!("Show {next} more"),
            size::SM,
            p.text_muted,
        ))
        .child(ui::label(
            format!("{hidden} hidden"),
            size::XS,
            p.text_subtle,
        ))
        .test_support()
}

/// One choice in the filter strip, with how many sessions it would show.
fn filter_chip(
    ix: usize,
    label: &'static str,
    count: usize,
    selected: bool,
    count_color: Hsla,
) -> Stateful<Div> {
    let p = palette();
    let empty = count == 0 && !selected;
    div()
        .id(("session-filter", ix))
        .role(gpui_kit::Role::Button)
        .aria_label(format!("{label}, {count}"))
        // Empty chips stay clickable but out of the tab order.
        .tab_index(if empty { -1 } else { 0 })
        .focus_visible(|style| style.border_1().border_color(p.accent))
        .flex_1()
        .min_w_0()
        .h(px(24.))
        .px(px(8.))
        .rounded(px(6.))
        .flex()
        .items_center()
        .gap(px(5.))
        .cursor_pointer()
        .bg(white(if selected { 0.09 } else { 0. }))
        .hover(|style| style.bg(white(0.06)))
        .child(div().flex_1().min_w_0().truncate().child(ui::label(
            label,
            size::XS,
            if selected { p.text } else { p.text_subtle },
        )))
        .child(
            div()
                .w(px(30.))
                .flex_shrink_0()
                .text_right()
                .child(ui::label(
                    if count > 999 {
                        "999+".to_string()
                    } else {
                        count.to_string()
                    },
                    size::XS,
                    count_color,
                )),
        )
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
        .role(gpui_kit::Role::Button)
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

/// A session's branch and token total, for the row's third line; `None` when
/// it has neither.
fn details_line(session: &crate::session::Session) -> Option<(Option<String>, Option<String>)> {
    let branch = project::branch(&session.workspace);
    let tokens = session.total_tokens();
    let usage = (tokens > 0).then(|| format!("{} tokens", ui::tokens(tokens)));
    (branch.is_some() || usage.is_some()).then_some((branch, usage))
}

fn session_row(app: &FlintApp, ix: usize, cx: &mut Context<FlintApp>) -> AnyElement {
    let p = palette();
    let session = &app.sessions[ix];
    let active = ix == app.active && session.selected_subagent.is_none();
    let status = session.status();
    let now = app.now();
    let glyph: AnyElement = match status {
        Status::Running => ui::spinner(now, 13., p.accent).into_any_element(),
        Status::Starting => ui::spinner(now, 13., p.text_subtle).into_any_element(),
        Status::NeedsApproval => ui::icon(IconName::ShieldAlert, 13., p.warning).into_any_element(),
        Status::Failed { seen } => ui::icon(
            IconName::CircleX,
            13.,
            if seen { p.text_subtle } else { p.danger },
        )
        .into_any_element(),
        Status::Stopped => ui::icon(IconName::CircleStop, 12., p.text_subtle).into_any_element(),
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
    let (subtitle, tone) = session.status_line(now);
    let subtitle_color = match tone {
        Tone::Muted => p.text_subtle,
        Tone::Warning => p.warning,
        Tone::Danger if matches!(status, Status::Failed { seen: true }) => p.text_subtle,
        Tone::Danger => p.danger,
    };
    // ACP sessions carry their agent's name, e.g. "Claude Code · Answered".
    let subtitle = match session.agent {
        flint_agent::AgentKind::Flint => subtitle,
        agent => format!("{} · {subtitle}", agent.label()),
    };
    // Under another grouping the heading no longer says which project it is.
    let subtitle = if app.grouping == SessionGrouping::Project {
        subtitle
    } else {
        format!(
            "{} · {subtitle}",
            project::identity(&session.workspace).label()
        )
    };
    let emphasized = active
        || matches!(
            status,
            Status::Unread | Status::NeedsApproval | Status::Failed { seen: false }
        );
    // A running session shows how long its turn has taken; others, when
    // they were last active.
    let (when, when_color) = match session.running_for(now) {
        Some(elapsed) => (crate::session::clock(elapsed), p.accent),
        None => (
            ui::ago(
                SystemTime::now()
                    .duration_since(session.touched)
                    .unwrap_or_default(),
            ),
            p.text_subtle,
        ),
    };
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
            .text_color(ui::tinted(
                if emphasized { p.text } else { p.text_muted },
                session.color_seed(),
            ))
            .when(
                matches!(status, Status::Unread | Status::Failed { seen: false }),
                |t| t.font_weight(FontWeight::MEDIUM),
            )
            .tooltip(move |window, cx| Tooltip::new(full_title.clone()).build(window, cx))
            .child(session.title())
            .test_support()
            .into_any_element(),
    };
    let row = div()
        .id(("session", ix))
        .role(gpui_kit::Role::Button)
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
                        .child(title),
                )
                .child(
                    div()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(
                            div()
                                .id(("sidebar-subtitle", ix))
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(px(size::XS))
                                .text_color(subtitle_color)
                                .tooltip(move |window, cx| {
                                    Tooltip::new(full_subtitle.clone()).build(window, cx)
                                })
                                .child(subtitle)
                                .test_support(),
                        )
                        .child(
                            div()
                                .id(("sidebar-when", ix))
                                .flex_shrink_0()
                                .child(ui::label(when, size::XS, when_color))
                                .test_support(),
                        ),
                )
                .when_some(details_line(session), |col, (branch, usage)| {
                    col.child(
                        div()
                            .id(("sidebar-details", ix))
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .text_size(px(size::XS))
                            .text_color(p.text_subtle)
                            .when_some(branch, |line, branch| {
                                line.child(
                                    div()
                                        .id(("sidebar-branch", ix))
                                        .min_w_0()
                                        .flex()
                                        .items_center()
                                        .gap(px(3.))
                                        .child(ui::icon(IconName::GitBranch, 11., p.text_subtle))
                                        .child(div().min_w_0().truncate().child(branch))
                                        .test_support(),
                                )
                            })
                            .when_some(usage, |line, usage| {
                                line.child(
                                    div()
                                        .id(("sidebar-usage", ix))
                                        .flex_shrink_0()
                                        .child(usage)
                                        .test_support(),
                                )
                            })
                            .test_support(),
                    )
                }),
        )
        .child(
            div()
                .id(("session-actions", ix))
                .role(gpui_kit::Role::Button)
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
    let row = if is_renaming {
        row
    } else {
        crate::session_workspace::drag_row(row, session.uid, session.title(), cx)
    };
    let item = |id: &'static str, icon: IconName, label: &'static str, color: Hsla| {
        div()
            .id((id, ix))
            .role(gpui_kit::Role::Button)
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
                            if session.stopping {
                                "Archive after engine saves"
                            } else {
                                "Archive"
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
    div()
        .flex()
        .flex_col()
        .child(ScrollableRow {
            id: ("session-scroll-row", session.uid).into(),
            row: container,
            scroll: app.sidebar_scroll.clone(),
        })
        .when(!session.view.subagents.is_empty(), |container| {
            container.child(crate::subagent_ui::sidebar_children(app, session, cx))
        })
        .into_any_element()
}
