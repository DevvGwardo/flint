//! Failure cards: a missing key, an unreachable endpoint, and model errors,
//! each with a plain explanation and a fix action.

use gpui_kit::assets::IconName;
use gpui_kit::component::button::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::rows::GUTTER;
use crate::app::FlintApp;
use crate::engine::NO_KEY;
use crate::theme::palette;
use crate::theme::size;
use crate::ui;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    NoKey,
    Unreachable,
    /// The endpoint ignores `reasoning_effort`: informational, not a failure.
    EffortUnsupported,
    Model,
}

pub fn classify(message: &str) -> ErrorKind {
    let lower = message.to_lowercase();
    if message.starts_with(NO_KEY) {
        ErrorKind::NoKey
    } else if lower.contains("reasoning_effort") {
        ErrorKind::EffortUnsupported
    } else if [
        "connection refused",
        "error sending request",
        "tcp connect",
        "dns error",
        "connect error",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
    {
        ErrorKind::Unreachable
    } else {
        ErrorKind::Model
    }
}

pub fn render(ix: usize, message: &str, base_url: &str, cx: &mut Context<FlintApp>) -> AnyElement {
    let p = palette();
    let kind = classify(message);
    if kind == ErrorKind::EffortUnsupported {
        return div()
            .pl(px(GUTTER))
            .child(ui::label(
                "This endpoint ignores reasoning effort, so the effort control is hidden.",
                size::SM,
                p.text_subtle,
            ))
            .into_any_element();
    }
    let (title, body) = match kind {
        ErrorKind::NoKey => ("No API key found", message.to_string()),
        ErrorKind::Unreachable => (
            "Can't reach the model endpoint",
            format!("Couldn't reach {base_url}. Check the endpoint in Settings.\n{message}"),
        ),
        _ => ("The model returned an error", message.to_string()),
    };
    let settings = matches!(kind, ErrorKind::NoKey | ErrorKind::Unreachable);
    let retry = matches!(
        kind,
        ErrorKind::Unreachable | ErrorKind::Model | ErrorKind::NoKey
    );
    div()
        .id(("error-card", ix))
        .rounded(px(14.))
        .border_1()
        .border_color(hsla(358. / 360., 0.85, 0.64, 0.45))
        .bg(hsla(358. / 360., 0.85, 0.64, 0.07))
        .p(px(16.))
        .flex()
        .flex_col()
        .gap(px(10.))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .child(ui::icon(IconName::CircleAlert, 17., p.danger))
                .child(ui::label(title, size::BASE, p.text).font_weight(FontWeight::MEDIUM)),
        )
        .child(
            div()
                .pl(px(27.))
                .text_size(px(size::BASE - 1.))
                .line_height(px(22.))
                .text_color(p.text_muted)
                .child(body),
        )
        .child(
            div()
                .pl(px(27.))
                .flex()
                .gap(px(8.))
                .when(settings, |row| {
                    row.child(
                        div()
                            .id(("error-settings", ix))
                            .child(
                                Button::new(("open-settings", ix))
                                    .primary()
                                    .label("Open settings")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.open_settings(window, cx)
                                    })),
                            )
                            .test_support(),
                    )
                })
                .when(retry, |row| {
                    row.child(
                        div()
                            .id(("error-retry", ix))
                            .child(
                                Button::new(("retry", ix))
                                    .label("Retry")
                                    .on_click(cx.listener(|this, _, _, cx| this.retry(cx))),
                            )
                            .test_support(),
                    )
                }),
        )
        .test_support()
        .into_any_element()
}

#[cfg(test)]
#[path = "errors_tests.rs"]
mod tests;
