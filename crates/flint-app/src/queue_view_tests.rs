use super::{MAX_EXACT_QUEUE_CHARS, character_detail, preview};
use crate::ui::MAX_HEADER_PREVIEW_CHARS;

#[test]
fn queued_character_detail_is_exact_through_the_limit_then_explicitly_lower_bound() {
    for scalar in ['x', '界', '🙂', 'é'] {
        for count in [0, 1, 1999, 2000, 2001, 8192] {
            let text = scalar.to_string().repeat(count);
            let expected = if count <= MAX_EXACT_QUEUE_CHARS {
                format!("{count} characters")
            } else {
                "More than 2,000 characters".into()
            };
            assert_eq!(character_detail(&text), expected);
        }
    }
    for text in ["\n \t", "é\u{301}🙂\r\n", "\u{2003}x "] {
        assert_eq!(
            character_detail(text),
            format!("{} characters", text.chars().count())
        );
    }
}

#[test]
fn queued_character_detail_does_not_mutate_full_text_or_captured_context() {
    let text = format!("{}\nORIGINAL_BODY", "界🙂".repeat(4096));
    let message = format!("{text}\n\n[Captured context]\nFROZEN_ATTACHMENT");
    let prompt = crate::prompt_queue::Prompt::new(text.clone(), message.clone(), Vec::new());
    let original = prompt.clone();
    assert_eq!(character_detail(&prompt.text), "More than 2,000 characters");
    assert_eq!(prompt, original);
    assert_eq!(prompt.text, text);
    assert_eq!(prompt.message, message);
}

#[test]
#[ignore = "manual release-mode performance probe"]
fn queued_character_detail_perf_probe() {
    for (case, text) in [
        ("ascii", "x".repeat(8 * 1024 * 1024)),
        ("unicode", "界🙂".repeat(8 * 1024 * 1024 / 7)),
        ("short_ascii", "x".repeat(64)),
        ("short_unicode", "界🙂".repeat(32)),
    ] {
        let start = std::time::Instant::now();
        for _ in 0..100 {
            std::hint::black_box(character_detail(std::hint::black_box(&text)));
        }
        eprintln!(
            "queued_character_detail_{case}_100_runs_ms={:.3}",
            start.elapsed().as_secs_f64() * 1000.
        );
    }
}

#[test]
#[ignore = "manual release-mode performance probe"]
fn queued_preview_perf_probe() {
    for (case, text) in [
        ("ascii", "x".repeat(8 * 1024 * 1024)),
        ("unicode", "界🙂".repeat(8 * 1024 * 1024 / 7)),
    ] {
        let start = std::time::Instant::now();
        let mut capacity = 0;
        for _ in 0..100 {
            let result = preview(std::hint::black_box(&text));
            capacity = capacity.max(result.capacity());
            std::hint::black_box(result);
        }
        eprintln!(
            "queued_preview_{case}_100_runs_ms={:.3}",
            start.elapsed().as_secs_f64() * 1000.
        );
        eprintln!("queued_preview_{case}_capacity_bytes={capacity}");
    }
}

#[test]
fn queued_prompt_preview_is_bounded_before_layout() {
    for text in [
        "x".repeat(8 * 1024 * 1024),
        format!("\n \t{}\nSECOND_LINE", "界🙂".repeat(512)),
    ] {
        let result = preview(&text);
        assert!(result.chars().count() <= MAX_HEADER_PREVIEW_CHARS + 2);
        assert!(result.ends_with(" …"));
        assert!(!result.contains("SECOND_LINE"));
        assert!(result.capacity() <= MAX_HEADER_PREVIEW_CHARS * 8);
    }
}

#[test]
fn queued_preview_selects_text_or_image_fallback_without_changing_the_request() {
    for (text, expected) in [
        ("", "Image prompt"),
        ("\n \t\u{2003}\r\n", "Image prompt"),
        ("ordinary prompt", "ordinary prompt"),
        ("\n \t first 界 \r\n", "first 界"),
        ("first\nsecond", "first …"),
        ("first\n \t", "first"),
    ] {
        assert_eq!(preview(text), expected);
    }
    let text = format!("{}\nORIGINAL_BODY", "🙂".repeat(512));
    let message = format!("{text}\n\n[Captured context]\nFROZEN_ATTACHMENT");
    let prompt = crate::prompt_queue::Prompt::new(text.clone(), message.clone(), Vec::new());
    let original = prompt.clone();
    assert!(preview(&prompt.text).ends_with(" …"));
    assert_eq!(prompt, original);
    assert_eq!(prompt.text, text);
    assert_eq!(prompt.message, message);
}
