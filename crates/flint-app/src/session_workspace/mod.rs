//! Session panes: one live session per tile, with independent terminal choices.

pub mod model;
mod render;

use std::collections::HashMap;
use std::path::PathBuf;

use gpui_kit::component::input::{InputEvent, TextareaState};
use gpui_kit::*;
use serde::{Deserialize, Serialize};

use crate::app::FlintApp;
use crate::docking::Edge;
use crate::term_view::TermView;

pub use model::{Layout, Node, PaneMode};
pub use render::{drag_row, render, targets};

pub struct PaneState {
    pub mode: PaneMode,
    pub terminal: Option<EntityId>,
    /// Follow new agent commands until the user chooses a terminal tab.
    pub(crate) follow_commands: bool,
    /// The user asked for the terminal half. Until then a `Both` pane shows
    /// the collapsed strip, even when agent commands have mirrored tabs.
    pub(crate) terminal_shown: bool,
    pub focus: FocusHandle,
    pub bounds: std::rc::Rc<std::cell::Cell<Option<Bounds<Pixels>>>>,
    pub(crate) _focus_subscription: Subscription,
}

pub struct Draft {
    composer: Entity<TextareaState>,
    attachments: Vec<String>,
    images: Vec<PathBuf>,
    pasted_files: Vec<tempfile::NamedTempFile>,
}

#[derive(Default)]
pub struct Workspace {
    pub layout: Layout,
    pub panes: HashMap<u64, PaneState>,
    pub dragging: Option<u64>,
    pub revision: u64,
    pub bounds: std::rc::Rc<std::cell::Cell<Option<Bounds<Pixels>>>>,
    pub(crate) pending_selection: Option<(u64, u64)>,
    drafts: HashMap<u64, Draft>,
    pub(crate) split_terminals: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Saved {
    version: u32,
    root: Option<Node<String>>,
    active: Option<String>,
    modes: Vec<(String, PaneMode)>,
}

impl Workspace {
    pub fn tiled(&self) -> bool {
        self.layout.tiled()
    }

    pub fn visible(&self, uid: u64) -> bool {
        self.layout.contains(uid)
    }
}

impl FlintApp {
    pub(crate) fn ensure_session_pane(
        &mut self,
        uid: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.session_workspace.panes.contains_key(&uid) || self.session_index(uid).is_none() {
            return;
        }
        let focus = cx.focus_handle().tab_stop(true);
        let terminal = self
            .terminal
            .tabs
            .iter()
            .rev()
            .find(|view| self.terminal_owner(view, cx) == Some(uid))
            .cloned();
        let follow_commands = terminal.as_ref().is_none_or(|view| view.read(cx).read_only);
        let terminal = terminal.as_ref().map(Entity::entity_id);
        // Agent command mirrors never open a terminal on their own: the
        // terminal half starts expanded only for a shell the user opened.
        let terminal_shown = self.has_user_shell(uid, cx);
        let subscription = cx.on_focus_in(&focus, window, move |app, window, cx| {
            if app.session_workspace.tiled() && app.session().uid != uid {
                app.focus_session_pane(uid, false, window, cx);
            }
        });
        self.session_workspace.panes.insert(
            uid,
            PaneState {
                mode: PaneMode::Both,
                terminal,
                terminal_shown,
                follow_commands,
                focus,
                bounds: Default::default(),
                _focus_subscription: subscription,
            },
        );
    }

    /// A sidebar click replaces the focused tile unless its session is already visible.
    pub(crate) fn follow_session_selection(
        &mut self,
        old: u64,
        new: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Drafts belong to sessions even when only one session is visible.
        self.swap_pane_draft(old, new, window, cx);
        if self.session_workspace.tiled() {
            if !self.session_workspace.visible(new) {
                self.session_workspace.layout.replace(old, new);
                self.session_workspace.revision += 1;
            }
            self.ensure_session_pane(new, window, cx);
        } else {
            self.session_workspace.layout.root = Some(Node::Session { session: new });
        }
    }

    fn swap_pane_draft(&mut self, old: u64, new: u64, window: &mut Window, cx: &mut Context<Self>) {
        if old == new {
            return;
        }
        let draft = self
            .session_workspace
            .drafts
            .remove(&new)
            .unwrap_or_else(|| {
                let composer = cx.new(|cx| {
                    TextareaState::new(window, cx)
                        .auto_grow(1, 8)
                        .submit_on_enter(true)
                        .placeholder("Ask anything, @ to mention, / for actions")
                });
                self.subscriptions.push(cx.subscribe_in(
                    &composer,
                    window,
                    |this, composer, event: &InputEvent, window, cx| {
                        // Hidden drafts can still emit deferred blur/change events.
                        if &this.composer != composer {
                            return;
                        }
                        match event {
                            InputEvent::PressEnter {
                                secondary: true,
                                shift: false,
                            } if !this.session().view.has_pending_approval() => {
                                this.steer_draft(window, cx)
                            }
                            InputEvent::PressEnter { shift: false, .. } => this.submit(window, cx),
                            InputEvent::Change => this.composer_changed(window, cx),
                            _ => {}
                        }
                    },
                ));
                Draft {
                    composer,
                    attachments: Vec::new(),
                    images: Vec::new(),
                    pasted_files: Vec::new(),
                }
            });
        let previous = Draft {
            composer: std::mem::replace(&mut self.composer, draft.composer),
            attachments: std::mem::replace(&mut self.attachments, draft.attachments),
            images: std::mem::replace(&mut self.image_attachments, draft.images),
            pasted_files: std::mem::replace(&mut self.pasted_image_files, draft.pasted_files),
        };
        self.session_workspace.drafts.insert(old, previous);
        self.mention = None;
        self.slash = None;
        self.option_menu = None;
        self.agent_menu = false;
        self.project_menu = None;
        self.help_open = false;
        self.copied = None;
        self.popover_room = Default::default();
        self.queue_popover = false;
        self.queue_edit = None;
        self.clear_file_preview();
    }

    pub fn focus_session_pane(
        &mut self,
        uid: u64,
        composer: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(ix) = self.session_index(uid) else {
            return;
        };
        if self.session().uid != uid {
            // select_session handles menus, drafts and changes without restarting engines.
            self.select_session_with_focus(ix, composer, window, cx);
        }
        if composer {
            self.focus_session_input(window, cx);
        }
        self.sessions[ix].unread = false;
        cx.notify();
    }

    pub(crate) fn focus_session_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sessions[self.active].selected_subagent = None;
        let uid = self.session().uid;
        if self.session_workspace.tiled()
            && let Some(pane) = self.session_workspace.panes.get(&uid)
            && pane.mode == PaneMode::Terminal
        {
            let focus = self
                .pane_terminal(uid)
                .map(|view| view.read(cx).focus.clone())
                .unwrap_or_else(|| pane.focus.clone());
            focus.focus(window, cx);
        } else {
            self.composer
                .update(cx, |state, cx| state.focus(window, cx));
        }
    }

    pub fn split_session_pane(
        &mut self,
        uid: u64,
        target: u64,
        edge: Edge,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.session_index(uid).is_none() || self.session_index(target).is_none() {
            return false;
        }
        if self.session_workspace.layout.root.is_none() {
            self.session_workspace.layout.root = Some(Node::Session {
                session: self.session().uid,
            });
        }
        if !self.session_workspace.layout.split(uid, target, edge) {
            if !self.session_workspace.visible(uid)
                && self.session_workspace.layout.sessions().len() >= model::MAX_PANES
            {
                self.store_error = Some(
                    "Up to eight session panes fit in one workspace. Close a pane to add another."
                        .into(),
                );
                cx.notify();
            }
            return false;
        }
        if !self.session_workspace.split_terminals {
            self.session_workspace.split_terminals = self.terminal.open;
        }
        self.ensure_session_pane(target, window, cx);
        self.ensure_session_pane(uid, window, cx);
        self.session_workspace.revision += 1;
        self.session_workspace.dragging = None;
        self.drag_armed = false;
        self.session_drawer = false;
        // Splitting only rearranges views: reuse shells the sessions already own,
        // but never start new ones (each would source the user's login profile).
        self.attach_pane_terminal(target, cx);
        self.attach_pane_terminal(uid, cx);
        self.focus_session_pane(uid, true, window, cx);
        self.save_session_layout();
        cx.notify();
        true
    }

    /// Closing a tile never closes its session, engine, shell, or history.
    pub fn close_session_pane(&mut self, uid: u64, window: &mut Window, cx: &mut Context<Self>) {
        if !self.session_workspace.tiled() || !self.session_workspace.layout.remove(uid) {
            return;
        }
        self.session_workspace.revision += 1;
        if self.session().uid == uid
            && let Some(next) = self.session_workspace.layout.sessions().first().copied()
        {
            let ix = self.session_index(next).expect("remaining session");
            self.select_session(ix, window, cx);
        }
        if !self.session_workspace.tiled() {
            self.restore_single_terminal(cx);
        }
        self.save_session_layout();
        cx.notify();
    }

    pub fn arrange_session_grid(&mut self, cx: &mut Context<Self>) {
        self.session_workspace.layout.grid();
        self.session_workspace.revision += 1;
        self.save_session_layout();
        cx.notify();
    }

    pub fn reset_session_panes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.session_workspace.layout.root = Some(Node::Session {
            session: self.session().uid,
        });
        self.session_workspace.revision += 1;
        self.session_workspace.dragging = None;
        self.restore_single_terminal(cx);
        self.save_session_layout();
        self.composer
            .update(cx, |state, cx| state.focus(window, cx));
        cx.notify();
    }

    pub fn set_session_pane_mode(
        &mut self,
        uid: u64,
        mode: PaneMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.ensure_session_pane(uid, window, cx);
        if let Some(pane) = self.session_workspace.panes.get_mut(&uid) {
            pane.mode = mode;
            pane.terminal_shown |= mode != PaneMode::Chat;
        }
        self.focus_session_pane(uid, mode == PaneMode::Chat, window, cx);
        if mode != PaneMode::Chat {
            self.ensure_pane_terminal(uid, window, cx);
            if mode == PaneMode::Terminal
                && let Some(view) = self.pane_terminal(uid)
            {
                let focus = view.read(cx).focus.clone();
                window.focus(&focus, cx);
            }
        }
        self.save_session_layout();
        cx.notify();
    }

    pub fn pane_terminal(&self, uid: u64) -> Option<&Entity<TermView>> {
        let selected = self
            .session_workspace
            .panes
            .get(&uid)
            .and_then(|pane| pane.terminal);
        selected.and_then(|id| {
            self.terminal
                .tabs
                .iter()
                .find(|view| view.entity_id() == id)
        })
    }

    /// Whether `uid` owns a shell the user opened (not an agent's mirror).
    pub(crate) fn has_user_shell(&self, uid: u64, cx: &App) -> bool {
        self.terminal.tabs.iter().any(|view| {
            let view = view.read(cx);
            view.agent.is_none() && view.session_uid == Some(uid)
        })
    }

    pub(crate) fn terminal_owner(&self, view: &Entity<TermView>, cx: &App) -> Option<u64> {
        let view = view.read(cx);
        view.session_uid
            .or_else(|| view.agent.as_ref().map(|(uid, _)| *uid))
    }

    pub(crate) fn remember_pane_terminal(&mut self, view: &Entity<TermView>, cx: &App) {
        if let Some(uid) = self.terminal_owner(view, cx)
            && let Some(pane) = self.session_workspace.panes.get_mut(&uid)
        {
            pane.terminal = Some(view.entity_id());
        }
    }

    fn restore_single_terminal(&mut self, _cx: &App) {
        if let Some(view) = self.pane_terminal(self.session().uid)
            && let Some(ix) = self.terminal.tabs.iter().position(|tab| tab == view)
        {
            self.terminal.active = ix;
        }
        self.terminal.open |= self.session_workspace.split_terminals;
        self.terminal.open |= self
            .session_workspace
            .panes
            .get(&self.session().uid)
            .is_some_and(|pane| pane.mode != PaneMode::Chat && pane.terminal_shown)
            && self.pane_terminal(self.session().uid).is_some();
    }

    /// Shows the session's newest existing shell in its pane, without spawning one.
    fn attach_pane_terminal(&mut self, uid: u64, cx: &App) -> bool {
        if self.pane_terminal(uid).is_some() {
            return true;
        }
        let Some(view) = self
            .terminal
            .tabs
            .iter()
            .rev()
            .find(|view| self.terminal_owner(view, cx) == Some(uid))
            .cloned()
        else {
            return false;
        };
        self.remember_pane_terminal(&view, cx);
        true
    }

    pub(crate) fn ensure_pane_terminal(
        &mut self,
        uid: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(pane) = self.session_workspace.panes.get_mut(&uid) {
            pane.terminal_shown = true;
        }
        if self.attach_pane_terminal(uid, cx) {
            return;
        }
        self.new_terminal_for_session(uid, None, None, window, cx);
        if let Some(pane) = self.session_workspace.panes.get_mut(&uid) {
            pane.follow_commands = true;
        }
    }

    pub(crate) fn prune_session_panes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let valid: Vec<_> = self.sessions.iter().map(|session| session.uid).collect();
        self.session_workspace
            .layout
            .retain(|uid| valid.contains(&uid));
        self.session_workspace
            .panes
            .retain(|uid, _| valid.contains(uid));
        self.session_workspace
            .drafts
            .retain(|uid, _| valid.contains(uid));
        if self.session_workspace.layout.root.is_none() {
            self.session_workspace.layout.root = Some(Node::Session {
                session: self.session().uid,
            });
        }
        if self.session_workspace.tiled() && !self.session_workspace.visible(self.session().uid) {
            let next = self.session_workspace.layout.sessions()[0];
            self.focus_session_pane(next, true, window, cx);
        }
        if !self.session_workspace.tiled() {
            self.restore_single_terminal(cx);
        }
        self.session_workspace.revision += 1;
        self.save_session_layout();
    }

    pub(crate) fn save_session_layout(&self) {
        if self.options.ephemeral() {
            return;
        }
        let id = |uid: &u64| {
            self.session_index(*uid)
                .and_then(|ix| self.sessions[ix].dir.as_ref())
                .and_then(|dir| dir.file_name())
                .and_then(|id| id.to_str())
                .map(str::to_string)
        };
        let saved = Saved {
            version: 1,
            root: self
                .session_workspace
                .layout
                .root
                .as_ref()
                .and_then(|root| root.map(&id)),
            active: id(&self.session().uid),
            modes: self
                .session_workspace
                .panes
                .iter()
                .filter_map(|(uid, pane)| id(uid).map(|id| (id, pane.mode)))
                .collect(),
        };
        let result = (|| -> std::io::Result<()> {
            std::fs::create_dir_all(&self.home)?;
            let tmp = self
                .home
                .join(format!("session-layout-{}.tmp", std::process::id()));
            std::fs::write(&tmp, serde_json::to_vec_pretty(&saved)?)?;
            std::fs::rename(tmp, self.home.join("session-layout.json"))
        })();
        if let Err(error) = result {
            eprintln!("flint: couldn't save session layout: {error}");
        }
    }

    pub(crate) fn load_session_layout(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.session_workspace.layout.root = Some(Node::Session {
            session: self.session().uid,
        });
        if self.options.ephemeral() {
            return;
        }
        let path = self.home.join("session-layout.json");
        if std::fs::metadata(&path).map_or(true, |meta| meta.len() > 32_768) {
            return;
        }
        let Some(saved) = std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Saved>(&bytes).ok())
        else {
            return;
        };
        if saved.version != 1
            || saved
                .root
                .as_ref()
                .is_none_or(|root| !model::valid_saved(root))
        {
            return;
        }
        let uid = |id: &String| {
            self.sessions
                .iter()
                .find(|session| {
                    session
                        .dir
                        .as_ref()
                        .and_then(|dir| dir.file_name())
                        .is_some_and(|name| name == id.as_str())
                })
                .map(|session| session.uid)
        };
        let root = saved.root.as_ref().and_then(|root| root.map(&uid));
        let active = saved.active.as_ref().and_then(&uid);
        let modes: Vec<_> = saved
            .modes
            .iter()
            .filter_map(|(id, mode)| uid(id).map(|uid| (uid, *mode)))
            .collect();
        if let Some(root) = root {
            self.session_workspace.layout.root = Some(root);
            for uid in self.session_workspace.layout.sessions() {
                self.ensure_session_pane(uid, window, cx);
            }
            for (uid, mode) in modes {
                if let Some(pane) = self.session_workspace.panes.get_mut(&uid) {
                    pane.mode = mode;
                }
            }
            if self.session_workspace.tiled() {
                let active = active
                    .filter(|uid| self.session_workspace.visible(*uid))
                    .unwrap_or(self.session_workspace.layout.sessions()[0]);
                self.active = self.session_index(active).expect("restored session");
            } else {
                self.session_workspace.layout.root = Some(Node::Session {
                    session: self.session().uid,
                });
            }
        }
    }
}
