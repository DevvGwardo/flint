//! Window controls and shortcuts around the dockable workspace panels.

use flint_agent::AgentKind;
use gpui_kit::base::FocusTrapElement as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::app::*;
use crate::theme::palette;
use crate::theme::size;

/// Inset of the floating sidebar from the window edges.
pub const SIDEBAR_INSET: f32 = 8.;
pub const SIDEBAR_WIDTH: f32 = 288.;
const TITLEBAR_HEIGHT: f32 = 36.;

impl Render for FlintApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let frame_started = std::time::Instant::now();
        if let Some((old, new)) = self.session_workspace.pending_selection.take() {
            self.follow_session_selection(old, new, window, cx);
            self.save_session_layout();
        }
        let p = palette();
        let sidebar = self.update_sidebar_visibility(window.viewport_size().width);
        if !sidebar && !self.session_drawer {
            self.session_menu = None;
            self.cancel_rename(window, cx);
        }
        let body = crate::docking::render(self, window, cx);
        // Window controls stay in one safe strip regardless of panel placement.
        let titlebar = self.drag_region(
            div()
                .id("workspace-titlebar")
                .h(px(TITLEBAR_HEIGHT))
                .w_full()
                .flex_shrink_0(),
            cx,
        );

        let root = div()
            .id("flint")
            .key_context(if self.mention.is_some() || self.slash.is_some() {
                "FlintApp menu"
            } else {
                "FlintApp"
            })
            .track_focus(&self.focus)
            // Shift+Tab and Esc are claimed before the composer's own bindings.
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                let key = event.keystroke.clone();
                if (this.settings_form.is_some() || this.archive_confirm.is_some())
                    && key.modifiers.platform
                    && (matches!(
                        key.key.as_str(),
                        "n" | "k" | "l" | "j" | "b" | "o" | "," | "."
                    ) || (key.key == "a" && key.modifiers.shift))
                {
                    cx.stop_propagation();
                    return;
                }
                if this.permission_choice_open {
                    if key.key == "escape" {
                        this.cancel_initial_permission(window, cx);
                        cx.stop_propagation();
                    } else if key.modifiers.platform && key.key != "q" {
                        cx.stop_propagation();
                    }
                    return;
                }
                if this.worktree_form.is_some() {
                    if key.key == "escape" {
                        this.close_worktrees(window, cx);
                        cx.stop_propagation();
                    } else if (key.modifiers.platform
                        && (matches!(
                            key.key.as_str(),
                            "n" | "k" | "l" | "j" | "b" | "o" | "," | "."
                        ) || (key.modifiers.shift
                            && matches!(key.key.as_str(), "a" | "r" | "t" | "g"))))
                        || (key.modifiers.control && key.key == "`")
                    {
                        cx.stop_propagation();
                    }
                    return;
                }
                if this.archive_confirm.is_some() {
                    if key.key == "escape" {
                        this.cancel_archive(window, cx);
                        cx.stop_propagation();
                    }
                    return;
                }
                if key.key == "escape"
                    && (this.docking.is_some() || this.session_workspace.dragging.is_some())
                {
                    this.docking = None;
                    this.session_workspace.dragging = None;
                    cx.stop_active_drag(window);
                    cx.notify();
                    cx.stop_propagation();
                    return;
                }
                if this.settings_form.is_some() {
                    if key.key == "escape" {
                        this.close_settings(window, cx);
                        cx.stop_propagation();
                    }
                    return;
                }
                if key.key == "escape" && this.approval_confirm.take().is_some() {
                    cx.notify();
                    cx.stop_propagation();
                    return;
                }
                if key.key == "escape" && this.approval_preview.take().is_some() {
                    cx.notify();
                    cx.stop_propagation();
                    return;
                }
                if key.key == "escape" && this.session_menu.take().is_some() {
                    cx.notify();
                    cx.stop_propagation();
                    return;
                }
                if this.renaming.is_some() {
                    if key.key == "escape" {
                        this.cancel_rename(window, cx);
                        cx.stop_propagation();
                    }
                    return;
                }
                if key.key == "escape" && (this.queue_edit.is_some() || this.queue_popover) {
                    let was_editing = this.queue_edit.is_some();
                    this.queue_edit = None;
                    this.queue_popover = false;
                    this.composer
                        .update(cx, |state, cx| state.focus(window, cx));
                    if was_editing {
                        this.dispatch_next_prompt(this.session().uid, cx);
                    }
                    cx.notify();
                    cx.stop_propagation();
                    return;
                }
                if key.key == "escape" && this.session_drawer {
                    this.close_session_drawer(window, cx);
                    cx.stop_propagation();
                    return;
                }
                if this.palette.is_some() {
                    return;
                }
                // Keys typed in a terminal belong to the shell (Esc, Tab, …).
                if this.terminal_focused(window, cx) {
                    return;
                }
                if this.handle_menu_key(&key, window, cx) {
                    cx.stop_propagation();
                } else if key.key == "escape" && this.help_open {
                    this.help_open = false;
                    cx.notify();
                    cx.stop_propagation();
                } else if key.key == "escape" && this.session().selected_subagent.is_some() {
                    this.select_session(this.active, window, cx);
                    cx.stop_propagation();
                } else if key.key == "escape" && this.session().view.running {
                    this.interrupt(cx);
                    cx.stop_propagation();
                }
            }))
            .on_action(cx.listener(|this, _: &NewSession, window, cx| this.new_session(window, cx)))
            .on_action(
                cx.listener(|this, _: &ToggleTerminal, window, cx| {
                    this.toggle_terminal(window, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &NewClaudeSession, window, cx| {
                this.new_agent_session(AgentKind::ClaudeCode, window, cx)
            }))
            .on_action(cx.listener(|this, _: &NewCodexSession, window, cx| {
                this.new_agent_session(AgentKind::Codex, window, cx)
            }))
            .on_action(cx.listener(|this, _: &NewDroidSession, window, cx| {
                this.new_agent_session(AgentKind::Droid, window, cx)
            }))
            .on_action(cx.listener(|this, _: &TogglePalette, window, cx| {
                if this.palette.is_some() {
                    this.close_palette(window, cx);
                } else {
                    this.open_palette(window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &ToggleChanges, _, cx| {
                this.changes_open = !this.changes_open;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ToggleSidebar, window, cx| {
                this.sidebar_open = !this.sidebar_open;
                if !this.sidebar_open {
                    this.session_menu = None;
                    this.cancel_rename(window, cx);
                    this.composer
                        .update(cx, |state, cx| state.focus(window, cx));
                }
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &CycleSessionGrouping, _, cx| {
                this.set_grouping(this.grouping.next(), cx);
            }))
            // Shift+Tab: the agent's own modes when it has them, else auto-run.
            .on_action(cx.listener(|this, _: &ToggleApproval, _, cx| {
                if !this.cycle_agent_mode(cx) {
                    this.toggle_approval(cx);
                }
            }))
            .on_action(cx.listener(|this, _: &OpenWorkspace, _, cx| this.open_workspace(cx)))
            .on_action(
                cx.listener(|this, _: &OpenWorktrees, window, cx| this.open_worktrees(window, cx)),
            )
            .on_action(cx.listener(|this, _: &RevealWorkspace, _, _| this.reveal_workspace()))
            .on_action(cx.listener(|this, _: &OpenTerminal, _, _| this.open_terminal()))
            .on_action(cx.listener(|this, _: &MenuUp, window, cx| this.menu_key("up", window, cx)))
            .on_action(
                cx.listener(|this, _: &MenuDown, window, cx| this.menu_key("down", window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &MenuAccept, window, cx| this.menu_key("enter", window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &MenuDismiss, window, cx| {
                    this.menu_key("escape", window, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &OpenSettings, window, cx| this.open_settings(window, cx)),
            )
            .on_action(cx.listener(|this, _: &RenameSession, window, cx| {
                let ix = this.active;
                this.start_rename(ix, window, cx);
            }))
            .on_action(cx.listener(|this, _: &DeleteSession, window, cx| {
                let ix = this.active;
                this.delete_session(ix, window, cx);
            }))
            .on_action(cx.listener(|this, _: &Interrupt, _, cx| this.interrupt(cx)))
            .on_action(
                cx.listener(|this, _: &crate::app::UndoLastTurn, _, cx| this.undo_last_turn(cx)),
            )
            .on_action(cx.listener(|this, _: &ResetPanelLayout, _, cx| this.reset_panel_layout(cx)))
            .on_action(
                cx.listener(|this, _: &ArrangeSessionGrid, _, cx| this.arrange_session_grid(cx)),
            )
            .on_action(cx.listener(|this, _: &ResetSessionPanes, window, cx| {
                this.reset_session_panes(window, cx)
            }))
            .on_action(cx.listener(|this, _: &FocusComposer, window, cx| {
                let uid = this.session().uid;
                if this.session_workspace.tiled()
                    && let Some(pane) = this.session_workspace.panes.get_mut(&uid)
                    && pane.mode == crate::session_workspace::PaneMode::Terminal
                {
                    pane.mode = crate::session_workspace::PaneMode::Both;
                    this.save_session_layout();
                    cx.notify();
                }
                this.focus_session_input(window, cx);
                cx.notify();
            }))
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .bg(p.window_tint)
            .text_color(p.text)
            .text_size(px(size::BASE))
            .child(titlebar)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .w_full()
                    .overflow_hidden()
                    .child(body),
            )
            .when(self.session_drawer && !sidebar, |root| {
                root.child(
                    div()
                        .id("sessions-drawer-backdrop")
                        .absolute()
                        .top(px(TITLEBAR_HEIGHT))
                        .bottom_0()
                        .left_0()
                        .right_0()
                        .bg(hsla(0., 0., 0., 0.6))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.close_session_drawer(window, cx);
                        }))
                        .child(
                            div()
                                .id("sessions-drawer")
                                .w(px(304.))
                                .h_full()
                                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                .on_click(|_, _, cx| cx.stop_propagation())
                                .child(
                                    div()
                                        .h_full()
                                        .child(crate::sidebar::render(self, cx))
                                        .focus_trap("sessions-focus-trap", &self.drawer_focus),
                                )
                                .test_support(),
                        )
                        .test_support(),
                )
            })
            .when(
                self.archived_session.is_some() || self.store_error.is_some(),
                |root| {
                    root.child(
                        div()
                            .id("archive-feedback")
                            .absolute()
                            .bottom(px(18.))
                            .left(px(18.))
                            .p(px(12.))
                            .rounded(px(10.))
                            .border_1()
                            .border_color(p.border_strong)
                            .bg(p.surface)
                            .flex()
                            .items_center()
                            .gap(px(12.))
                            .children(self.store_error.as_ref().map(|error| {
                                crate::ui::label(
                                    error.clone(),
                                    size::SM,
                                    if error.starts_with("Couldn't") {
                                        p.danger
                                    } else {
                                        p.text_muted
                                    },
                                )
                            }))
                            .when(self.archived_session.is_some(), |toast| {
                                toast
                                    .child(crate::ui::label("Session archived", size::SM, p.text))
                                    .child(
                                        div()
                                            .id("undo-archive")
                                            .role(gpui_kit::Role::Button)
                                            .aria_label("Undo archive")
                                            .tab_index(0)
                                            .focus_visible(|style| {
                                                style.border_1().border_color(p.accent)
                                            })
                                            .cursor_pointer()
                                            .text_color(p.accent)
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.undo_archive(window, cx)
                                            }))
                                            .child("Undo (this run)")
                                            .test_support(),
                                    )
                            })
                            .test_support(),
                    )
                },
            )
            .when_some(self.palette.clone(), |root, state| {
                root.child(crate::palette::render(
                    &state,
                    &self.palette_focus,
                    f32::from(window.viewport_size().height),
                    cx,
                ))
            })
            .when_some(
                crate::worktree_picker::render(self, window, cx),
                |root, picker| root.child(picker),
            )
            .when_some(self.archive_confirm, |root, uid| {
                let session = self.session_index(uid).map(|ix| &self.sessions[ix]);
                let archived_title = session.map(|s| s.title()).unwrap_or_default();
                let archived_children = session.map_or(0, |s| s.running_subagents());
                root.child(
                    deferred(
                        div()
                            .absolute()
                            .inset_0()
                            .bg(hsla(0., 0., 0., 0.6))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                div()
                                    .id("archive-confirmation")
                                    .w(px(460.))
                                    .p(px(20.))
                                    .bg(p.surface)
                                    .rounded(px(14.))
                                    .border_1()
                                    .border_color(p.border_strong)
                                    .flex()
                                    .flex_col()
                                    .gap(px(16.))
                                    .child(archive_prompt(&archived_title))
                                    .child(crate::ui::label(
                                        archive_detail(archived_children),
                                        size::SM,
                                        p.text_muted,
                                    ))
                                    .child(Button::new("archive-cancel").label("Cancel").on_click(
                                        cx.listener(|this, _, window, cx| {
                                            this.cancel_archive(window, cx)
                                        }),
                                    ))
                                    .child(
                                        Button::new("archive-stop")
                                            .primary()
                                            .label("Stop work")
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                if let Some(ix) = this.session_index(uid) {
                                                    this.delete_session(ix, window, cx);
                                                }
                                                if let Some(focus) =
                                                    this.archive_return_focus.take()
                                                {
                                                    focus.focus(window, cx);
                                                }
                                            })),
                                    )
                                    .focus_trap("archive-focus-trap", &self.archive_focus),
                            ),
                    )
                    .with_priority(30),
                )
            })
            .children(crate::settings_view::render(self, window, cx))
            .children(crate::permission_choice::render(self, cx));
        crate::automation::record_frame(frame_started);
        root
    }
}

impl FlintApp {
    fn update_sidebar_visibility(&mut self, width: Pixels) -> bool {
        // Narrow windows give the transcript the room: the sidebar steps
        // aside below 1000px, or below 1280px while the changes panel is open.
        self.sidebar_visible =
            self.sidebar_open && width >= px(1000.) && !(self.changes_open && width < px(1280.));
        if self.sidebar_visible && self.chat_panel_width(f32::from(width)) < 560. {
            self.sidebar_visible = false;
        }
        self.sidebar_visible
    }

    pub(crate) fn open_sessions(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.session_drawer && !self.sidebar_open {
            self.sidebar_open = true;
            if self.update_sidebar_visibility(window.viewport_size().width) {
                self.search.update(cx, |state, cx| state.focus(window, cx));
                cx.notify();
                return;
            }
        }
        self.toggle_session_drawer(window, cx);
    }

    /// Makes an element drag the window (and zoom on double click), the way a
    /// native title bar does. Clicks on children still work: the move only
    /// starts once the pointer moves with the button held.
    pub(crate) fn drag_region(
        &self,
        element: Stateful<Div>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        element
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.drag_armed = true),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.drag_armed = false),
            )
            .on_mouse_move(cx.listener(|this, _, window, _| {
                if this.drag_armed {
                    this.drag_armed = false;
                    window.start_window_move();
                }
            }))
            .on_click(|event, window, _| {
                if event.click_count() == 2 {
                    window.titlebar_double_click();
                }
            })
    }
}

/// The archive confirmation's question, naming the task.
fn archive_prompt(title: &str) -> String {
    format!("Stop \u{201c}{title}\u{201d} and archive it?")
}

/// What stopping takes down, so archiving a session with subagents running
/// under it is never a surprise.
fn archive_detail(child_agents: usize) -> String {
    let scope = match child_agents {
        0 => "This stops the task".to_string(),
        1 => "This stops the task and its child agent".to_string(),
        n => format!("This stops the task and its {n} child agents"),
    };
    format!(
        "{scope}, along with any pending approvals. History moves to Archived once the \
         engine has stopped and finished saving; archive it again then. You can restore it \
         from Archived."
    )
}

#[cfg(test)]
mod tests {
    use super::{archive_detail, archive_prompt};

    #[test]
    fn archive_prompt_names_the_task() {
        assert_eq!(
            archive_prompt("Fix the build"),
            "Stop \u{201c}Fix the build\u{201d} and archive it?"
        );
    }

    #[test]
    fn archive_detail_names_the_child_agents_it_stops() {
        assert!(archive_detail(0).starts_with("This stops the task, along"));
        assert!(archive_detail(1).starts_with("This stops the task and its child agent,"));
        assert!(archive_detail(3).starts_with("This stops the task and its 3 child agents,"));
        assert!(archive_detail(0).contains("restore it from Archived"));
    }
}
