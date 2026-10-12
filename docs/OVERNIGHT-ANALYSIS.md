# Flint: overnight analysis and blueprint oversight

Analysis date: 2026-10-04. This is an overlay on the current blueprint and
improvement history, not a replacement or a new passing validation checkpoint.

## Starting point

The inspected working tree is substantially ahead of committed `main`.
The initial Git diff spans 99 tracked files, with 13,498 additions and 1,145
deletions, plus existing untracked implementation and regression modules.
These are starting changes, not work produced by this overnight task.

A fresh source inventory counts physical lines, including tests, comments and
blank lines. It excludes generated/ignored output:

| Area | Source files | Physical lines |
| --- | ---: | ---: |
| `crates/flint-agent` | 46 | 13,628 |
| `crates/flint-acp` | 19 | 5,817 |
| `crates/flint-app` | 99 | 37,928 |
| `crates/flint-term` | 8 | 1,600 |
| `tools/blueprint` | 16 | 1,850 |

Counts precede the new overnight tooling. They are code-shape observations,
not a numerical debt score. The 6,818-line UI integration entry point and
large engine/view modules are review-cost leads, not proof that splitting them
would improve runtime.

The 28-pass improvement ledger already provides strong regression and
measurement practice. Many pathological helper costs have been reduced.
Most corresponding native comparisons do **not** establish whole-app gains.
Do not repeat those completed fixes or market their helper ratios as app speed.

## Architecture and evidence-backed investigation lanes

### 1. Event flow and orchestration pressure

Sources:

- `crates/flint-agent/src/lib.rs`: unbounded operation/event channels.
- `crates/flint-acp/src/lib.rs`: unbounded operation/event channels.
- `crates/flint-acp/src/shared.rs`: `emit_all` uses `try_send`.
- `crates/flint-agent/src/subagents.rs`: unbounded child event channel,
  forwarded events, retained `work` records.
- `crates/flint-agent/src/session.rs`: `run_subagents`, at most four
  concurrently polled child runs.
- `crates/flint-app/src/app_engine.rs`: eight-millisecond coalescing and
  at most 256 events folded per pump batch.

**Finding:** a bounded UI batch limits work per pump, not upstream queue
retention. Child forwarding and final work accounting can also retain owned
event payloads. This deserves slow-consumer measurements across concurrent
sessions, not a cosmetic scheduler rewrite.

**Frozen invariants:** no dropped accepted events, ordered transcript/replay,
approval progress, interruption, child identity and resume behavior.

**First useful experiment:** record producer/consumer counts, peak queued work,
per-session update latency and cancellation completion with synthetic
one/four-child traffic and a deliberately slower consumer. Include approval
events in the fixture. Observe memory without conflating a counter with RSS.

**Do not:** replace `unbounded()` with `bounded()` while keeping `try_send`,
or introduce blocking sends into protocol handlers without a reviewed delivery
contract.

### 2. Saved-session persistence pressure

Sources: `crates/flint-app/src/store.rs`, `session.rs`, `app_engine.rs`,
and the saved-stream checks in `tools/blueprint/workloads.py`.

**Finding:** `EventWriter` uses an unbounded command channel, drains adjacent
appends into a pending batch, serializes that batch into another byte vector,
and retains failed records. The existing ordering/retry behavior is valuable
and must not be removed to improve a benchmark.

**First useful experiment:** deterministic slow/failing-writer fixtures and
serialization-batch measurements. Measure batch records/bytes, caller and
flush latency, retained data and replay counts. Include barrier, retry,
archive and shutdown transitions.

**Potential bounded improvement:** limiting serialization batch work without
changing acceptance policy or losing failure retention. It remains a hypothesis
until measured and tested.

### 3. Long-history tool-event lookup

Source: `crates/flint-app/src/view_model.rs`, `SessionView::tool_index`.

**Finding:** lookup walks the transcript backwards to find the latest matching
tool call. Typical updates near the tail can be cheap; interleaved old calls
in long histories are a different workload.

**First useful experiment:** mixed histories with 1/4/32 live tools, repeated
IDs, child updates, and replay. Compare exact folded output with the existing
lookup as an oracle. Record update time and any index's size/maintenance cost.

**Do not:** add a second source of truth without testing rebuild, clone,
duplicate-ID precedence and replay behavior. The public `items` collection
also makes externally constructed view state relevant to invalidation.

### 4. Harness and documentation debt

Sources: `tools/blueprint/bp.py`, `perf.py`, `interact.py`, script reports,
`.github/workflows/ci.yml`, and `docs/ARCHITECTURE.md`.

**Finding:** local evidence is rich but distributed across many evidence roots
and manual ledger entries. A successful test process can include disabled
provider entry points; missing frame evidence cannot be interpreted as zero
render cost. CI currently runs the basic Rust gates, not this complete native
evidence workflow.

The architecture document's description of a single atomic session JSON file
does not match the inspected app store: session directories contain
`meta.json`, `events.jsonl`, and the engine's `history.json`. Verify other
architecture claims against source before updating them.

**First useful improvement:** a deterministic oversight ledger tying each
proposal to its starting source, actual commands, review, raw reports and
follow-up questions. Keep baseline failure and skip states visible.

### 5. Large task projection and idle picker CPU

Source: the final candidates section of `docs/IMPROVEMENT-LOOP.md`.

The first-eight-task iterator lead was already disproven. Measure the
over-eight tail and realistic invalidation before adding retained state.

Picker idle CPU has both failing historical samples and later passing samples.
That is an unresolved causal investigation, not proof of a current regression
or a resolved fix. Do not alter its strict budget.

Native timing on the current active-user-app host is not clean-host evidence.
The overnight default therefore emphasizes offline engine/model/harness
fixtures and records native checks as skipped.

## Oversight cycle

1. Capture the current dirty source and freeze the run's contract and plan.
2. Independently validate the snapshot before any experimental change.
3. Select one scoped, evidence-anchored hypothesis.
4. Require builder pushback, focused regressions and raw measurements.
5. Check actual changed paths and immutable boundaries.
6. Run independent offline gates and independent review.
7. Retain either a rejected experiment or a `proposed-validated` candidate.
8. Derive follow-up questions from the actual delta and measured limits.
9. Reserve final integration validation and produce the morning review.
10. Human reviews promotion. Nothing automatically changes the main checkout.

## Morning review questions

- What changed relative to tonight's dirty starting tree, not just Git HEAD?
- Which cost or failure was reproduced before fixing it?
- Do source and binary hashes match the artifacts that were actually measured?
- Are measurements helper-specific, end-to-end, paired or after-only?
- Did queues, replay, approvals, retries and shutdown preserve their invariants?
- Did the change add state, allocations, cache invalidation or new maintenance?
- Are multiple proposals independently valid and cumulatively compatible?
- Which new risks or opportunities did each accepted experiment reveal?
- Which checks failed, were skipped, or are still waiting for human review?
- How much time went to baseline compilation, useful experiments, validation,
  review and scheduling? Wall time is not automatically active engineering time.

## Environment blocker discovered during setup

The installed OpenCode CLI rejects the specified independent reviewer model
`opencode/deepseek-v4-flash-free` with `Model unavailable`. Its local model
catalog also returned no models during preflight.

This does not invalidate offline baseline work or the tooling tests. It does
block unattended proposal acceptance through that reviewer. Do not silently
switch to a paid model or pretend a review occurred. Any run must retain the
blocker and require an explicitly configured, available independent reviewer
before reporting `proposed-validated`.
