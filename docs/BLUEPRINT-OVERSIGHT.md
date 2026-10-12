# Blueprint improvement oversight

This is a checklist for a single engineering pass, not a scheduler or an
overnight run. Source facts below were checked against the current implementation.
Historical measurements remain in `IMPROVEMENT-LOOP.md` and `UI-AUDIT.md`.

## Current closeout status, 2026-10-05

- The three scoped passes below are complete. This document does not authorize
  another run, an overnight supervisor, or recurring work.
- During the prior documentation closeout, no loops were listed in the
  session/root scheduler, and no local automations were listed.
  The historical oversight loop `020192d1` was not
  found when cancellation was requested. Known validation processes have
  finished; no persistent process matching the project's known supervisor
  paths was found. Unrelated sessions and cron jobs were left untouched.
- The latest completed checkpoint is the persistence/approval lifecycle pass:
  full serial Rust workspace, strict Clippy, formatting, isolated release build
  and 68 Python
  harness tests pass. Its raw results were reread for this closeout; they
  still agree with the counts and timings below.
- Demonstrated improvements bound duplicate event serialization and superseded
  engine-history retention. Queued/failed transcript records remain unbounded,
  write timings are mixed, and no whole-app speedup or power-loss durability
  improvement is claimed.
- Follow-ups below are suggestions for a separately requested bounded pass,
  not scheduled tasks. This pass has finished; no follow-up was scheduled.

## Completed measurement prerequisites

### Interaction test result integrity

Implemented and regression-covered: `tools/blueprint/interact.py` judges the
whole Cargo process separately from individual parsed test lines. A passing
line followed by Cargo exit 101 cannot produce a passing report. Stderr and
partial results are retained. Ignored tests are skipped evidence, not executed
successes.

### Performance evidence integrity

Implemented and regression-covered: missing samples remain unknown, never
become zero CPU, zero dropped frames, or a passing budget. JSON reports use
standard finite JSON values. Non-finite measurements become `null` with a
missing-data reason and the appropriate failed check. Optional live
measurements remain explicitly skipped.

These are correctness fixes to the measurement harness. They do not establish
a runtime speedup in Flint.

## Performance and orchestration leads

| Area | Checked source | Next useful experiment | Invariants to preserve |
| --- | --- | --- | --- |
| Tool event lookup | `SessionView::tool_index`, `view_model.rs` | Release-mode mixed histories with 1/4/32 live calls; compare complete folded views against reverse lookup | Latest duplicate-ID match, replay, clone, public item replacement, late-delta guards |
| App persistence | `EventWriter`, `store.rs` | Completed: bounded duplicate serialization and gated failure/barrier fixtures. Next: real batch distributions, sustained queue age and retained bytes; fixed-buffer/throughput trade-offs remain | Accepted-event ordering, barriers, rollback, retry without duplicates, archive/quit acknowledgment |
| Engine persistence | `Saver`, `flint-agent/src/persist.rs` | Completed: one pending complete snapshot and lifecycle regressions. Next: caller serialization cost and individual snapshot sizes | Newer complete snapshots may supersede older snapshots; transcript events may not |
| Subagents | `run_child`, `subagents.rs` | One/four children; retained observation bytes, approval and cancellation progress | Child identity, parent approvals, completion, resume and event order |
| UI event pump | `app_engine.rs` | Slow-consumer traffic; per-session update latency and peak queued work | A 256-event batch bounds UI work, not producer queue retention |
| Large task projection | `turns.rs`, prior improvement ledger | Measure realistic over-eight task counts and invalidation | Reference ordering; no speculative cache or repeated disproven iterator fix |

Do not replace unbounded channels with bounded channels while leaving
`try_send` unchanged. That can lose events. Delivery-policy changes need a
separate design and regression proof.

## Double-check every proposed update

1. Reproduce one concrete cost or failure before changing code.
2. Freeze the fixture, scope and acceptance checks before results exist.
3. Retain before/after raw samples and source/binary hashes.
4. Preserve an unchanged differential reference where practical.
5. Check replay, approvals, cancellation, retries and shutdown, not only speed.
6. Record added state, memory, invalidation and maintenance costs.
7. Judge the actual process exit, not only a parsed success message.
8. Preserve failed samples, skipped checks and missing evidence.
9. Separate a helper improvement from a whole-app performance improvement.
10. Derive the next question from the actual delta, not from a speculative
    rewrite or invented technical-debt score.

## Validation and overhead

Use narrow Python regressions for harness changes and the relevant Rust
regressions for application changes. Run native checks once, offline, with
fresh output/home directories and an explicitly supplied release binary.
Do not rebuild or restart the user's app implicitly.

Record command exits, elapsed time, test/check counts, unavailable metrics,
changed paths and evidence size. Distinguish wall time from active execution.
Budget passes are not speedups. Closing Flint does not prove the entire host
is idle; label that limit on native measurements.

No loop, automatic promotion, commit or publication is part of this checklist.

## Completed focused pass, 2026-10-05

### Scope and selection

Reviewed the current dirty blueprint, commit `4f282c1`, the 28-pass improvement
ledger, and the tool lookup, UI pump, app/engine persistence and subagent
forwarding implementations. The app-level leads above do not yet establish a
representative performance regression or a safe queue-policy change. This pass
therefore fixes verified measurement and harness orchestration failures, rather
than adding speculative caches or changing event delivery.

The existing strict JSON, missing-frame and Cargo-exit fixes were already
uncommitted at the start. They were preserved, not credited to this pass.
Historical overnight plans are outside this pass: no supervisor, recurring job,
engineering loop, model invocation, commit or publication was created or run.

### Changes and reasons

| Changed area | Verified issue | Bounded fix |
| --- | --- | --- |
| `bp.py`: RSS/CPU probes | Empty or failed `ps` output became zero; a decreasing CPU counter produced a negative, passing value | Check probe exit and parsed values; represent unavailable data as non-finite internally and `null` in the existing strict reports; preserve valid zero CPU |
| `perf.py`: aggregation | Dropping a missing startup sample let incomplete runs pass; raw startup/settled samples were discarded | Require every planned sample to be finite; retain cold, idle, demo and long-session samples |
| `perf.py`: workload validity | Frame data could pass after a failed/timed-out process or missing window | Retain window/exit status and require both, alongside measured intervals, for frame aggregates; reject failed live-TTFT workloads |
| `perf.py`: paired evidence | Both binaries used the same run-number log/frame paths and a single timing/live-state path | Include binary/run labels in raw paths; reject `--baseline-bin --tag before`, which otherwise silently overwrites the baseline entry |
| `bp.py`, `perf.py`: cleanup | Forced termination did not wait after kill; successful frame/live exits left log handles open; exceptions bypassed cleanup | Reap killed processes; close logs in `finally`; stop each performance process in `finally`; clean only the live probe's owned temporary workspace |
| `bp.py`: output preservation | `fresh_dir` recursively deleted any existing directory | Refuse existing paths without touching their contents |
| `sweep.py` | Making missing RSS non-finite exposed an integer-rounding exception before reporting | Preserve the rounded value while allowing missing RSS to reach strict JSON and failed checks |
| Harness tests and README | These failure paths were uncovered, and startup documentation said five runs although code used seven | Add 32 regressions, preserve existing tests, and document actual evidence semantics and naming |

Changed paths relative to the starting bytes: `tools/blueprint/{bp.py,perf.py,
sweep.py,test_bp.py,test_perf.py,README.md}` and this oversight document.
No Rust source, dependency, permission policy, performance threshold, aggregate
report filename or user's release executable changed. Valid numeric samples
retain their units and statistics. Intentional compatibility changes: incomplete
runs fail, repeated sweep directories are refused, and raw paired paths now
include labels. Reports retain 14 additional small startup/settled sample records
per binary, not a new application cache or background service.

### Observed results, not a runtime speedup

Identical deterministic fixtures were run against the preserved starting
helpers and the updated helpers:

| Fixture | Starting result | Updated result |
| --- | --- | --- |
| Failed/empty RSS probe | `0.0 MB` | Unknown, serialized as `null` |
| Failed/empty CPU-time probe | `0.0 s` | Unknown, serialized as `null` |
| Median of `[10.0, None]` | `10.0` | Unknown; incomplete evidence fails |
| Two runs for each of two binaries | Two distinct labels for four samples | Four distinct labels; no cross-binary raw-path collision |

Mocked native fixtures additionally prove missing samples fail the final
report, failed/timeout exits cannot pass with otherwise valid frame data, and
valid/offline/skipped evidence remains intact. Two real disposable Python
subprocess tests verify live/dead `ps` probes, termination, and forced-kill
reaping with closed logs. They do not launch Flint or measure its performance.

**No application speed, CPU, memory or frame improvement is claimed.**
Native timings and live TTFT were not measured in this pass.

### Validation and retained evidence

Evidence root: `target/focused-blueprint-20261005-3jj3jnfe/`.
It contains starting dirty-file copies, status/diff, source and release SHA-256
provenance, before/after deterministic results, command logs, and this pass's
diff relative to the dirty starting bytes, not merely Git HEAD.

- Starting harness suite: **36/36**, exit 0, 0.036 s test runtime,
  0.129 s including Python startup.
- Failing-before reproduction: 57 tests, exit 1, `failures=23, errors=9`
  including subtests. A later two-test review reproduction also fails before
  fixing missing process-status defaults and the colliding baseline tag.
- Reviewed suite: **68/68**, exit 0, 5.144 s test runtime, 5.341 s including
  Python startup. The intentional SIGTERM-ignore fixture spends five seconds
  exercising the existing forced-stop grace period.
- Closeout rerun: **68/68**, exit 0, 5.115 s test runtime, 5.216 s including
  Python startup; lint and whitespace checks also pass again.
- `python3 -W error::ResourceWarning -m unittest discover -s tools/blueprint
  -p 'test_*.py' -v`: passes, including interaction, diff and queue regressions.
- `ruff check --no-cache` on all five changed Python files: exit 0.
  AST parsing: all 19 blueprint Python files pass.
- `perf.py --help`, `sweep.py --help`, and `git diff --check`: exit 0.
- Source preservation check: 248 starting files fingerprinted; only intended
  paths changed. Existing unrelated tracked/untracked changes, performance
  limits and the release-executable hash remain intact.
- Rust gates, release build, native windows/screenshots, provider-backed
  inference, physical input and VoiceOver: **not run**. No Rust code changed;
  the active app and executable were left untouched. Historical passes are not
  current results.

Command timings are individual execution times, not active engineering-time
accounting or a before/after performance benchmark. See
`reviewed-validation.json`, `deterministic-results.json` and
`closeout-preservation.json` for machine-readable closeout evidence.

### Remaining risks and recommended next steps

1. Run one explicitly approved, sequential offline native comparison on a quiet
   host, using retained binaries and a fresh output root. Verify the newly
   labelled artifacts and stricter failure behavior. Budget passes alone still
   do not establish a speedup.
2. Measure slow-consumer event and persistence pressure before changing queues:
   retained bytes, enqueue/flush latency, replay ordering, approvals,
   cancellation and shutdown. Snapshot supersession is not transcript dropping.
3. Measure mixed long histories with 1/4/32 live tools against the existing
   reverse-lookup oracle before adding an index; public item replacement and
   duplicate-ID precedence remain invalidation concerns.
4. Harness limits remain: `ps` CPU timing is coarse; two steady frames satisfy
   the existing minimum but do not prove a representative workload; malformed
   evidence aborts instead of always producing a partial JSON report; process
   cleanup covers the owned process, not an audited descendant-group policy.
   Launch-environment isolation and command deadlines deserve a separate pass.
5. Fresh-directory refusal is not a global output lock. Concurrent harness
   invocations or reusing a report output root can still overwrite other
   reports; follow the sequential/fresh-output rule. Older raw-path consumers
   must adopt the new labelled names.

## Completed persistence-pressure pass, 2026-10-05

### Verified cost and bounded change

`store.rs::append_batch` retained the accepted `Logged` records and serialized
the entire batch into another growing `Vec<u8>` before opening `events.jsonl`.
The frozen 256-record ASCII fixture produces 8,392,594 JSONL bytes but needs a
16,782,848-byte serialization vector. An unavailable log path paid that same
serialization/allocation cost before reporting an error.

The change opens the log first and serializes through a 64 KiB `BufWriter`.
It explicitly flushes before acknowledging success and restores the original
file length after any serialization/write/flush error. Accepted records stay
in the worker until the write succeeds. Queue acceptance, adjacent-append
coalescing, failure notification, explicit retry, barrier order and shutdown
policy are unchanged.

Two private, statically dispatched callbacks make actual worker/file failure
paths testable without sleeps or a new production abstraction. Public APIs,
on-disk JSONL bytes, dependencies and the engine's snapshot writer are unchanged.
Changed source paths: `crates/flint-app/src/store.rs` and the new
`store_pressure_tests.rs`; this section is the only documentation update.

### Frozen fixtures and release measurements

Evidence: `target/persistence-pressure-20261005-8wqgf0r6/`.
`frozen-plan.json` precedes the implementation and records fixtures, metrics
and acceptance checks. `probe-before.rs` embeds the original store source;
`probe-after.rs` embeds the changed source. Both use identical measurement code,
matched cached dependency identities and Rust 1.95.0 optimized ThinLTO builds.
Five paired process runs alternate ordering, execute serially, preserve all
raw samples and check exact JSONL byte equivalence.

These are helper-only fixtures. Allocation tracking measures temporary requested
heap bytes on the calling thread, excluding retained records, allocator
internals, queues, the oracle and app RSS. Filesystem timings use warm writes on
this external APFS volume, include measurement overhead and do not establish a
quiet-host, durable-storage or whole-app performance result.

Median of each process's mean append times:

| Fixture | Temporary append allocation before → after | File append before → after |
| --- | ---: | ---: |
| One short record | 276 → 65,536 B | 0.0565 → 0.0517 ms |
| 4,000 short records, 98,890 JSONL bytes | 131,318 → 65,536 B | 0.1448 → 0.1795 ms |
| 256 × 32 KiB ASCII payloads | 16,783,100 → 65,536 B | 10.3569 → 12.1938 ms |
| 256 escaped/Unicode payloads, 11,523,986 JSONL bytes | 16,777,480 → 65,536 B | 18.1102 → 16.1164 ms |
| One 8 MiB payload | 16,777,476 → 65,536 B | 11.6356 → 9.8571 ms |

The failure fixture makes `events.jsonl` a directory, so file opening fails.
For the ASCII batch, temporary allocation drops 16,783,114 → 264 B and mean
failure time drops 2.8034 → 0.0038 ms. This is fail-fast behavior for this exact
open failure, not a claim about every storage error.

The gated debug diagnostic retains two batches, first one record and then all
256 queued records (8,392,594 JSONL bytes), before acknowledging the barrier.
Its single before/after samples show enqueue 0.0431 → 0.7700 ms and
flush-after-release 188.7844 → 192.2687 ms. Record construction and the size
oracle are outside the enqueue timer. This proves controlled backlog/order
and nonblocking acceptance, not a latency improvement; debug single samples
are not interchangeable with the paired optimized measurements above.

**The allocation improvement is demonstrated; throughput is mixed.**
The 4,000-short-record and ASCII file fixtures are slower. Small writes now
allocate a larger fixed buffer than before. This trade-off is retained openly
because the bounded goal is removing backlog-sized duplicate serialization and
wasted failed-open work, not claiming universal speed or total-memory limits.

### Correctness and validation

Ten new ordinary regressions and one opt-in diagnostic cover exact nested-event
JSONL/replay, empty batches, 64 KiB boundaries, oversized records, partial writes,
final flush errors, rollback/retry without duplicates, gated adjacent batches
and barriers, failed-record retention, recovery at shutdown and independent
session progress while another writer is blocked.

The initial three store regressions pass before the change. Four additional
oracle/gated/failure fixtures also pass before the serialization change; the
remaining tests exercise the new buffered write path. Allocation assertions
pass across all ten paired probe runs. The diagnostics do not use real
credentials, providers, native windows, timing assertions or scheduling.
The final compiled unit binary was also checked directly: all 13 ordinary store
tests pass again; the opt-in pressure diagnostic remains explicitly ignored.

Two initial standalone-probe builds failed: selecting cached dependencies by
mtime mixed Serde identities, then the native linker could not read Rust's
LLVM bitcode. Matching Cargo dependency fingerprints and using the workspace's
ThinLTO settings resolved both. Failed build logs remain; neither generated a
valid measurement sample.

Validation runs use a sanitized environment, disposable HOME/FLINT_HOME outside
`/tmp`, `FLINT_LIVE=0`, offline/locked Cargo and an owned build target. Debug and
release caches are independent APFS copy-on-write copies, not hardlinks. The
original release executable and running user app are not replaced or launched.
Headless UI validation does not force background-focus environment settings.

The first full workspace attempt failed three existing non-repository fixtures:
putting TMPDIR inside this checkout let Git discover the parent Flint repository.
Moving owned scratch space outside the checkout fixes all seven project and
eight worktree tests without changing their code or assertions. The failed
attempt remains in `workspace-tests.log`; it is not a passing checkpoint.

Final gate results are recorded in `final-validation.json`:

- Formatting and strict offline/locked workspace/all-target Clippy: exit 0.
- Full offline/locked serial workspace: exit 0, 744.76 s including Cargo.
  ACP 68/68, agent 154/154, app 184/184, headless UI 260/260 (716.55 s), and
  terminal 19/19; integration/doc entry points pass. Twenty-three opt-in
  benchmarks/diagnostics are ignored. The live-provider entry point returns
  without inference because `FLINT_LIVE=0`.
- Owned-target release build: exit 0, 149.46 s. The rebuilt executable remains
  under this pass's `build/release/`, not `target/release/flint`.
- Opt-in gated debug diagnostic: 1/1, exit 0; ordinary store double-check:
  13/13 with the diagnostic skipped.
- Blueprint Python suite with ResourceWarnings treated as errors: 68/68,
  exit 0, 5.125 s test runtime.
- Whitespace and measured-source checks pass. All 248 starting source files
  are fingerprinted; only `store.rs` and this oversight document change, plus
  the new regression file. All other starting bytes and the original release
  hash remain intact. `closeout.json` records final hashes and the pass-only
  diff, separate from pre-existing dirty work.

These are command/test execution times, not active engineering time or app
performance measurements. Native screenshots, clean-host performance, live
provider inference and power-loss tests were not run. No recurring job or
engineering loop was created, no checkout commit or external publication was
made, and the user's app was not restarted.

Later documentation-only closeout intentionally changes this document's hash.
The retained `closeout.json` remains the immutable evidence for the earlier
implementation checkpoint; it is not rewritten to imply another Rust test run.
No application, harness or test source changed during the documentation review.

### Remaining risks and follow-up

1. The accepted-record queue and failed-record retention remain unbounded.
   This removes a duplicate serialized batch, not event backpressure or an
   end-to-end memory cap. Pending vector capacity can still remain resident.
2. The fixed 64 KiB allocation costs more than the old vector for tiny batches,
   and chunked file writes can cost more syscalls. Measure real batch-size
   distributions before considering buffer sizing; do not add a speculative
   cache or delivery-policy change.
3. `BufWriter` may forward an oversized string directly. Its allocation bound
   is not a per-syscall byte limit or a cap on the retained input record.
4. Flush acknowledgment is not `fsync`, atomic batch append or power-loss
   durability. Partial rows may be visible before completion. A failed truncate
   during rollback remains an existing unresolved storage limit.
5. Next useful experiment: sustained multi-session producers against deliberately
   slower storage, measuring queue age/retained bytes and cancellation/approval
   progress. Freeze a delivery contract before attempting bounded channels.

## Persistence lifecycle and approval integrity pass, 2026-10-05

Status: complete. Focused regressions, final combined workspace, strict Clippy,
formatting, isolated release build and Python harness validation all pass.
This is a directly requested, bounded engineering pass, not an overnight
supervisor, recurring task or automatic promotion.

Evidence: `target/history-reliability-20261005-tfivqe7b/`. Starting source copies,
the dirty diff/status, frozen fixtures, failed-before results, optimized paired
probes, actual command exits and source/binary fingerprints are retained there.
The approval follow-up has its own frozen scope and failing fixture. The first
workspace checkpoint covers persistence only; it is not the final combined gate.

### Demonstrated gaps and fixes

1. **Superseded engine-history retention.** `Saver` previously queued every
   serialized history snapshot while its writer was unpolled or blocked.
   Draining obsolete snapshots after storage recovered did not bound the
   backlog. It now replaces one mutex-protected pending complete snapshot and
   uses a capacity-one channel for wakeups only. An in-flight write owns its
   separate snapshot. The mutex is never held during storage work, and replaced
   payloads are dropped outside it. The latest write determines the flush
   result; a terminal failure or writer panic cannot acknowledge saved history.
   Transcript events, approval delivery and their queues are unchanged.
2. **Invalid restored tool results.** Restoration previously supplied missing
   tool results but also retained orphan, duplicate and late results. Those
   messages are invalid in the next provider request. Restoration now accepts
   only a result matching an unanswered call in the current contiguous tool
   group. Missing results still get the existing interruption message. Valid
   result order, reasoning, images and reused IDs in later call groups survive.
   Loading repairs the in-memory history without rewriting the saved file.
3. **Stale approval inspection.** The transcript's tool-event folding already
   selects the latest matching call ID, but approval details selected the first.
   Reused parent IDs and resumed child IDs could therefore show an old command
   and working directory. Parent and child preview lookup now both walk
   backwards. Missing current arguments stay missing rather than revealing
   stale details. Approval decisions, permission rules and full argument text
   are unchanged; no cache or index was added.

Changed implementation/test paths are
`crates/flint-agent/src/{persist.rs,persist_tests.rs}` and
`crates/flint-app/src/{view_model.rs,view_model_tests.rs}`. This oversight
document is the only documentation change. Existing storage/harness changes,
dependencies, protocol and on-disk schemas remain intact.

### Retained release evidence

The frozen fixture submits 128 complete snapshots, each containing a 65,536-byte
user message, without yielding its current-thread runtime. Both binaries verify
that turn 128 is restored before judging pending retention.

Five paired process runs alternate before/after order, use Cargo's optimized
release profile and Rust 1.95.0, and preserve exact binary hashes and raw logs:

| Retained serialized snapshot payload | Before | After |
| --- | ---: | ---: |
| Pending snapshots before polling | 128 | 1 |
| Pending payload bytes | 8,395,156 | 65,588 |

Every pair has those same counts. The old regression intentionally exits 101
because its retention assertion fails; those are failing-before reproductions,
not passing tests. The new fixture exits 0. These are retained JSON payload
bytes, not allocation capacity, total app memory, filesystem latency or a
whole-app speedup. Serialization still runs on the caller, individual snapshots
can be large, and one storage write can remain in flight.

### Focused verification

- Original persistence fixture: 7 pass, 4 fail, exit 101. The four failures
  reproduce retention and orphan/duplicate/late result handling.
- Final persistence regressions: 21/21 in debug and release, exit 0. Sixteen
  are new. Gated writers cover replacement during blocked storage, flush
  acknowledgment, recovery after an earlier failure, terminal failure, panic,
  drop, unused shutdown, independent sessions and concurrent producers.
- A separate structural oracle checks all 6,561 four-message combinations
  from nine representative message types. Repaired histories are valid for
  that fixture and idempotent. A saved-file fixture also checks actual provider
  request serialization and leaves original file bytes intact.
- Isolated approval fixture before the fix: 1 pass, 3 fail, exit 101.
  Afterward: 4/4 pass. All 187 ordinary app model tests pass, with 21 opt-in
  probes explicitly ignored.
- Formatting and strict offline/locked workspace/all-target Clippy pass.
- Full offline/locked serial workspace: exit 0, 278.39 s including Cargo.
  ACP 68/68, agent 170/170, app 187/187, headless UI 260/260 (246.21 s), and
  terminal 19/19; integration/doc entry points pass. Twenty-three opt-in
  diagnostics are ignored. The live-provider entry point returns without
  inference because `FLINT_LIVE=0`.
- Owned-target release build: exit 0, 63.73 s. The executable is
  `target/history-reliability-20261005-tfivqe7b/build/release/flint`;
  the user's `target/release/flint` is unchanged.
- Python harness with ResourceWarnings treated as errors: 68/68, exit 0,
  5.119 s test runtime. No Python implementation changed in this pass.
- Whitespace, validated Rust source stability and starting-source preservation
  checks pass. The 222 fingerprinted starting text-source files have only the
  five intended path changes. `closeout.json` and `pass-only.diff` distinguish
  this pass from the pre-existing dirty work.
- These are command/test execution times, not app-performance or active
  engineering-time measurements. Native, live-provider, physical-input,
  VoiceOver and power-loss checks remain explicitly unperformed.

### Limits and next useful checks

The wakeup channel's bound is safe because complete snapshots supersede each
other; applying that policy to transcript, protocol or approval events would
lose work. Accepted transcript queues and failed-event retention remain
unbounded. A single snapshot is not size-limited by this change. Flush remains
an atomic rename acknowledgment, not `fsync` or proven power-loss durability.

Next, measure sustained event pressure across multiple sessions and children:
retained bytes and queue age, ordered replay, approval progress, cancellation
and shutdown. A separate delivery contract is still required before changing
those queues. Approval lookup remains a scan; long-history indexing still
needs representative measurements and public-item invalidation tests.

Validation uses an owned Cargo target, APFS copy-on-write cache copies and
sanitized HOME/FLINT_HOME, with test scratch outside the checkout and `/tmp`.
No native app is launched, restarted or replaced. No provider credentials, commits,
pushes, publication or recurring jobs are used.
