use super::*;

fn reference(query: &str) -> Vec<(SlashCommand, &'static str, &'static str)> {
    let query = query.to_lowercase();
    let mut found: Vec<_> = COMMANDS
        .iter()
        .copied()
        .filter(|(_, name, _)| name[1..].starts_with(&query))
        .collect();
    found.sort_by_key(|(_, name, _)| name[1..] != query);
    found
}

#[test]
fn slash_matches_preserve_case_prefix_order_and_unicode_reference() {
    let mut cases = 0;
    for (_, name, _) in COMMANDS {
        let command = &name[1..];
        for boundary in 0..=command.len() {
            let prefix = &command[..boundary];
            for query in [
                prefix.to_string(),
                prefix.to_uppercase(),
                format!("{prefix}x"),
            ] {
                assert_eq!(matches(&query), reference(&query), "{query:?}");
                cases += 1;
            }
        }
    }
    for repeats in [0, 1, 7, 8, 9, 16, 1024] {
        for value in [
            "İ", "Σ", "ΟΣ", "K", "界🙂", "e\u{301}", "\0", "\u{200b}", " ",
        ] {
            for prefix in ["", "mo", "APPROVAL"] {
                let query = format!("{prefix}{}", value.repeat(repeats));
                assert_eq!(matches(&query), reference(&query), "{query:?}");
                cases += 1;
            }
        }
    }
    assert_eq!(cases, 369);
    assert_eq!(matches("mode")[0].0, SlashCommand::Mode);
    assert_eq!(matches("MO")[0].0, SlashCommand::Model);
}

#[test]
fn slash_matches_reject_large_queries_without_mutating_them() {
    for query in [
        "A".repeat(1024 * 1024),
        "界🙂".repeat(1024 * 1024 / 7),
        format!("approval{}", "\u{200b}".repeat(1024)),
    ] {
        let original = query.clone();
        assert_eq!(matches(&query), reference(&query));
        assert!(matches(&query).is_empty());
        assert_eq!(query, original);
    }
}

#[test]
#[ignore = "manual release-mode performance probe"]
fn slash_matching_perf_probe() {
    for (case, query) in [
        ("empty", String::new()),
        ("short", "MO".to_string()),
        ("exact", "approval".to_string()),
        ("boundary", "APPROVALX".to_string()),
        ("ascii", "A".repeat(8 * 1024 * 1024)),
        ("unicode", "界🙂".repeat(8 * 1024 * 1024 / 7)),
        ("expanding", "İ".repeat(1024 * 1024 / 2)),
    ] {
        let expected = reference(&query);
        let start = std::time::Instant::now();
        for _ in 0..100 {
            assert_eq!(
                std::hint::black_box(matches(std::hint::black_box(&query))),
                expected,
            );
        }
        eprintln!(
            "slash_matching_{case}_100_runs_ms={:.3}",
            start.elapsed().as_secs_f64() * 1000.
        );
    }
}
