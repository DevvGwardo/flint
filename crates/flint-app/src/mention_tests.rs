use pretty_assertions::assert_eq;

use super::*;

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
