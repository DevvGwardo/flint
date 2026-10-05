# Flint UI and performance audit

Date: 2026-10-03. Scope: current working tree, including pre-existing changes.

## Current gap-closure checkpoint

Evidence: `target/gap-closure-20261003.n8SJNS/`; current dispositions are in
[the gap-closure ledger](GAP-CLOSURE-20261003.md), rather than the historical
sections below. Final integrated formatting, strict Clippy, workspace tests,
172 serial UI tests, release build and whitespace checks pass for `final3`.
The compact-pane/accessibility follow-up also passes all six gates, including
strict Clippy, workspace tests (app 100, UI 173), a separate serial UI run
(173/173, 117.40 s including Cargo), release build and whitespace checks.
The retained follow-up binary also passes native sweep 100/100, interleaved
performance 10/10, diff 16/16 and workloads 71/71. Final native assertions pass
with no source drift since validation.

The app limits ordered pump batches to 256 events, remeasures changed existing
rows once per batch, retains a Unicode-safe live-output tail, and offers
session-local **Jump to latest**. Terminal forwarding follows ownership.
Agent option chips retain confirmed state until acknowledgement. Drafts and
attachments remain session-local in ordinary and tiled views, including
project-created sessions. Mention indexing runs off-thread with workspace
guards; regular text attachments read a bounded prefix inside the workspace.

The first fixed release passes sweep 100/100, interleaved performance 10/10,
diff 16/16 and workloads 71/71. Streaming CPU is 53.4 -> 53.6%, frame p95
15.27 -> 15.23 ms, and scrolling p95 15.09 -> 15.16 ms. Budgets pass, but
there is no meaningful measured streaming speedup. The latest paired rerun
is authoritative for the follow-up binary: streaming CPU 75.4 -> 79.4%,
frame p95 13.37 -> 13.23 ms, first frame 195.63 -> 206.16 ms. Both binaries
have a different frame cadence from the earlier run; do not compare CPU
across phases. The latest budgets pass without a meaningful speedup.

Fresh screenshots were inspected for major states, 900x560 welcome/settings,
large diff, terminal output and restored panes. Normal-state controls are
readable, and the settings footer remains visible while the form scrolls.
Review found compact-pane input clipping: wrapped options crushed the input
to 30.5px. A failing-before/passing-after fixture now requires 44px of readable
input, a visible send button and scroll-reachable lower options. Latest native
1/2/4/8-pane screenshots were inspected: the compact eight-pane input is
readable and Send remains visible; two/four-pane option chips do not overlap.
Lower wrapped options are scroll-reachable in the headless regression, not
exercised by this native probe. Idle pane RSS is 77.125/80.297/84.203/88.750 MB,
CPU 0–1%, with correct restored counts and no panics. The single-pane launch
intentionally opens a fresh session with the seeded one still in the sidebar.

The native AX tree activates lazily: six elements/one named on the initial
query, then 13/four named after 500 ms. Source inspection found custom Div
labels without roles. Primary custom navigation, session, composer, option,
permission and file controls now have explicit Button roles/names; existing
native click handlers also supply AccessKit Click actions. The composer role
fixture fails before/passes after. The final trusted, error-free activated AX
probe sees 37 elements/28 named, including 31 buttons, with named primary
buttons advertising AXPress. Actions were enumerated, not invoked. The AX demo
screenshot was inspected; full VoiceOver/transcript/terminal reading is not
verified.

The latest 10,000-line diff fixture measures 81.97 MB RSS, CPU 28%, frame p95
16.54 ms and root-render p95 0.183 ms, without a comparable phase baseline.
Saved streaming preserves accepted records in order at approximately 2,000
events/s, with batch p95 0.339–0.378 ms. These do not establish global queue
bounds or performance on slow storage. Terminal workload frame p95 reaches
16.73 ms; that script checks behavior, not a 16.7 ms performance budget.

These are not physical keyboard/clipboard, VoiceOver, full transcript/terminal
semantic reading, or concurrent native multipane streaming passes. Very small
popover and styled combining-cell paint remain incompletely verified.
Historical evidence and limits below remain historical.

## Sidebar closeout

Verified through 2026-10-03 11:32:50 EDT. Continuation began at 10:53:08 EDT:
39m42s wall time, not the total duration of sidebar work or proof of active-work
accounting. This finishes the requested sidebar follow-up, not the earlier
four-hour goal. Evidence root: `/private/tmp/flint-sidebar-20261003-it5gpl/`.
Release binary SHA-256:
`90ac070b62e7f35ee9600e5d1f3ba1531c6de274fa43a5dbf9d7291804e500c9`.

- Session and archive lists have persistent scrollbars and reserved scrollbar
  space. Long workspace names, session titles, and status labels truncate with
  tooltips. Mouse wheel, scrollbar-track clicks, and dragging preserve selection.
- Search trims whitespace and matches titles and workspace paths without case
  sensitivity. Empty-filter recovery clears search/filter and resets scrolling.
  Running includes sessions waiting for approval.
- Navigation and session actions support keyboard focus and activation.
  Focused off-screen session/archive rows reveal once after layout, guarded by
  unchanged focus and scroll offset so redraws do not override manual scrolling.
- Rename cancels on blur, session changes, archive/undo, drawer closing, and
  automatic sidebar collapse. Stale input events cannot affect another editor.
  Palette rename opens a visible, focused editor in narrow or filtered windows.
  Closing an already-closed palette cannot schedule focus theft.

Focused edge tests reproduced two bugs before their fixes: cancelled rename
applied a delayed scroll, and Tab could focus an off-screen session without
revealing it. Both now pass in the workspace and serial UI suites. Evidence:
`sidebar-edge-cases.log`, `sidebar-edge-fix-2.log`, `sidebar-keyboard.log`,
`workspace-tests-final.log`, and `ui-tests-final.log`. The keyboard fixture was
also corrected to advance platform frames between Tabs. Strict Clippy findings
were fixed without suppression or changes to permission safeguards.

| Check | Result | Evidence under sidebar root |
| --- | --- | --- |
| `cargo fmt --all -- --check` | Pass | `fmt-final-verified.log` |
| `cargo clippy --workspace --all-targets -- -D warnings` | Pass | `clippy-final-verified.log` |
| `cargo test --workspace`, `FLINT_LIVE=0` | Pass: ACP 25, agent 73, app 65, UI 136, terminal 13 | `workspace-tests-final.log` |
| Separate serial `blueprint_ui` tests | 136/136, 85.13 s | `ui-tests-final.log` |
| `cargo build --release -p flint-app` | Pass, 1m02s | `release-final.log` |
| `git diff --check` | Pass | Command output |
| Offline UI sweep | 100/100, 94.8 s | `sweep/data/report-sweep.json` |
| `perf.py --no-live` | 10/10, 92.4 s | `perf-clean/data/report-perf.json` |

Current performance: cold window 324.95 ms, first frame 225.11 ms, idle RSS
70.59 MB, 200-turn RSS 82.67 MB, idle CPU 0.75%, streaming CPU 82.0%,
streaming frame p95 13.60 ms, scrolling p95 12.98 ms, binary size 31.1 MB.
Worst streaming run dropped 10 frames; both scrolling runs dropped zero.
These establish budget compliance, not speedups: no comparable sidebar baseline
was collected. Mention-picker idle CPU passes at 1.3%; this does not explain
the historical 2.0% cutoff failure.

The successful sweep and clean performance run used disposable homes/workspaces
and separate fresh output directories, without overlap. Accidental duplicate
performance runs were stopped and their partial `perf/` evidence is excluded;
the final measurement is exclusively from `perf-clean/`. Duplicate Cargo batches
also completed successfully; the table uses only the identified final logs.
Screenshots were captured under `sweep/ui/`, but image viewing was rejected.
Visual clipping/overlap/readability review, physical macOS clipboard testing, and
VoiceOver review remain unverified. No live-provider tests or real credentials
were used. No commits, pushes, or publication. Existing user work and the open
user app process were preserved; restart Flint to load the rebuilt binary.

## Dropdown and image-paste follow-up

Verified through 2026-10-03 10:08:44 EDT. Scope is the user's dropdown-scrollbar
and composer-image-paste reports, not a restart of the earlier four-hour goal.
The follow-up start was not reliably recorded; no total-duration claim is made.
Evidence root: `/private/tmp/flint-paste-dropdown-20261003-FVyKoL/`.
Release binary SHA-256:
`a38b4e231655c7bc26c42aac1b9fc4e17eea9c3bb01fe93d645961bfcd35a59a`.

- Composer popovers have bounded scrolling and visible GPUI scrollbars.
  Option dropdowns keep their header fixed, reserve space for the scrollbar,
  open with the current choice visible after initial layout, and follow
  keyboard selection without overriding manual scrolling on redraw.
- Composer-local image paste explicitly consumes the Paste action. PNG,
  JPEG, GIF, and WebP bytes become private temporary attachments; TIFF/BMP
  conversion runs off-thread with input, dimension, and allocation limits.
  Copied supported image files retain their original paths and are never
  deleted by attachment removal.
- Pending pastes count toward the four-image limit and block submission until
  preparation completes. Results require the original session/workspace.
  Temporary files are removed after encoding or attachment removal, and
  removing an image restores composer focus. Ordinary text paste, replacement,
  and undo remain intact.

Focused tests cover selected draft preservation, image-only sends, invalid and
oversized data, temporary cleanup, external-file preservation, rapid pastes,
session switching, send-during-preparation, and TIFF/BMP conversion. The
40-choice dropdown test checks current/keyboard-selected row bounds, wheel
scrolling, scrollbar-track clicks, and redraw stability at 900x560.
Initial focus, observation, and first-open scrolling failures were corrected.
Formatting failures in the new code were corrected without lint suppression.

| Check | Result | Evidence under follow-up root |
| --- | --- | --- |
| Formatting and strict workspace/all-target Clippy | Pass | `fmt.log`, `clippy.log` |
| Workspace tests, `FLINT_LIVE=0` | Pass: ACP 25, agent 73, app 65, UI 119, terminal 13 | `workspace-tests.log` |
| Separate serial UI tests | 119/119, 58.54 s | `ui-tests-serial.log` |
| Release build and `git diff --check` | Pass | `release.log`, command output |
| Offline UI sweep | 100/100 | `sweep/data/report-sweep.json` |
| Offline performance | 10/10 | `perf/data/report-perf.json` |

Current performance: cold window 357.41 ms, first frame 270.97 ms, idle RSS
71.27 MB, 200-turn RSS 82.77 MB, idle CPU 0.5%, streaming CPU 83.2%,
streaming frame p95 13.95 ms, scrolling p95 12.86 ms, binary size 31.0 MB.
These are budget checks, not a measured speedup; no comparable follow-up
baseline was collected. Mention-picker idle CPU passes at 0.7%, but this does
not establish the cause of the earlier 2.0% cutoff failure.

The completed sweep and performance scripts ran sequentially with disposable
homes/workspaces and separate output roots. An accidental duplicate sweep
invocation failed during workspace setup; the completed sweep's report is
retained. Screenshots were captured but image viewing was rejected, so no
visual/VoiceOver approval or physical macOS clipboard end-to-end check is
claimed. No provider-backed tests or real credentials were used. The existing
app process remains untouched; restart Flint to load the rebuilt binary.

## Final closeout

The user requested finish. Recorded start: 2026-10-03 02:41:23 EDT; minimum
completion: 06:41:23 EDT; planned regression window: 05:56:23 EDT onward.
Closeout resumed at 08:19 EDT and was verified through 08:32:33 EDT,
5h51m10s wall time from start. This includes an interruption: earlier goal
accounting was blocked at 2h55m41s. Four hours of active work and the requested
final 45-minute window are not proven. The goal is not claimed fully achieved.

Final evidence root: `/private/tmp/flint-final-20261003-0819/`.
Final release binary SHA-256:
`46e95ec4273dbc6393161cf872c44c94864726b28a0efbd1fce0a1dd29445850`.
No commits, pushes, publication, paid/live inference, or real credentials.
The existing dirty checkout and native Rust/GPUI architecture were preserved.

| Required final check | Result | Evidence under final root |
| --- | --- | --- |
| `cargo fmt --all -- --check` | Pass | `fmt-final.log` |
| `cargo clippy --workspace --all-targets -- -D warnings` | Pass | `clippy-final.log` |
| `cargo test --workspace` | Pass | `workspace-tests-final.log` |
| `cargo test -p flint-app --test blueprint_ui -- --test-threads=1` | 108/108 pass, 44.89 s | `ui-tests-final.log` |
| `cargo build --release -p flint-app` | Pass | `release-final.log` |
| `git diff --check` | Pass | Command output |
| Offline UI sweep | 99/100 | `sweep/data/report-sweep.json` |
| `perf.py --no-live` | 10/10 | `perf/data/report-perf.json` |
| 10,000-line diff workload | 16/16 | `diff/data/report-diff.json` |
| Saved stream, terminal, many sessions | 71/71 | `workloads/data/report-workloads.json` |

Workspace suites passed: ACP 24, agent 72, app 60, UI 108, terminal 13,
plus four integration-test entry points. The live-provider test entry point
returned without live execution because `FLINT_LIVE=0`. The native-engine UI
tests use loopback fixtures and dummy credentials.

The scripts ran sequentially, each with a fresh `FLINT_BP_OUTPUT_DIR` and
disposable homes/workspaces. Screenshots were captured at native scale, including
900x560 settings and empty states. Image viewing was rejected by this session's
tool, so clipping, overlap, readability, contrast, and visual consistency remain
unverified. Bounds, pixel variance, and interaction checks are not visual review
or a physical-keyboard/VoiceOver compliance assessment.

The sweep's only failure was mention-picker idle CPU at 2.0% against a strict
`< 2.0%` budget. It is retained, not waived or proven to be a regression.
Earlier frame captures had only startup frames, also with the original binary;
final runs captured steady-state frames successfully. Do not infer speedups
from comparisons between those different rendering conditions.

Final gates found missing `SessionStopped` match arms in the headless example
and live-test target. They now handle the event. Initially recorded
`collapsible_if` warnings and new diff/persistence lint findings were corrected
without changing safeguards or suppressing warnings. One earlier docking Escape
test failure was reported as flaky; it passed both final workspace and serial
UI runs. It was not removed.

## Component inventory

| Component | Current foundation | Outcome |
| --- | --- | --- |
| Transcript | GPUI virtual list, incremental row updates | Preserved |
| Engine event pump | Coalesced batches with an 8 ms delay | Preserved |
| Welcome workspace | Composer, project selector, readiness state | Replaced oversized greeting with compact, left-aligned project-first layout |
| Composer | Agent selector, native/ACP options, approvals | Separated send controls from wrapping option row |
| Changes panel | Combined diff and selectable file list | Virtualized diff rows, bounded file navigation, retained document selection |
| File picker | Ignore-aware index capped at 20,000 files | Select top eight matches before sorting; preserve original ranking |
| Terminal | Coalesced output and native grid painting | Preserved |
| Theme | Shared dark palette and semantic colors | Preserved; no dependency or palette migration |

## Implemented changes

- `src/transcript/empty.rs`: compact heading and independent project selector
  eliminate the single oversized sentence containing an arbitrary folder name.
  Suggestions and the selector have focus styling and accessible labels.
- `src/composer.rs`: send/attach/agent controls stay in a dedicated row.
  Reasoning and permission options wrap below, retaining their existing behavior.
- `src/changes_panel.rs`: file navigation stops at 160 px and scrolls independently,
  preserving space for the diff. Filenames and directories use separate truncated
  lines. Combined diffs are borrowed rather than cloned on every redraw.
- `src/mention.rs`: partial selection replaces sorting every matching path.
  Only the eight displayed matches are sorted. A regression test compares results
  against the original full-ranking algorithm over 20,000 paths.

Source paths above are relative to `crates/flint-app/`.

## Completed hotpaths

Source paths below are relative to `crates/flint-app/`. The earlier UI changes
above remain present; this pass concentrated on measured rendering and storage
bottlenecks and focused reliability fixes.

1. **Diff virtualization.** `src/diff.rs` parses once per session/file revision
   and renders visible fixed-height rows using GPUI's uniform list. The widest
   row determines horizontal range, including long off-screen content. Selection
   belongs to the document, not recycled row entities. Tests cover reverse and
   partial selection, Unicode/word selection, blank lines, scrolling out of view,
   horizontal scrolling, content changes, file switching, and 10,000 additions.
   Parser tests cover old/new line numbers; no UI-level gutter-text assertion or
   visual numbering inspection was completed.
2. **Ordered background event persistence.** `src/store.rs::EventWriter`
   coalesces adjacent appends without crossing flush barriers. Failed batches
   remain ordered for retry. `src/session.rs::log` enqueues rather than doing
   streaming disk writes on the UI thread. Failures surface in the app and
   prevent new turns. Shutdown/archive retain explicit flush acknowledgments;
   the writer stops before a directory moves. Tests verify ordering, retries
   without duplicates, drop/quit draining, late events, failed storage, and
   native-engine archive/undo/restart recovery.
3. **Versioned background combined diffs.** `src/app_engine.rs` computes an
   owned snapshot on the background executor. Results require matching session,
   workspace, turn, revision, and non-running state. Restored edit history
   supports continued edits and archive recovery. Historical turn counts use
   their own contents; edit-free turns stay empty. Unreadable files are not
   converted into phantom deletions.
4. **Current-turn command tray.** `src/turns.rs::running_commands` scans only
   the current turn, preventing interrupted commands from leaking into later
   turns. A focused regression test passes. Many-session measurements did not
   justify an unrelated sidebar-cache refactor, so none was added.

## Earlier UI verification

These are historical results from the initial bounded UI pass, not final counts.
- `cargo build --release -p flint-app`: passed.
- `git diff --check`: passed.
- `cargo test -p flint-app --lib`: 48 passed.
- `cargo test -p flint-app --test blueprint_ui -- --test-threads=1`: 96 passed.
- Added a 900x560 layout check for a long workspace name, composer bounds,
  and separation between toolbar and options.
- Strict Clippy initially failed on three pre-existing `collapsible_if` warnings
  in `flint-agent/src/session.rs` and `flint-agent/src/subagents.rs`.
- App-only strict Clippy also found pre-existing `collapsible_if` warnings in
  `src/app_engine.rs`, `src/app_store.rs`, and `src/view_model.rs`. A redundant
  composer branch was removed without changing permission behavior.
- Screenshot viewing is unavailable in this session. Bounds and interaction
  tests do not substitute for a visual or VoiceOver review.
- No provider-backed tests or real-credential inference were run.

## Earlier UI performance

The isolated release-build run of `tools/blueprint/perf.py --no-live` completed
on 2026-10-03. All ten configured performance checks passed:

| Metric | Measurement | Budget |
| --- | --- | --- |
| Cold start to window (median) | 331.58 ms | 1,500 ms |
| Cold start to first frame (median) | 223.79 ms | 1,500 ms |
| Idle memory | 70.97 MB | 250 MB |
| RSS at 200 turns | 74.45 MB | 600 MB |
| Idle CPU | 0.5% | 1% |
| Streaming CPU at 2,000 deltas/s | 89.4% | 120% |
| Streaming frame interval p95 | 14.32 ms | 16.7 ms |
| Streaming dropped frames (worst run) | 11 | 30 |
| Scroll frame interval p95 | 15.15 ms | 16.7 ms |
| Release binary size | 29.9 MB | 80 MB |

Evidence: `/private/tmp/flint-ui-perf-20261003-0157/data/report-perf.json`.
No before/after baseline was collected, so these results establish budget
compliance, not a measured speedup. The measured binary predates only the final
behavior-equivalent removal of a redundant composer permission-label branch.

## Measured hotpaths

Earlier comparable fixture results are retained in
`/private/tmp/flint-four-hour-20261003-024123/`. They establish scoped
call-cost and resource improvements, not guarantees for every filesystem.

| Workload | Before | After | Scope |
| --- | --- | --- | --- |
| 10,000-line diff RSS, median of three | 337.59 MB | 69.41 MB | Same offline diff fixture |
| Diff CPU, median of three | 100.0% | 35.5% | Same fixture; baseline frame samples missing |
| 4,000 event records, five runs | 154.23-232.24 ms | 0.49-0.81 ms | Call time including enqueue, flush, and count verification; not fsync durability |
| 100,000-line combined diff, 20 edits, three runs | 120.00-225.59 ms | 92.02-93.43 ms | Snapshot + compute + apply, fixture preparation excluded |
| Combined-diff UI-facing snapshot + apply | Synchronous compute above | 0.011-0.017 ms | Computation moved off-thread; not total diff time |

Evidence: `diff/data/diff-before.json`, `diff/data/diff-after.json`,
`hotpaths-before.json`, and `hotpaths-after.json`. The final diff rerun measured
80.23 MB RSS, 27% CPU, 16.41 ms frame interval p95, and 0.091 ms root-render
p95. Baseline frame samples are missing, so no before/after frame-speed claim
is made. Earlier unoccluded and final-display measurements are not interleaved.

## Final performance

| Metric | Final measurement | Budget |
| --- | --- | --- |
| Cold start to window, median of seven | 350.52 ms | 1,500 ms |
| Cold start to first frame, median of seven | 206.90 ms | 1,500 ms |
| Idle memory | 71.42 MB | 250 MB |
| RSS at 200 turns | 82.88 MB | 600 MB |
| Idle CPU | 0.5% | 1% |
| Streaming CPU at 2,000 deltas/s | 82.0% | 120% |
| Streaming frame interval p95 | 13.96 ms | 16.7 ms |
| Streaming dropped frames, worst run | 10 | 30 |
| Scrolling frame interval p95 | 16.21 ms | 16.7 ms |
| Release binary size | 30.2 MB | 80 MB |

Evidence: final `perf/data/report-perf.json`. These establish budget compliance,
not an end-to-end speedup against the older UI pass.

Final saved streaming persisted all 10,002-10,003 accepted events in order,
with event-batch p95 0.361-0.395 ms and 118.7-125.4 MB RSS. Mirrored terminal
output ran at approximately 19,960 updates/s, event-batch p95 0.154-0.230 ms,
114.1-114.5 MB RSS, and frame interval p95 9.48-16.70 ms.
The workload script verifies behavior/frame evidence, not a terminal 16.7 ms
performance budget.

| Saved sessions | Window time, three runs | RSS | Idle CPU |
| --- | --- | --- | --- |
| 10 | 331.6-339.3 ms | 72.7-73.0 MB | 0-0.5% |
| 200 | 323.4-407.6 ms | 86.8-87.3 MB | 0.5% |
| 1,000 | 493.0-584.5 ms | 146.2-147.7 MB | 0.5-1.0% |

Evidence: final `workloads/data/report-workloads.json`. Earlier workloads
had startup-only frame samples and much lower streaming CPU; those cannot
serve as comparable rendering baselines.

## Remaining risks

- Inspect final screenshots in `sweep/ui`, `diff/ui`, and `workloads/ui` for
  clipping, overlap, readability, contrast, and consistency. Image viewing was
  unavailable; no visual or VoiceOver approval is claimed.
- Diagnose the mention-picker idle CPU result without relaxing its threshold.
  The single cutoff failure is not classified as pre-existing or a regression.
- Event writes are buffered in an unbounded queue while storage is stalled;
  failure-triggered shutdown limits production but is not a bounded-memory
  guarantee. Flush/archive/quit barriers and metadata writes can still block
  their caller on a slow filesystem. Writes are not fsync durability promises.
- Full arbitrary docking-layout and long-path visual matrices remain unverified.
  Behavioral tests cover panel identity, drag cancellation, resizing, persistence,
  focus containment, approvals, settings, terminal, and narrow-session reachability.
- Review focused on this pass's diff, persistence, combined-diff, task-tray, UI
  layout/search changes, and final validation fixes. This is not an audit of every
  pre-existing modified/untracked file.

## Reproduce

Run the five Cargo commands listed in Final closeout. Set `FLINT_LIVE=0` for
tests. Run blueprint scripts sequentially against `target/release/flint`, each
with a new absolute `FLINT_BP_OUTPUT_DIR`: `sweep.py --tag final`,
`perf.py --tag final --no-live`, `diff.py --tag final`, and
`workloads.py --tag final`. Pass `--bin` with the absolute release binary path.
Scripts create disposable homes/workspaces. No live-provider run is required.
