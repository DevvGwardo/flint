use pretty_assertions::assert_eq;

use super::*;

// The pre-streaming scalar-vector scorer is an independent equivalence oracle.
fn vector_score(query: &str, path: &str) -> Option<i64> {
    if query.is_empty() {
        return Some(0);
    }
    let path_lower = path.to_lowercase();
    let chars: Vec<char> = path_lower.chars().collect();
    let name_start = chars.iter().rposition(|&ch| ch == '/').map_or(0, |i| i + 1);
    let mut score = 0i64;
    let mut at = 0usize;
    let mut previous = None;
    for q in query.to_lowercase().chars() {
        let found = (at..chars.len()).find(|&i| chars[i] == q)?;
        score += 1;
        if previous.is_some_and(|p| p + 1 == found) {
            score += 5;
        }
        if found >= name_start {
            score += 3;
        }
        if found == 0 || matches!(chars[found - 1], '/' | '_' | '-' | '.') {
            score += 4;
        }
        previous = Some(found);
        at = found + 1;
    }
    Some(score * 100 - path.len() as i64)
}

#[test]
fn streaming_scorer_matches_vector_scores_and_full_top_eight_order() {
    let parts = [
        "", "Main", "目录", "界🙂", "É", "e\u{301}", "İ", "ΟΣ", "Σ", "_", "-", ".",
    ];
    let mut files = Vec::new();
    for directory in parts {
        for stem in parts {
            files.push(format!("{directory}/{stem}.rs"));
            files.push(format!("{directory}_{stem}"));
            files.push(format!("{directory}/{stem}/"));
        }
    }
    files.extend(["", "root.rs", "a-b_c.d", "İ🙂/İ🙂.rs"].map(String::from));
    files.reverse();
    let original = files.clone();
    let queries = [
        "", "MAIN", "ma", "aa", "目录", "界🙂", "🙂", "É", "e\u{301}", "İ", "i\u{307}", "İ🙂",
        "ΟΣ", "Σ", "_", "-", ".", "/", "rs", "a_c", "xyz",
    ];
    for query in queries {
        let mut expected = Vec::new();
        for path in &files {
            let old_score = vector_score(query, path);
            assert_eq!(
                score(query, path),
                old_score,
                "query {query:?}, path {path:?}"
            );
            if let Some(score) = old_score {
                expected.push((score, path));
            }
        }
        expected.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(b.1)));
        let expected: Vec<_> = expected
            .into_iter()
            .take(MAX_RESULTS)
            .map(|(_, path)| path)
            .collect();
        assert_eq!(search(&files, query), expected, "query {query:?}");
    }
    assert_eq!(files, original);
}

#[test]
#[ignore = "manual release-mode performance probe"]
fn mention_search_perf_probe() {
    let files: Vec<_> = (0..MAX_INDEXED_FILES)
        .map(|n| format!("src/目录_{n:05}/Module_{n:05}/Main.RS"))
        .collect();
    for query in ["", "MAIN", "module_1", "目录", "zzzz"] {
        let label = if query.is_empty() { "empty" } else { query };
        let start = std::time::Instant::now();
        for _ in 0..20 {
            std::hint::black_box(search(
                std::hint::black_box(&files),
                std::hint::black_box(query),
            ));
        }
        eprintln!(
            "mention_search_{label}_20_runs_ms={:.3}",
            start.elapsed().as_secs_f64() * 1000.
        );
        let hits = ranked_hits(&files, &query.to_lowercase());
        eprintln!(
            "mention_search_{label}_hit_capacity_bytes={}",
            hits.capacity() * std::mem::size_of::<(i64, &String)>()
        );
        let results = search(&files, query);
        eprintln!(
            "mention_search_{label}_result_capacity_bytes={}",
            results.capacity() * std::mem::size_of::<&String>()
        );
    }
    assert_eq!(search(&files, "MAIN").len(), MAX_RESULTS);
    assert!(search(&files, "zzzz").is_empty());
}

#[test]
#[ignore = "manual release-mode ordering stress probe"]
fn mention_ordering_perf_probe() {
    let mut files: Vec<_> = (0..MAX_INDEXED_FILES)
        .map(|n| format!("src/目录_{n:05}/Module_{n:05}/Main.RS"))
        .collect();
    for order in ["sorted", "reverse"] {
        if order == "reverse" {
            files.reverse();
        }
        for query in ["", "main"] {
            let normalized = query.to_lowercase();
            for selector in ["all_hits", "bounded"] {
                let start = std::time::Instant::now();
                for _ in 0..20 {
                    let hits = if selector == "all_hits" {
                        let mut hits: Vec<_> = files
                            .iter()
                            .filter_map(|path| {
                                score_lowercase(&normalized, path).map(|score| (score, path))
                            })
                            .collect();
                        let rank = |a: &(i64, &String), b: &(i64, &String)| {
                            b.0.cmp(&a.0).then_with(|| a.1.cmp(b.1))
                        };
                        if hits.len() > MAX_RESULTS {
                            hits.select_nth_unstable_by(MAX_RESULTS, rank);
                            hits.truncate(MAX_RESULTS);
                        }
                        hits.sort_by(rank);
                        hits
                    } else {
                        ranked_hits(&files, &normalized)
                    };
                    std::hint::black_box(hits);
                }
                let label = if query.is_empty() { "empty" } else { query };
                eprintln!(
                    "mention_order_{order}_{label}_{selector}_20_runs_ms={:.3}",
                    start.elapsed().as_secs_f64() * 1000.
                );
            }
        }
    }
}

#[test]
fn broad_query_hit_storage_is_bounded_by_visible_results() {
    let files: Vec<_> = (0..MAX_INDEXED_FILES)
        .map(|n| format!("src/module_{n:05}/main.rs"))
        .collect();
    for query in ["", "main"] {
        let hits = ranked_hits(&files, query);
        assert_eq!(hits.len(), MAX_RESULTS);
        assert!(hits.capacity() <= MAX_RESULTS);
        assert!(search(&files, query).capacity() <= MAX_RESULTS);
    }
}

#[test]
fn bounded_hit_selection_matches_full_ranking_across_orders_and_cutoff_ties() {
    let mut files: Vec<_> = (0..256)
        .map(|n| format!("目录/module_{n:03}/Main.rs"))
        .collect();
    files.extend(["root.rs", "same", "same", "İ/İ.rs", "Σ/ΟΣ.rs", "a/x.rs"].map(String::from));
    for order in 0..4 {
        match order {
            1 => files.reverse(),
            2 => files.rotate_left(137),
            3 => files.sort(),
            _ => {}
        }
        for query in [
            "", "MAIN", "module_1", "目录", "i\u{307}", "ΟΣ", "same", "zzzz",
        ] {
            let mut expected: Vec<_> = files
                .iter()
                .filter_map(|path| vector_score(query, path).map(|score| (score, path)))
                .collect();
            expected.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(b.1)));
            let expected: Vec<_> = expected
                .into_iter()
                .take(MAX_RESULTS)
                .map(|(_, path)| path)
                .collect();
            assert_eq!(
                search(&files, query),
                expected,
                "order {order}, query {query:?}"
            );
            assert!(search(&files, query).capacity() <= MAX_RESULTS);
        }
    }
    for count in 0..=MAX_RESULTS + 1 {
        assert_eq!(search(&files[..count], "").len(), count.min(MAX_RESULTS));
    }
}

#[test]
fn file_name_matches_rank_first() {
    let files: Vec<String> = [
        "docs/stats_notes.md",
        "src/stats.py",
        "tests/test_stats.py",
        "README.md",
    ]
    .map(String::from)
    .to_vec();
    assert_eq!(
        search(&files, "stats.py")
            .into_iter()
            .cloned()
            .collect::<Vec<_>>(),
        vec![
            "src/stats.py".to_string(),
            "tests/test_stats.py".to_string()
        ]
    );
    assert_eq!(search(&files, "zzz"), Vec::<&String>::new());
}

#[test]
fn unicode_directories_keep_the_filename_match_bonus() {
    for directory in ["目录", "界", "é", "İ", "🙂", "文档/目录"] {
        let path = format!("{directory}/X.rs");
        assert_eq!(score("x", &path), Some(800 - path.len() as i64));
    }
    let files = vec!["x/note.rs".to_string(), "目录/x.rs".to_string()];
    assert_eq!(
        search(&files, "X")
            .into_iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec!["目录/x.rs", "x/note.rs"]
    );
}

#[test]
fn limited_search_matches_full_ranking() {
    let files: Vec<String> = (0..MAX_INDEXED_FILES)
        .rev()
        .map(|n| format!("src/module_{n:05}/main.rs"))
        .collect();
    for query in ["", "main", "module_1", "zzz"] {
        let mut expected: Vec<_> = files
            .iter()
            .filter_map(|path| score(query, path).map(|score| (score, path)))
            .collect();
        expected.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(b.1)));
        let expected: Vec<_> = expected
            .into_iter()
            .take(MAX_RESULTS)
            .map(|(_, path)| path)
            .collect();
        assert_eq!(search(&files, query), expected);
    }
}

#[test]
fn normalized_search_preserves_unicode_scores_and_complete_top_eight_order() {
    let mut files: Vec<_> = (0..50)
        .rev()
        .map(|n| format!("目录/Module_{n:02}/MAIN.rs"))
        .collect();
    files.extend(
        [
            "İ/İ.rs",
            "src/Éclair.rs",
            "src/e\u{301}clair.rs",
            "Σ/ΟΣ.rs",
            "src/🙂.rs",
            "README.md",
        ]
        .map(String::from),
    );
    let original = files.clone();
    for query in [
        "", "MAIN", "MODULE_1", "目录", "İ", "i\u{307}", "É", "ΟΣ", "🙂", "zzzz",
    ] {
        let mut expected: Vec<_> = files
            .iter()
            .filter_map(|path| score(query, path).map(|score| (score, path)))
            .collect();
        expected.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(b.1)));
        let expected: Vec<_> = expected
            .into_iter()
            .take(MAX_RESULTS)
            .map(|(_, path)| path)
            .collect();
        assert_eq!(search(&files, query), expected, "query {query:?}");
        assert!(search(&[], query).is_empty());
    }
    assert_eq!(files, original);
}

#[test]
fn active_query_needs_a_word_start() {
    assert_eq!(active_query("look at @src/ma"), Some("src/ma"));
    assert_eq!(active_query("@"), Some(""));
    assert_eq!(active_query("mail me@home"), None);
    assert_eq!(active_query("@src/main.rs done"), None);
}

#[test]
fn attachments_are_capped_and_labelled() {
    let dir = std::env::temp_dir().join(format!("flint-mention-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let long: String = (0..MAX_ATTACH_LINES + 10)
        .map(|n| format!("line {n}\n"))
        .collect();
    std::fs::write(dir.join("big.txt"), &long).unwrap();
    std::fs::write(dir.join("small.txt"), "hi\n").unwrap();
    let out = attach("read these", &dir, &["small.txt".into(), "big.txt".into()]);
    assert!(out.starts_with("read these\n\n[Attached file small.txt (1 lines)]\n```\nhi\n```"));
    assert!(out.contains(&format!(
        "[Attached file big.txt ({} lines, truncated to the first {MAX_ATTACH_LINES} of {} lines)]",
        MAX_ATTACH_LINES + 10,
        MAX_ATTACH_LINES + 10
    )));
    assert!(!out.contains(&format!("line {}", MAX_ATTACH_LINES + 1)));
    std::fs::remove_dir_all(dir).ok();
}

#[cfg(unix)]
#[test]
fn workspace_attachment_refuses_outside_symlinks_and_special_files() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    std::fs::write(
        root.path().join("outside.txt"),
        "outside fixture must not be attached",
    )
    .unwrap();
    symlink(
        root.path().join("outside.txt"),
        workspace.join("selected.txt"),
    )
    .unwrap();
    let out = attach("read selected", &workspace, &["selected.txt".into()]);
    assert!(!out.contains("outside fixture must not be attached"));
    assert!(out.contains("could not be read as text"));
    let out = attach("read selected", &workspace, &["../outside.txt".into()]);
    assert!(!out.contains("outside fixture must not be attached"));
    assert!(out.contains("could not be read as text"));
    let out = attach("read selected", &workspace, &[".".into()]);
    assert!(out.contains("could not be read as text"));
}

#[test]
fn huge_attachment_keeps_a_bounded_unicode_prefix() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("large.txt");
    let file = std::fs::File::create(&path).unwrap();
    file.set_len(512 * 1024 * 1024).unwrap();
    // A sparse large file must not require reading its complete content.
    use std::io::{Seek, SeekFrom, Write};
    let mut file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
    file.seek(SeekFrom::Start(0)).unwrap();
    file.write_all("界🙂".repeat(4_000).as_bytes()).unwrap();
    let out = attach("read", root.path(), &["large.txt".into()]);
    assert!(out.contains("truncated"));
    assert!(out.contains("界🙂"));
    assert!(!out.contains('\u{fffd}'));
    assert!(out.len() < MAX_ATTACH_BYTES + 300);
}
