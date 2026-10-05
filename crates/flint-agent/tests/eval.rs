//! A small, reproducible task suite for measuring the agent against a live
//! model, so claims like "the JEV judge is worth N points" can be checked
//! from this repo. Runs only with FLINT_EVAL=1:
//!
//! ```sh
//! FLINT_EVAL=1 FLINT_API_KEY=… FLINT_MODEL=… \
//!   cargo test -p flint-agent --release --test eval -- --nocapture
//! ```
//!
//! - `FLINT_BASE_URL` / `FLINT_MODEL`: the endpoint and model under test.
//! - `FLINT_EVAL_TRIALS` (default 3): runs per task and variant.
//! - `FLINT_EVAL_TASKS`: comma-separated task names to run (default all).
//! - With `TYPESAFE_API_KEY` set, every task also runs without the judge,
//!   so the report compares `jev` with `no-jev` on the same tasks.
//!
//! Each task starts from fixed files in a fresh directory and is graded by a
//! shell check the agent never sees. The report goes to stderr and to
//! `target/eval-<unix time>.json` (or `FLINT_EVAL_OUT`).

use std::path::Path;
use std::time::Duration;
use std::time::Instant;

use flint_agent::AgentConfig;
use flint_agent::AgentEvent;
use flint_agent::ApprovalMode;
use flint_agent::Op;
use flint_agent::TurnEndReason;
use serde_json::json;

struct Task {
    name: &'static str,
    files: &'static [(&'static str, &'static str)],
    prompt: &'static str,
    /// Passes when this exits 0 in the task directory.
    check: &'static str,
}

const TASKS: &[Task] = &[
    Task {
        name: "implement-function",
        files: &[
            ("calc.py", "def add(a, b):\n    return a + b\n"),
            (
                "test_calc.py",
                "import unittest\nfrom calc import add\n\nclass T(unittest.TestCase):\n    def test_add(self):\n        self.assertEqual(add(2, 3), 5)\n",
            ),
        ],
        prompt: "Add subtract(a, b) and divide(a, b) to calc.py. divide must raise ValueError \
                 when b is 0. Add tests for both to test_calc.py.",
        check: "python3 - <<'EOF'\nfrom calc import subtract, divide\nassert subtract(5, 3) == 2\nassert divide(6, 3) == 2\ntry:\n    divide(1, 0)\n    raise SystemExit(1)\nexcept ValueError:\n    pass\nEOF\npython3 -m unittest -q && grep -q subtract test_calc.py",
    },
    Task {
        name: "fix-off-by-one",
        files: &[
            (
                "pages.py",
                "def page_count(items, per_page):\n    \"\"\"Pages needed to show `items` items, `per_page` per page.\"\"\"\n    return items // per_page\n",
            ),
            (
                "test_pages.py",
                "import unittest\nfrom pages import page_count\n\nclass T(unittest.TestCase):\n    def test_exact(self):\n        self.assertEqual(page_count(20, 10), 2)\n    def test_partial(self):\n        self.assertEqual(page_count(21, 10), 3)\n",
            ),
        ],
        prompt: "The tests in test_pages.py fail. Fix the bug.",
        check: "python3 -m unittest -q && python3 -c 'from pages import page_count as p; assert p(0, 10) == 0 and p(1, 10) == 1'",
    },
    Task {
        name: "rename-keeps-api",
        files: &[(
            "store.py",
            "class Store:\n    def __init__(self, items):\n        self.items = items\n\n    def total(self):\n        return sum(self.items)\n\n\ndef make_store(items):\n    return Store(items)\n",
        )],
        prompt: "Rename the class Store to Inventory. Keep everything else that callers use working.",
        check: "python3 -c 'from store import Inventory, make_store; s = make_store([1, 2]); assert isinstance(s, Inventory) and s.items == [1, 2] and s.total() == 3' && ! grep -q 'class Store' store.py",
    },
    Task {
        name: "csv-standard",
        files: &[("README.md", "# csvtool\n")],
        prompt: "Create csvtool.py with a function to_csv(rows) that turns a list of lists of \
                 strings into CSV text following RFC 4180, with CRLF line endings. Don't use the \
                 csv module.",
        check: "python3 - <<'EOF'\nfrom csvtool import to_csv\nout = to_csv([['a', 'b,c'], ['he said \"hi\"', ''], ['x\\ny', 'z']])\nassert out.startswith('a,\"b,c\"\\r\\n'), repr(out)\nassert '\"he said \"\"hi\"\"\",' in out, repr(out)\nassert '\"x\\ny\",z' in out, repr(out)\nEOF",
    },
    Task {
        name: "question-only",
        files: &[(
            "router.py",
            "ROUTES = {}\n\ndef route(path):\n    def register(fn):\n        ROUTES[path] = fn\n        return fn\n    return register\n",
        )],
        prompt: "How does route() in router.py register a handler? Answer briefly.",
        // A question: nothing may change.
        check: "test \"$(ls | sort | tr '\\n' ' ')\" = 'router.py ' && grep -q 'ROUTES\\[path\\] = fn' router.py",
    },
    Task {
        name: "cli-flag",
        files: &[(
            "greet.py",
            "import argparse\n\n\ndef main(argv=None):\n    parser = argparse.ArgumentParser()\n    parser.add_argument('name')\n    args = parser.parse_args(argv)\n    print(f'Hello, {args.name}!')\n\n\nif __name__ == '__main__':\n    main()\n",
        )],
        prompt: "Add a --json flag to greet.py that prints {\"greeting\": \"Hello, NAME!\"} as JSON \
                 instead of the plain text.",
        check: "plain=$(python3 greet.py Ada)\ntest \"$plain\" = 'Hello, Ada!'\noutput=$(python3 greet.py Ada --json)\nprintf '%s' \"$output\" | python3 -c 'import json,sys; assert json.load(sys.stdin) == {\"greeting\": \"Hello, Ada!\"}'",
    },
];

struct Outcome {
    passed: bool,
    reason: TurnEndReason,
    steps: u32,
    nudges: u32,
    input_tokens: u64,
    output_tokens: u64,
    seconds: f64,
}

fn grade_check(workspace: &Path, check: &str) -> std::io::Result<std::process::Output> {
    std::process::Command::new("sh")
        .arg("-ec")
        .arg(check)
        .current_dir(workspace)
        .output()
}

/// A later passing unittest/grep must not erase a failed hidden assertion.
#[test]
fn grader_rejects_failed_python_assertion_before_passing_checks() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("calc.py"),
        "def add(a,b): return a+b\ndef subtract(a,b): return 0\ndef divide(a,b): return a/b\n",
    )
    .expect("calc");
    std::fs::write(dir.path().join("test_calc.py"),
        "import unittest\nfrom calc import add\n# subtract\nclass T(unittest.TestCase):\n def test_add(self): self.assertEqual(add(2,3),5)\n"
    ).expect("tests");
    let task = TASKS
        .iter()
        .find(|task| task.name == "implement-function")
        .expect("task");
    let output = grade_check(dir.path(), task.check).expect("grader");
    assert!(
        !output.status.success(),
        "failed Python assertion was masked by passing later checks"
    );
}

#[test]
fn grader_rejects_failed_cli_process_after_valid_output() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("greet.py"),
        "import json,sys\nprint(json.dumps({'greeting':'Hello, Ada!'}) if '--json' in sys.argv else 'Hello, Ada!')\nraise SystemExit(1)\n"
    ).expect("cli");
    let task = TASKS
        .iter()
        .find(|task| task.name == "cli-flag")
        .expect("task");
    let output = grade_check(dir.path(), task.check).expect("grader");
    assert!(
        !output.status.success(),
        "failed CLI process was masked by valid stdout"
    );
}

fn run_task(task: &Task, base: &AgentConfig, jev: bool) -> Outcome {
    let dir = tempfile::tempdir().expect("tempdir");
    let workspace = dir.path().canonicalize().expect("canonical");
    for (path, content) in task.files {
        std::fs::write(workspace.join(path), content).expect("task file");
    }
    let mut config = base.clone();
    config.workspace = workspace.clone();
    config.approval = ApprovalMode::Auto;
    if !jev {
        config.jev = None;
    }
    let started = Instant::now();
    let handle = flint_agent::spawn_session(config);
    handle
        .ops
        .send_blocking(Op::UserMessage(task.prompt.to_string()))
        .expect("send");
    let deadline = Instant::now() + Duration::from_secs(600);
    let (mut steps, mut nudges, mut usage) = (0, 0, flint_agent::Usage::default());
    let mut reason = TurnEndReason::Failed("timed out".into());
    while Instant::now() < deadline {
        let Ok(event) = handle.events.recv_blocking() else {
            break;
        };
        match event {
            AgentEvent::StepStarted { step, .. } => steps = step + 1,
            AgentEvent::HarnessNudge { .. } => nudges += 1,
            AgentEvent::Usage(u) => usage = u,
            AgentEvent::TurnFinished { reason: r, .. } => {
                reason = r;
                break;
            }
            _ => {}
        }
    }
    let _ = handle.ops.send_blocking(Op::Shutdown);
    let passed = grade_check(&workspace, task.check).is_ok_and(|out| out.status.success());
    Outcome {
        passed,
        reason,
        steps,
        nudges,
        input_tokens: usage.input_tokens,
        output_tokens: usage.output_tokens,
        seconds: started.elapsed().as_secs_f64(),
    }
}

#[test]
fn eval_suite() {
    if std::env::var("FLINT_EVAL").as_deref() != Ok("1") {
        eprintln!("skipped: set FLINT_EVAL=1 to run the eval suite against a live model");
        return;
    }
    let base = AgentConfig::from_env(std::env::temp_dir()).expect("FLINT_API_KEY must be set");
    let trials: usize = std::env::var("FLINT_EVAL_TRIALS")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(3);
    let only: Option<Vec<String>> = std::env::var("FLINT_EVAL_TASKS")
        .ok()
        .map(|list| list.split(',').map(|s| s.trim().to_string()).collect());
    let variants: Vec<(&str, bool)> = if base.jev.is_some() {
        vec![("jev", true), ("no-jev", false)]
    } else {
        vec![("no-jev", false)]
    };
    let mut results = Vec::new();
    let mut summary = Vec::new();
    for (variant, jev) in &variants {
        let (mut passed, mut total) = (0, 0);
        for task in TASKS
            .iter()
            .filter(|t| only.as_ref().is_none_or(|o| o.iter().any(|n| n == t.name)))
        {
            for trial in 0..trials {
                let outcome = run_task(task, &base, *jev);
                eprintln!(
                    "{variant:7} {:20} #{trial} {} {:?} steps={} nudges={} {:.0}s",
                    task.name,
                    if outcome.passed { "PASS" } else { "FAIL" },
                    outcome.reason,
                    outcome.steps,
                    outcome.nudges,
                    outcome.seconds
                );
                total += 1;
                passed += usize::from(outcome.passed);
                results.push(json!({
                    "variant": variant, "task": task.name, "trial": trial,
                    "passed": outcome.passed, "reason": format!("{:?}", outcome.reason),
                    "steps": outcome.steps, "nudges": outcome.nudges,
                    "input_tokens": outcome.input_tokens, "output_tokens": outcome.output_tokens,
                    "seconds": outcome.seconds,
                }));
            }
        }
        let rate = if total == 0 {
            0.0
        } else {
            passed as f64 / total as f64
        };
        eprintln!(
            "== {variant}: {passed}/{total} passed ({:.0}%)",
            rate * 100.0
        );
        summary.push(json!({"variant": variant, "passed": passed, "total": total, "rate": rate}));
    }
    let report = json!({
        "model": base.model,
        "base_url": base.base_url,
        "trials": trials,
        "summary": summary,
        "results": results,
    });
    let out = std::env::var("FLINT_EVAL_OUT").unwrap_or_else(|_| {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        format!(
            "{}/../../target/eval-{now}.json",
            env!("CARGO_MANIFEST_DIR")
        )
    });
    std::fs::write(
        Path::new(&out),
        serde_json::to_string_pretty(&report).expect("json"),
    )
    .expect("write report");
    eprintln!("report: {out}");
}

/// The graders must not pass work that wasn't done: every check fails on
/// the starting files, except the question, whose files must stay as they
/// are (doing nothing passes it).
#[test]
fn checks_fail_on_the_starting_files() {
    for task in TASKS {
        let dir = tempfile::tempdir().expect("tempdir");
        for (path, content) in task.files {
            std::fs::write(dir.path().join(path), content).expect("task file");
        }
        let passed = grade_check(dir.path(), task.check)
            .expect("sh")
            .status
            .success();
        assert_eq!(passed, task.name == "question-only", "{}", task.name);
    }
}

/// Correct solutions, applied over the starting files: every check passes.
const REFERENCE: &[(&str, &[(&str, &str)])] = &[
    (
        "implement-function",
        &[
            (
                "calc.py",
                "def add(a, b):\n    return a + b\n\ndef subtract(a, b):\n    return a - b\n\ndef divide(a, b):\n    if b == 0:\n        raise ValueError('division by zero')\n    return a / b\n",
            ),
            (
                "test_calc.py",
                "import unittest\nfrom calc import add, subtract, divide\n\nclass T(unittest.TestCase):\n    def test_add(self):\n        self.assertEqual(add(2, 3), 5)\n    def test_subtract(self):\n        self.assertEqual(subtract(5, 3), 2)\n    def test_divide(self):\n        self.assertEqual(divide(6, 3), 2)\n        with self.assertRaises(ValueError):\n            divide(1, 0)\n",
            ),
        ],
    ),
    (
        "fix-off-by-one",
        &[(
            "pages.py",
            "def page_count(items, per_page):\n    return -(-items // per_page)\n",
        )],
    ),
    (
        "rename-keeps-api",
        &[(
            "store.py",
            "class Inventory:\n    def __init__(self, items):\n        self.items = items\n\n    def total(self):\n        return sum(self.items)\n\n\ndef make_store(items):\n    return Inventory(items)\n",
        )],
    ),
    (
        "csv-standard",
        &[(
            "csvtool.py",
            "def field(f):\n    if any(c in f for c in ',\"\\r\\n'):\n        return '\"' + f.replace('\"', '\"\"') + '\"'\n    return f\n\ndef to_csv(rows):\n    return ''.join(','.join(field(f) for f in row) + '\\r\\n' for row in rows)\n",
        )],
    ),
    ("question-only", &[]),
    (
        "cli-flag",
        &[(
            "greet.py",
            "import argparse, json\n\n\ndef main(argv=None):\n    parser = argparse.ArgumentParser()\n    parser.add_argument('name')\n    parser.add_argument('--json', action='store_true')\n    args = parser.parse_args(argv)\n    text = f'Hello, {args.name}!'\n    print(json.dumps({'greeting': text}) if args.json else text)\n\n\nif __name__ == '__main__':\n    main()\n",
        )],
    ),
];

#[test]
fn checks_pass_correct_solutions() {
    assert_eq!(REFERENCE.len(), TASKS.len());
    for task in TASKS {
        let dir = tempfile::tempdir().expect("tempdir");
        let (_, solution) = REFERENCE
            .iter()
            .find(|(name, _)| *name == task.name)
            .expect("reference solution");
        for (path, content) in task.files.iter().chain(solution.iter()) {
            std::fs::write(dir.path().join(path), content).expect("file");
        }
        let out = grade_check(dir.path(), task.check).expect("sh");
        assert!(
            out.status.success(),
            "{}: {}{}",
            task.name,
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }
}
