use super::{composer_has_text, composer_is_empty, composer_query_value};
use gpui_kit::component::input::TextareaState;
use gpui_kit::{
    AppContext as _, Context, Entity, IntoElement, Render, TestAppContext, Window, WindowHandle,
    div,
};

struct QueryRoot;

impl Render for QueryRoot {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

fn input_window(cx: &mut TestAppContext) -> (WindowHandle<QueryRoot>, Entity<TextareaState>) {
    cx.update(crate::init);
    let mut input = None;
    let window = cx.add_window(|window, cx| {
        input = Some(cx.new(|cx| TextareaState::new(window, cx)));
        QueryRoot
    });
    (window, input.unwrap())
}

#[gpui_kit::test]
fn composer_is_empty_preserves_raw_empty_and_nonempty_whitespace(cx: &mut TestAppContext) {
    let (window, input) = input_window(cx);
    for text in [
        String::new(),
        " ".into(),
        "\t\r\n".into(),
        "\u{85}\u{a0}\u{2003}\u{2028}\u{3000}".into(),
        "\0".into(),
        "\u{200b}".into(),
        "界🙂e\u{301}".into(),
        " ".repeat(8192),
        "界🙂".repeat(8192),
    ] {
        window
            .update(cx, |_, window, cx| {
                input.update(cx, |state, cx| state.set_value(text.clone(), window, cx));
                let state = input.read(cx);
                assert_eq!(composer_is_empty(state), state.value().is_empty());
                assert_eq!(composer_is_empty(state), text.is_empty());
                assert_eq!(&*state.value(), text.as_str());
            })
            .unwrap();
    }
}

#[gpui_kit::test]
#[ignore = "manual release-mode performance probe"]
fn approval_empty_draft_perf_probe(cx: &mut TestAppContext) {
    let (window, input) = input_window(cx);
    for (case, text) in [
        ("empty", String::new()),
        ("short", "ordinary draft".to_string()),
        ("ascii", "x".repeat(8 * 1024 * 1024)),
        ("unicode", "界🙂".repeat(8 * 1024 * 1024 / 7)),
        ("blank_ascii", " ".repeat(1024 * 1024)),
        ("blank_unicode", "\u{2003}".repeat(1024 * 1024 / 3)),
    ] {
        let expected = text.is_empty();
        window
            .update(cx, |_, window, cx| {
                input.update(cx, |state, cx| state.set_value(text, window, cx))
            })
            .unwrap();
        let start = std::time::Instant::now();
        for _ in 0..100 {
            window
                .update(cx, |_, _, cx| {
                    assert_eq!(
                        std::hint::black_box(composer_is_empty(std::hint::black_box(
                            input.read(cx),
                        ))),
                        expected,
                    );
                })
                .unwrap();
        }
        eprintln!(
            "approval_empty_draft_{case}_100_runs_ms={:.3}",
            start.elapsed().as_secs_f64() * 1000.
        );
    }
}

#[gpui_kit::test]
fn composer_has_text_matches_trimmed_value_across_unicode_and_rope_boundaries(
    cx: &mut TestAppContext,
) {
    let (window, input) = input_window(cx);
    let mut cases = 0;
    for blank in ["", " ", "\t\r\n", "\u{85}\u{a0}\u{2003}\u{2028}\u{3000}"] {
        for repeats in [0, 1, 511, 512, 513, 1024, 4096] {
            for content in ["", "x", "界🙂", "\0", "\u{200b}", "e\u{301}"] {
                let text = format!(
                    "{}{content}{}",
                    blank.repeat(repeats),
                    blank.repeat(repeats)
                );
                window
                    .update(cx, |_, window, cx| {
                        input.update(cx, |state, cx| state.set_value(text.clone(), window, cx));
                        let state = input.read(cx);
                        assert_eq!(composer_has_text(state), !state.value().trim().is_empty());
                        assert_eq!(&*state.value(), text.as_str());
                    })
                    .unwrap();
                cases += 1;
            }
        }
    }
    assert_eq!(cases, 168);
}

#[gpui_kit::test]
#[ignore = "manual release-mode performance probe"]
fn composer_eligibility_perf_probe(cx: &mut TestAppContext) {
    let (window, input) = input_window(cx);
    for (case, text) in [
        ("empty", String::new()),
        ("short", "ordinary draft".to_string()),
        ("ascii", "x".repeat(8 * 1024 * 1024)),
        ("unicode", "界🙂".repeat(8 * 1024 * 1024 / 7)),
        ("blank_ascii", " ".repeat(1024 * 1024)),
        ("blank_unicode", "\u{2003}".repeat(1024 * 1024 / 3)),
        ("late_text", format!("{}x", " ".repeat(1024 * 1024))),
    ] {
        let expected = !text.trim().is_empty();
        window
            .update(cx, |_, window, cx| {
                input.update(cx, |state, cx| state.set_value(text, window, cx))
            })
            .unwrap();
        let start = std::time::Instant::now();
        for _ in 0..100 {
            window
                .update(cx, |_, _, cx| {
                    assert_eq!(
                        std::hint::black_box(composer_has_text(std::hint::black_box(
                            input.read(cx),
                        ))),
                        expected,
                    );
                })
                .unwrap();
        }
        eprintln!(
            "composer_eligibility_{case}_100_runs_ms={:.3}",
            start.elapsed().as_secs_f64() * 1000.
        );
    }
}

#[gpui_kit::test]
#[ignore = "manual release-mode performance probe"]
fn composer_query_perf_probe(cx: &mut TestAppContext) {
    let (window, input) = input_window(cx);
    for (case, text) in [
        ("ascii", "x".repeat(8 * 1024 * 1024)),
        ("unicode", "界🙂".repeat(1_198_373)),
        ("short", "ordinary draft".to_string()),
    ] {
        window
            .update(cx, |_, window, cx| {
                input.update(cx, |input, cx| input.set_value(text, window, cx))
            })
            .unwrap();
        let start = std::time::Instant::now();
        for _ in 0..100 {
            window
                .update(cx, |_, _, cx| {
                    std::hint::black_box(composer_query_value(std::hint::black_box(
                        input.read(cx),
                    )));
                })
                .unwrap();
        }
        eprintln!(
            "composer_query_{case}_100_runs_ms={:.3}",
            start.elapsed().as_secs_f64() * 1000.
        );
    }
}

#[gpui_kit::test]
fn composer_query_value_keeps_full_text_and_clones_without_another_payload_copy(
    cx: &mut TestAppContext,
) {
    let (window, input) = input_window(cx);
    let original = format!("{} @界🙂", "full draft\n".repeat(1000));
    window
        .update(cx, |_, window, cx| {
            input.update(cx, |input, cx| {
                input.set_value(original.clone(), window, cx);
                let projected = composer_query_value(input);
                assert_eq!(&*projected, original.as_str());
                let clone = projected.clone();
                assert_eq!(clone.as_ptr(), projected.as_ptr());
                input.set_value("changed", window, cx);
                assert_eq!(&*projected, original.as_str());
                assert_eq!(&*clone, original.as_str());
            })
        })
        .unwrap();
}
