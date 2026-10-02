//! A finished turn's frame: the "Worked for …" disclosure, the promoted final
//! answer, and the closing files-changed card with copy/feedback actions.

use std::time::Duration;

use flint_agent::TurnEndReason;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::*;
use gpui_kit::component::text::TextView;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::app::FlintApp;
use crate::theme::palette;
use crate::theme::size;
use crate::turns::TurnInfo;
use crate::ui;
use crate::view_model::Item;
use crate::view_model::SessionView;

/// Files listed in the card before "and N more".
const CARD_FILES: usize = 4;

pub fn worked_line(ix: usize, turn: &TurnInfo, cx: &mut Context<FlintApp>) -> impl IntoElement {
    let p = palette();
    let mut extras = Vec::new();
    if turn.nudges > 0 {
        extras.push(format!(
            "{} guard nudge{}",
            turn.nudges,
            if turn.nudges == 1 { "" } else { "s" }
        ));
    }
    div()
        .id(("worked", ix))
        .flex()
        .items_center()
        .gap(px(10.))
        .cursor_pointer()
        .on_click(cx.listener(move |this, _, _, cx| this.toggle_work(ix, cx)))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(5.))
                .text_size(px(size::BASE))
                .text_color(p.text_muted)
                .hover(|style| style.text_color(p.text))
                .child(format!(
                    "Worked for {}",
                    ui::duration(Duration::from_secs(turn.duration.as_secs().max(1)))
                ))
                .when(!extras.is_empty(), |row| {
                    row.child(ui::label(
                        format!("· {}", extras.join(" · ")),
                        size::BASE,
                        p.text_subtle,
                    ))
                })
                .child(ui::icon(
                    if turn.expanded {
                        IconName::ChevronDown
                    } else {
                        IconName::ChevronRight
                    },
                    12.,
                    p.text_subtle,
                )),
        )
        .child(div().flex_1().h(px(1.)).bg(p.border))
        .test_support()
}

pub fn answer(ix: usize, item: &Item) -> AnyElement {
    let Item::Assistant { text, .. } = item else {
        return div().into_any_element();
    };
    div()
        .w_full()
        .text_size(px(size::PROSE))
        .line_height(px(24.))
        .text_color(palette().text)
        .child(TextView::markdown(("md", ix), text.clone()).selectable(true))
        .into_any_element()
}

pub fn summary(
    ix: usize,
    item: &Item,
    turn: &TurnInfo,
    view: &SessionView,
    copied: bool,
    cx: &mut Context<FlintApp>,
) -> AnyElement {
    let p = palette();
    let Item::TurnSummary { reason, usage, .. } = item else {
        return div().into_any_element();
    };
    let ended: Option<(IconName, String, Hsla)> = match reason {
        TurnEndReason::Completed => None,
        TurnEndReason::Interrupted => Some((IconName::CircleStop, "Stopped".into(), p.warning)),
        TurnEndReason::StepLimit => Some((
            IconName::CircleAlert,
            "Hit the step limit".into(),
            p.warning,
        )),
        TurnEndReason::Failed(why) => Some((IconName::CircleX, format!("Failed: {why}"), p.danger)),
    };

    let card = (!turn.files.is_empty()).then(|| {
        let n = turn.files.len();
        let first = turn.files.first().cloned();
        let rows = turn.files.iter().take(CARD_FILES).map(|path| {
            let stats = view.changes.iter().find(|f| &f.path == path);
            let path_owned = path.clone();
            div()
                .id(SharedString::from(format!("card-file-{ix}-{path}")))
                .px(px(16.))
                .h(px(38.))
                .flex()
                .items_center()
                .gap(px(10.))
                .cursor_pointer()
                .hover(|style| style.bg(p.raised))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.review(Some(path_owned.clone()), cx);
                }))
                .child(ui::icon(file_icon(path), 15., p.text_muted))
                .child(
                    ui::mono(path.clone(), size::SM, p.text)
                        .flex_1()
                        .min_w_0()
                        .truncate(),
                )
                .when_some(stats, |row, f| {
                    row.child(ui::mono(format!("+{}", f.added), size::XS, p.success))
                        .child(ui::mono(format!("−{}", f.removed), size::XS, p.danger))
                })
        });
        div()
            .w_full()
            .rounded(px(14.))
            .border_1()
            .border_color(p.border_strong)
            .bg(p.surface)
            .overflow_hidden()
            .child(
                div()
                    .h(px(56.))
                    .px(px(16.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .border_b_1()
                    .border_color(p.border)
                    .child(ui::label(
                        format!("{n} file{} changed", if n == 1 { "" } else { "s" }),
                        size::BASE,
                        p.text,
                    ))
                    .child(ui::mono(format!("+{}", turn.added), size::SM, p.success))
                    .child(ui::mono(format!("−{}", turn.removed), size::SM, p.danger))
                    .child(div().flex_1())
                    .child(ui::label("⌘J", size::SM, p.text_subtle))
                    .child(
                        div()
                            .id(("review-button", ix))
                            .child(
                                Button::new(("review", ix))
                                    .outline()
                                    .label("Review")
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.review(first.clone(), cx);
                                    })),
                            )
                            .test_support(),
                    ),
            )
            .child(div().py(px(6.)).children(rows))
            .when(n > CARD_FILES, |card| {
                card.child(div().px(px(16.)).pb(px(10.)).child(ui::label(
                    format!("and {} more", n - CARD_FILES),
                    size::XS,
                    p.text_subtle,
                )))
            })
    });

    let answer_text = turn.final_answer.and_then(|a| match view.items.get(a) {
        Some(Item::Assistant { text, .. }) => Some(text.clone()),
        _ => None,
    });
    let action = |id: &'static str, icon: IconName, active: bool, tip: &'static str| {
        Button::new((id, ix))
            .ghost()
            .icon(ui::icon(
                icon,
                16.,
                if active { p.accent } else { p.text_subtle },
            ))
            .tooltip(tip)
    };
    let actions = div()
        .flex()
        .items_center()
        .gap(px(2.))
        .child(
            div()
                .id(("copy-button", ix))
                .flex()
                .items_center()
                .child(
                    action(
                        "copy",
                        if copied {
                            IconName::Check
                        } else {
                            IconName::Copy
                        },
                        copied,
                        "Copy answer",
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(text) = &answer_text {
                            this.copy_answer(ix, text.clone(), cx);
                        }
                    })),
                )
                .when(copied, |row| {
                    row.child(ui::label("Copied", size::SM, p.accent))
                })
                .test_support(),
        )
        .child(
            action(
                "up",
                IconName::ThumbsUp,
                turn.feedback == Some(true),
                "Good response (kept on this device only)",
            )
            .on_click(cx.listener(move |this, _, _, cx| this.feedback(ix, true, cx))),
        )
        .child(
            action(
                "down",
                IconName::ThumbsDown,
                turn.feedback == Some(false),
                "Bad response (kept on this device only)",
            )
            .on_click(cx.listener(move |this, _, _, cx| this.feedback(ix, false, cx))),
        )
        .child(ui::label(
            format!(
                "{} in · {} out",
                ui::tokens(usage.input_tokens),
                ui::tokens(usage.output_tokens)
            ),
            size::XS,
            p.text_subtle,
        ));

    div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(14.))
        .when_some(ended, |col, (icon, text, color)| {
            col.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(ui::icon(icon, 15., color))
                    .child(ui::label(text, size::BASE, color)),
            )
        })
        .children(card)
        .child(actions)
        .into_any_element()
}

/// A file-type icon for a path.
pub fn file_icon(path: &str) -> IconName {
    match path.rsplit('.').next().unwrap_or("") {
        "rs" | "ts" | "tsx" | "js" | "jsx" | "py" | "go" | "c" | "h" | "cpp" | "java" | "rb"
        | "swift" | "kt" => IconName::FileCode,
        "json" | "toml" | "yaml" | "yml" | "lock" => IconName::FileBraces,
        "md" | "txt" => IconName::FileText,
        "sh" | "zsh" | "bash" => IconName::FileTerminal,
        _ => IconName::File,
    }
}
