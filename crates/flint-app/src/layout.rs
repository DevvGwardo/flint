//! Root layout: a floating, inset sidebar over the blurred window backdrop,
//! and the main column (header, transcript or empty state, composer) with the
//! resizable changes panel.

use flint_agent::AgentKind;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::app::*;
use crate::theme::palette;
use crate::theme::size;

/// Inset of the floating sidebar from the window edges.
pub const SIDEBAR_INSET: f32 = 8.;
pub const SIDEBAR_WIDTH: f32 = 288.;

impl Render for FlintApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let frame_started = std::time::Instant::now();
        let p = palette();
        // Narrow windows give the transcript the room: the sidebar steps
        // aside below 1000px, or below 1280px while the changes panel is open.
        let width = window.viewport_size().width;
        let sidebar =
            self.sidebar_open && width >= px(1000.) && !(self.changes_open && width < px(1280.));
        self.sidebar_visible = sidebar;
        let main = div()
            .id("main-column")
            .size_full()
            .flex()
            .flex_col()
            // Dragging the terminal dock's top edge.
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, window, cx| {
                if event.pressed_button == Some(MouseButton::Left) {
                    this.drag_terminal_edge(event.position.y, window, cx);
                } else {
                    this.end_terminal_resize();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.end_terminal_resize()),
            )
            .child(crate::header::render(self, window, cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .child(crate::transcript::render_main(self, window, cx)),
            )
            .children(crate::term_panel::render(self, cx));
        let body = h_resizable("flint-body")
            .child(resizable_panel().child(main))
            .child(
                resizable_panel()
                    .size(px(460.))
                    .size_range(px(340.)..px(820.))
                    .visible(self.changes_open)
                    .when(self.changes_open, |panel| {
                        // Built only while visible: the diff can be large.
                        panel.child(crate::changes_panel::render(self, cx))
                    }),
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
                if this.settings_form.is_some() {
                    if key.key == "escape" {
                        this.close_settings(window, cx);
                        cx.stop_propagation();
                    }
                    return;
                }
                if this.renaming.is_some() {
                    if key.key == "escape" {
                        this.cancel_rename(window, cx);
                        cx.stop_propagation();
                    }
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
            .on_action(cx.listener(|this, _: &ToggleSidebar, _, cx| {
                this.sidebar_open = !this.sidebar_open;
                cx.notify();
            }))
            // Shift+Tab: the agent's own modes when it has them, else auto-run.
            .on_action(cx.listener(|this, _: &ToggleApproval, _, cx| {
                if !this.cycle_agent_mode(cx) {
                    this.toggle_approval(cx);
                }
            }))
            .on_action(cx.listener(|this, _: &OpenWorkspace, _, cx| this.open_workspace(cx)))
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
            .on_action(cx.listener(|this, _: &FocusComposer, window, cx| {
                this.composer
                    .update(cx, |state, cx| state.focus(window, cx));
            }))
            .size_full()
            .flex()
            .bg(p.window_tint)
            .text_color(p.text)
            .text_size(px(size::BASE))
            .when(sidebar, |root| {
                root.child(
                    div()
                        .flex_shrink_0()
                        .h_full()
                        .p(px(SIDEBAR_INSET))
                        .pr(px(0.))
                        .child(crate::sidebar::render(self, cx)),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .when(sidebar, |main| {
                        main.ml(px(SIDEBAR_INSET))
                            .rounded_tl(px(12.))
                            .rounded_bl(px(12.))
                            .border_l_1()
                            .border_color(p.border)
                    })
                    .bg(p.bg)
                    .overflow_hidden()
                    .child(body),
            )
            .when_some(self.palette.clone(), |root, state| {
                root.child(crate::palette::render(&state, cx))
            })
            .children(crate::settings_view::render(self, cx));
        crate::automation::record_frame(frame_started);
        root
    }
}

impl FlintApp {
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
