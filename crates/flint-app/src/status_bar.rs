//! Status bar: run state, step, token usage with cache ratio, elapsed time,
//! approval mode and workspace path.

use flint_agent::ApprovalMode;
use gpui_kit::assets::IconName;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::app::FlintApp;
use crate::engine;
use crate::theme::palette;
use crate::theme::size;
use crate::ui;

pub fn render(app: &FlintApp, cx: &mut Context<FlintApp>) -> impl IntoElement {
    let p = palette();
    let view = &app.session().view;
    let usage = view.usage;

    let state = if view.running {
        div()
            .flex()
            .items_center()
            .gap(px(6.))
            .child(Spinner::new().xsmall().color(p.accent))
            .child(ui::label("Working", size::XS, p.text))
            .when(view.step > 0, |row| {
                row.child(ui::label(
                    format!("step {}", view.step),
                    size::XS,
                    p.text_subtle,
                ))
            })
    } else {
        div()
            .flex()
            .items_center()
            .gap(px(6.))
            .child(div().size(px(6.)).rounded_full().bg(p.success))
            .child(ui::label("Ready", size::XS, p.text_muted))
    };

    let cached_pct = (usage.cached_input_tokens * 100)
        .checked_div(usage.input_tokens)
        .unwrap_or(0);
    let tokens = div()
        .flex()
        .items_center()
        .gap(px(10.))
        .child(metric(
            IconName::ArrowDown,
            ui::tokens(usage.input_tokens),
            "in",
        ))
        .child(ui::label(
            format!("{cached_pct}% cached"),
            size::XS,
            p.text_subtle,
        ))
        .child(metric(
            IconName::ArrowUp,
            ui::tokens(usage.output_tokens),
            "out",
        ));

    let elapsed = view
        .elapsed(app.now())
        .map(|d| ui::duration(std::time::Duration::from_secs(d.as_secs())));

    let approval = match app.approval {
        ApprovalMode::Auto => ("Auto-run", IconName::Zap, p.text_subtle),
        ApprovalMode::AskForChanges => ("Ask before changes", IconName::ShieldCheck, p.warning),
    };

    div()
        .h(px(26.))
        .flex_shrink_0()
        .px(px(12.))
        .flex()
        .items_center()
        .justify_between()
        .bg(p.chrome)
        .border_t_1()
        .border_color(p.border)
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(12.))
                .child(state)
                .when(usage.input_tokens > 0, |row| {
                    row.child(ui::vdivider()).child(tokens)
                })
                .when_some(elapsed, |row, elapsed| {
                    row.child(ui::vdivider()).child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .child(ui::icon(IconName::Clock, 11., p.text_subtle))
                            .child(ui::label(elapsed, size::XS, p.text_muted)),
                    )
                }),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(12.))
                .child(
                    div()
                        .id("approval-mode")
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .cursor_pointer()
                        .on_click(cx.listener(|this, _, _, cx| this.toggle_approval(cx)))
                        .child(ui::icon(approval.1, 11., approval.2))
                        .child(ui::label(approval.0, size::XS, approval.2)),
                )
                .child(ui::vdivider())
                .child(
                    ui::mono(
                        engine::display_path(&app.workspace),
                        size::XS,
                        p.text_subtle,
                    )
                    .max_w(px(360.))
                    .truncate(),
                ),
        )
}

fn metric(icon: IconName, value: String, unit: &'static str) -> Div {
    let p = palette();
    div()
        .flex()
        .items_center()
        .gap(px(3.))
        .child(ui::icon(icon, 10., p.text_subtle))
        .child(ui::mono(value, size::XS, p.text_muted))
        .child(ui::label(unit, size::XS, p.text_subtle))
}
