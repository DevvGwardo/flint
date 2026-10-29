//! A finished turn's frame: the "Worked for …" disclosure, the promoted final
//! answer, and the closing files-changed card with copy/feedback actions.

use std::time::Duration;

use flint_agent::TurnEndReason;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::app::FlintApp;
use crate::subagent_ui::Conversation;
use crate::theme::palette;
use crate::theme::size;
use crate::turns::TurnInfo;
use crate::ui;
use crate::view_model::Item;
use crate::view_model::SessionView;

/// Files listed in the card before "and N more".
const CARD_FILES: usize = 4;

/// A completed execution with a clean assessment has recovered its routine
/// guard guidance. User acceptance alone cannot clear unresolved evidence.
pub(super) fn guard_recovered(turn: &TurnInfo) -> bool {
    turn.end.is_some()
        && turn.task.as_ref().is_some_and(|task| {
            task.blockers.is_empty()
                && !task.evidence_overflow
                && task.assessment.as_ref().is_some_and(|assessment| {
                    matches!(assessment.reason, TurnEndReason::Completed)
                        && assessment.issues.is_empty()
                        && matches!(
                            assessment.outcome,
                            flint_agent::TaskOutcome::NeedsReview
                                | flint_agent::TaskOutcome::Accepted
                                | flint_agent::TaskOutcome::Answered
                                | flint_agent::TaskOutcome::Verified
                        )
                })
        })
}

fn completed_label(outcome: flint_agent::TaskOutcome) -> &'static str {
    if outcome.allows_queue() {
        "Done"
    } else {
        "Turn ended"
    }
}

fn analysis_ready(task: Option<&flint_agent::TaskRecord>) -> bool {
    task.is_some_and(|task| {
        task.analysis_only()
            && !task.evidence_overflow
            && task.blockers.is_empty()
            && task.assessment.as_ref().is_some_and(|assessment| {
                assessment.outcome == flint_agent::TaskOutcome::NeedsReview
                    && assessment.reason == TurnEndReason::Completed
                    && assessment.issues.is_empty()
            })
    })
}

pub fn worked_line(
    target: &Conversation,
    ix: usize,
    turn: &TurnInfo,
    cx: &mut Context<FlintApp>,
) -> impl IntoElement {
    let p = palette();
    let target = target.clone();
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
        .role(gpui_kit::Role::Button)
        .aria_label(format!(
            "{} work for turn {}",
            if turn.expanded { "Hide" } else { "Show" },
            ix + 1
        ))
        .flex()
        .items_center()
        .gap(px(10.))
        .cursor_pointer()
        .on_click(cx.listener(move |this, _, _, cx| this.toggle_conversation_work(&target, ix, cx)))
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
                        if guard_recovered(turn) {
                            p.text_muted
                        } else {
                            p.warning
                        },
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

pub fn answer(ix: usize, markdown: AnyElement) -> AnyElement {
    div()
        .id(("answer", ix))
        .w_full()
        .text_size(px(size::PROSE))
        .line_height(px(24.))
        .text_color(palette().text)
        .child(markdown)
        .test_support()
        .into_any_element()
}

pub fn summary(
    target: &Conversation,
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
    let outcome = turn
        .task
        .as_ref()
        .and_then(|task| task.assessment.as_ref())
        .map_or(flint_agent::TaskOutcome::Unknown, |assessment| {
            assessment.outcome
        });
    let ended: Option<(IconName, String, Hsla)> = match reason {
        TurnEndReason::Completed => Some((
            if outcome.allows_queue() {
                IconName::Check
            } else {
                IconName::CircleStop
            },
            completed_label(outcome).into(),
            if outcome.allows_queue() {
                p.success
            } else {
                p.text_muted
            },
        )),
        TurnEndReason::Interrupted => Some((IconName::CircleStop, "Stopped".into(), p.warning)),
        TurnEndReason::StepLimit => Some((
            IconName::CircleAlert,
            "Hit the step limit".into(),
            p.warning,
        )),
        TurnEndReason::ContinuationLimit => Some((
            IconName::CircleAlert,
            turn.task
                .as_ref()
                .and_then(|task| task.continuation_stop.as_ref())
                .map_or("Automatic follow-ups stopped", |stop| stop.label())
                .into(),
            p.warning,
        )),
        TurnEndReason::Failed(why) => Some((IconName::CircleX, format!("Failed: {why}"), p.danger)),
    };
    let (outcome_icon, outcome_color) = crate::task_ui::outcome_style(outcome);
    let ready = analysis_ready(turn.task.as_ref());
    let outcome_label = if ready {
        "Analysis ready"
    } else {
        outcome.label()
    };
    let details_target = target.clone();
    let turn_id = turn.turn_id;
    let outcome_strip = div()
        .id(("task-outcome", ix))
        .aria_label(outcome_label)
        .w_full()
        .px(px(14.))
        .py(px(10.))
        .rounded(px(10.))
        .border_1()
        .border_color(p.border_strong)
        .bg(p.surface)
        .flex()
        .items_start()
        .gap(px(10.))
        .child(ui::icon(outcome_icon, 15., outcome_color).mt(px(3.)))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(3.))
                .child(ui::label(outcome_label, size::SM, outcome_color))
                .child(ui::label(
                    turn.task
                        .as_ref()
                        .and_then(|task| {
                            task.blockers
                                .first()
                                .map(|blocker| blocker.text.clone())
                                .or_else(|| {
                                    task.assessment
                                        .as_ref()
                                        .and_then(|assessment| assessment.issues.first().cloned())
                                })
                        })
                        .unwrap_or_else(|| match outcome {
                            flint_agent::TaskOutcome::Accepted => {
                                "Your acceptance was recorded.".into()
                            }
                            flint_agent::TaskOutcome::Answered => {
                                "An answer, not deliverable acceptance.".into()
                            }
                            _ if ready => "Here are the findings. No changes were made by this review. Suggested next steps need your approval.".into(),
                            _ => "Check the goal and evidence before accepting this result.".into(),
                        }),
                    size::XS,
                    p.text_muted,
                )),
        )
        .child(
            Button::new(("task-details", ix))
                .outline()
                .label("Details")
                .accessibility_label("Inspect task goal, evidence and blockers")
                .on_click(cx.listener(move |app, _, window, cx| {
                    app.open_task_details(details_target.clone(), turn_id, window, cx)
                })),
        )
        .test_support();

    let card = (!turn.files.is_empty()).then(|| {
        let n = turn.files.len();
        let first = turn.files.first().cloned();
        let rows = turn.files.iter().take(CARD_FILES).map(|path| {
            let stats = view.changes.iter().find(|f| &f.path == path);
            let path_owned = path.clone();
            let owner = target.parent;
            div()
                .id(SharedString::from(format!("card-file-{ix}-{path}")))
                .px(px(16.))
                .h(px(38.))
                .flex()
                .items_center()
                .gap(px(10.))
                .cursor_pointer()
                .hover(|style| style.bg(p.raised))
                .on_click(cx.listener(move |this, _, window, cx| {
                    if let Some(ix) = this.session_index(owner) {
                        this.select_session_with_focus(ix, false, window, cx);
                    }
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
        let owner = target.parent;
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
                    .child(ui::label(
                        crate::shortcuts::label("⌘J").into_owned(),
                        size::SM,
                        p.text_subtle,
                    ))
                    .child(
                        div()
                            .id(("review-button", ix))
                            .child(
                                Button::new(("review", ix))
                                    .outline()
                                    .label("Review")
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        if let Some(ix) = this.session_index(owner) {
                                            this.select_session_with_focus(ix, false, window, cx);
                                        }
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
    let copy_target = target.clone();
    let up_target = target.clone();
    let down_target = target.clone();
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
                            this.copy_conversation_answer(&copy_target, ix, text.clone(), cx);
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
            .on_click(cx.listener(move |this, _, _, cx| {
                this.conversation_feedback(&up_target, ix, true, cx)
            })),
        )
        .child(
            action(
                "down",
                IconName::ThumbsDown,
                turn.feedback == Some(false),
                "Bad response (kept on this device only)",
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.conversation_feedback(&down_target, ix, false, cx)
            })),
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
                    .id(("turn-ended", ix))
                    .aria_label(text.clone())
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(ui::icon(icon, 15., color))
                    .child(ui::label(text, size::BASE, color))
                    .test_support(),
            )
        })
        .child(outcome_strip)
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

#[cfg(test)]
mod tests {
    use super::{completed_label, guard_recovered};
    use crate::turns::TurnInfo;
    use flint_agent::{TaskMode, TaskOutcome, TaskRecord, TurnEndReason};

    #[test]
    fn completed_execution_label_does_not_claim_unaccepted_work_is_done() {
        for outcome in [
            TaskOutcome::Partial,
            TaskOutcome::Blocked,
            TaskOutcome::Unknown,
            TaskOutcome::NeedsReview,
        ] {
            assert_eq!(completed_label(outcome), "Turn ended");
        }
        for outcome in [TaskOutcome::Accepted, TaskOutcome::Answered] {
            assert_eq!(completed_label(outcome), "Done");
        }
    }

    #[test]
    fn quiet_guard_summary_requires_finished_clean_evidence_not_just_acceptance() {
        let mut task = TaskRecord::new(1, "Explain the interface.".into(), 0);
        task.contract.mode = TaskMode::ReadOnly;
        task.assess(TurnEndReason::Completed, true);
        let mut turn = TurnInfo {
            task: Some(task),
            ..TurnInfo::default()
        };
        assert!(!guard_recovered(&turn));
        turn.end = Some(1);
        assert!(guard_recovered(&turn));
        let task = turn.task.as_mut().unwrap();
        task.assessment.as_mut().unwrap().outcome = TaskOutcome::Accepted;
        task.assessment
            .as_mut()
            .unwrap()
            .issues
            .push("Unresolved verification failure".into());
        assert!(!guard_recovered(&turn));
    }
}
