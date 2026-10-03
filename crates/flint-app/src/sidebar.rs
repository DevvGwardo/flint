//! The floating sidebar: new agent, search, sessions grouped by workspace with
//! live status glyphs, and filter/settings at the bottom.

use std::time::SystemTime;

use gpui_kit::assets::IconName;
use gpui_kit::component::input::Input;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::app::FlintApp;
use crate::app::NewSession;
use crate::app::OpenWorkspace;
use crate::app::SessionFilter;
use crate::app::ToggleSidebar;
use crate::app::ToggleTerminal;
use crate::layout::SIDEBAR_WIDTH;
use crate::session::Status;
use crate::session::folder_name;
use crate::theme::palette;
use crate::theme::size;
use crate::ui;

fn white(alpha: f32) -> Hsla {
    hsla(0., 0., 1., alpha)
}

pub fn render(app: &FlintApp, cx: &mut Context<FlintApp>) -> impl IntoElement {
    let p = palette();

    // Traffic lights sit in this row; the rest of it drags the window.
    let top = app.drag_region(
        div()
            .id("sidebar-top")
            .h(px(46.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_end()
            .px(px(8.))
            .child(
                div()
                    .id("hide-sidebar")
                    .size(px(32.))
                    .rounded(px(8.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .hover(|style| style.bg(white(0.06)))
                    .on_click(cx.listener(|_, _, window, cx| {
                        window.dispatch_action(Box::new(ToggleSidebar), cx);
                    }))
                    .child(ui::icon(IconName::PanelLeft, 16., p.text_subtle)),
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

    let search = div().mx(px(10.)).mt(px(12.)).mb(px(4.)).child(
        Input::new(&app.search)
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
        let rows: Vec<_> = rows
            .into_iter()
            .map(|ix| session_row(app, ix, cx))
            .collect();
        list.push(
            div()
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
                                .text_size(px(size::SM))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(p.text_subtle)
                                .child(folder_name(&workspace)),
                        ),
                )
                .children(rows),
        );
    }

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
                .child(ui::label(filter_label, size::XS, p.text_subtle)),
        )
        .child(
            nav_row("settings", IconName::Settings, "Settings", Some("⌘,"))
                .on_click(cx.listener(|this, _, window, cx| this.open_settings(window, cx)))
                .test_support(),
        );

    div()
        .w(px(SIDEBAR_WIDTH))
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
                .id("session-list")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .pb(px(8.))
                .children(list),
        )
        .child(bottom)
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
        .mx(px(8.))
        .px(px(12.))
        .h(px(38.))
        .rounded(px(9.))
        .flex()
        .items_center()
        .gap(px(11.))
        .cursor_pointer()
        .hover(|style| style.bg(white(0.05)))
        .child(ui::icon(icon, 16., p.text_muted))
        .child(div().flex_1().child(ui::label(label, size::BASE, p.text)))
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
    let title: AnyElement = match renaming {
        Some(input) => Input::new(&input).small().into_any_element(),
        None => div()
            .flex_1()
            .min_w_0()
            .truncate()
            .text_size(px(size::BASE))
            .text_color(if emphasized { p.text } else { p.text_muted })
            .when(status == Status::Unread, |t| {
                t.font_weight(FontWeight::MEDIUM)
            })
            .child(session.title())
            .into_any_element(),
    };
    let row = div()
        .id(("session", ix))
        .mx(px(8.))
        .px(px(12.))
        .py(px(9.))
        .rounded(px(9.))
        .flex()
        .gap(px(11.))
        .cursor_pointer()
        .when(active, |row| row.bg(white(0.075)))
        .when(!active, |row| row.hover(|style| style.bg(white(0.04))))
        .on_click(cx.listener(move |this, _, window, cx| this.select_session(ix, window, cx)))
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
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(title)
                        .child(ui::label(ui::ago(ago), size::XS, p.text_subtle)),
                )
                .child(
                    div()
                        .truncate()
                        .text_size(px(size::XS))
                        .text_color(subtitle_color)
                        .child(subtitle),
                ),
        )
        .test_support();
    if app.session_menu != Some(ix) {
        return row.into_any_element();
    }
    let item = |id: &'static str, icon: IconName, label: &'static str, color: Hsla| {
        div()
            .id((id, ix))
            .h(px(34.))
            .px(px(12.))
            .flex()
            .items_center()
            .gap(px(10.))
            .rounded(px(7.))
            .cursor_pointer()
            .hover(|s| s.bg(white(0.06)))
            .child(ui::icon(icon, 14., color))
            .child(ui::label(label, size::BASE - 1., color))
    };
    div()
        .flex()
        .flex_col()
        .child(row)
        .child(
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
                        .on_click(
                            cx.listener(move |this, _, window, cx| {
                                this.start_rename(ix, window, cx)
                            }),
                        )
                        .test_support(),
                )
                .child(
                    item("delete-session", IconName::Trash, "Delete", p.danger)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.delete_session(ix, window, cx)
                        }))
                        .test_support(),
                ),
        )
        .into_any_element()
}
