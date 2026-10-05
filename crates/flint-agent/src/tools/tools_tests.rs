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

/// An unsandboxed agent context in `dir`.
fn agent(dir: &Path) -> ToolContext {
    ToolContext::new(dir.to_path_buf(), false)
}

async fn run(dir: &Path, name: &str, value: Value) -> ToolOutcome {
    run_as(&agent(dir), name, value).await
}

/// Runs a call as the agent `ctx`.
async fn run_as(ctx: &ToolContext, name: &str, value: Value) -> ToolOutcome {
    execute(ctx, name, &args(value), &|_| {}, &CancellationToken::new()).await
}

fn workspace() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().canonicalize().expect("canonical");
    (dir, path)
}

#[tokio::test]
async fn mutation_expansion_refuses_oversized_result_without_changing_file() {
    let (_dir, path) = workspace();
    let original = "a".repeat(8192);
    std::fs::write(path.join("expand"), &original).expect("file");
    let outcome = run(
        &path,
        EDIT_FILE,
        json!({"path":"expand","old_string":"a","new_string":"b".repeat(1025),"replace_all":true}),
    )
    .await;
    assert!(!outcome.success, "oversized replacement must be refused");
    assert_eq!(
        std::fs::read_to_string(path.join("expand")).unwrap(),
        original
    );
}

#[tokio::test]
async fn mutation_refuses_oversized_edit_without_changing_file() {
    let (_dir, path) = workspace();
    let original = format!("{}\nneedle\n", "x".repeat(8 * 1024 * 1024));
    std::fs::write(path.join("big"), &original).expect("big");
    let outcome = run(
        &path,
        EDIT_FILE,
        json!({"path":"big","old_string":"needle","new_string":"changed"}),
    )
    .await;
    assert!(!outcome.success, "{}", outcome.output);
    assert_eq!(
        std::fs::read_to_string(path.join("big")).expect("unchanged"),
        original
    );
}

#[tokio::test]
async fn mutation_refuses_oversized_overwrite_even_when_tracker_matches() {
    let (_dir, path) = workspace();
    let original = vec![b'x'; 8 * 1024 * 1024 + 1];
    std::fs::write(path.join("big"), &original).expect("big");
    let ctx = agent(&path);
    ctx.seen.saw(&path.join("big"), &original);
    let outcome = run_as(&ctx, WRITE_FILE, json!({"path":"big","content":"changed"})).await;
    assert!(!outcome.success, "{}", outcome.output);
    assert_eq!(
        std::fs::read(path.join("big")).expect("unchanged"),
        original
    );
}

#[cfg(unix)]
#[tokio::test]
async fn mutation_refuses_write_only_file_without_treating_it_as_new() {
    use std::os::unix::fs::PermissionsExt;
    let (_dir, path) = workspace();
    let full = path.join("write-only");
    std::fs::write(&full, "keep").expect("file");
    std::fs::set_permissions(&full, std::fs::Permissions::from_mode(0o200)).expect("write only");
    let read = std::fs::read(&full);
    if read.is_ok() {
        std::fs::set_permissions(&full, std::fs::Permissions::from_mode(0o600)).expect("restore");
        eprintln!("permission regression skipped: process bypasses Unix read permissions");
        return;
    }
    assert_eq!(
        read.expect_err("unreadable").kind(),
        std::io::ErrorKind::PermissionDenied
    );
    let outcome = run(
        &path,
        WRITE_FILE,
        json!({"path":"write-only","content":"changed"}),
    )
    .await;
    std::fs::set_permissions(&full, std::fs::Permissions::from_mode(0o600)).expect("restore");
    assert!(!outcome.success, "{}", outcome.output);
    assert_eq!(std::fs::read_to_string(&full).expect("unchanged"), "keep");
}

#[cfg(unix)]
async fn mutation_fifo_rejected(name: &str, value: Value) {
    use std::os::unix::fs::FileTypeExt;
    let (_dir, path) = workspace();
    assert!(
        std::process::Command::new("mkfifo")
            .arg(path.join("pipe"))
            .status()
            .expect("mkfifo")
            .success()
    );
    let result =
        tokio::time::timeout(std::time::Duration::from_secs(2), run(&path, name, value)).await;
    if result.is_err() {
        // Release the pre-fix blocking read so the red test runtime can exit.
        let writer = std::fs::OpenOptions::new()
            .write(true)
            .open(path.join("pipe"))
            .expect("release FIFO reader");
        drop(writer);
    }
    let outcome = result.expect("mutation must not wait for a FIFO writer");
    assert!(!outcome.success, "{}", outcome.output);
    assert!(
        std::fs::symlink_metadata(path.join("pipe"))
            .expect("pipe")
            .file_type()
            .is_fifo()
    );
}

#[cfg(unix)]
#[tokio::test]
async fn mutation_write_refuses_fifo_without_waiting() {
    mutation_fifo_rejected(WRITE_FILE, json!({"path":"pipe","content":"changed"})).await;
}

#[cfg(unix)]
#[tokio::test]
async fn mutation_edit_refuses_fifo_without_waiting() {
    mutation_fifo_rejected(
        EDIT_FILE,
        json!({"path":"pipe","old_string":"old","new_string":"changed"}),
    )
    .await;
}

#[tokio::test]
async fn mutation_pre_cancelled_does_not_create_or_change_files() {
    let (_dir, path) = workspace();
    let ctx = agent(&path);
    std::fs::write(path.join("existing"), "old").expect("file");
    ctx.seen.saw(&path.join("existing"), b"old");
    let cancel = CancellationToken::new();
    cancel.cancel();
    for (name, value) in [
        (WRITE_FILE, json!({"path":"new","content":"new"})),
        (WRITE_FILE, json!({"path":"existing","content":"changed"})),
        (
            EDIT_FILE,
            json!({"path":"existing","old_string":"old","new_string":"changed"}),
        ),
    ] {
        let outcome = execute(&ctx, name, &args(value), &|_| {}, &cancel).await;
        assert!(!outcome.success, "{}", outcome.output);
    }
    assert!(!path.join("new").exists());
    assert_eq!(
        std::fs::read_to_string(path.join("existing")).expect("unchanged"),
        "old"
    );
}

#[tokio::test]
async fn mutation_cancellation_interrupts_pending_write_lock() {
    let (_dir, path) = workspace();
    let ctx = agent(&path);
    let lock = tracker::write_lock(&path.join("new"));
    let held = lock.lock().await;
    let cancel = CancellationToken::new();
    let trigger = cancel.clone();
    let canceller = tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        trigger.cancel();
    });
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        execute(
            &ctx,
            WRITE_FILE,
            &args(json!({"path":"new","content":"new"})),
            &|_| {},
            &cancel,
        ),
    )
    .await;
    drop(held);
    canceller.await.expect("canceller");
    assert!(!result.expect("cancellable lock wait").success);
    assert!(!path.join("new").exists());
}

#[tokio::test]
async fn mutation_normal_creation_read_edit_and_undo() {
    let (_dir, path) = workspace();
    let ctx = agent(&path);
    assert!(
        run_as(
            &ctx,
            WRITE_FILE,
            json!({"path":"nested/file","content":"old"})
        )
        .await
        .success
    );
    assert!(
        run_as(&ctx, READ_FILE, json!({"path":"nested/file"}))
            .await
            .success
    );
    ctx.seen.journal().lock().expect("journal").begin_turn(1);
    assert!(
        run_as(
            &ctx,
            EDIT_FILE,
            json!({"path":"nested/file","old_string":"old","new_string":"new"})
        )
        .await
        .success
    );
    let report = ctx
        .seen
        .journal()
        .lock()
        .expect("journal")
        .undo_last(&ctx.seen)
        .expect("undo");
    assert_eq!(report.diffs.len(), 1);
    assert!(report.skipped.is_empty());
    assert_eq!(
        std::fs::read_to_string(path.join("nested/file")).expect("restored"),
        "old"
    );
    assert!(
        !run_as(&ctx, WRITE_FILE, json!({"path":"nested","content":"bad"}))
            .await
            .success
    );
    assert!(
        !run_as(
            &ctx,
            EDIT_FILE,
            json!({"path":"nested","old_string":"old","new_string":"new"})
        )
        .await
        .success
    );
}

#[test]
fn extreme_read_summary_does_not_overflow() {
    assert_eq!(
        summary(
            READ_FILE,
            &args(json!({"path":"a", "offset":u64::MAX, "limit":u64::MAX}))
        ),
        format!("a:{}-{}", u64::MAX, u64::MAX)
    );
}

#[tokio::test]
async fn read_rejects_nonregular_and_oversized_files_and_obeys_cancellation() {
    let (_dir, path) = workspace();
    let directory = run(&path, READ_FILE, json!({"path":"."})).await;
    assert!(!directory.success);
    // Text, not sparse zeroes: the old reader must not accidentally reject it
    // as binary after already downloading the entire oversized input.
    std::fs::write(path.join("big"), vec![b'x'; 8 * 1024 * 1024 + 1]).expect("big text");
    assert!(!run(&path, READ_FILE, json!({"path":"big"})).await.success);
    std::fs::write(path.join("small"), "text").expect("write");
    let ctx = agent(&path);
    let cancel = CancellationToken::new();
    cancel.cancel();
    let outcome = execute(
        &ctx,
        READ_FILE,
        &args(json!({"path":"small"})),
        &|_| {},
        &cancel,
    )
    .await;
    assert!(!outcome.success);
}

#[cfg(unix)]
#[tokio::test]
async fn read_rejects_fifo_without_waiting_for_writer() {
    let (_dir, path) = workspace();
    assert!(
        std::process::Command::new("mkfifo")
            .arg(path.join("pipe"))
            .status()
            .expect("mkfifo")
            .success()
    );
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        run(&path, READ_FILE, json!({"path":"pipe"})),
    )
    .await;
    if result.is_err() {
        // The pre-fix tokio::fs::read leaves a blocking open behind when its
        // future is cancelled. Release that open so a red run can terminate.
        let writer = std::fs::OpenOptions::new()
            .write(true)
            .open(path.join("pipe"))
            .expect("release blocked FIFO reader");
        drop(writer);
    }
    let outcome = result.expect("no FIFO blocking");
    assert!(!outcome.success);
}

#[tokio::test]
async fn native_undo_restores_edits_and_deletes_created_files() {
    let (_dir, path) = workspace();
    let ctx = agent(&path);
    std::fs::write(path.join("existing"), "before").expect("write");
    ctx.seen.journal().lock().expect("journal").begin_turn(1);
    assert!(
        run_as(&ctx, READ_FILE, json!({"path":"existing"}))
            .await
            .success
    );
    assert!(
        run_as(
            &ctx,
            WRITE_FILE,
            json!({"path":"existing","content":"after"})
        )
        .await
        .success
    );
    assert!(
        run_as(&ctx, WRITE_FILE, json!({"path":"created","content":"new"}))
            .await
            .success
    );
    let report = ctx
        .seen
        .journal()
        .lock()
        .expect("journal")
        .undo_last(&ctx.seen)
        .expect("undo");
    assert_eq!(report.diffs.len(), 2);
    assert!(report.skipped.is_empty(), "{:?}", report.skipped);
    assert_eq!(
        std::fs::read_to_string(path.join("existing")).expect("restored"),
        "before"
    );
    assert!(!path.join("created").exists());
}

#[cfg(unix)]
#[tokio::test]
async fn symlink_aliases_share_read_tracking_and_write_locks() {
    let (_dir, path) = workspace();
    std::fs::write(path.join("real"), "old").expect("write");
    std::os::unix::fs::symlink("real", path.join("alias")).expect("symlink");
    let ctx = agent(&path);
    assert!(
        run_as(&ctx, READ_FILE, json!({"path":"alias"}))
            .await
            .success
    );
    assert!(
        run_as(&ctx, WRITE_FILE, json!({"path":"real", "content":"new"}))
            .await
            .success
    );
    assert!(std::sync::Arc::ptr_eq(
        &tracker::write_lock(&path.join("real")),
        &tracker::write_lock(&path.join("alias"))
    ));
}

#[cfg(unix)]
#[tokio::test]
async fn undo_refuses_replaced_symlink_even_with_matching_content() {
    let (_dir, path) = workspace();
    let outside = tempfile::tempdir().expect("outside");
    let ctx = agent(&path);
    std::fs::write(path.join("a"), "before").expect("write");
    ctx.seen.journal().lock().expect("journal").begin_turn(1);
    assert!(run_as(&ctx, READ_FILE, json!({"path":"a"})).await.success);
    assert!(
        run_as(&ctx, WRITE_FILE, json!({"path":"a","content":"after"}))
            .await
            .success
    );
    std::fs::write(outside.path().join("victim"), "after").expect("victim");
    std::fs::remove_file(path.join("a")).expect("remove");
    std::os::unix::fs::symlink(outside.path().join("victim"), path.join("a")).expect("symlink");
    let report = ctx
        .seen
        .journal()
        .lock()
        .expect("journal")
        .undo_last(&ctx.seen)
        .expect("undo");
    assert!(report.diffs.is_empty());
    assert_eq!(report.skipped.len(), 1);
    assert_eq!(
        std::fs::read_to_string(outside.path().join("victim")).expect("victim"),
        "after"
    );
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

fn reference_head_tail(text: &str, max: usize) -> String {
    let total = text.chars().count();
    if total <= max {
        return text.to_string();
    }
    let head_len = max * 2 / 5;
    let tail_len = max - head_len;
    let head: String = text.chars().take(head_len).collect();
    let tail: String = text.chars().skip(total - tail_len).collect();
    let omitted = total - head_len - tail_len;
    format!("{head}\n… [{omitted} characters omitted] …\n{tail}")
}

#[test]
fn head_tail_matches_frozen_reference_at_unicode_and_budget_boundaries() {
    let mut cases = 0;
    for fragment in [
        "a",
        "界",
        "🙂",
        "e\u{301}",
        "\0\t\r\n",
        "a界🙂 e\u{301}\r\n",
    ] {
        for repeats in [0, 1, 2, 5, 47, 48, 49, 129, 256] {
            for suffix in ["", "\n", "\r\n", " \t\u{2003}"] {
                let text = format!("{}{suffix}", fragment.repeat(repeats));
                for max in [
                    0,
                    1,
                    2,
                    3,
                    4,
                    5,
                    7,
                    15,
                    16,
                    17,
                    31,
                    48,
                    49,
                    63,
                    64,
                    65,
                    127,
                    128,
                    129,
                    159,
                    160,
                    161,
                    8000,
                    usize::MAX,
                ] {
                    assert_eq!(head_tail(&text, max), reference_head_tail(&text, max));
                    cases += 1;
                }
            }
        }
    }
    assert_eq!(cases, 5184);
}

#[test]
fn head_tail_large_output_keeps_exact_ends_and_an_owned_result() {
    for fragment in ["x", "界🙂", "e\u{301}\r\n"] {
        let (actual, expected) = {
            let text = format!("HEAD{}TAIL\n\t", fragment.repeat(500_000));
            (head_tail(&text, 8000), reference_head_tail(&text, 8000))
        };
        assert_eq!(actual, expected);
        assert!(actual.starts_with("HEAD"));
        assert!(actual.ends_with("TAIL\n\t"));
    }
}

#[test]
#[ignore = "manual release-mode performance probe"]
fn output_head_tail_perf_probe() {
    for (case, text, max) in [
        ("short", "ok\n".to_string(), 8000),
        ("at_limit", "x".repeat(8000), 8000),
        ("capture_ascii", "x".repeat(32_000), 8000),
        ("capture_unicode", "界🙂".repeat(16_000), 8000),
        ("large_ascii", "x".repeat(8 * 1024 * 1024), 8000),
        ("large_unicode", "界🙂".repeat(8 * 1024 * 1024 / 7), 8000),
        ("zero_budget", "界🙂".repeat(500_000), 0),
    ] {
        let start = std::time::Instant::now();
        for _ in 0..100 {
            std::hint::black_box(head_tail(std::hint::black_box(&text), max));
        }
        eprintln!(
            "output_head_tail_{case}_100_runs_ms={:.3}",
            start.elapsed().as_secs_f64() * 1000.
        );
    }
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
async fn edits_refuse_a_file_changed_since_it_was_read() {
    let (_guard, ws) = workspace();
    std::fs::write(ws.join("a.rs"), "let x = 1;\nlet y = 1;\n").expect("write");
    let (parent, sibling) = (agent(&ws), agent(&ws));
    run_as(&parent, READ_FILE, json!({"path": "a.rs"})).await;
    run_as(&sibling, READ_FILE, json!({"path": "a.rs"})).await;
    let theirs = run_as(
        &sibling,
        EDIT_FILE,
        json!({"path": "a.rs", "old_string": "x = 1", "new_string": "x = 2"}),
    )
    .await;
    assert!(theirs.success, "{}", theirs.output);

    // The parent's view is stale: its edit would apply, but it is refused.
    let stale = run_as(
        &parent,
        EDIT_FILE,
        json!({"path": "a.rs", "old_string": "y = 1", "new_string": "y = 2"}),
    )
    .await;
    assert!(!stale.success);
    assert!(
        stale.output.contains("changed since you last read it"),
        "{}",
        stale.output
    );
    let overwrite = run_as(
        &parent,
        WRITE_FILE,
        json!({"path": "a.rs", "content": "lost\n"}),
    )
    .await;
    assert!(!overwrite.success);

    run_as(&parent, READ_FILE, json!({"path": "a.rs"})).await;
    let fresh = run_as(
        &parent,
        EDIT_FILE,
        json!({"path": "a.rs", "old_string": "y = 1", "new_string": "y = 2"}),
    )
    .await;
    assert!(fresh.success, "{}", fresh.output);
    assert_eq!(
        std::fs::read_to_string(ws.join("a.rs")).expect("read"),
        "let x = 2;\nlet y = 2;\n"
    );
    // Its own edit keeps its view current.
    let again = run_as(
        &parent,
        EDIT_FILE,
        json!({"path": "a.rs", "old_string": "y = 2", "new_string": "y = 3"}),
    )
    .await;
    assert!(again.success, "{}", again.output);
}

#[tokio::test]
async fn write_file_needs_a_read_before_replacing_a_file() {
    let (_guard, ws) = workspace();
    std::fs::write(ws.join("keep.txt"), "important\n").expect("write");
    let seen = agent(&ws);
    let blind = run_as(
        &seen,
        WRITE_FILE,
        json!({"path": "keep.txt", "content": "x"}),
    )
    .await;
    assert!(!blind.success);
    assert!(blind.output.contains("haven't read it"), "{}", blind.output);
    run_as(&seen, READ_FILE, json!({"path": "keep.txt"})).await;
    let replaced = run_as(
        &seen,
        WRITE_FILE,
        json!({"path": "keep.txt", "content": "x"}),
    )
    .await;
    assert!(replaced.success, "{}", replaced.output);
}

#[cfg(unix)]
#[tokio::test]
async fn symlinks_cannot_escape_the_workspace() {
    let (_guard, ws) = workspace();
    let outside = tempfile::tempdir().expect("outside");
    std::os::unix::fs::symlink(outside.path(), ws.join("out")).expect("link");
    std::os::unix::fs::symlink(outside.path().join("new.txt"), ws.join("dangling")).expect("link");
    std::fs::create_dir(ws.join("src")).expect("mkdir");
    std::os::unix::fs::symlink(ws.join("src"), ws.join("inside")).expect("link");

    for path in ["out/evil.txt", "dangling"] {
        let wrote = run(&ws, WRITE_FILE, json!({"path": path, "content": "x"})).await;
        assert!(!wrote.success, "{path}: {}", wrote.output);
        assert!(
            wrote.output.contains("outside the workspace"),
            "{}",
            wrote.output
        );
    }
    assert!(!outside.path().join("evil.txt").exists());
    assert!(!outside.path().join("new.txt").exists());
    let inside = run(
        &ws,
        WRITE_FILE,
        json!({"path": "inside/ok.txt", "content": "x"}),
    )
    .await;
    assert!(inside.success, "{}", inside.output);
}

#[test]
fn plans_parse_render_and_summarize() {
    let value = json!({"plan": [
        {"step": "Read the parser", "status": "completed"},
        {"step": "Fix escaping", "status": "in progress"},
        {"step": "Add tests", "status": "pending"},
    ]});
    let plan = parse_plan(&args(value.clone())).expect("plan");
    assert_eq!(plan[1].status, StepStatus::InProgress);
    assert_eq!(
        render_plan(&plan),
        "Plan updated (1/3 closed):\n[x] Read the parser\n[>] Fix escaping\n[ ] Add tests"
    );
    assert_eq!(summary(UPDATE_PLAN, &args(value)), "Plan: 1/3 done");
    let as_string = json!({"plan": "[{\"step\": \"a\", \"status\": \"done\"}]"});
    assert_eq!(parse_plan(&args(as_string)).expect("string plan").len(), 1);
    assert!(parse_plan(&args(json!({"plan": [{"step": "a", "status": "maybe"}]}))).is_err());
}

#[tokio::test]
async fn run_command_reports_exit_code_and_streams() {
    let (_guard, ws) = workspace();
    let chunks = std::sync::Mutex::new(String::new());
    let on_output = |chunk: String| chunks.lock().expect("lock").push_str(&chunk);
    let outcome = execute(
        &agent(&ws),
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
async fn run_command_timeout_still_applies_after_both_pipes_close() {
    let (_guard, ws) = workspace();
    let outcome = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        run(
            &ws,
            RUN_COMMAND,
            json!({"command": "exec >/dev/null 2>&1; sleep 5", "timeout_secs": 1}),
        ),
    )
    .await
    .expect("closing output pipes must not bypass the command timeout");
    assert_eq!(outcome.output, "[timed out after 1s; process killed]");
    assert!(!outcome.success);
}

#[tokio::test]
async fn run_command_cancellation_still_applies_after_both_pipes_close() {
    let (_guard, ws) = workspace();
    let cancel = CancellationToken::new();
    let trigger = cancel.clone();
    let canceller = tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        trigger.cancel();
    });
    let outcome = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        execute(
            &agent(&ws),
            RUN_COMMAND,
            &args(json!({"command": "exec >/dev/null 2>&1; sleep 5"})),
            &|_| {},
            &cancel,
        ),
    )
    .await
    .expect("closing output pipes must not bypass cancellation");
    canceller.await.expect("canceller");
    assert_eq!(outcome.output, "[interrupted; process killed]");
}

#[cfg(unix)]
#[tokio::test]
async fn run_command_flood_keeps_the_tail_and_pauses_live_output_once() {
    let (_guard, ws) = workspace();
    let chunks = std::sync::Mutex::new(String::new());
    let outcome = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        execute(
            &agent(&ws),
            RUN_COMMAND,
            &args(json!({
                "command": "head -c 1048576 /dev/zero | tr '\\000' x; printf '\\nTAIL\\n'"
            })),
            &|chunk| chunks.lock().expect("lock").push_str(&chunk),
            &CancellationToken::new(),
        ),
    )
    .await
    .expect("bounded output must still drain the process");
    assert!(outcome.success, "{}", outcome.output);
    assert!(outcome.output.ends_with("TAIL\n[exit code: 0]"));
    assert!(outcome.output.contains("bytes omitted"));
    assert!(outcome.output.len() < 8200);
    let streamed = chunks.lock().expect("lock");
    assert_eq!(streamed.matches("[live output paused").count(), 1);
    assert!(streamed.len() < 270_000);
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

#[test]
fn capture_stays_bounded_and_keeps_both_ends() {
    let mut capture = command::Capture::default();
    capture.push(b"START\n");
    for _ in 0..10_000 {
        capture.push(&[b'x'; 1000]);
    }
    capture.push(b"\nerror: the real failure is here");
    let text = capture.model_text(8_000);
    assert!(text.starts_with("START\n"));
    assert!(text.ends_with("error: the real failure is here"));
    assert!(text.contains("bytes omitted"));
    assert!(text.chars().count() < 8_100);

    let mut small = command::Capture::default();
    small.push(b"hello ");
    small.push(b"world\n");
    assert_eq!(small.model_text(8_000), "hello world");
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn sandboxed_commands_only_write_inside_the_workspace() {
    let (_guard, ws) = workspace();
    let ctx = ToolContext::new(ws.clone(), true);
    assert!(ctx.sandbox.is_some(), "sandbox-exec is available on macOS");
    let inside = run_as(
        &ctx,
        RUN_COMMAND,
        json!({"command": "echo hi > inside.txt"}),
    )
    .await;
    assert!(inside.success, "{}", inside.output);
    let home = std::env::var("HOME").expect("home");
    let probe = format!("{home}/.flint-sandbox-probe-{}", std::process::id());
    let outside = run_as(
        &ctx,
        RUN_COMMAND,
        json!({"command": format!("touch '{probe}'")}),
    )
    .await;
    let escaped = Path::new(&probe).exists();
    let _ = std::fs::remove_file(&probe);
    assert!(!escaped, "the sandbox let a command write {probe}");
    assert!(!outside.success);
    assert!(
        outside.output.contains("[flint sandbox:"),
        "{}",
        outside.output
    );
}
