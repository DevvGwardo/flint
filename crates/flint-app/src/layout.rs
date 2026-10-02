//! Root layout: a floating, inset sidebar over the blurred window backdrop,
//! and the main column (header, transcript or empty state, composer) with the
//! resizable changes panel.

use flint_agent::ApprovalDecision;
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
        let p = palette();
        // Narrow windows give the transcript the room: the sidebar steps
        // aside below 1000px, or below 1280px while the changes panel is open.
        let width = window.viewport_size().width;
        let sidebar =
            self.sidebar_open && width >= px(1000.) && !(self.changes_open && width < px(1280.));
        self.sidebar_visible = sidebar;
        let main = div()
            .size_full()
            .flex()
            .flex_col()
            .child(crate::header::render(self, window, cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .child(crate::transcript::render_main(self, window, cx)),
            );
        let body = h_resizable("flint-body")
            .child(resizable_panel().child(main))
            .child(
                resizable_panel()
                    .size(px(460.))
                    .size_range(px(340.)..px(820.))
                    .visible(self.changes_open)
                    .child(crate::changes_panel::render(self, cx)),
            );

        div()
            .id("flint")
            .key_context("FlintApp")
            .track_focus(&self.focus)
            // Shift+Tab and Esc are claimed before the composer's own bindings.
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                let key = &event.keystroke;
                let pending = this.session().view.pending_approvals > 0;
                if pending
                    && key.modifiers.platform
                    && (key.key == "enter" || key.key == "backspace")
                {
                    let decision = match (key.key.as_str(), key.modifiers.shift) {
                        ("backspace", _) => ApprovalDecision::Deny,
                        (_, true) => ApprovalDecision::ApproveAlways,
                        _ => ApprovalDecision::Approve,
                    };
                    this.answer_pending(decision, cx);
                    cx.stop_propagation();
                } else if key.key == "tab" && key.modifiers.shift && this.palette.is_none() {
                    this.toggle_approval(cx);
                    cx.stop_propagation();
                } else if key.key == "escape"
                    && this.palette.is_none()
                    && this.session().view.running
                {
                    this.interrupt(cx);
                    cx.stop_propagation();
                }
            }))
            .on_action(cx.listener(|this, _: &NewSession, window, cx| this.new_session(window, cx)))
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
            .on_action(cx.listener(|this, _: &ToggleApproval, _, cx| this.toggle_approval(cx)))
            .on_action(cx.listener(|this, _: &OpenWorkspace, _, cx| this.open_workspace(cx)))
            .on_action(cx.listener(|this, _: &RevealWorkspace, _, _| this.reveal_workspace()))
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
