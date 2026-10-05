use std::time::Duration;

use flint_agent::AgentEvent;
use flint_agent::ApprovalDecision;
use flint_agent::FileDiff;
use flint_agent::NudgeReason;
use flint_agent::ToolKind;
use flint_agent::TurnEndReason;
use flint_agent::Usage;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::*;

fn secs(s: u64) -> Duration {
    Duration::from_secs(s)
}

#[test]
#[ignore = "manual release-mode performance probe"]
fn session_title_perf_probe() {
    for (case, text) in [
        ("ascii", "x".repeat(8 * 1024 * 1024)),
        ("unicode", "界🙂".repeat(8 * 1024 * 1024 / 7)),
        (
            "whitespace_tail",
            format!("short{}", " ".repeat(8 * 1024 * 1024)),
        ),
        (
            "multiline",
            format!("first line\n{}", "x".repeat(8 * 1024 * 1024)),
        ),
    ] {
        let start = std::time::Instant::now();
        for _ in 0..100 {
            std::hint::black_box(title_from(std::hint::black_box(&text)));
        }
        eprintln!(
            "session_title_{case}_100_runs_ms={:.3}",
            start.elapsed().as_secs_f64() * 1000.
        );
    }
}

fn reference_title(text: &str) -> String {
    let line = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    let line = line.trim();
    let title: String = line.chars().take(48).collect();
    if line.chars().count() > 48 {
        format!("{}…", title.trim_end())
    } else if title.is_empty() {
        "New session".into()
    } else {
        title
    }
}

#[test]
fn title_projection_matches_full_line_reference_at_unicode_and_whitespace_boundaries() {
    for prefix in ["", " \t", "\r\n\n \u{2003}", "\u{2028}"] {
        for scalar in ["a", "界", "🙂", "é\u{301}"] {
            for count in [0, 1, 47, 48, 49, 50, 64] {
                for suffix in ["", " \t\r", "\nnext line", "\r\nnext line", "\u{2003}x"] {
                    let text = format!("{prefix}{}{suffix}", scalar.repeat(count));
                    assert_eq!(title_from(&text), reference_title(&text), "{text:?}");
                }
            }
        }
    }
    for text in [
        format!("{} \t\nignored", "x".repeat(48)),
        format!("{} \u{2003}later\nignored", "x".repeat(47)),
        format!("short{}\nignored", " ".repeat(100_000)),
        "\n\r\n \t\u{2003}".into(),
        "before\rafter".into(),
    ] {
        assert_eq!(title_from(&text), reference_title(&text));
    }
}

#[test]
fn bounded_title_keeps_the_complete_first_message_and_does_not_retitle() {
    let text = format!("\n \t{}\nsecond line", "界🙂".repeat(100_000));
    let mut view = SessionView::default();
    assert_eq!(view.push_user(text.clone()).appended, 0..1);
    assert_eq!(
        view.title.as_deref(),
        Some(format!("{}…", "界🙂".repeat(24)).as_str())
    );
    assert_eq!(view.items, [Item::User(text)]);
    view.push_user("another message".into());
    assert_eq!(
        view.title.as_deref(),
        Some(format!("{}…", "界🙂".repeat(24)).as_str())
    );
}

fn started(call_id: &str, kind: ToolKind, summary: &str) -> AgentEvent {
    AgentEvent::ToolCallStarted {
        call_id: call_id.to_string(),
        name: "run_command".to_string(),
        kind,
        args: json!({}),
        summary: summary.to_string(),
    }
}

#[test]
#[ignore = "manual release-mode performance probe"]
fn argument_count_perf_probe() {
    let value = json!({"content": "x".repeat(8 * 1024 * 1024)});
    let start = std::time::Instant::now();
    for _ in 0..100 {
        std::hint::black_box(serialized_chars(std::hint::black_box(&value)));
    }
    eprintln!(
        "argument_count_100_runs_ms={:.3}",
        start.elapsed().as_secs_f64() * 1000.
    );
    assert_eq!(serialized_chars(&value), 8 * 1024 * 1024 + 14);
}

#[test]
fn argument_character_count_matches_serialized_json_without_changing_arguments() {
    for args in [
        json!(null),
        json!(false),
        json!([true, i64::MIN, u64::MAX, -2.3e100, 0.125]),
        json!(""),
        json!({"é\u{301}🙂界": ["a\"b\\c\n\r\t\u{0000}\u{001f}", null, {"x": []}]}),
        json!({"content": "界🙂\\\"\n".repeat(128 * 1024)}),
    ] {
        let expected = args.to_string().chars().count();
        assert_eq!(serialized_chars(&args), expected);
        let mut view = SessionView::default();
        view.fold(AgentEvent::TurnStarted { turn_id: 1 }, secs(0));
        view.fold(
            AgentEvent::ToolCallStarted {
                call_id: "counted".into(),
                name: "edit_file".into(),
                kind: ToolKind::Edit,
                summary: "fixture".into(),
                args: args.clone(),
            },
            secs(1),
        );
        assert_eq!(view.streamed_chars, expected);
        assert_eq!(view.output_tokens(), (expected / 4) as u64);
        assert!(matches!(&view.items[0], Item::Tool(call) if call.args == args));
        view.fold(AgentEvent::TurnStarted { turn_id: 2 }, secs(2));
        assert_eq!(view.streamed_chars, 0);
    }
}

#[test]
fn argument_character_count_handles_split_utf8_and_empty_writes() {
    use std::io::Write as _;
    let text = "é\u{301}🙂界 ASCII";
    for size in 1..=text.len() {
        let mut count = CharacterCount(0);
        count.write_all(&[]).unwrap();
        for chunk in text.as_bytes().chunks(size) {
            count.write_all(chunk).unwrap();
        }
        count.flush().unwrap();
        assert_eq!(count.0, text.chars().count());
    }
}

#[test]
fn approval_preview_uses_full_arguments_not_a_truncated_summary() {
    let mut view = SessionView::default();
    view.fold(
        AgentEvent::ToolCallStarted {
            call_id: "c1".into(),
            name: "run_command".into(),
            kind: ToolKind::Command,
            args: json!({"command": "printf 'first\\nsecond\\n'", "timeout_secs": 10}),
            summary: "printf 'first".into(),
        },
        secs(0),
    );
    let preview = view.approval_preview("c1", std::path::Path::new("/tmp/work"), true);
    assert_eq!(
        preview.fields,
        vec![
            ("Command", "printf 'first\\nsecond\\n'".into()),
            ("Working directory", "/tmp/work".into())
        ]
    );
    let missing = view.approval_preview("unknown", std::path::Path::new("/tmp/work"), false);
    assert!(missing.fields.is_empty());

    view.fold(
        AgentEvent::ToolCallStarted {
            call_id: "e1".into(),
            name: "edit_file".into(),
            kind: ToolKind::Edit,
            args: json!({"path": "src/a.rs", "old_string": "before", "new_string": "after"}),
            summary: "src/a.rs".into(),
        },
        secs(1),
    );
    assert_eq!(
        view.approval_preview("e1", std::path::Path::new("/tmp/work"), true)
            .fields,
        vec![
            ("Affected file", "src/a.rs".into()),
            ("Existing text", "before".into()),
            ("Replacement text", "after".into())
        ]
    );
}

#[test]
fn acp_broad_approval_keeps_other_requests_pending() {
    let mut view = SessionView::default();
    for id in ["a", "b"] {
        view.fold(
            AgentEvent::ApprovalRequested {
                call_id: id.into(),
                kind: ToolKind::Command,
                summary: id.into(),
            },
            secs(0),
        );
    }
    view.resolve_approval_scoped("a", ApprovalDecision::ApproveAlways, false);
    assert_eq!(view.pending_approvals, 1);
    assert_eq!(view.pending_approval().unwrap().0, "b");
}

#[test]
fn streams_reasoning_then_text_into_separate_items() {
    let mut view = SessionView::default();
    view.fold(AgentEvent::TurnStarted { turn_id: 1 }, secs(0));
    view.fold(
        AgentEvent::StepStarted {
            turn_id: 1,
            step: 1,
        },
        secs(0),
    );
    view.fold(AgentEvent::ReasoningDelta("Look at ".into()), secs(1));
    view.fold(AgentEvent::ReasoningDelta("the tests.".into()), secs(2));
    let change = view.fold(AgentEvent::TextDelta("On it".into()), secs(4));
    view.fold(AgentEvent::TextDelta(".".into()), secs(5));

    assert_eq!(
        change,
        Change {
            appended: 1..2,
            updated: vec![0],
            children: Vec::new(),
        }
    );
    assert_eq!(
        view.items,
        vec![
            Item::Thinking {
                text: "Look at the tests.".into(),
                started: secs(1),
                duration: Some(secs(3)),
                expanded: false,
            },
            Item::Assistant {
                text: "On it.".into(),
                streaming: true,
            },
        ]
    );
    assert!(view.running);
}

#[test]
fn tool_output_streams_then_finishes_and_failed_commands_open() {
    let mut view = SessionView::default();
    view.fold(AgentEvent::TextDelta("Running tests.".into()), secs(0));
    let change = view.fold(started("c1", ToolKind::Command, "npm test"), secs(1));
    assert_eq!(
        change,
        Change {
            appended: 1..2,
            updated: vec![0],
            children: Vec::new(),
        }
    );
    view.fold(
        AgentEvent::ToolOutputDelta {
            call_id: "c1".into(),
            chunk: "FAIL ".into(),
        },
        secs(2),
    );
    let change = view.fold(
        AgentEvent::ToolCallFinished {
            call_id: "c1".into(),
            output: "FAIL src/a.test.ts".into(),
            exit_code: Some(1),
            success: false,
            diff: None,
            duration_ms: 1200,
        },
        secs(3),
    );

    assert_eq!(change, Change::updated(1));
    assert_eq!(
        view.items[1],
        Item::Tool(Box::new(ToolCall {
            call_id: "c1".into(),
            name: "run_command".into(),
            kind: ToolKind::Command,
            summary: "npm test".into(),
            args: json!({}),
            output: "FAIL src/a.test.ts".into(),
            live_output_newlines: 0,
            terminal_id: None,
            subagent: None,
            started: secs(1),
            result: Some(ToolResult {
                exit_code: Some(1),
                success: false,
                diff: None,
                duration_ms: 1200,
            }),
            expanded: false,
        }))
    );
}

#[test]
fn edits_accumulate_in_changes() {
    let mut view = SessionView::default();
    let diff = |added, removed| FileDiff {
        path: "src/lru.ts".into(),
        unified: format!("+{added} -{removed}"),
        added,
        removed,
        created: false,
    };
    for (id, d) in [("e1", diff(3, 1)), ("e2", diff(2, 2))] {
        view.fold(started(id, ToolKind::Edit, "src/lru.ts"), secs(0));
        view.fold(
            AgentEvent::ToolCallFinished {
                call_id: id.into(),
                output: "ok".into(),
                exit_code: None,
                success: true,
                diff: Some(d),
                duration_ms: 5,
            },
            secs(1),
        );
    }

    assert_eq!(
        view.changes,
        vec![ChangedFile {
            path: "src/lru.ts".into(),
            added: 5,
            removed: 3,
            created: false,
            diffs: vec!["+3 -1".into(), "+2 -2".into()],
            combined: None,
        }]
    );
}

#[test]
fn a_new_edit_invalidates_the_previous_combined_diff() {
    let mut view = SessionView::default();
    for n in 0..2 {
        let id = format!("e{n}");
        view.fold(started(&id, ToolKind::Edit, "a.txt"), secs(0));
        view.fold(
            AgentEvent::ToolCallFinished {
                call_id: id,
                output: "ok".into(),
                exit_code: None,
                success: true,
                diff: Some(FileDiff {
                    path: "a.txt".into(),
                    unified: format!("+edit {n}"),
                    added: 1,
                    removed: 0,
                    created: false,
                }),
                duration_ms: 5,
            },
            secs(1),
        );
        assert_eq!(view.changes[0].combined, None);
        let revision = view.changes_revision;
        view.set_combined("a.txt", format!("+settled {n}"), n + 1, 0);
        assert_ne!(view.changes_revision, revision);
    }
    assert_eq!(view.changes[0].diffs, ["+edit 0", "+edit 1"]);
}

#[test]
fn turn_finish_closes_streams_and_summarizes() {
    let mut view = SessionView::default();
    view.push_user("Fix the LRU eviction bug\nmore detail".into());
    view.fold(AgentEvent::TurnStarted { turn_id: 7 }, secs(10));
    view.fold(
        AgentEvent::StepStarted {
            turn_id: 7,
            step: 3,
        },
        secs(11),
    );
    view.fold(AgentEvent::TextDelta("Done.".into()), secs(12));
    let usage = Usage {
        input_tokens: 1000,
        cached_input_tokens: 800,
        output_tokens: 50,
        reasoning_tokens: 10,
    };
    view.fold(AgentEvent::Usage(usage), secs(12));
    view.fold(
        AgentEvent::TurnFinished {
            turn_id: 7,
            reason: TurnEndReason::Completed,
        },
        secs(52),
    );

    assert_eq!(view.title.as_deref(), Some("Fix the LRU eviction bug"));
    assert_eq!(
        view.items[1..],
        [
            Item::Assistant {
                text: "Done.".into(),
                streaming: false,
            },
            Item::TurnSummary {
                reason: TurnEndReason::Completed,
                duration: secs(42),
                steps: 3,
                usage,
            },
        ]
    );
    assert!(!view.running);
    assert_eq!(view.session_usage, usage);
    assert_eq!(view.elapsed(secs(100)), Some(secs(42)));
}

#[test]
fn output_tokens_are_estimated_until_usage_is_reported() {
    let mut view = SessionView::default();
    view.fold(AgentEvent::TurnStarted { turn_id: 1 }, secs(0));
    assert_eq!(view.output_tokens(), 0);
    view.fold(AgentEvent::ReasoningDelta("a".repeat(40)), secs(1));
    view.fold(AgentEvent::TextDelta("b".repeat(40)), secs(2));
    view.fold(
        AgentEvent::ToolCallStarted {
            call_id: "c1".into(),
            name: "Terminal".into(),
            kind: ToolKind::Command,
            args: serde_json::json!({"command": "ls"}),
            summary: "ls".into(),
        },
        secs(3),
    );
    // 80 streamed characters plus 17 of arguments, about 4 a token.
    assert_eq!(view.output_tokens(), 24);
    view.fold(
        AgentEvent::Usage(Usage {
            output_tokens: 275,
            ..Usage::default()
        }),
        secs(4),
    );
    assert_eq!(view.output_tokens(), 275);
    // The next turn starts counting again.
    view.fold(AgentEvent::TurnStarted { turn_id: 2 }, secs(5));
    assert_eq!(view.output_tokens(), 0);
}

#[test]
fn approvals_and_nudges() {
    let mut view = SessionView::default();
    view.fold(
        AgentEvent::ApprovalRequested {
            call_id: "c9".into(),
            kind: ToolKind::Command,
            summary: "rm -rf build".into(),
        },
        secs(0),
    );
    assert_eq!(view.pending_approvals, 1);
    assert_eq!(
        view.resolve_approval("c9", ApprovalDecision::Deny),
        Change::updated(0)
    );
    assert_eq!(view.pending_approvals, 0);
    assert_eq!(
        view.resolve_approval("c9", ApprovalDecision::Approve),
        Change::default()
    );

    view.fold(
        AgentEvent::HarnessNudge {
            reason: NudgeReason::Verify,
            message: "run the tests".into(),
        },
        secs(1),
    );
    assert_eq!(
        view.items,
        vec![
            Item::Approval {
                call_id: "c9".into(),
                kind: ToolKind::Command,
                summary: "rm -rf build".into(),
                decision: Some(ApprovalDecision::Deny),
            },
            Item::Nudge {
                reason: NudgeReason::Verify,
                message: "run the tests".into(),
                expanded: false,
            },
        ]
    );
}

#[test]
fn toggles_only_expandable_items() {
    let mut view = SessionView::default();
    view.push_user("hi".into());
    view.fold(AgentEvent::ReasoningDelta("hmm".into()), secs(0));
    assert_eq!(view.toggle_expanded(0), Change::default());
    assert_eq!(view.toggle_expanded(1), Change::updated(1));
    assert!(matches!(
        view.items[1],
        Item::Thinking { expanded: true, .. }
    ));
}

#[test]
fn titles_are_trimmed() {
    assert_eq!(title_from("\n  hello world  \n"), "hello world");
    assert_eq!(title_from(""), "New session");
    let long = "a".repeat(60);
    assert_eq!(title_from(&long), format!("{}…", "a".repeat(48)));
}

#[test]
fn live_output_keeps_only_the_tail() {
    let mut view = SessionView::default();
    view.fold(started("c1", ToolKind::Command, "yes"), secs(0));
    for n in 0..MAX_LIVE_OUTPUT_LINES + 50 {
        view.fold(
            AgentEvent::ToolOutputDelta {
                call_id: "c1".into(),
                chunk: format!("line {n}\n"),
            },
            secs(1),
        );
    }
    let Item::Tool(call) = &view.items[0] else {
        panic!("expected a tool card");
    };
    assert_eq!(call.output.lines().count(), MAX_LIVE_OUTPUT_LINES);
    assert_eq!(call.output.lines().next(), Some("line 50"));
}

#[test]
fn newline_free_live_output_has_a_utf8_safe_byte_limit() {
    let mut view = SessionView::default();
    view.fold(
        AgentEvent::ToolCallStarted {
            call_id: "flood".into(),
            name: "run_command".into(),
            kind: ToolKind::Command,
            args: json!({}),
            summary: "controlled output".into(),
        },
        Duration::ZERO,
    );
    for _ in 0..32 {
        view.fold(
            AgentEvent::ToolOutputDelta {
                call_id: "flood".into(),
                chunk: "界🙂".repeat(4_096),
            },
            Duration::ZERO,
        );
    }
    view.fold(
        AgentEvent::ToolOutputDelta {
            call_id: "flood".into(),
            chunk: "final tail".into(),
        },
        Duration::ZERO,
    );
    let Item::Tool(call) = &view.items[0] else {
        panic!("missing tool");
    };
    assert!(call.output.len() <= 256 * 1024);
    assert!(call.output.ends_with("final tail"));
    assert!(!call.output.contains('\u{fffd}'));
}

#[test]
fn oversized_live_delta_does_not_retain_its_allocation() {
    let mut view = SessionView::default();
    view.fold(started("large", ToolKind::Command, "output"), secs(0));
    view.fold(
        AgentEvent::ToolOutputDelta {
            call_id: "large".into(),
            chunk: format!("{}last line\n", "界🙂\n".repeat(1_000_000)),
        },
        secs(1),
    );
    let Item::Tool(call) = &view.items[0] else {
        panic!("expected tool");
    };
    assert!(call.output.ends_with("last line\n"));
    assert!(call.output.len() <= MAX_LIVE_OUTPUT_BYTES);
    assert!(
        call.output.capacity() <= MAX_LIVE_OUTPUT_BYTES * 2,
        "retained {} bytes for {} bytes of output",
        call.output.capacity(),
        call.output.len()
    );
}

#[test]
fn tiny_lines_cannot_bypass_live_output_retention() {
    let mut view = SessionView::default();
    view.fold(started("blank", ToolKind::Command, "output"), secs(0));
    view.fold(
        AgentEvent::ToolOutputDelta {
            call_id: "blank".into(),
            chunk: "\n".repeat(MAX_LIVE_OUTPUT_LINES * 2),
        },
        secs(1),
    );
    let Item::Tool(call) = &view.items[0] else {
        panic!("expected tool");
    };
    assert_eq!(
        call.output.bytes().filter(|&b| b == b'\n').count(),
        MAX_LIVE_OUTPUT_LINES
    );
}

#[test]
fn live_output_matches_a_bounded_reference_across_mixed_chunks() {
    let mut view = SessionView::default();
    view.fold(started("mixed", ToolKind::Command, "output"), secs(0));
    let mut expected = String::new();
    for chunk in [
        "line\n".repeat(2_001),
        "界🙂".repeat(60_000),
        "\n".repeat(1_000),
        "short".into(),
        "another line\r\n".repeat(5_000),
        "final tail".into(),
    ] {
        expected.push_str(&chunk);
        let cut = tail_start(&expected, MAX_LIVE_OUTPUT_BYTES);
        expected.drain(..cut);
        let lines = expected.bytes().filter(|&b| b == b'\n').count();
        if lines > MAX_LIVE_OUTPUT_LINES {
            let cut = expected
                .match_indices('\n')
                .nth(lines - MAX_LIVE_OUTPUT_LINES - 1)
                .unwrap()
                .0
                + 1;
            expected.drain(..cut);
        }
        view.fold(
            AgentEvent::ToolOutputDelta {
                call_id: "mixed".into(),
                chunk,
            },
            secs(1),
        );
        let Item::Tool(call) = &view.items[0] else {
            panic!("expected tool");
        };
        assert_eq!(call.output, expected);
        assert_eq!(
            call.live_output_newlines,
            expected.bytes().filter(|&b| b == b'\n').count()
        );
    }
}

#[test]
fn late_live_output_cannot_replace_a_finished_result() {
    let mut view = SessionView::default();
    view.fold(started("done", ToolKind::Command, "output"), secs(0));
    view.fold(
        AgentEvent::ToolCallFinished {
            call_id: "done".into(),
            output: "final output".into(),
            exit_code: Some(0),
            success: true,
            diff: None,
            duration_ms: 1,
        },
        secs(1),
    );
    assert_eq!(
        view.fold(
            AgentEvent::ToolOutputDelta {
                call_id: "done".into(),
                chunk: "late".into(),
            },
            secs(2)
        ),
        Change::default()
    );
    let Item::Tool(call) = &view.items[0] else {
        panic!("expected tool");
    };
    assert_eq!(call.output, "final output");
}

#[test]
#[ignore = "manual release-mode performance probe"]
fn live_output_perf_probe() {
    let mut view = SessionView::default();
    view.fold(started("probe", ToolKind::Command, "output"), secs(0));
    view.fold(
        AgentEvent::ToolOutputDelta {
            call_id: "probe".into(),
            chunk: "x".repeat(128 * 1024),
        },
        secs(0),
    );
    let start = std::time::Instant::now();
    for _ in 0..10_000 {
        view.fold(
            AgentEvent::ToolOutputDelta {
                call_id: "probe".into(),
                chunk: "hello\n".into(),
            },
            secs(1),
        );
    }
    eprintln!(
        "live_output_10000_chunks_ms={:.3}",
        start.elapsed().as_secs_f64() * 1000.
    );
    view.fold(
        AgentEvent::ToolOutputDelta {
            call_id: "probe".into(),
            chunk: "z".repeat(8 * 1024 * 1024),
        },
        secs(2),
    );
    let Item::Tool(call) = &view.items[0] else {
        panic!("expected tool");
    };
    eprintln!(
        "live_output_retained_capacity_bytes={}",
        call.output.capacity()
    );
    std::hint::black_box(view);
}

#[test]
fn a_terminals_call_gets_the_terminals_id() {
    let mut view = SessionView::default();
    view.fold(started("c1", ToolKind::Command, "npm test"), secs(0));
    let change = view.fold(
        AgentEvent::TerminalStarted {
            terminal_id: "t1".into(),
            call_id: Some("c1".into()),
            label: "claude: npm test".into(),
            cwd: None,
        },
        secs(1),
    );
    assert_eq!(change, Change::updated(0));
    let Item::Tool(call) = &view.items[0] else {
        panic!("expected a tool card");
    };
    assert_eq!(call.terminal_id.as_deref(), Some("t1"));
    // A terminal with no call (an agent's own shell) touches nothing.
    assert_eq!(
        view.fold(
            AgentEvent::TerminalStarted {
                terminal_id: "t2".into(),
                call_id: None,
                label: "claude: zsh".into(),
                cwd: None,
            },
            secs(2),
        ),
        Change::default()
    );
}

#[test]
fn child_events_stay_nested_but_edits_contribute_to_parent_changes() {
    let mut view = SessionView::default();
    view.fold(AgentEvent::TurnStarted { turn_id: 1 }, secs(0));
    view.fold(started("delegate", ToolKind::Other, "Research"), secs(1));
    view.fold(
        AgentEvent::SubagentStarted {
            call_id: "delegate".into(),
            session_id: "agent-1".into(),
            model: "child-model".into(),
        },
        secs(1),
    );
    for event in [
        AgentEvent::TurnStarted { turn_id: 1 },
        AgentEvent::StepStarted {
            turn_id: 1,
            step: 3,
        },
        AgentEvent::TextDelta("Child answer.".into()),
        started("agent-1:edit", ToolKind::Edit, "a.txt"),
        AgentEvent::ToolCallFinished {
            call_id: "agent-1:edit".into(),
            output: "Created.".into(),
            exit_code: None,
            success: true,
            duration_ms: 2,
            diff: Some(FileDiff {
                path: "a.txt".into(),
                unified: "+hi".into(),
                added: 1,
                removed: 0,
                created: true,
            }),
        },
        AgentEvent::TurnFinished {
            turn_id: 1,
            reason: TurnEndReason::Completed,
        },
    ] {
        assert_eq!(
            view.fold(
                AgentEvent::SubagentEvent {
                    call_id: "delegate".into(),
                    event: Box::new(event),
                },
                secs(2)
            )
            .updated,
            vec![0]
        );
    }
    assert!(view.running);
    assert_eq!(view.step, 0);
    assert_eq!(view.items.len(), 1);
    assert_eq!(view.changes[0].path, "a.txt");
    assert_eq!(view.turns[0].files, vec!["a.txt"]);
    let Item::Tool(call) = &view.items[0] else {
        panic!("tool")
    };
    let child = view
        .subagent(&call.subagent.as_ref().expect("child").session_id)
        .expect("child conversation");
    assert_eq!(child.model, "child-model");
    assert!(!child.view.running);
    assert!(child.view.items.iter().any(|item| matches!(item,
        Item::Assistant { text, .. } if text == "Child answer."
    )));
}

#[test]
fn approve_always_resolves_parallel_child_approvals_and_interrupt_expires_them() {
    let mut view = SessionView::default();
    view.fold(AgentEvent::TurnStarted { turn_id: 1 }, secs(0));
    for call_id in ["agent-1:edit", "agent-2:edit"] {
        view.fold(
            AgentEvent::ApprovalRequested {
                call_id: call_id.into(),
                kind: ToolKind::Edit,
                summary: "a.txt".into(),
            },
            secs(1),
        );
    }
    assert_eq!(view.pending_approvals, 2);
    let change = view.resolve_approval("agent-1:edit", ApprovalDecision::ApproveAlways);
    assert_eq!(change.updated, vec![0, 1]);
    assert_eq!(view.pending_approvals, 0);
    assert!(view.pending_approval().is_none());
    view.fold(
        AgentEvent::ApprovalRequested {
            call_id: "agent-3:edit".into(),
            kind: ToolKind::Edit,
            summary: "a.txt".into(),
        },
        secs(2),
    );
    view.fold(
        AgentEvent::TurnFinished {
            turn_id: 1,
            reason: TurnEndReason::Interrupted,
        },
        secs(3),
    );
    assert_eq!(view.pending_approvals, 0);
    assert!(view.pending_approval().is_none());
}
