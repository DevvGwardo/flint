//! The composer: auto-growing input, send/stop, approval mode and model.

use flint_agent::ApprovalMode;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::*;
use gpui_kit::component::input::Textarea;
use gpui_kit::component::*;
use gpui_kit::*;

use crate::app::FlintApp;
use crate::theme::palette;
use crate::theme::size;
use crate::transcript::COLUMN_WIDTH;
use crate::ui;

pub fn render(
    app: &FlintApp,
    _window: &mut Window,
    cx: &mut Context<FlintApp>,
) -> impl IntoElement {
    let p = palette();
    let running = app.session().view.running;
    let (mode_label, mode_icon, mode_color) = match app.approval {
        ApprovalMode::Auto => ("Auto-run", IconName::Zap, p.text_muted),
        ApprovalMode::AskForChanges => ("Ask first", IconName::ShieldCheck, p.warning),
    };

    let action: AnyElement = if running {
        Button::new("stop")
            .small()
            .icon(ui::icon(IconName::Square, 11., p.text))
            .label("Stop")
            .tooltip("Stop the turn  ⌘.")
            .on_click(cx.listener(|this, _, _, cx| this.interrupt(cx)))
            .into_any_element()
    } else {
        Button::new("send")
            .small()
            .primary()
            .icon(IconName::ArrowUp)
            .tooltip("Send  ⏎")
            .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx)))
            .into_any_element()
    };

    let card = div()
        .w_full()
        .max_w(px(COLUMN_WIDTH))
        .rounded(px(12.))
        .border_1()
        .border_color(p.border_strong)
        .bg(p.surface)
        .shadow_md()
        .flex()
        .flex_col()
        .child(
            div().px(px(6.)).pt(px(6.)).child(
                Textarea::new(&app.composer)
                    .appearance(false)
                    .bordered(false)
                    .text_size(px(size::MD)),
            ),
        )
        .child(
            div()
                .h(px(38.))
                .px(px(10.))
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(
                            div()
                                .id("composer-approval")
                                .flex()
                                .items_center()
                                .gap(px(5.))
                                .px(px(7.))
                                .h(px(22.))
                                .rounded(px(6.))
                                .border_1()
                                .border_color(p.border)
                                .cursor_pointer()
                                .hover(|style| style.bg(p.raised))
                                .on_click(cx.listener(|this, _, _, cx| this.toggle_approval(cx)))
                                .child(ui::icon(mode_icon, 11., mode_color))
                                .child(ui::label(mode_label, size::XS, mode_color)),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(5.))
                                .px(px(7.))
                                .h(px(22.))
                                .rounded(px(6.))
                                .border_1()
                                .border_color(p.border)
                                .child(ui::icon(IconName::Cpu, 11., p.text_muted))
                                .child(ui::mono(app.model.clone(), size::XS, p.text_muted)),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .child(ui::label(
                            if running {
                                "esc to stop"
                            } else {
                                "⇧⏎ newline"
                            },
                            size::XS,
                            p.text_subtle,
                        ))
                        .child(action),
                ),
        );

    div()
        .w_full()
        .flex_shrink_0()
        .flex()
        .justify_center()
        .px(px(28.))
        .pb(px(16.))
        .pt(px(4.))
        .child(card)
}
