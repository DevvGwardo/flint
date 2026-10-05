use std::path::Path;

use super::{LinkTarget, MAX_BYTES, link_target, read_file};

#[test]
fn local_links_resolve_against_the_document_or_session_directory() {
    let base = Path::new("/tmp/work/docs");
    for (link, path) in [
        ("README.md", "/tmp/work/docs/README.md"),
        ("../src/main.rs#L12", "/tmp/work/src/main.rs"),
        ("../src/main.rs:12:4", "/tmp/work/src/main.rs"),
        ("README.md:12", "/tmp/work/docs/README.md"),
        ("guide%20one.md", "/tmp/work/docs/guide one.md"),
        ("/tmp/a%23b.md", "/tmp/a#b.md"),
        ("file:///tmp/guide%20one.md#intro", "/tmp/guide one.md"),
        ("file://localhost/tmp/guide.md", "/tmp/guide.md"),
    ] {
        assert_eq!(
            link_target(link, base),
            LinkTarget::File(path.into()),
            "{link}"
        );
    }
}

#[test]
fn only_web_links_are_sent_to_the_system_handler() {
    let base = Path::new("/tmp/work");
    for link in [
        "https://example.com/docs",
        "http://example.com:8080",
        "mailto:help@example.com",
    ] {
        assert_eq!(link_target(link, base), LinkTarget::Web(link.into()));
    }
    for link in [
        "",
        "#heading",
        "javascript:alert(1)",
        "vscode://file/tmp/a",
        "file://remote/tmp/a",
        "//remote/tmp/a",
    ] {
        assert_eq!(link_target(link, base), LinkTarget::Ignore, "{link}");
    }
}

#[test]
fn markdown_is_rendered_and_source_files_stay_literal() {
    let dir = tempfile::tempdir().unwrap();
    let markdown = dir.path().join("guide.MD");
    std::fs::write(&markdown, "# Guide\n\n[Source](main.rs)").unwrap();
    assert_eq!(
        read_file(&markdown).unwrap().as_ref(),
        "# Guide\n\n[Source](main.rs)"
    );
    let source = dir.path().join("main.rs");
    std::fs::write(&source, "let x = \"```\";\n<a> *literal*").unwrap();
    assert_eq!(
        read_file(&source).unwrap().as_ref(),
        "````\nlet x = \"```\";\n<a> *literal*\n````"
    );
    std::fs::write(&source, "").unwrap();
    assert_eq!(read_file(&source).unwrap().as_ref(), "```\n\n```");
}

#[test]
fn missing_binary_directory_and_oversized_files_fail_inside_the_preview() {
    let dir = tempfile::tempdir().unwrap();
    assert!(
        read_file(&dir.path().join("missing.md"))
            .unwrap_err()
            .starts_with("Cannot read")
    );
    assert!(read_file(dir.path()).unwrap_err().contains("regular file"));
    let binary = dir.path().join("binary");
    std::fs::write(&binary, [0xff, 0x00]).unwrap();
    assert!(read_file(&binary).unwrap_err().contains("UTF-8"));
    std::fs::write(&binary, b"text\0binary").unwrap();
    assert!(read_file(&binary).unwrap_err().contains("binary"));
    let large = dir.path().join("large.md");
    std::fs::File::create(&large)
        .unwrap()
        .set_len(MAX_BYTES + 1)
        .unwrap();
    assert!(read_file(&large).unwrap_err().contains("too large"));
}
