use super::url_at_column;

#[test]
fn terminal_url_hits_follow_wide_and_combining_prefix_cells() {
    let url = "file:///tmp/guide.md";
    for (prefix, start) in [("界 ", 3), ("🙂 ", 3), ("e\u{301} ", 2), ("界e\u{301} ", 4)] {
        let line = format!("{prefix}{url} suffix");
        assert_eq!(url_at_column(&line, start - 1), None, "{prefix:?}");
        assert_eq!(url_at_column(&line, start), Some(url), "{prefix:?}");
        assert_eq!(
            url_at_column(&line, start + url.len() - 1),
            Some(url),
            "{prefix:?}"
        );
        assert_eq!(url_at_column(&line, start + url.len()), None, "{prefix:?}");
    }
}

#[test]
fn terminal_url_hits_cover_wide_and_zero_width_characters_inside_urls() {
    for (url, cells) in [
        ("https://界.test/🙂", 18),
        ("https://x.test/e\u{301}", 16),
        ("https://x.test/a", 16),
    ] {
        let line = format!("({url}) after");
        for col in 1..=cells {
            assert_eq!(
                url_at_column(&line, col),
                Some(url),
                "url={url:?}, col={col}"
            );
        }
        assert_eq!(url_at_column(&line, 0), None);
        assert_eq!(url_at_column(&line, cells + 1), None);
    }
}

#[test]
fn terminal_url_hits_preserve_ascii_delimiters_schemes_and_multiple_links() {
    let line = "no-link (https://x.test/a) file:///tmp/b] tail";
    for col in 9..25 {
        assert_eq!(url_at_column(line, col), Some("https://x.test/a"));
    }
    for col in 27..40 {
        assert_eq!(url_at_column(line, col), Some("file:///tmp/b"));
    }
    for col in [0, 8, 25, 26, 40, 44, usize::MAX] {
        assert_eq!(url_at_column(line, col), None, "col={col}");
    }
    for line in ["", "plain output", "ftp://x.test/a"] {
        assert_eq!(url_at_column(line, 0), None);
    }
}
