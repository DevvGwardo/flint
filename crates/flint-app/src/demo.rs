//! A scripted, realistic agent turn for `flint --demo`: reads, thinks, runs a
//! failing test, edits with a diff, gets a verify nudge, reruns tests green,
//! and writes a markdown summary. Lets the UI be built and screenshotted
//! without a live engine.

use std::time::Duration;

use flint_agent::AgentEvent;
use flint_agent::FileDiff;
use flint_agent::NudgeReason;
use flint_agent::ToolKind;
use flint_agent::TurnEndReason;
use flint_agent::Usage;
use serde_json::json;

pub const PROMPT: &str = "The LRU cache evicts the wrong entry once it hits capacity. \
Fix it and make sure the tests pass.";

/// One scripted event and how long to wait before emitting it.
pub struct Beat {
    pub delay: Duration,
    pub event: AgentEvent,
}

const LRU_SOURCE: &str = r#"export class LRUCache<K, V> {
  private map = new Map<K, V>();
  constructor(private capacity: number) {}

  get(key: K): V | undefined {
    return this.map.get(key);
  }

  set(key: K, value: V): void {
    if (this.map.size >= this.capacity) {
      const oldest = this.map.keys().next().value;
      this.map.delete(oldest!);
    }
    this.map.set(key, value);
  }
}"#;

const FAILING_RUN: &[&str] = &[
    "> lru@1.0.0 test\n",
    "> vitest run lru\n\n",
    " RUN  v2.1.8 /Users/dev/lru\n\n",
    " ✓ src/lru.test.ts > stores and reads values\n",
    " ✓ src/lru.test.ts > respects capacity\n",
    " × src/lru.test.ts > evicts the least recently used entry\n",
    "   → expected 'b' to be undefined\n\n",
    " Test Files  1 failed (1)\n",
    "      Tests  1 failed | 2 passed (3)\n",
];

const PASSING_RUN: &[&str] = &[
    "> lru@1.0.0 test\n",
    "> vitest run\n\n",
    " ✓ src/lru.test.ts (3 tests) 4ms\n",
    " ✓ src/cache-stats.test.ts (5 tests) 6ms\n\n",
    " Test Files  2 passed (2)\n",
    "      Tests  8 passed (8)\n",
];

const DIFF: &str = "@@ -4,12 +4,17 @@ export class LRUCache<K, V> {

   get(key: K): V | undefined {
-    return this.map.get(key);
+    if (!this.map.has(key)) return undefined;
+    const value = this.map.get(key)!;
+    // Re-insert so the key becomes the most recently used.
+    this.map.delete(key);
+    this.map.set(key, value);
+    return value;
   }

   set(key: K, value: V): void {
-    if (this.map.size >= this.capacity) {
+    if (this.map.has(key)) this.map.delete(key);
+    else if (this.map.size >= this.capacity) {
       const oldest = this.map.keys().next().value;
       this.map.delete(oldest!);
     }
";

const SUMMARY: &str = "Fixed the eviction bug in `src/lru.ts`.

**Root cause:** `get()` read from the map without refreshing recency, so a key \
that was just read was still the first one evicted. `set()` on an existing key \
also evicted an unrelated entry.

**Changes**
- `get()` re-inserts the key so it becomes the most recently used.
- `set()` refreshes an existing key instead of evicting when the cache is full.

```ts
const value = this.map.get(key)!;
this.map.delete(key);
this.map.set(key, value);
```

**Verification:** `npm test` passes — 8 tests across 2 files.";

const REASONING_1: &str = "The user says eviction picks the wrong entry. A Map-based LRU \
relies on insertion order, so reads have to move the key to the end. Let me read the \
implementation and the tests before changing anything.";

const REASONING_2: &str = "Confirmed: get() never refreshes recency. The failing test reads \
'a', inserts 'c', and expects 'b' (the least recently used) to be evicted. I'll re-insert on \
read, and handle set() on an existing key so it doesn't evict.";

const REASONING_3: &str = "Edit applied. Now run the whole suite to verify, not just the LRU file.";

/// `with_approval` asks before the final test run, to show the approval card.
pub fn script(with_approval: bool) -> Vec<Beat> {
    let mut s = Script::default();
    s.at(80, AgentEvent::TurnStarted { turn_id: 1 });
    s.step(1);
    s.stream_reasoning(REASONING_1);
    s.stream_text("I'll start with the cache implementation and its tests.");
    s.tool(
        "c1",
        "read_file",
        ToolKind::Read,
        json!({"path": "src/lru.ts"}),
        "src/lru.ts",
    );
    s.finish("c1", LRU_SOURCE, None, true, None, 12);
    s.tool(
        "c2",
        "grep",
        ToolKind::Search,
        json!({"pattern": "evict", "path": "src"}),
        "\"evict\" in src",
    );
    s.finish(
        "c2",
        "src/lru.test.ts:18:  it('evicts the least recently used entry', () => {",
        None,
        true,
        None,
        40,
    );
    s.usage(4_812, 0, 196, 88);

    s.step(2);
    s.tool(
        "c3",
        "run_command",
        ToolKind::Command,
        json!({"command": "npm test -- lru"}),
        "npm test -- lru",
    );
    s.stream_output("c3", FAILING_RUN);
    s.finish("c3", &FAILING_RUN.concat(), Some(1), false, None, 2_310);
    s.usage(10_240, 4_608, 420, 210);

    s.step(3);
    s.stream_reasoning(REASONING_2);
    s.tool(
        "c4",
        "edit_file",
        ToolKind::Edit,
        json!({"path": "src/lru.ts"}),
        "src/lru.ts",
    );
    s.finish(
        "c4",
        "Applied edit to src/lru.ts",
        None,
        true,
        Some(FileDiff {
            path: "src/lru.ts".into(),
            unified: DIFF.into(),
            added: 7,
            removed: 2,
            created: false,
        }),
        18,
    );
    s.usage(16_904, 9_984, 980, 512);

    s.step(4);
    s.stream_text("Fixed `get()` so reads refresh recency.");
    s.at(
        500,
        AgentEvent::HarnessNudge {
            reason: NudgeReason::Verify,
            message: "You modified files during this turn but haven't run any verification \
                      commands. Run the relevant tests/build/lint and continue."
                .into(),
        },
    );

    s.step(5);
    s.stream_reasoning(REASONING_3);
    if with_approval {
        s.at(
            200,
            AgentEvent::ApprovalRequested {
                call_id: "c5".into(),
                kind: ToolKind::Command,
                summary: "npm test".into(),
            },
        );
    }
    s.tool(
        "c5",
        "run_command",
        ToolKind::Command,
        json!({"command": "npm test"}),
        "npm test",
    );
    s.stream_output("c5", PASSING_RUN);
    s.finish("c5", &PASSING_RUN.concat(), Some(0), true, None, 1_870);
    s.usage(23_310, 16_640, 1_312, 640);

    s.step(6);
    s.stream_text(SUMMARY);
    s.usage(24_902, 19_968, 1_688, 702);
    s.at(
        200,
        AgentEvent::TurnFinished {
            turn_id: 1,
            reason: TurnEndReason::Completed,
        },
    );
    s.beats
}

#[derive(Default)]
struct Script {
    beats: Vec<Beat>,
}

impl Script {
    fn at(&mut self, delay_ms: u64, event: AgentEvent) {
        self.beats.push(Beat {
            delay: Duration::from_millis(delay_ms),
            event,
        });
    }

    fn step(&mut self, step: u32) {
        self.at(350, AgentEvent::StepStarted { turn_id: 1, step });
    }

    fn stream_reasoning(&mut self, text: &str) {
        for chunk in chunks(text, 4) {
            self.at(45, AgentEvent::ReasoningDelta(chunk));
        }
    }

    fn stream_text(&mut self, text: &str) {
        for chunk in chunks(text, 3) {
            self.at(28, AgentEvent::TextDelta(chunk));
        }
    }

    fn stream_output(&mut self, call_id: &str, lines: &[&str]) {
        for line in lines {
            self.at(
                160,
                AgentEvent::ToolOutputDelta {
                    call_id: call_id.into(),
                    chunk: (*line).into(),
                },
            );
        }
    }

    fn tool(
        &mut self,
        call_id: &str,
        name: &str,
        kind: ToolKind,
        args: serde_json::Value,
        summary: &str,
    ) {
        self.at(
            220,
            AgentEvent::ToolCallStarted {
                call_id: call_id.into(),
                name: name.into(),
                kind,
                args,
                summary: summary.into(),
            },
        );
    }

    fn finish(
        &mut self,
        call_id: &str,
        output: &str,
        exit_code: Option<i32>,
        success: bool,
        diff: Option<FileDiff>,
        duration_ms: u64,
    ) {
        self.at(
            260,
            AgentEvent::ToolCallFinished {
                call_id: call_id.into(),
                output: output.into(),
                exit_code,
                success,
                diff,
                duration_ms,
            },
        );
    }

    fn usage(&mut self, input: u64, cached: u64, output: u64, reasoning: u64) {
        self.at(
            0,
            AgentEvent::Usage(Usage {
                input_tokens: input,
                cached_input_tokens: cached,
                output_tokens: output,
                reasoning_tokens: reasoning,
            }),
        );
    }
}

/// Splits text into chunks of `words` words, keeping the whitespace.
fn chunks(text: &str, words: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut count = 0;
    for piece in text.split_inclusive(char::is_whitespace) {
        current.push_str(piece);
        count += 1;
        if count == words {
            out.push(std::mem::take(&mut current));
            count = 0;
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

/// A background session for the demo.
pub struct Extra {
    pub workspace: std::path::PathBuf,
    pub prompt: &'static str,
    pub beats: Vec<Beat>,
    pub instant: bool,
}

/// Two more sessions: one still running in a sibling workspace, one finished
/// (unread) in the main workspace.
pub fn extra_sessions(workspace: &std::path::Path) -> Vec<Extra> {
    let sibling = workspace
        .parent()
        .map(|parent| parent.join("spark"))
        .unwrap_or_else(|| workspace.join("spark"));

    let mut running = Script::default();
    running.at(300, AgentEvent::TurnStarted { turn_id: 1 });
    running.step(1);
    running.stream_reasoning("Find the router first, then add the route and a test.");
    running.tool(
        "b1",
        "read_file",
        ToolKind::Read,
        json!({"path": "server/index.ts"}),
        "server/index.ts",
    );
    running.finish("b1", "import express from 'express';", None, true, None, 9);
    running.step(2);
    running.tool(
        "b2",
        "run_command",
        ToolKind::Command,
        json!({"command": "npm run build -- --watch"}),
        "npm run build -- --watch",
    );
    for n in 0..400 {
        running.at(
            900,
            AgentEvent::ToolOutputDelta {
                call_id: "b2".into(),
                chunk: format!(
                    "[watch] rebuilt in {}ms ({} modules)\n",
                    180 + n % 40,
                    212 + n
                ),
            },
        );
    }

    let mut done = Script::default();
    done.at(0, AgentEvent::TurnStarted { turn_id: 1 });
    done.step(1);
    done.tool(
        "d1",
        "grep",
        ToolKind::Search,
        json!({"pattern": "SessionStore"}),
        "\"SessionStore\" in src",
    );
    done.finish(
        "d1",
        "src/store.ts:4: export class SessionStore {",
        None,
        true,
        None,
        30,
    );
    done.step(2);
    done.stream_text(
        "Sessions live in `SessionStore` (`src/store.ts`), an append-only JSONL log per \
         session that is replayed on startup.",
    );
    done.usage(3_120, 1_024, 140, 60);
    done.at(
        0,
        AgentEvent::TurnFinished {
            turn_id: 1,
            reason: TurnEndReason::Completed,
        },
    );

    vec![
        Extra {
            workspace: sibling,
            prompt: "Add a /health endpoint to the API server",
            beats: running.beats,
            instant: false,
        },
        Extra {
            workspace: workspace.to_path_buf(),
            prompt: "Explain how session storage works",
            beats: done.beats,
            instant: true,
        },
    ]
}
