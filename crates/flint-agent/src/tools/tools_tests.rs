use pretty_assertions::assert_eq;
use serde_json::json;

use super::*;

fn args(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Array(_) => {
            panic!("expected object")
        }
    }
}

async fn run(dir: &Path, name: &str, value: Value) -> ToolOutcome {
    execute(dir, name, &args(value), &|_| {}, &CancellationToken::new()).await
}

fn workspace() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().canonicalize().expect("canonical");
    (dir, path)
}

#[test]
fn head_tail_keeps_both_ends() {
    assert_eq!(head_tail("short", 100), "short");
    let text = format!("{}{}", "a".repeat(600), "z".repeat(600));
    let out = head_tail(&text, 100);
    assert_eq!(
        out,
        format!(
            "{}\n… [1100 characters omitted] …\n{}",
            "a".repeat(40),
            "z".repeat(60)
        )
    );
}

#[test]
fn diff_counts_and_headers() {
    let diff = file_diff("a.txt", Some("one\ntwo\n"), "one\n2\nthree\n");
    assert_eq!(
        diff,
        FileDiff {
            path: "a.txt".to_string(),
            unified: "--- a/a.txt\n+++ b/a.txt\n@@ -1,2 +1,3 @@\n one\n-two\n+2\n+three\n"
                .to_string(),
            added: 2,
            removed: 1,
            created: false,
        }
    );
    assert!(file_diff("n.txt", None, "x\n").created);
}

#[test]
fn paths_cannot_leave_the_workspace() {
    let ws = Path::new("/w/proj");
    assert_eq!(
        resolve(ws, "src/../a.rs"),
        Ok(PathBuf::from("/w/proj/a.rs"))
    );
    assert!(resolve(ws, "../other/a.rs").is_err());
    assert!(resolve(ws, "/etc/passwd").is_err());
}

#[tokio::test]
async fn write_edit_and_read_round_trip() {
    let (_guard, ws) = workspace();
    let wrote = run(
        &ws,
        WRITE_FILE,
        json!({"path": "src/a.py", "content": "x = 1\ny = 1\n"}),
    )
    .await;
    assert_eq!(wrote.output, "Created src/a.py (+2 -0)");
    assert!(wrote.diff.as_ref().is_some_and(|d| d.created));

    let ambiguous = run(
        &ws,
        EDIT_FILE,
        json!({"path": "src/a.py", "old_string": "1", "new_string": "2"}),
    )
    .await;
    assert!(!ambiguous.success);
    assert!(ambiguous.output.contains("matches 2 places"));

    let missing = run(
        &ws,
        EDIT_FILE,
        json!({"path": "src/a.py", "old_string": "z = 3", "new_string": "q"}),
    )
    .await;
    assert!(!missing.success);
    assert!(missing.output.contains("was not found"));

    let edited = run(
        &ws,
        EDIT_FILE,
        json!({"path": "src/a.py", "old_string": "y = 1", "new_string": "y = 2"}),
    )
    .await;
    assert_eq!(edited.output, "Edited src/a.py (+1 -1)");

    let read = run(&ws, READ_FILE, json!({"path": "src/a.py"})).await;
    assert_eq!(read.output, "1\tx = 1\n2\ty = 2\n");
    let window = run(
        &ws,
        READ_FILE,
        json!({"path": "src/a.py", "offset": 1, "limit": 1}),
    )
    .await;
    assert_eq!(
        window.output,
        "1\tx = 1\n[showing lines 1-1 of 2; pass offset=2 to read more]"
    );
}

#[tokio::test]
async fn run_command_reports_exit_code_and_streams() {
    let (_guard, ws) = workspace();
    let chunks = std::sync::Mutex::new(String::new());
    let on_output = |chunk: String| chunks.lock().expect("lock").push_str(&chunk);
    let outcome = execute(
        &ws,
        RUN_COMMAND,
        &args(json!({"command": "echo hi; echo oops >&2; exit 3"})),
        &on_output,
        &CancellationToken::new(),
    )
    .await;
    assert_eq!(outcome.exit_code, Some(3));
    assert!(!outcome.success);
    assert!(outcome.output.ends_with("[exit code: 3]"));
    let streamed = chunks.lock().expect("lock").clone();
    assert!(streamed.contains("hi") && streamed.contains("oops"));
}

#[tokio::test]
async fn run_command_times_out_and_returns_after_background_children() {
    let (_guard, ws) = workspace();
    let slow = run(
        &ws,
        RUN_COMMAND,
        json!({"command": "sleep 5", "timeout_secs": 1}),
    )
    .await;
    assert_eq!(slow.output, "[timed out after 1s; process killed]");
    let started = std::time::Instant::now();
    let bg = run(
        &ws,
        RUN_COMMAND,
        json!({"command": "(sleep 5 &) ; echo started"}),
    )
    .await;
    assert!(bg.success, "{}", bg.output);
    assert!(started.elapsed() < std::time::Duration::from_secs(3));
}

#[tokio::test]
async fn list_and_grep() {
    let (_guard, ws) = workspace();
    run(
        &ws,
        WRITE_FILE,
        json!({"path": "src/lib.rs", "content": "fn alpha() {}\nfn beta() {}\n"}),
    )
    .await;
    let listing = run(&ws, LIST_DIR, json!({"depth": 2})).await;
    assert_eq!(listing.output, "src/\nsrc/lib.rs");
    let found = run(&ws, GREP, json!({"pattern": "fn b"})).await;
    assert_eq!(found.output, "src/lib.rs:2:fn beta() {}");
    let none = run(&ws, GREP, json!({"pattern": "gamma"})).await;
    assert_eq!(none.output, "No matches.");
}

#[test]
fn summaries_and_kinds() {
    assert_eq!(
        summary(RUN_COMMAND, &args(json!({"command": "npm test\necho"}))),
        "npm test"
    );
    assert_eq!(
        summary(
            READ_FILE,
            &args(json!({"path": "a.rs", "offset": 10, "limit": 31}))
        ),
        "a.rs:10-40"
    );
    assert_eq!(summary(GREP, &args(json!({"pattern": "foo"}))), "foo in .");
    assert_eq!(tool_kind(EDIT_FILE), ToolKind::Edit);
    assert_eq!(
        edit_path(WRITE_FILE, &args(json!({"path": "b.rs"}))),
        Some("b.rs".to_string())
    );
}
