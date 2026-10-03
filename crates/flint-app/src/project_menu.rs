//! The composer's "+" menu: open a project folder, jump to a recent one, or
//! attach a file. Choosing a folder sets the workspace of a session that
//! hasn't started yet, or opens a new session there.

use std::path::Path;
use std::path::PathBuf;

use flint_agent::Op;
use gpui_kit::assets::IconName;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::app::FlintApp;
use crate::session::folder_name;
use crate::theme::palette;
use crate::theme::size;
use crate::ui;

/// Recent folders shown in the menu.
const RECENT_FOLDERS: usize = 6;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectItem {
    OpenFolder,
    Recent(PathBuf),
    AttachFile,
}

impl FlintApp {
    /// Menu rows: open, recent folders (newest first, not the current one), attach.
    pub fn project_items(&self) -> Vec<ProjectItem> {
        let current = &self.session().workspace;
        let mut sessions: Vec<_> = self.sessions.iter().collect();
        sessions.sort_by_key(|s| std::cmp::Reverse(s.touched));
        let mut recent: Vec<PathBuf> = Vec::new();
        for session in sessions {
            let path = &session.workspace;
            if path != current && !recent.contains(path) && path.is_dir() {
                recent.push(path.clone());
            }
        }
        recent.truncate(RECENT_FOLDERS);
        std::iter::once(ProjectItem::OpenFolder)
            .chain(recent.into_iter().map(ProjectItem::Recent))
            .chain(std::iter::once(ProjectItem::AttachFile))
            .collect()
    }

    pub fn toggle_project_menu(&mut self, cx: &mut Context<Self>) {
        self.project_menu = match self.project_menu {
            Some(_) => None,
            None => Some(0),
        };
        self.mention = None;
        self.slash = None;
        self.agent_menu = false;
        self.option_menu = None;
        cx.notify();
    }

    pub fn pick_project_item(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.project_menu = None;
        match self.project_items().into_iter().nth(ix) {
            Some(ProjectItem::OpenFolder) => self.open_workspace(cx),
            Some(ProjectItem::Recent(path)) => self.set_project_folder(path, cx),
            Some(ProjectItem::AttachFile) => self.open_mention_picker(window, cx),
            None => {}
        }
        cx.notify();
    }

    /// Keys while the menu is open; returns whether the key was consumed.
    pub fn project_menu_key(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(selected) = self.project_menu else {
            return false;
        };
        let len = self.project_items().len();
        match key {
            "up" => self.project_menu = Some(selected.saturating_sub(1)),
            "down" => self.project_menu = Some((selected + 1).min(len.saturating_sub(1))),
            "enter" | "tab" => self.pick_project_item(selected, window, cx),
            "escape" => self.project_menu = None,
            _ => return false,
        }
        cx.notify();
        true
    }

    /// Makes `path` the project: the active session's workspace when it has
    /// no messages yet (restarting an agent that was started early), else a
    /// new session with the same agent in that folder.
    pub fn set_project_folder(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.workspace = path.clone();
        let ix = self.active;
        let agent = self.sessions[ix].agent;
        if self.sessions[ix].view.items.is_empty() {
            let session = &mut self.sessions[ix];
            if let Some(ops) = session.ops.take() {
                ops.try_send(Op::Shutdown).ok();
            }
            session.pump = None;
            session.options.clear();
            session.agent_ready = false;
            session.dir = None;
            session.workspace = path;
            self.start_agent_early(ix, cx);
        } else {
            let mut session = self.new_session_value(path);
            session.agent = agent;
            self.sessions.push(session);
            self.active = self.sessions.len() - 1;
            let ix = self.active;
            self.start_agent_early(ix, cx);
        }
        self.selected_change = None;
        cx.notify();
    }
}

fn item_label(item: &ProjectItem) -> (IconName, String, Option<String>) {
    match item {
        ProjectItem::OpenFolder => (IconName::FolderOpen, "Open project folder…".to_string(), Some("⌘O".to_string())),
        ProjectItem::Recent(path) => (IconName::Folder, folder_name(path), Some(short_path(path))),
        ProjectItem::AttachFile => (IconName::Paperclip, "Attach file…".to_string(), Some("@".to_string())),
    }
}

/// `~/code/app` style path for the menu.
fn short_path(path: &Path) -> String {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    match home.as_deref().and_then(|home| path.strip_prefix(home).ok()) {
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

/// The "+" menu panel, when open.
pub fn render(app: &FlintApp, cx: &mut Context<FlintApp>) -> Option<AnyElement> {
    let p = palette();
    let selected = app.project_menu?;
    let items = app.project_items();
    let recent_start = 1;
    let has_recent = items.iter().any(|i| matches!(i, ProjectItem::Recent(_)));
    let rows = items.iter().enumerate().map(|(n, item)| {
        let (icon, label, detail) = item_label(item);
        div()
            .id(("project-item", n))
            .mx(px(6.))
            .px(px(10.))
            .h(px(36.))
            .rounded(px(8.))
            .flex()
            .items_center()
            .gap(px(10.))
            .cursor_pointer()
            .when(n == selected, |row| row.bg(p.raised))
            .on_click(cx.listener(move |this, _, window, cx| this.pick_project_item(n, window, cx)))
            .child(ui::icon(icon, 15., p.text_muted))
            .child(ui::label(label, size::SM, p.text))
            .child(div().flex_1())
            .children(detail.map(|d| ui::label(d, size::XS, p.text_subtle)))
            .test_support()
    });
    let mut column: Vec<AnyElement> = Vec::new();
    for (n, row) in rows.enumerate() {
        if n == recent_start && has_recent {
            column.push(
                div()
                    .px(px(16.))
                    .pt(px(6.))
                    .pb(px(2.))
                    .child(ui::label("Recent folders", size::XS, p.text_subtle))
                    .into_any_element(),
            );
        }
        column.push(row.into_any_element());
    }
    Some(
        div()
            .id("project-menu")
            .w(px(380.))
            .rounded(px(14.))
            .border_1()
            .border_color(p.border_strong)
            .bg(p.surface)
            .shadow_lg()
            .py(px(8.))
            .flex()
            .flex_col()
            .children(column)
            .test_support()
            .into_any_element(),
    )
}
