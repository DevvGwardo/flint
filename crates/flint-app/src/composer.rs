//! The floating composer, with the live status line above it and the
//! running-task tray attached to its top edge.

use std::time::Duration;

use flint_agent::ApprovalMode;
use gpui_kit::assets::IconName;
use gpui_kit::component::input::Textarea;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::app::Effort;
use crate::app::FlintApp;
use crate::app::TogglePalette;
use crate::theme::MONO_FONT;
use crate::theme::palette;
use crate::theme::size;
use crate::transcript::COLUMN_WIDTH;
use crate::turns::Activity;
use crate::ui;

/// Status glyph frames, played back and forth.
const GLYPHS: &[&str] = &["·", "✢", "✳", "✶", "✻", "✽", "✻", "✶", "✳", "✢"];
const THINKING_VERBS: &[&str] = &["Thinking", "Reasoning", "Pondering", "Mulling"];
const WORKING_VERBS: &[&str] = &["Working", "Forging", "Tempering", "Kindling"];

pub fn render(app: &FlintApp, cx: &mut Context<FlintApp>) -> AnyElement {
    let p = palette();
    let view = &app.session().view;
    let running = view.running;
    let tasks = view.running_commands();

    let tray = (!tasks.is_empty()).then(|| {
        let count = tasks.len();
        div()
            .px(px(18.))
            .pt(px(14.))
            .pb(px(12.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .border_b_1()
            .border_color(p.border)
            .child(ui::label(
                format!("{count} task{} running", if count == 1 { "" } else { "s" }),
                size::XS,
                p.text_subtle,
            ))
            .children(tasks.iter().map(|call| {
                let last = call
                    .output
                    .lines()
                    .rev()
                    .find(|line| !line.trim().is_empty())
                    .unwrap_or("")
                    .trim()
                    .to_string();
                let elapsed = app.now().saturating_sub(call.started);
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(Spinner::new().small().color(p.accent))
                    .child(ui::mono(call.summary.clone(), size::SM, p.text).flex_shrink_0())
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .font_family(MONO_FONT)
                            .font_features(FontFeatures::disable_ligatures())
                            .text_size(px(size::XS))
                            .text_color(p.text_subtle)
                            .child(last),
                    )
                    .child(ui::label(
                        ui::duration(Duration::from_secs(elapsed.as_secs())),
                        size::XS,
                        p.text_subtle,
                    ))
            }))
    });

    let effort = match app.effort {
        Effort::Low => "Low",
        Effort::Medium => "Medium",
        Effort::High => "High",
    };
    let (mode_icon, mode_text, mode_color) = match app.approval {
        ApprovalMode::Auto => (IconName::ChevronsRight, "auto-run on", p.text_muted),
        ApprovalMode::AskForChanges => (IconName::Hand, "ask before changes", p.warning),
    };

    let send: AnyElement = if running {
        round_button("stop", p.text, p.raised)
            .child(div().size(px(11.)).rounded(px(2.5)).bg(p.text))
            .on_click(cx.listener(|this, _, _, cx| this.interrupt(cx)))
            .into_any_element()
    } else {
        round_button("send", p.on_accent, p.accent)
            .child(ui::icon(IconName::ArrowUp, 18., p.on_accent))
            .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx)))
            .into_any_element()
    };

    let toolbar = div()
        .h(px(52.))
        .pl(px(10.))
        .pr(px(10.))
        .flex()
        .items_center()
        .gap(px(6.))
        .child(
            round_button("attach", p.text_muted, hsla(0., 0., 0., 0.))
                .border_1()
                .border_color(p.border_strong)
                .child(ui::icon(IconName::Plus, 17., p.text_muted))
                .on_click(cx.listener(|_, _, window, cx| {
                    window.dispatch_action(Box::new(TogglePalette), cx);
                })),
        )
        .child(
            chip("model-chip")
                .on_click(cx.listener(|this, _, _, cx| this.cycle_effort(cx)))
                .child(ui::label(app.model.clone(), size::BASE - 1., p.text))
                .child(ui::label(effort, size::BASE - 1., p.text_subtle))
                .child(ui::icon(IconName::ChevronsUpDown, 13., p.text_subtle)),
        )
        .child(div().flex_1())
        .child(
            chip("approval-hint")
                .on_click(cx.listener(|this, _, _, cx| this.toggle_approval(cx)))
                .child(ui::icon(mode_icon, 14., mode_color))
                .child(ui::label(mode_text, size::SM, mode_color))
                .child(ui::label("shift+tab", size::SM, p.text_subtle)),
        )
        .child(div().w(px(4.)))
        .child(send);

    let card = div()
        .w_full()
        .rounded(px(20.))
        .border_1()
        .border_color(p.border_strong)
        .bg(p.surface)
        .shadow_lg()
        .overflow_hidden()
        .children(tray)
        .child(
            div().px(px(12.)).pt(px(12.)).min_h(px(44.)).child(
                Textarea::new(&app.composer)
                    .appearance(false)
                    .bordered(false)
                    .text_size(px(size::MD)),
            ),
        )
        .child(toolbar);

    div()
        .w_full()
        .max_w(px(COLUMN_WIDTH + 40.))
        .flex()
        .flex_col()
        .gap(px(10.))
        .when(running, |col| col.child(status_line(app)))
        .child(card)
        .into_any_element()
}

/// `✻ Forging… (29s · ↓ 383 tokens) · esc to interrupt`
fn status_line(app: &FlintApp) -> impl IntoElement {
    let p = palette();
    let view = &app.session().view;
    let now = app.now();
    let tick = (now.as_millis() / 120) as usize;
    let rotate = (now.as_millis() / 2400) as usize;
    let verb = match view.activity() {
        Activity::Thinking => THINKING_VERBS[rotate % THINKING_VERBS.len()].to_string(),
        Activity::Writing => "Writing".to_string(),
        Activity::Running(_) => "Running".to_string(),
        Activity::Editing(_) => "Editing".to_string(),
        Activity::Reading(_) => "Reading".to_string(),
        Activity::Searching => "Searching".to_string(),
        Activity::Working => WORKING_VERBS[rotate % WORKING_VERBS.len()].to_string(),
    };
    let elapsed = view
        .elapsed(now)
        .map(|d| ui::duration(Duration::from_secs(d.as_secs())))
        .unwrap_or_default();
    let tokens = view.usage.output_tokens + view.usage.reasoning_tokens;
    div()
        .px(px(18.))
        .flex()
        .items_center()
        .gap(px(7.))
        .text_size(px(size::BASE))
        .child(
            div()
                .w(px(14.))
                .text_color(p.accent)
                .child(GLYPHS[tick % GLYPHS.len()]),
        )
        .child(
            div()
                .text_color(p.accent)
                .font_weight(FontWeight::MEDIUM)
                .child(format!("{verb}…")),
        )
        .child(ui::label(
            format!("({elapsed} · ↓ {} tokens)", ui::tokens(tokens)),
            size::BASE,
            p.text_subtle,
        ))
        .child(ui::label("· esc to interrupt", size::BASE, p.text_subtle))
}

fn round_button(id: &'static str, fg: Hsla, bg: Hsla) -> Stateful<Div> {
    div()
        .id(id)
        .size(px(36.))
        .flex_shrink_0()
        .rounded_full()
        .bg(bg)
        .text_color(fg)
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .hover(|style| style.opacity(0.85))
}

fn chip(id: &'static str) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(34.))
        .px(px(11.))
        .rounded(px(17.))
        .flex()
        .items_center()
        .gap(px(7.))
        .cursor_pointer()
        .hover(|style| style.bg(hsla(0., 0., 1., 0.05)))
}
