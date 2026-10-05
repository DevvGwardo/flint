# Flint improvement loop

## Pass rules

- Start with the current source, this ledger, and the working-tree status.
  Preserve existing dirty and untracked work.
- Pick a bounded issue with a reproducible cost or user-visible failure.
  Add regressions, implement the fix, and validate it before marking it done.
- Measure release builds for performance. Distinguish microbenchmarks from
  native app RSS, CPU, and frame cadence. Passing a budget is not a speedup.
- Keep the native dark-and-ember visual system. Prioritize readable spacing,
  accessible controls, keyboard operation, compact panes, and bounded rendering.
  Do not add decorative animation or idle repainting.
- Run native scripts sequentially, offline, with disposable homes/workspaces.
  Do not use real credentials, restart the user's app, commit, push, or publish.
- Finish the current pass before starting another. Keep failures and limits
  visible instead of adding them to the accomplished list.

## Pass 1: bounded output and modern composer

Status: completed for the fixes listed below. Code regressions, release
comparison, and composer screenshot review pass. The major-state sweep is
99/100; the remaining file-picker CPU cutoff also fails on the retained
before binary and remains an explicit open performance gap.

Evidence root: `target/composer-perf-ou43emv_/`. The `source-before/` snapshot
preserves this pass's starting files independently of the pre-existing changes.
Retained baseline binary: `flint-before`.

### Verified fixes

1. **Live output retention.** Bound UTF-8 output before copying oversized
   deltas into the tool card. Track newline counts incrementally rather than
   rescanning the retained buffer on each delta. Both the 256 KiB byte limit
   and 2,000-newline limit apply even to blank/very short lines. Late deltas
   cannot append to a completed tool's final result.
2. **Terminal forwarding.** Select the last 200 lines using a reverse iterator
   instead of allocating one reference for every line of scrollback. Preserve
   original order and existing LF/CRLF/Unicode behavior.
3. **Composer design.** Add restrained rounded controls, a focus-aware ember
   outline, explicit primary action states, keyboard hints, and a bounded
   live-task tray. Empty or preparing drafts cannot activate Send/Steer.
   Truncate attachment chips safely and cap the task output preview at 160
   characters before text layout. Add keyboard focus treatment to custom
   composer and removal controls. Preserve queue, stop, paste, session-local
   drafts, and compact-pane behavior.

### Verification

- App unit suite: 127 passed; two manual performance probes ignored.
- Composer refresh regressions: 2/2 passed.
- Compact eight-pane composer regression: passed.
- `cargo fmt --all -- --check`: passed.
- Offline/locked workspace, all-target strict Clippy: passed.
- Offline/locked full workspace tests, serial: passed. ACP 68, agent 150,
  app 127, UI 224 (148.34 seconds), terminal 17, integration and doc tests pass.
  Two manual performance probes are ignored in the normal suite and were run
  explicitly in release mode.
- Offline/locked release build: passed, 46.14 seconds.
- Queue/composer native captures: 4/4 passed. Inspected empty 900×560,
  working 1440×900, compact 900×560, and paused 1440×900 screenshots.
  Input, primary action, options, live tasks, and keyboard hints are readable.
  The compact queue and short welcome suggestions intentionally scroll.
- Major-state native sweep: 99/100. Every window/state, memory, screenshot,
  and panic check passes. The mention picker measures exactly 2.0% CPU,
  failing the strict `< 2.0%` cutoff. Retained the failed result without
  relaxing the budget or replacing it with a favorable rerun.
- Inspected fresh running, approval, and mention-menu screenshots. The pinned
  approval and composer remain separate, and the file picker overlays the
  welcome content without moving the input or overlapping its controls.
- Final formatting, strict Clippy, whitespace, and source SHA-256 comparison
  against the validated snapshot: passed. No source drift after validation.
- Initial offline native baseline: 10/10 budgets passed. Idle RSS 76.00 MB,
  200-turn RSS 85.58 MB, idle CPU 0.75%, streaming CPU 75.4%, streaming frame
  p95 12.36 ms, scrolling frame p95 14.16 ms.

### Measured outcomes

`hotpaths-paired.json` records five interleaved runs of retained release test
binaries, with each probe executed serially. Medians:

| Targeted workload | Before | After |
| --- | ---: | ---: |
| 10,000 small live-output chunks after a 128 KiB prefix | 20.947 ms | 1.295 ms |
| Retained buffer capacity after an 8 MiB live delta | 8,404,580 B | 262,144 B |
| Forward the tail of 1,000,000 terminal lines, 20 times | 170.537 ms | 0.086 ms |

The retained allocation drops about 97% for the oversized-delta fixture.
These are synthetic hotpath measurements, not whole-app RAM or throughput
claims, and do not measure the producer's temporary chunk allocation.

`native-paired/data/report-perf.json` passes 10/10 budgets using interleaved
before/after binaries:

| Native metric | Before | After |
| --- | ---: | ---: |
| Cold window, median of 7 | 225.04 ms | 227.07 ms |
| First painted frame, median of 7 | 162.60 ms | 173.90 ms |
| Idle RSS | 76.20 MB | 76.41 MB |
| RSS after 200 turns | 86.05 MB | 86.02 MB |
| Idle CPU | 0.50% | 0.50% |
| Streaming CPU, 2,000 deltas/s | 51.60% | 50.40% |
| Streaming frame p95 | 14.84 ms | 14.79 ms |
| Scrolling frame p95 | 15.32 ms | 15.17 ms |

General native RSS and frame cadence remain essentially unchanged; the first
paint measurement is slightly slower. No meaningful overall streaming speedup
is claimed. The initial standalone baseline above has a different cadence and
is not the comparison used for these outcomes.

The first compilation exposed missing palette bindings in the new shared
control styles; fixed. The short-window regression exposed a primary button
below the intended minimum height; it now has an explicit 36 px height and
passes. GPUI's observed Button wrapper did not expose disabled/name metadata;
the new tests use actual interaction and geometry rather than inferring
semantic compliance from missing observation data.

### Open limits

The paired mention diagnostic (`mention-paired/data/mention-paired.json`) uses
the same single-file disposable workspace and 2.5-second settle/3-second CPU
window as the sweep. Three interleaved before/after samples:

- Before: 2.000%, 1.667%, 2.000%.
- After: 1.667%, 1.667%, 2.000%.

Both binaries fail the strict cutoff in at least one sample. This is not a
new regression attributable to the composer changes, nor a resolved issue.
Investigate it in a later pass without weakening the threshold.

Final tool results and the producer/event/
persistence queues are not bounded by this live-card fix. It does not establish
global RAM limits. Physical macOS keyboard/clipboard and VoiceOver remain
unverified. No real inference is exercised.

## Pass 2: bounded command-header previews

Status: completed. Focused and full-workspace regressions, strict checks,
release comparison, and native performance/composer checks pass.

Evidence root: `target/command-preview-1ld082vr/`. Starting files are retained
under `source-before/`; `flint-before` is the validated Pass 1 release binary.
Previous pass's benchmark and native processes were confirmed finished before
this pass started.

### Verified fix

The display-only `ui::one_line` helper previously copied the entire first
nonempty line, even when the UI later clipped it. The baseline release probe
allocated 8,388,613 bytes for an oversized command-header preview.

Previews now retain at most 240 Unicode scalar values plus a continuation
ellipsis. They stop before copying the rest of a long line. Tool rows borrow
their command/path/name and build one preview for both their display and
accessible name instead of cloning and previewing the entire summary twice.
Nested tool summaries use bounded previews too. Terminal tab labels borrow
their fixed/program title before previewing it and retain their exit suffix.

The original summary and command arguments remain unchanged. Expanded command
details and terminal command actions still read the literal command, not the
display preview. Short commands, CRLF, whitespace, and existing multiline
continuation behavior are preserved.

### Focused verification

- Preview unit tests: 3 passed; one manual performance probe ignored.
- New command-preview UI tests: 2/2 passed. A long Unicode command fits its
  header and live task row, retains its full arguments, and expands. A 10,000-
  character mirrored terminal label is bounded and keeps `(exit 7)`.
- Existing multiline command, approval, task-tray, and terminal regressions:
  4/4 passed.
- Compact eight-pane composer regression: passed.
- Formatting and strict workspace/all-target Clippy: passed.
- Full offline/locked serial workspace suite: passed. ACP 68, agent 150,
  app 129 (three manual probes ignored), UI 226 in 155.34 seconds, terminal 17,
  integration and doc tests pass.
- Offline/locked release build: passed, 47.37 seconds.
- Interleaved offline native performance: 10/10 budgets passed.
- Native queue/composer captures: 4/4 passed. Inspected working, paused,
  compact 900×560, and empty 900×560 screenshots. Short command headers and
  the Pass 1 composer remain readable; no control overlap was found.
- Source hashes match the validated snapshot. Formatting, strict Clippy,
  and whitespace checks pass.

### Measured outcomes

`hotpath-paired.json` contains five interleaved runs of retained release test
binaries. For 100 display previews of an 8 MiB ASCII command:

| Microbenchmark | Before median | After median |
| --- | ---: | ---: |
| Preview generation | 34.034 ms | 0.047 ms |
| Returned preview allocation capacity | 8,388,613 B | 256 B |

Reproduce the final probe with
`cargo test --offline --locked --release -p flint-app --lib command_preview_perf_probe -- --ignored --nocapture --test-threads=1`.
The helper benchmark does not include GPUI shaping or full tool-row rendering.
Unicode previews have a bounded allocation too, but the 256 B figure above is
specifically the ASCII fixture, not a universal per-header byte limit.

`native-paired/data/report-perf.json` uses interleaved before/after binaries:

| Native metric | Before | After |
| --- | ---: | ---: |
| Idle RSS | 76.28 MB | 76.19 MB |
| RSS after 200 turns | 85.96 MB | 86.03 MB |
| Idle CPU | 0.50% | 0.50% |
| Streaming CPU | 50.40% | 49.40% |
| Streaming frame p95 | 14.09 ms | 14.07 ms |
| Scrolling frame p95 | 13.74 ms | 13.71 ms |
| First painted frame, median of 7 | 171.38 ms | 187.64 ms |

General RSS and frame cadence remain essentially unchanged. The first-frame
median is higher, although well inside the startup budget. No overall app
speedup is claimed from these native runs. The improvement is the bounded
allocation and work for pathological long command previews.

### Open limits

This bounds display previews, not retained model
strings, incoming events, full expanded commands, or arbitrary tool-output
lines. Unicode scalar boundaries remain valid UTF-8 but are not a guarantee
that a cut falls on a complete grapheme cluster. Native captures use normal
demo commands; oversized Unicode command and terminal-label behavior is
verified by headless real-view regressions, not physical keyboard/VoiceOver
testing. The Pass 1 mention-picker CPU gap remains open; its major-state sweep
was not rerun or represented as passing in this pass.

## Pass 3: avoid discarded event copies

Status: implementation and validation complete. Saved event ownership,
failure retention/retry, ordered streaming, and the existing composer UI
remain covered by passing regressions and native checks.

Evidence root: `target/event-copy-fzhglk3i/`. Starting sources are retained in
`source-before/`, and `flint-before` is the validated Pass 2 release binary.
Earlier benchmark/native processes were confirmed finished before this pass.
`source-benchmark-before/` preserves the eager-copy helper/probe that models
the original call site. `source-after/` and `source-after.sha256` retain the
validated code, and both release app/test binaries are preserved.

### Implemented change

`apply_events` previously cloned every event before calling `Session::log`,
even when an unsaved session immediately discarded the record. The release
baseline takes 10.595 ms to log-and-discard 100 events with 8 MiB payloads.

The new borrowed `Session::log_event` checks the session's saved directory
before making an owned copy. Unsaved sessions return without copying the
payload. Saved sessions still use the existing `log`/writer code, with the
same ordered ownership, error checks, retained failures, and flush barriers.
The UI still folds the original event after the logging attempt.

The decision uses the per-session directory, not a global automation flag.
Explicitly saved offline stream fixtures must keep persisting their events.

### Verification

- Three new logging unit regressions pass: no unsaved storage/consumption,
  independently owned saved records and ordered flushes, and failed writes
  retained/retried without duplicates.
- The new real-view unsaved event-pump regression passes: ordered text deltas
  still reach the transcript and the turn completes without a saved directory.
- Existing background persistence-failure/shutdown and late-event/archive
  regressions: 2/2 passed.
- Formatting and strict workspace/all-target Clippy: passed.
- Full offline/locked serial workspace suite: passed. ACP 68, agent 150,
  app 132 (four manual probes ignored), UI 227 in 152.31 seconds, terminal 17;
  integration and doc tests pass. The new manual probe was run separately
  in release mode.
- Offline/locked release build: passed, 45.32 seconds.
- Interleaved offline native performance: 10/10 budgets passed.
- Explicitly saved native streams at 2,000 deltas/s: 19/19 checks passed.
  All accepted events were persisted: 10,003, 10,003, and 10,001 records.
  `saved-stream-sequence.json` additionally verifies every alternating
  text/tool delta is contiguous and ordered, with no missing or duplicate
  delta numbers (9,999, 9,999, and 9,997 deltas).
- Native queue/composer captures: 4/4 passed. Inspected working, paused,
  compact 900×560, and empty 900×560 screenshots, plus the saved-stream
  screenshot. No composer control overlap was found.

### Measured outcomes

`hotpath-paired.json` contains five alternating runs of retained release test
binaries. For 100 unsaved logging calls with an 8 MiB tool-output payload:

| Helper microbenchmark | Before median | After median |
| --- | ---: | ---: |
| Event log-and-discard | 9.827 ms | <0.001 ms (reporting precision) |

Every after sample reports rounded `0.000 ms`. This is not zero runtime or
evidence for a precise speedup ratio. The probe excludes event folding,
GPUI rendering, saved logging, allocation counts, and total app RSS.
Reproduce the final probe with
`cargo test --offline --locked --release -p flint-app --lib unsaved_event_logging_perf_probe -- --ignored --nocapture --test-threads=1`.

`native-paired/data/report-perf.json` uses interleaved before/after binaries:

| Native metric | Before | After |
| --- | ---: | ---: |
| Idle RSS | 76.11 MB | 76.45 MB |
| RSS after 200 turns | 85.95 MB | 86.08 MB |
| Idle CPU | 0.75% | 0.75% |
| Streaming CPU | 61.60% | 64.40% |
| Streaming frame p95 | 14.54 ms | 14.62 ms |
| Scrolling frame p95 | 15.03 ms | 14.77 ms |
| First painted frame, median of 7 | 195.05 ms | 191.57 ms |

RSS and frame cadence remain essentially unchanged. Streaming CPU is higher
in this sample, still within budget. No overall app speedup is claimed.
The verified improvement is avoiding discarded owned payload copies.

The saved-stream checks are after-only durability checks, not a paired
performance comparison. Their frame p95 values are 14.80, 14.70, and
14.69 ms; event-batch maxima remain below 0.78 ms. Native scripts ran
sequentially with disposable homes/workspaces, and all fixture processes
exited before the final helper comparison.

### Open limits

The optimization is for unsaved sessions, demos, and
ephemeral workloads. Normal saved conversations still require owned event
records; their copies, event queues, writer retention, and incoming payload
sizes are not reduced or globally bounded by this change. No whole-app RAM
or saved-session streaming speedup is inferred from the helper benchmark.
The mention-picker CPU gap remains open; its major-state sweep was not
rerun or represented as passing. Live-provider, physical keyboard/clipboard,
and VoiceOver behavior were not tested in this pass.

## Pass 4: count tool arguments without a temporary JSON string

Status: implementation and validation complete. Exact token-estimate behavior
and retained arguments are covered by focused regressions; full workspace,
release, strict checks, and sequential native checks pass.

Evidence root: `target/argument-count-zgmgkz2z/`. Starting sources, git status,
the validated Pass 3 app binary, eager-count benchmark sources, and release
test binaries are retained. The original Pass 1 baseline process and the
Pass 3 validation process were confirmed exited before measuring.

### Implemented change

On each tool start, `SessionView::fold` serialized the complete argument
value into a temporary `String` just to count Unicode scalars for its token
estimate. Large edit content therefore acquired an avoidable serialized copy.

`serialized_chars` now sends the same compact JSON serialization to a
counting writer, retaining only a scalar count. ASCII chunks use their byte
length; non-ASCII chunks count UTF-8 leading bytes. Split UTF-8 scalars and
JSON escape sequences keep the same count as `args.to_string().chars().count()`.
The original arguments still move into the tool call without truncation or
mutation. The output-token estimate and turn-start reset remain unchanged.

No persistent cache, new repaint timer, or public API was added.

### Focused verification and measurements

- Differential JSON/fold regression passes for null, booleans, numbers,
  nested data, Unicode keys, combining characters, escaped control characters,
  and large Unicode edit content. The original arguments remain intact.
- Split-UTF-8, empty-write, and flush regression passes.
- Existing token-estimate/reported-usage regression passes.
- Formatting and strict workspace/all-target Clippy pass.
- Full offline/locked serial workspace suite passes: ACP 68, agent 150,
  app 134 (five manual probes ignored), UI 227 in 153.08 seconds, terminal 17;
  integration and doc tests pass. The new ignored probe ran separately in
  release mode.
- Offline/locked release build passes, 46.31 seconds.
- Interleaved offline native performance: 10/10 budgets passed.
- Native queue/composer captures: 4/4 passed. Inspected working, paused,
  compact 900×560, and empty 900×560 screenshots; no composer control
  overlap was found.

`hotpath-paired.json` contains five alternating retained-binary samples for
100 counts of an 8 MiB ASCII JSON content field:

| Helper probe | Before median | After median |
| --- | ---: | ---: |
| Character-count time | 244.266 ms | 231.879 ms |
| Peak process RSS | 34.75 MiB | 26.63 MiB |

RSS comes from Darwin `/usr/bin/time -l`, not an allocation counter or the
native app. The 8.125 MiB reduction includes the probe process and fixture,
not only the helper. The fixture allocation lies outside the timing boundary.
These measurements exclude event folding, saved-event copies, GPUI rendering,
and normal command sizes. Unicode/escaping correctness is verified, but its
runtime and RSS have not been paired.

The first writer prototype reduced RSS but slowed this probe from 241.059
to 297.182 ms. It was not retained as the final implementation; the ASCII
fast path removes that observed slowdown. Its sources, release binary, and
paired data remain in `prototype-view_model.rs`, `hotpath-prototype-test`,
and `prototype-paired.json`.

Reproduce the final probe with
`cargo test --offline --locked --release -p flint-app --lib argument_count_perf_probe -- --ignored --nocapture --test-threads=1`.

`native-paired/data/report-perf.json` contains the interleaved app comparison:

| Native metric | Before | After |
| --- | ---: | ---: |
| Idle RSS | 76.36 MB | 76.12 MB |
| RSS after 200 turns | 86.08 MB | 85.77 MB |
| Idle CPU | 0.75% | 0.75% |
| Streaming CPU | 59.80% | 59.60% |
| Streaming frame p95 | 13.97 ms | 13.78 ms |
| Scrolling frame p95 | 14.12 ms | 14.14 ms |
| First painted frame, median of 7 | 188.53 ms | 187.57 ms |

Normal native RSS and frame cadence are essentially unchanged. No whole-app
speedup or meaningful native RAM reduction is claimed. Native scripts ran
sequentially with disposable homes/workspaces; all fixture processes exited.

### Open limits

This removes the temporary
serialized copy, not the retained argument value. It still serializes and
scans the complete arguments; incoming sizes, saved event ownership, writer
retention, and transcript history remain outside its scope. No whole-app
speedup or RSS reduction is claimed from the helper probe.
The mention-picker CPU gap remains open; the major-state sweep and saved-stream
durability workload were not rerun. Live-provider, physical keyboard/clipboard,
and VoiceOver behavior were not tested.

## Pass 5: correct Unicode mention-picker ranking

Status: ranking fix and validation complete. Full workspace, release probes,
strict checks, and native layout/performance checks pass. The separate
mention-picker idle-CPU gap remains unresolved.

Evidence root: `target/unicode-mention-4nqslv85/`. Starting sources, git status,
and the validated Pass 4 release binary are preserved. The original baseline
and previous validation processes were confirmed exited before this pass.

### Reproduced gap and implemented change

The fuzzy scorer computed the filename's starting position as a UTF-8 byte
offset, then compared it with Unicode scalar indexes in its matching loop.
Multibyte directory names, including lowercase expansions such as `İ`,
could therefore suppress the filename-match bonus.

Both baseline regressions failed on the original source:

- `regression-before.log`: `目录/X.rs` scored 489 instead of 789.
- `ui-regression-before.log`: the real picker ranked `x/note.rs` ahead of
  `目录/x.rs` for `@X`, selecting the directory-only match by default.

The filename boundary now uses an index into the same character vector as
the matching loop. Case-insensitive matching, original path strings, byte-length
tie-breaking, the top-eight limit, and attachment contents remain unchanged.
No UI timer, animation, persistent cache, or public API was added.

### Verification

- All seven mention unit tests pass. The new regression covers CJK, accented,
  dotted-I, emoji, and nested Unicode directories, plus filename-vs-directory
  ranking.
- The new real-view regression passes at 1440×900 and 900×560: the filename
  match is first, its selected row fits the picker, Enter retains the exact
  Unicode attachment path, and submitting sends the selected file contents
  without the directory-only fixture's contents.
- Existing inline picker, project-menu picker, gitignore, and asynchronous
  indexing/workspace-switch tests pass.
- Formatting and strict workspace/all-target Clippy pass.
- Full offline/locked serial workspace suite passes: ACP 68, agent 150,
  app 135 (five manual probes ignored), UI 228 in 152.68 seconds, terminal 17;
  integration and doc tests pass.
- All five existing manual release probes pass: bounded live output,
  terminal-tail selection, command previews, unsaved logging, and argument
  counting. These are smoke/correctness runs, not new paired speedup evidence.
- Offline/locked release app build passes, 46.62 seconds.
- Interleaved offline native performance: 10/10 budgets passed.
- Native queue/composer captures: 4/4 passed. Inspected working, paused,
  compact, and empty composer screenshots; no control overlap was found.
- Native mention captures: 8/8 launch/state/nonblank/no-panic checks passed
  at 1440×900 and 900×560. Inspected both screenshots: Unicode directory
  labels render and the unchanged picker fits below the welcome composer.
  These capture checks do not include an idle-CPU budget verdict.

### Native observations and open limits

`native-paired/data/report-perf.json` records the app comparison:

| Native metric | Before | After |
| --- | ---: | ---: |
| Idle RSS | 76.14 MB | 75.94 MB |
| RSS after 200 turns | 85.77 MB | 85.67 MB |
| Idle CPU | 0.75% | 0.50% |
| Streaming CPU | 57.80% | 59.40% |
| Streaming frame p95 | 14.09 ms | 14.05 ms |
| Scrolling frame p95 | 13.99 ms | 14.09 ms |
| First painted frame, median of 7 | 188.38 ms | 193.65 ms |

These runs show essentially unchanged native RSS/frame cadence, not a
performance improvement attributable to the ranking correction.

The after-only empty-query picker samples in
`native-mention/data/report-mention-captures.json` are:

| Picker size | Settled RSS | Idle CPU over 3 seconds | Strict `<2.0%` limit |
| --- | ---: | ---: | --- |
| 1440×900 | 78.75 MB | 2.67% | Over budget |
| 900×560 | 77.14 MB | 1.67% | This sample below budget |

The normal-size diagnostic remains over budget. A passing compact sample
does not close the known finding; neither these samples nor the capture
checks are represented as a passing major-state CPU sweep. The full sweep
and saved-stream durability workload were not rerun. All native scripts
ran sequentially with disposable homes/workspaces, and fixture processes
exited.

This is a reproduced ranking/keyboard UX
fix, not a performance optimization. No speedup or RAM reduction is claimed.
The separate mention-picker idle-CPU finding remains open. Native captures
open the picker with an empty query; queried ranking and keyboard attachments
are verified in headless real views, not physical keyboard or VoiceOver tests.
No live-provider or physical clipboard tests were run.

## Pass 6: contain long picker labels

Status: layout fix and validation complete. Full workspace, release probes,
strict checks, and native comparisons pass. The separate strict picker
idle-CPU limit still fails at normal size.

Evidence root: `target/picker-overflow-hzkygiyg/`. Starting sources, git status,
and the validated Pass 5 app binary are preserved. The original baseline
and previous validation processes were confirmed exited before this pass.

### Reproduced gap and implemented change

A valid 223-byte basename and a long inline `@` query overflowed the picker.
`regression-before.log` records a 1,807-pixel filename in a 734-pixel row.
In the 760-pixel panel, the keyboard hint ended at x=1777 even though the
panel ended at x=1252, pushing attachment/close instructions out of view.

The shared mention/agent/slash header now gives its title a shrinkable,
single-line ellipsis area and caps the hint's share on narrow panels. The
full hint remains in its hover tooltip. Mention rows constrain filenames
to leave space for directory context, keep the file icon fixed, and clip
overflow at the row. Full paths remain in the row's hover tooltip and
accessible button label, and click/keyboard actions retain the original path.

The existing dark-and-ember palette and row dimensions remain unchanged.
No custom animation, recurring timer, or background repaint task was added.

### Focused verification

- The new real-view regression passes at 1440×900, 900×560, and a narrower
  600×560 headless window. Labels stay single-line and within the picker;
  selecting/submitting retains the complete Unicode path and file contents.
- At the normal 760-pixel picker width, the filename measures 500 pixels
  instead of 1,807. The keyboard hint now ends at x=1235 inside x=1252.
  These are geometry measurements, not runtime or memory benchmarks.
- Existing Unicode ranking/attachment, selected slash-command scrolling,
  composer queue/stop, and compact eight-pane regressions pass.
- Formatting and strict workspace/all-target Clippy pass. Two unnecessary
  expected-value clones in the new test were removed after Clippy flagged them.
- Full offline/locked serial workspace suite passes: ACP 68, agent 150,
  app 135 (five manual probes ignored), UI 229 in 153.03 seconds, terminal 17;
  integration and doc tests pass.
- All five existing manual release probes pass separately. These smoke runs
  are not new paired speedup evidence.
- Offline/locked release app build passes, 46.22 seconds.
- Interleaved offline native performance: 10/10 budgets passed.
- Native queue/composer captures: 4/4 passed. Inspected working, paused,
  compact, and empty states; no composer control overlap was found.
- Paired long-name picker captures: 16/16 launch/state/nonblank/no-panic
  checks passed. Inspected before/after images at 1440×900 and 900×560:
  the baseline filename runs past the row and hides directory context;
  the updated filename ellipsizes inside the row, leaving `目录` visible.
  These capture checks exclude an idle-CPU budget verdict.

### Native observations

`native-paired/data/report-perf.json` records the general app comparison:

| Native metric | Before | After |
| --- | ---: | ---: |
| Idle RSS | 76.14 MB | 76.05 MB |
| RSS after 200 turns | 85.77 MB | 85.73 MB |
| Idle CPU | 0.75% | 0.75% |
| Streaming CPU | 59.80% | 61.20% |
| Streaming frame p95 | 14.06 ms | 14.12 ms |
| Scrolling frame p95 | 14.17 ms | 14.17 ms |
| First painted frame, median of 7 | 192.12 ms | 189.22 ms |

RSS/frame cadence remain essentially unchanged; streaming CPU is somewhat
higher in this sample, still inside budget. No general speedup is inferred.

`native-picker/data/report-picker-overflow.json` contains the short
before/after empty-query long-name picker samples:

| Picker size | Before RSS | After RSS | Before idle CPU | After idle CPU |
| --- | ---: | ---: | ---: | ---: |
| 1440×900 | 78.09 MB | 79.16 MB | 2.67% | 2.00% |
| 900×560 | 76.33 MB | 76.98 MB | 1.67% | 1.33% |

The normal-size after sample is exactly 2.00%, which **fails** the strict
`<2.0%` limit. Single compact samples below the cutoff do not close the
finding. The extra hover/accessible labels do not establish a memory saving;
picker RSS is higher in these samples. Native scripts ran sequentially with
disposable homes/workspaces, and all fixture processes exited.

### Open limits

Native long-name
captures use an empty query; long-query/header behavior is checked in real
headless views. Display truncation does not bound retained model strings or
GPUI text-shaping work. No speedup or RAM reduction is claimed.

The mention-picker idle-CPU gap remains open. Full major-state and saved-stream
durability sweeps, physical keyboard/clipboard, and VoiceOver testing are
outside this pass. Full-path hover text and accessible names are configured,
but physical hover/assistive-technology behavior was not verified.

## Pass 7: reuse normalized mention queries

Status: query-reuse optimization and validation complete. Full workspace,
release probes, strict checks, paired native performance, and composer
captures pass. No native idle-CPU or whole-app RAM improvement is claimed.

Evidence root: `target/picker-query-y8nostlu/`. Starting sources, git status,
the validated Pass 6 app binary, eager-normalization benchmark sources,
retained release test binaries, and source hashes are preserved. Previous
baseline/validation processes were confirmed exited before measuring.

### Measured gap and implemented change

Each picker search called `score` for every candidate, lowercasing the same
query repeatedly across as many as 20,000 indexed files. `search` now
normalizes once and borrows that string throughout scoring. The public
standalone scorer still normalizes its own input, and the matching/ranking
algorithm remains unchanged.

An empty file index returns before normalizing, preserving the prior
empty-index fast path. This adds no persistent index/cache, UI timer,
animation, dependency, or public API.

### Focused verification and helper measurements

- Eight mention unit tests pass. The new differential test checks full
  top-eight ordering against independently invoked standalone scores for
  mixed case, Unicode directories, dotted-I expansions, accented/combining
  text, Greek sigma, emoji, empty queries/indexes, and no-match queries.
  Original candidate strings remain unchanged.
- Existing Unicode filename ranking/keyboard attachment and long-query/
  filename-containment real-view regressions pass.
- Formatting and strict workspace/all-target Clippy pass.

`hotpath-paired.json` contains five alternating retained-release-binary
samples. Each query runs 20 searches over 20,000 mixed-case paths with CJK
directories; candidate creation is outside the timing boundary.

| Query, 20 searches | Before median | After median |
| --- | ---: | ---: |
| `MAIN` | 72.082 ms | 65.173 ms |
| `module_1` | 71.766 ms | 65.297 ms |
| `目录` | 69.227 ms | 59.623 ms |
| `zzzz` (no match) | 66.817 ms | 60.286 ms |

These measure `mention::search`, not file indexing, GPUI input callbacks,
text shaping, idle CPU, allocation counts, or whole-app RAM. The verified
improvement is less repeated query-normalization work during filtering;
no whole-app speedup or memory reduction is inferred.

Reproduce the final probe with
`cargo test --offline --locked --release -p flint-app --lib mention_search_perf_probe -- --ignored --nocapture --test-threads=1`.

### Full and native validation

- Locked/offline serial workspace tests pass: ACP 68, agent 150, app 136
  (six manual probes ignored), UI 229 in 152.80s, terminal 17; remaining
  integration and doc tests pass. All six release probes pass separately.
- Final formatting, strict workspace/all-target Clippy, and whitespace
  checks pass. The release app builds in 46.90s.
- Sequential isolated native performance checks pass **10/10**, paired
  against the preserved, hash-matched Pass 6 release binary.
- Native composer/queue state and screenshot checks pass **4/4**. All four
  captures were inspected: normal/compact layouts preserve the existing
  queue separation, scroll containment, and dark-and-ember composer.
- Current scorer/test sources match the retained measured snapshots. Native
  and helper binary hashes and the final test/report summary are preserved
  in `validation-summary.json`. The owned validation process has exited.

| Paired native metric | Before | After |
| --- | ---: | ---: |
| Idle RSS | 76.08 MB | 76.12 MB |
| 200-turn RSS | 85.88 MB | 85.80 MB |
| Empty-state idle CPU | 0.75% | 0.75% |
| Streaming p95 frame interval | 13.92 ms | 13.89 ms |
| Scrolling p95 frame interval | 14.04 ms | 14.06 ms |

These native differences do not establish a whole-app performance or RAM
improvement. Physical keyboard, clipboard, hover, VoiceOver, and live-provider
behavior were not exercised. All native sessions used disposable
homes/workspaces; no user-app restart, real credentials, commit, push, live
provider, overlapping pass, or additional loop was used.

### Remaining limits

Candidate paths are still lowercased and converted
to character vectors per search, and the hits vector may retain all matches
before selecting the top eight. Incoming query/path sizes are not bounded
by this optimization.

The normal-size picker idle-CPU gap remains open; this changes active
filtering, not focus/caret repaint scheduling. The last picker sample remains
Pass 6's **2.00%**, failing the strict **<2.0%** requirement; it was not
remeasured here. The major-state CPU and saved-stream durability sweeps are
outside this pass.

## Pass 8: borrow failure text for status projection

Status: borrowed failure projection and validation complete. Full workspace,
release probes, strict checks, paired native performance, and composer
captures pass. No whole-app performance or RAM gain is claimed.

Evidence root: `target/failure-status-f5r2bw_o/`. Targeted starting sources,
git status, the Pass 7 release binary, benchmark sources, paired release test
binaries, raw measurements, and measured-source snapshots are preserved.
The old baseline PID 69769 and Pass 7 validation PID 4964 were confirmed
exited before measuring. Pass 1 remains complete, not an overlapping task.

### Measured gap and implemented change

`Session::status` called the owned `failure()` API just to check whether a
failure existed, cloning the entire error. A failed `status_line` then copied
the error again before selecting its first line. Sidebar classification,
filtering, grouping, and row rendering call these methods.

A private borrowed `failure_text` now selects the same preferred source:
startup/idle error, failed turn, then the static step-limit message. Both
status methods borrow it. The public `failure() -> Option<String>` still
returns an independently owned, complete value when callers request one.
Classification precedence, unread/seen state, tone, first-line trimming,
queue behavior, and retained detailed errors are unchanged.

No persistent cache, new UI element, timer, animation, dependency, or public
API change is introduced.

### Focused verification and helper measurements

- Two new unit regressions prove source-buffer borrowing, idle-error
  precedence, owned-return independence, complete retained Unicode details,
  approval/running precedence, seen/unread status, step-limit fallback,
  absence of failures, and empty/multiline/CRLF first-line behavior.
- Existing failed/stopped/starting status regressions pass.
- A new real-view regression checks a large retained error's status text and
  sidebar containment at 1440×900 and 1000×560, without projecting the
  detailed tail into the sidebar.
- Formatting, strict workspace/all-target Clippy, and whitespace checks pass.

`hotpath-paired.json` contains five alternating retained-release-binary
samples. Each source holds 8 MiB of error details after a short Unicode first
line. Fixture creation is outside both timing boundaries.

| Helper, 100 calls | Before median | After median |
| --- | ---: | ---: |
| Idle-error classification | 10.013 ms | <0.001 ms |
| Idle-error status line | 19.932 ms | 0.012 ms |
| Failed-turn classification | 10.067 ms | <0.001 ms |
| Failed-turn status line | 20.001 ms | 0.007 ms |

Classification prints `0.000 ms` after the change, below the probe's
0.001 ms reporting precision, **not zero runtime**. These are synthetic
helper measurements, not typical-error, native-input, rendering, or
whole-app improvements. Allocation counts and peak RSS were not measured;
the source-level fix removes unnecessary detailed-error copies.

Reproduce with
`cargo test --offline --locked --release -p flint-app --lib failure_status_perf_probe -- --ignored --nocapture --test-threads=1`.

### Full and native validation

- Locked/offline serial workspace tests pass: ACP 68, agent 150, app 138
  (seven manual probes ignored), UI 230 in 152.48s, terminal 17; remaining
  integration and doc tests pass. All seven release probes pass separately.
- Composer-refresh, compact eight-pane, queue/stop/failure, and persistence
  regressions pass within the full suite.
- Final formatting, strict workspace/all-target Clippy, and whitespace
  checks pass. The release app builds in 47.08s.
- Sequential isolated native performance checks pass **10/10**, paired
  against the preserved, hash-matched Pass 7 release binary.
- Native composer/queue state and screenshot checks pass **4/4**. All four
  captures were inspected; the existing dark-and-ember composer, queue/task
  separation, and compact scroll containment remain intact.
- Current modified code/test files match their measured snapshots. Native
  and helper binary hashes, report summaries, and limitations are in
  `validation-summary.json`. The owned validation process has exited.

| Paired native metric | Before | After |
| --- | ---: | ---: |
| Idle RSS | 76.16 MB | 76.06 MB |
| 200-turn RSS | 85.67 MB | 85.45 MB |
| Empty-state idle CPU | 0.75% | 0.75% |
| Streaming p95 frame interval | 13.93 ms | 13.85 ms |
| Scrolling p95 frame interval | 13.96 ms | 14.08 ms |

These general native scenes do not measure large failed-session projection.
The small native differences do not establish a whole-app speed or memory
improvement. Native sessions used disposable homes/workspaces, without
restarting the user's app, real credentials, live providers, commits,
pushes, overlapping passes, or an additional loop.

### Remaining limits

Full errors remain stored and the owned API intentionally still copies.
A newline-free first error line can still be arbitrarily long; it is not
bounded or shortened by this behavior-preserving pass.

The picker idle-CPU finding remains open: the last normal-size sample,
Pass 6's **2.00%**, still fails **<2.0%** and was not remeasured here.
Physical keyboard, clipboard, hover, VoiceOver, live providers, the
major-state CPU sweep, and the saved-stream durability sweep are outside
this pass.

## Pass 9: bound collapsed tool-output detail

Status: bounded detail projection and validation complete. Full workspace,
release probes, strict checks, paired native performance, and composer
captures pass. No whole-app speed or RAM gain is claimed.

Evidence root: `target/tool-detail-jl0wlki7/`. Starting sources/status, the
Pass 8 app binary, equivalent pre-fix projection/marker benchmark sources,
retained release test binaries, the failed baseline regression, raw paired
samples, and final measured-source snapshots are preserved. Old baseline
PID 69769 and Pass 8 validation PID 38531 were confirmed exited first.

### Proven gap and implemented change

Collapsed command/search/failed-edit detail text was copied in full and sent
to a visually truncated div. Visual ellipsis did not bound copying or text
shaping. The unit baseline failed the 240-scalar preview bound for a long
Unicode output line.

Collapsed detail now uses the existing bounded `ui::one_line` projection:
240 Unicode scalars plus a continuation hint when necessary. The command
uses the same newest nonblank, non-marker line; search and failed edits keep
the first line; read details keep their line count. A single borrowed command
line no longer needs a temporary one-element vector.

The shared exit-code-line predicate also allocated/lowercased entire output
lines merely to compare `[exit code:`. It now compares the borrowed ASCII
prefix case-insensitively and checks the closing bracket, preserving the
previous trim/case/pattern rule.

Original outputs, command arguments/actions, expansion, result metadata,
line-selection rules, fonts, colors, and layout remain intact. No persistent
cache, UI timer, animation, dependency, or public API was added.

### Focused validation and helper measurements

- Three new unit regressions pass: Unicode/scalar/capacity bounds,
  command-tail/head/blank/marker/read-count selection, and differential
  marker equivalence across ASCII casing, Unicode/whitespace boundaries,
  malformed patterns, and a large non-marker line.
- A real-view regression passes for command/search/failed-edit detail at
  1440×900 and 900×560. It checks containment, expansion, original output,
  and unchanged success/failure metadata.
- Existing command/terminal-preview and literal-command regressions pass.
- Formatting, strict workspace/all-target Clippy, and whitespace checks pass.

An initial test module glob imported GPUI's `test` macro and caused recursive
attribute expansion. Explicit imports fixed it before the actual failing
baseline and release probe; `setup-macro-import-error.log` retains that
setup diagnostic. `regression-before.log` contains the intended bound
assertion failure, not a compilation failure.

`hotpath-paired.json` contains five alternating retained-release-binary
samples, each projecting the same newline-free 8 MiB ASCII output 100 times.
Input creation is outside timing. The baseline helper retains the original
copying and marker behavior, including its one-element vector.

| Detail projection, 100 calls | Before median | After median |
| --- | ---: | ---: |
| Command | 57.250 ms | 21.384 ms |
| Search | 33.692 ms | 22.282 ms |
| Failed edit | 33.563 ms | 22.153 ms |

Returned preview capacity is **8,388,608 → 256 bytes** for all three ASCII
cases. This is a measured temporary-string capacity reduction, not a peak
process RSS or whole-app RAM result. The helper timings exclude GPUI
shaping/rendering; no native-input or whole-app speedup is inferred.

Reproduce with
`cargo test --offline --locked --release -p flint-app --lib collapsed_detail_perf_probe -- --ignored --nocapture --test-threads=1`.

### Full and native validation

- Locked/offline serial workspace tests pass: ACP 68, agent 150, app 141
  (eight manual probes ignored), UI 231 in 153.88s, terminal 17; remaining
  integration and doc tests pass. All eight release probes pass separately.
- Composer-refresh, compact eight-pane, queue/stop/failure, literal-command,
  and persistence regressions pass within the full suite.
- Final formatting, strict workspace/all-target Clippy, and whitespace
  checks pass. The release app builds in 47.18s.
- Sequential isolated native performance checks pass **10/10**, paired
  against the preserved, hash-matched Pass 8 release binary.
- Native composer/queue state and screenshot checks pass **4/4**. All four
  captures were inspected: dark-and-ember styling, queue/task separation,
  and compact scroll containment remain intact.
- Current modified code/test files match the measured snapshots. Native
  and helper binary hashes, reports, raw paired samples, and limitations are
  preserved in `validation-summary.json`. The owned validation process exited.

| Paired native metric | Before | After |
| --- | ---: | ---: |
| Idle RSS | 75.91 MB | 76.12 MB |
| 200-turn RSS | 85.34 MB | 85.88 MB |
| Empty-state idle CPU | 0.75% | 0.75% |
| Streaming p95 frame interval | 14.03 ms | 14.16 ms |
| Scrolling p95 frame interval | 14.46 ms | 14.29 ms |

These general native scenes do not measure the oversized collapsed-detail
case. Native RAM is not lower and frame timing does not establish an overall
speedup. Sessions used disposable homes/workspaces, without restarting the
user's app, real credentials, live providers, commits, pushes, overlapping
passes, or an additional loop.

### Remaining limits

Line selection and read counts still scan the retained output as needed.
Stored/final output is not globally bounded; expanded and live-tail output
lines remain literal and can be long. This cap applies only to the collapsed
single-line detail.

Picker idle CPU remains open: the last normal-size sample, Pass 6's
**2.00%**, still fails **<2.0%** and was not remeasured here. Physical keyboard,
clipboard, hover, VoiceOver, live providers, the major-state CPU sweep, and
saved-stream durability sweep are outside this pass.

## Pass 10: stream mention-path scoring

Status: streaming scorer and validation complete. Full workspace, release
probes, strict checks, paired native performance, and composer captures
pass. No whole-app speed or RAM gain is claimed.

Evidence root: `target/mention-stream-rhz3m895/`. Starting sources/status,
the Pass 9 app binary, retained release test binaries, raw paired samples,
and measured-source snapshots are preserved. Old baseline PID 69769 and
Pass 9 validation PID 72272 were confirmed exited before measuring.

### Measured gap and bounded change

Every nonempty-query candidate built a temporary `Vec<char>` for its entire
lowercased path, although subsequence matching only moves forward. Scoring
now iterates `char_indices` directly, tracking the previous path scalar and
the previous match's UTF-8 end. Filename boundaries and match positions use
byte offsets from the same lowercased string, preserving the Unicode
filename fix without mixing byte/scalar positions.

The weights, greedy subsequence matching, word-start and adjacency bonuses,
original path-byte-length tie break, lowercasing behavior, standalone score
API, normalized-query reuse, empty-index fast path, and top-eight selection
remain unchanged. The lowercase path string still exists; this removes its
additional character-vector buffer, not all per-path allocation.

No persistent cache/index, UI element, timer, animation, dependency, or
public API change is added.

### Focused verification and measurements

- A frozen pre-change vector scorer supplies an independent differential
  oracle. The new regression compares **9,156 exact scores** (436 paths ×
  21 queries) and complete top-eight ordering for empty/repeated/delimiter
  matches, Unicode directories, combining accents, dotted-I case expansion,
  Greek sigma, multibyte adjacency, trailing slashes, and no matches.
  Candidate strings remain unchanged.
- Nine mention unit tests pass, including the existing exact Unicode
  filename-bonus regression and 20,000-file full-ranking comparison.
- Existing Unicode keyboard attachment and long-query/filename containment
  real-view regressions pass.
- Formatting, strict workspace/all-target Clippy, and whitespace checks pass.

`hotpath-paired.json` contains five alternating retained-release-binary
samples. Each query runs 20 searches over 20,000 mixed-case paths with CJK
directories; fixture creation is outside the timing boundary.

| Query, 20 searches | Before median | After median |
| --- | ---: | ---: |
| `MAIN` | 63.192 ms | 41.122 ms |
| `module_1` | 63.036 ms | 40.985 ms |
| `目录` | 58.599 ms | 34.703 ms |
| `zzzz` (no match) | 59.968 ms | 38.362 ms |

These measure search helpers, not file indexing, GPUI input callbacks,
rendering, idle CPU, allocation counts, or peak/whole-app RSS. The verified
speedup is specific to this active-filtering fixture. Removal of the
character vector is visible in the implementation; no numerical process
memory saving or overall application speedup is inferred.

Reproduce with
`cargo test --offline --locked --release -p flint-app --lib mention_search_perf_probe -- --ignored --nocapture --test-threads=1`.

### Full and native validation

- Locked/offline serial workspace tests pass: ACP 68, agent 150, app 142
  (eight manual probes ignored), UI 231 in 155.49s, terminal 17; remaining
  integration and doc tests pass. All eight release probes pass separately.
- Composer-refresh, compact eight-pane, queue/stop/failure, Unicode picker,
  literal-command, and persistence regressions pass within the full suite.
- Final formatting, strict workspace/all-target Clippy, and whitespace
  checks pass. The release app builds in 47.67s.
- Sequential isolated native performance checks pass **10/10**, paired
  against the preserved, hash-matched Pass 9 release binary.
- Native composer/queue state and screenshot checks pass **4/4**. All four
  captures were inspected; dark-and-ember styling, queue/task separation,
  and compact scroll containment remain intact.
- Current scorer/test sources match their measured snapshots. Native/helper
  binary hashes, reports, raw paired samples, and limitations are preserved
  in `validation-summary.json`. The owned validation process has exited.

| Paired native metric | Before | After |
| --- | ---: | ---: |
| Idle RSS | 76.19 MB | 76.19 MB |
| 200-turn RSS | 85.85 MB | 85.95 MB |
| Empty-state idle CPU | 0.75% | 0.75% |
| Streaming p95 frame interval | 13.80 ms | 13.88 ms |
| Scrolling p95 frame interval | 14.12 ms | 14.04 ms |

These general native scenes do not measure active filtering of the
20,000-path fixture. Native RAM is not lower and frame timing does not
establish an overall speedup. Sessions used disposable homes/workspaces,
without restarting the user's app, real credentials, live providers,
commits, pushes, overlapping passes, or an additional loop.

### Remaining limits

Path lowercasing and full matching-hit collection remain. Query/path sizes
are not globally bounded by this optimization, and indexing is unchanged.

The picker idle-CPU finding remains open; this changes active scoring, not
focus/caret repaint scheduling. The last normal-size sample, Pass 6's
**2.00%**, still fails **<2.0%** and was not remeasured here. Physical keyboard,
clipboard, hover, VoiceOver, live providers, the major-state CPU sweep, and
saved-stream durability sweep are outside this pass.

## Pass 11: bound mention hit and result storage

Status: complete. Focused regressions, strict checks, full workspace tests,
all nine release probes/build, isolated paired capacity/timing probes,
ordering-stress checks, and sequential native performance/composer validation
pass. All four native captures were inspected.

Evidence root: `target/mention-topk-tave2c1j/`. Starting sources/status, the
Pass 10 app binary, equivalent all-hit benchmark sources, retained release
test binaries, failed capacity regression, rejected heap prototype, raw
paired/stress samples, and final measured-source snapshots are preserved.
Old baseline PID 69769 and Pass 10 validation PID 17378 were confirmed exited.

### Proven storage gap and final fix

Search collected all matching `(score, path)` pairs before truncating to
eight. The baseline capacity regression failed. With 20,000 matching paths,
both the ranked-hit buffer and returned reference vector reported
**524,288 bytes** of capacity. The returned vector inherited the oversized
allocation through in-place iterator collection; these are capacity readings
at different stages, not two independent peak-memory savings to sum.

The final selector maintains at most eight sorted scored hits. Candidates
that cannot improve the cutoff are discarded immediately; a full buffer
pops its worst hit before inserting a better one. The reference result uses
a separately sized allocation instead of inheriting scored-hit capacity.

Scoring, Unicode behavior, descending-score/ascending-path ordering,
duplicates, visible count, empty/no-match results, original paths, keyboard
attachments, and menu UI remain unchanged. No persistent cache/index, UI
timer, animation, dependency, or public API change was added.

### Rejected prototype and accepted tradeoff

An eight-entry max-heap fixed storage but slowed the same-process reversed
`main` stress fixture from **41.155 → 63.472 ms** per 20 selections. It was
rejected; source, binary, and measurements remain in `heap-prototype-*`.

A sorted eight-hit buffer avoids repeated heap restoration. Five final
same-process stress samples compare the previous all-hit reference and the
bounded selector, sharing the current scorer. The selector order is fixed,
so these supplement, rather than replace, retained-binary paired probes.

| Stress case, 20 selections | All-hit reference | Final bounded |
| --- | ---: | ---: |
| Sorted empty query | 2.213 ms | 0.879 ms |
| Sorted `main` | 41.173 ms | 39.329 ms |
| Reversed empty query | 1.965 ms | 2.367 ms |
| Reversed `main` | 40.967 ms | 40.070 ms |

The reversed empty-query cost rises **0.402 ms per 20 calls** (about
0.020 ms each). This small, measured replacement-order cost is accepted for
the bounded storage. No uniform speedup is claimed.

### Focused validation and isolated paired measurements

- Eleven mention unit tests pass. New tests enforce hit/result capacities,
  exact full-ranking equivalence across four candidate orders, Unicode,
  duplicate/equal cutoff values, no matches, and counts around the limit.
- Existing 9,156-score differential, 20,000-file ranking, Unicode filename,
  keyboard attachment, and long-query/filename regressions pass.
- Formatting, strict workspace/all-target Clippy, and whitespace checks pass.

`hotpath-paired.json` contains five alternating retained-release-binary
samples without concurrent builds, tests, or native scripts. Each query runs
20 searches over 20,000 sorted Unicode-directory paths.

| Query, 20 searches | Before median | After median |
| --- | ---: | ---: |
| Empty | 2.101 ms | 0.861 ms |
| `MAIN` | 40.869 ms | 40.900 ms |
| `module_1` | 41.347 ms | 41.145 ms |
| `目录` | 33.903 ms | 32.599 ms |
| `zzzz` | 38.198 ms | 37.894 ms |

Broad-query ranked capacity is **524,288 → 128 bytes**, and returned
reference capacity is **524,288 → 64 bytes** on this build. The partial
`module_1` case falls **262,144 → 128/64 bytes**; no-match capacities remain
zero. This is buffer capacity, not measured peak RSS or whole-app RAM.
Earlier samples overlapping a test compilation are retained as
`preliminary-concurrent-test-*.json` and excluded from these figures.

Reproduce the normal probe with
`cargo test --offline --locked --release -p flint-app --lib mention_search_perf_probe -- --ignored --nocapture --test-threads=1`;
the additional stress probe is `mention_ordering_perf_probe` with the same
flags.

### Full validation and remaining limits

`workspace-tests.log`: ACP 68, agent 150, app **144** (nine manual probes
ignored), blueprint UI **231**, terminal 17; integration and doc tests pass.
`release-probes.log`: all nine probes pass. Release build passes in 58.70 s.
Formatting, strict workspace/all-target Clippy, and whitespace checks pass.

Sequential validation PID 37337 exited after both scripts passed:
`native-paired/data/report-perf.json` **10/10**, and
`native-ui/data/report-queue.json` **4/4**. Before uses the preserved,
validated Pass 10 app; after uses the retained final release app. Final
measured sources and app/test binary hashes were checked against snapshots.

| Paired native metric | Before | After |
| --- | ---: | ---: |
| Cold start, window | 267.81 ms | 255.18 ms |
| First painted frame | 168.53 ms | 158.22 ms |
| Idle RSS | 76.12 MB | 76.31 MB |
| 200-turn RSS | 85.55 MB | 85.90 MB |
| Idle CPU | 0.25% | 0.50% |
| 2,000-delta/s CPU | 61.2% | 61.0% |
| Streaming frame interval p95 | 14.09 ms | 13.98 ms |
| Scroll frame interval p95 | 14.11 ms | 14.07 ms |
| Streaming dropped frames | 0 | 0 |
| Release binary | 32.4 MB | 32.4 MB |

These native scenes do not exercise picker queries. No whole-app RSS,
idle-CPU, or frame-cadence improvement is claimed from these small changes.
The storage claim concerns the measured helper buffers.

Working, paused, compact, and empty composer captures retain the
dark-and-ember styling, distinct queue/task controls, stop/resume actions,
permission/model chips, and shortcut hints. The compact queue remains a
scrollable viewport; its second entry is intentionally only partly visible.
This inspection does not establish physical keyboard or accessibility
behavior.

Path lowercasing, indexing, and retained candidate strings remain unchanged.
All candidates are still scored; only retained hits/result storage is bounded.
No typical/native-input, global RAM, or idle-CPU improvement is inferred.

Picker idle CPU remains open: the latest actual normal-picker sample remains
**2.00%**, failing strict **`<2.0%`**, and was not remeasured here. Physical
keyboard, clipboard, hover,
VoiceOver, live providers, major-state CPU, and saved-stream durability
sweeps are outside this pass.

## Pass 12: short-circuit session-title projection

Status: complete. Focused regressions, strict checks, full workspace tests,
all ten release probes/build, isolated retained-release measurements, and
sequential native validation pass. All four captures were inspected.
Passes 1–11 are complete; original baseline PID 69769 and Pass 11 validation
PID 37337 were confirmed exited before benchmarking.

Evidence root: `target/session-title-lwfj7pre/`. Starting sources, ledger,
status/diff stat, validated Pass 11 app binary, benchmark-only baseline
sources, before/after release test binaries, raw samples, and measured final
sources are preserved.

### Proven gap and bounded fix

`title_from` selected the full first nonempty line, trimmed it, and counted
all its Unicode scalars to decide whether a 48-scalar title needed an
ellipsis. A long non-whitespace first line therefore incurred full-line
scans for a tiny label. The baseline release probe reproduces that cost.

The final projection skips leading whitespace, collects at most 48 scalars
before LF, trims the short prefix, and stops at the first omitted
non-whitespace scalar. Whitespace-only remainder does not add an ellipsis.
It appends the ellipsis in place rather than formatting another string.

First nonempty-line selection, blank fallback, Unicode scalar boundaries,
LF/CRLF/bare-CR behavior, internal/trailing whitespace, and title text remain
unchanged. The full user message remains intact, and later messages do not
rename an existing session. No cache, timer, animation, UI layout, dependency,
or public API change was added.

### Focused verification and isolated measurements

- Two new regressions pass: **565 exact comparisons** against a frozen
  full-line reference across boundary lengths, Unicode/combining marks,
  whitespace, and line endings; and complete first-message retention with
  stable title on subsequent messages.
- All 24 view-model tests pass; three manual probes are ignored in that
  focused invocation. Existing title, bounded-output, completion, approval,
  terminal, and subagent regressions remain passing.
- Formatting, strict offline/locked workspace/all-target Clippy, and
  whitespace checks pass.

`hotpath-paired.json` records five alternating retained-release binaries
without concurrent builds, tests, or native scripts. Each case projects a
title 100 times; fixture construction is outside timing.

| Fixture, approximately 8 MiB | Before median | After median |
| --- | ---: | ---: |
| ASCII first line | 49.531 ms | 0.021 ms |
| Unicode first line | 49.412 ms | 0.032 ms |
| Short title, whitespace-only tail | 1,172.125 ms | 410.869 ms |
| Short first line, large following body | 0.009 ms | 0.007 ms |

These are synthetic helper costs, not measured GPUI latency, allocation
counts, or peak/native RSS. The multiline case was already cheap; no
meaningful gain is claimed there. Whitespace-only tails and leading blank
content still require scanning to preserve semantics, so total work is not
universally bounded independently of input size.

Reproduce with
`cargo test --offline --locked --release -p flint-app --lib session_title_perf_probe -- --ignored --nocapture --test-threads=1`.

### Full validation and remaining limits

`workspace-tests.log`: ACP 68, agent 150, app **146** (ten manual probes
ignored), blueprint UI **231** (159.53 s), terminal 17; integration and doc
tests pass. Explicit composer-refresh tests **2/2** and compact eight-pane
composer test **1/1** pass, including queue/stop/task separation and reachable
options. `release-probes.log`: all ten probes pass, including bounded live
output and terminal forwarding. Release build passes in **49.35 s**.
Formatting, strict workspace/all-target Clippy, and whitespace checks pass.

Sequential validation PID **70879** exited after native performance **10/10**
and composer **4/4** passed. Reports:
`native-paired/data/report-perf.json` and
`native-ui/data/report-queue.json`. Final measured-source snapshots and
retained app/test binaries were checked for drift; baseline matches the
validated Pass 11 app.

| Paired native metric | Before | After |
| --- | ---: | ---: |
| Cold start, window | 285.73 ms | 277.60 ms |
| First painted frame | 192.52 ms | 192.12 ms |
| Idle RSS | 76.33 MB | 76.17 MB |
| 200-turn RSS | 86.04 MB | 85.66 MB |
| Idle CPU | 0.50% | 0.75% |
| 2,000-delta/s CPU | 50.2% | 49.6% |
| Streaming frame interval p95 | 14.06 ms | 14.04 ms |
| Scroll frame interval p95 | 13.75 ms | 13.87 ms |
| Streaming dropped frames | 0 | 0 |
| Release binary | 32.4 MB | 32.4 MB |

The native fixtures do not exercise oversized first-message titles. These
small changes do not establish whole-app RSS, CPU, or frame-cadence gains.
Idle CPU and scroll p95 are slightly worse; no overall gain is claimed.

Working/paused 1440×900 and compact/empty 900×560 captures retain title
truncation, dark-and-ember styling, queue/task separation, stop/resume controls,
model/permission chips, and shortcut hints. Compact queue entries and welcome
suggestions intentionally scroll. Screenshot inspection does not prove
physical keyboard or accessibility behavior.

No typical input, whole-app RAM, idle-CPU, or frame-cadence improvement is
inferred. Tool lookup, saved-stream pressure, queue bounds, and the last
actual normal-picker **2.00%**, failing strict **`<2.0%`**, remain open.
Physical keyboard, clipboard, hover, VoiceOver, live providers, major-state
CPU, and saved-stream durability are outside this pass.

## Pass 13: bound queued-prompt display previews

Status: complete. Focused regressions, strict checks, full workspace tests,
all eleven release probes/build, isolated retained-release measurements, and
sequential native validation pass. All four captures were inspected.
Passes 1–12 are complete; baseline PID 69769 and previous validation PID 70879
were confirmed exited before benchmarking.

Evidence root: `target/queue-preview-qpuhdado/`. Starting sources, ledger,
status/diff stat, validated Pass 12 app, benchmark-only equivalent projection,
before/after release test binaries, failed bound regression, raw paired
samples, and final measured-source snapshots are preserved.

### Proven gap and bounded fix

The "Up next" row copied its complete first nonempty prompt line into a
display string and relied on visual truncation. An 8 MiB first line retained
an 8 MiB preview allocation and passed that text to layout on every render.
The baseline regression fails the scalar bound; the release probe reproduces
the full-size copied buffer.

The row now reuses the shared 240-scalar `ui::one_line` projection before
allocation/layout. Leading/trailing display whitespace is trimmed, and omitted
body text gets an ellipsis. Empty/blank image-only text still reads
`Image prompt`. A preview observation ID supports real-view bounds checks.

Full editable instruction, frozen context, images, stored/sent message, queue
IDs/order, and controls remain unchanged. No persistent cache, timer,
animation, dependency, or queue-storage/API change was added.

### Focused regressions and measurements

- Two queue-preview unit tests pass: ASCII/Unicode bound and capacity,
  first meaningful line/blank fallback, multiline ellipsis, and intact
  instruction/frozen-context snapshots.
- New real-view regression passes at **1440×900** and **900×560**: preview
  stays one row within queue width; editing sees full Unicode/multiline text;
  keyboard save preserves the complete instruction/context; explicit steering
  sends the full frozen message and keeps the entry until acknowledgement.
- Formatting, strict offline/locked workspace/all-target Clippy, and
  whitespace checks pass.

`hotpath-paired.json` records five alternating retained-release binaries,
without concurrent builds, tests, or native scripts. Each fixture projects a
display string 100 times from an approximately 8 MiB first line.

| Fixture | Before median | After median | Before capacity | After capacity |
| --- | ---: | ---: | ---: | ---: |
| ASCII | 34.211 ms | 0.044 ms | 8,388,608 B | 256 B |
| Unicode | 34.178 ms | 0.064 ms | 8,388,604 B | 1,024 B |

These are projection time and individual-buffer capacity, not total queue
rendering, GPUI shaping measurements, allocation counts, or peak/native RSS.
Character-count detail still scans the full prompt during rendering. Full
prompt/context/image storage and edit-input layout are not bounded by this
preview limit. Whitespace-only input can still require scanning.

Reproduce with
`cargo test --offline --locked --release -p flint-app --lib queued_preview_perf_probe -- --ignored --nocapture --test-threads=1`.

### Full validation and remaining limits

`queue-ui-tests.log`: **14/14** pass, covering reordering, editing, native/ACP
steering, acknowledgement, FIFO, snapshots/restart, failures, background
sessions, and compact controls. Explicit composer refresh **2/2** and compact
eight-pane **1/1** pass.

`workspace-tests.log`: ACP 68, agent 150, app **148** (eleven manual probes
ignored), blueprint UI **232** (156.59 s), terminal 17; integration/doc tests
pass. `release-probes.log`: all eleven pass. Release build passes in
**47.16 s**. Formatting, strict workspace/all-target Clippy, and whitespace
checks pass.

Sequential validation PID **10035** exited after native performance **10/10**
and composer **4/4** passed. Reports:
`native-paired/data/report-perf.json` and
`native-ui/data/report-queue.json`. Final measured-source snapshots and
retained app/test binaries were checked for drift; baseline matches validated
Pass 12.

| Paired native metric | Before | After |
| --- | ---: | ---: |
| Cold start, window | 249.10 ms | 239.73 ms |
| First painted frame | 161.40 ms | 163.06 ms |
| Idle RSS | 76.62 MB | 76.31 MB |
| 200-turn RSS | 86.09 MB | 85.75 MB |
| Idle CPU | 0.50% | 0.50% |
| 2,000-delta/s CPU | 54.4% | 51.2% |
| Streaming frame interval p95 | 14.02 ms | 13.98 ms |
| Scroll frame interval p95 | 14.18 ms | 14.27 ms |
| Streaming dropped frames | 0 | 0 |
| Release binary | 32.4 MB | 32.4 MB |

Native fixtures do not exercise oversized queued instructions. No whole-app
RSS, CPU, or frame-cadence gain is established. First paint and scroll p95
are slightly worse; the verified allocation claim concerns the projection
buffer only.

Working/paused 1440×900 and compact/empty 900×560 captures retain readable
short queue previews, dark-and-ember styling, separate task/composer controls,
stop/resume actions, permission/model chips, and shortcut hints. Compact
queue entries and welcome suggestions intentionally scroll. Screenshot review
does not establish physical keyboard or accessibility behavior.

The initial long-editor pointer-save test left edit mode active, so the
subsequent steering button was absent. The retained
`ui-save-click-diagnostic.log` records this failure. The keyboard-save path
passes; pointer-save reachability/scrolling for long editors remains a
separate investigation, not a verified fix or established root cause.
Pass 14 reproduces and resolves this diagnostic.

No whole-app RAM/CPU/frame or ordinary-input gain is inferred. The last
actual normal-picker **2.00%**, failing strict **`<2.0%`**, remains open and
was not remeasured. Physical keyboard, clipboard, hover, VoiceOver, live
providers, major-state CPU, and saved-stream durability are outside this pass.

## Pass 14: keep long queue-editor actions reachable

Status: complete. Focused geometry/action regressions, all queue UI tests,
strict checks, full workspace, all eleven release probes/build, and sequential
native validation pass. All six captures, including both editors, were
inspected. Passes 1–13 are complete; baseline PID 69769 and previous validation
PID 10035 were confirmed exited.

Evidence root: `target/queue-editor-hpwz7bfl/`. Starting sources, ledger/status,
validated Pass 13 app, intentional baseline geometry failure, final geometry
log, queue regressions, and measured final source snapshots are retained.

### Reproduced cause and bounded fix

The Pass 13 pointer-save diagnostic now has a reproduced cause. A long queued
instruction grows its textarea to five lines. Save/Cancel were inside the
scrollable row, below the queue's clipped viewport. At **1440×900**, the queue
ended at **668 px**, while Save ended at **687 px** and its click center was
outside the clip. `regression-before.log` fails the button containment check.

Save and Cancel now occupy a fixed **44 px** queue footer outside the
scrollable content. The row viewport reserves header, footer, and border
height; its wrapper clips editor content instead of covering the actions.
The footer only appears for an existing prompt edited in the active session.
Listeners, original text/context, queue persistence, dispatch/pause policy,
keyboard saving, and button labels remain unchanged.

The footer uses existing border, surface, spacing, and button styles. No
animation, idle repaint, cache, provider behavior, or stored queue format
change was added.

### Focused verification

- A new real-view regression passes at **1440×900**, **1000×700**, and
  **900×560**, with a long Unicode first line and twelve following lines.
  Pointer Save closes the editor and preserves the full instruction and
  frozen context. Pointer Cancel discards changes. A rejected blank save
  retains the editor, original prompt, and visible error; Cancel stays
  reachable. The paused queue never sends work during these edits.
- Scrolling rows do not overlap the footer. Save/Cancel bounds stay inside
  the queue and window. Final Save bottoms are **656.5**, **456.5**, and
  **344.5 px**, within queue bottoms **668**, **468**, and **356 px**.
- All **15** queue UI tests pass, including the existing keyboard-save/full
  steering case, short editor pointer actions, snapshots/restart, failure,
  session isolation, FIFO, and compact controls.
- Formatting, strict offline/locked workspace/all-target Clippy, whitespace,
  and Python native-script syntax checks pass.

### Native fixture support and full validation

An automation-only hook creates a synthetic long queued instruction and
opens its editor for capture. It requires demo mode, the demo queue,
`FLINT_BP_STATE`, and explicit `FLINT_BP_QUEUE_EDITOR=1`; normal sessions
remain unaffected. The state dump reports editing state, and `queue.py`
adds normal/compact editor scenes to the existing four captures.

`workspace-tests.log`: ACP 68, agent 150, app **148** (eleven manual probes
ignored), blueprint UI **233** (157.46 s), terminal 17; integration/doc tests
pass. Explicit composer refresh **2/2** and compact eight-pane composer
**1/1** pass. All eleven release probes pass. Release build passes in
**46.84 s**. Final formatting, strict workspace/all-target Clippy, whitespace,
and Python syntax checks pass.

Sequential validation PID **43837** exited after native performance **10/10**
and queue/composer **6/6** passed. Reports:
`native-paired/data/report-perf.json` and
`native-ui/data/report-queue.json`. The editor scenes confirm active editor
state in the isolated app's dump. Final sources and retained binaries were
checked for drift; baseline matches validated Pass 13.

| Paired native metric | Before | After |
| --- | ---: | ---: |
| Cold start, window | 256.20 ms | 250.64 ms |
| First painted frame | 164.33 ms | 157.78 ms |
| Idle RSS | 76.30 MB | 76.17 MB |
| 200-turn RSS | 86.09 MB | 85.69 MB |
| Idle CPU | 0.25% | 0.50% |
| 2,000-delta/s CPU | 59.2% | 57.2% |
| Streaming frame interval p95 | 14.27 ms | 14.19 ms |
| Scroll frame interval p95 | 14.39 ms | 14.31 ms |
| Streaming dropped frames | 0 | 0 |
| Release binary | 32.4 MB | 32.4 MB |

These general scenes do not measure editor interaction performance. No speed,
RAM, frame-cadence, or idle-CPU gain is claimed; idle CPU is slightly worse.

Inspected working, paused, compact, empty, long-editor **1440×900**, and
long-editor **900×560** captures. The fixed dark-and-ember footer visibly
separates readable Cancel/Save controls from clipped, scrollable editor
content. Queue header, composer/task tray, stop/resume, options, and shortcuts
remain separate. Compact queue content and welcome suggestions intentionally
scroll. Screenshots do not establish physical input or VoiceOver behavior.

This is a reproduced layout/reachability fix, not a performance or RAM gain.
No CPU, latency, frame, allocation, or global-memory improvement is inferred.
The last actual normal-picker **2.00%**, failing strict **`<2.0%`**, is open
and not remeasured. Physical input, VoiceOver, unusually tiny editor widths,
live providers, major-state CPU, and saved-stream durability remain outside
this pass.

## Pass 15: bound sidebar approval-status projection

Status: focused/full tests, strict checks, isolated retained-release
measurements, release build, and sequential native validation are complete.
Passes 1–14 were already complete; baseline PID 69769 and previous validation
PID 43837 were confirmed exited. This pass's validation PID 77873 also exited.

Evidence root: `target/approval-status-vsvgnmix/`. Starting sources, ledger,
status, validated Pass 14 app, equivalent benchmark-only baseline, intentional
bound failure, release test binaries/apps, raw paired probes, native reports,
six inspected captures, final source snapshots/status, pass diffs,
`final-integrity.json`, and `validation-summary.json` are preserved.

### Proven gap and bounded fix

The approval sidebar line formatted the complete first request summary and
collected every unanswered summary into a temporary reference vector before
counting additional requests. An approximately 8 MiB summary therefore
produced an approximately 8 MiB display buffer. The baseline regression fails
the scalar bound, and release probes reproduce the returned allocation.

The projection now borrows the first unanswered summary, counts the remaining
matching items without a vector, and reuses the shared 240-scalar preview
before formatting. Display whitespace is trimmed and omitted multiline
content gets an ellipsis. The fallback, warning tone, first-request order,
additional unanswered count, and ignored answered/non-approval items stay
unchanged.

Original summaries, command arguments, approval preview details, decision
handling, and request IDs remain complete. No cache, timer, animation, stored
format, dependency, or public API signature change was added.

### Focused verification and isolated measurements

- Two unit regressions pass: bounded Unicode line/capacity with retained full
  request, and exact first-request/additional-count behavior through answered,
  empty, missing, and non-approval rows.
- New real-view regression passes at **1440×900** and **1000×560**: bounded
  warning subtitle stays within sidebar width, and its projected text includes
  `(+1 more)`. Full Unicode/multiline summaries and the actual command argument
  stay in the request/approval preview.
- Formatting, strict offline/locked workspace/all-target Clippy, and
  whitespace checks pass.

`hotpath-paired.json` records five alternating retained-release binaries,
without concurrent builds, tests, or native scripts. Each case projects a
line 100 times; fixture construction is outside timing.

| Fixture | Before median | After median | Before returned capacity | After |
| --- | ---: | ---: | ---: | ---: |
| Approximately 8 MiB ASCII summary | 10.621 ms | 0.060 ms | 8,388,617 B | 253 B |
| Approximately 8 MiB Unicode summary | 10.711 ms | 0.080 ms | 8,388,613 B | 853 B |
| 20,000 short unanswered requests | 3.001 ms | 1.142 ms | 36 B | 36 B |

These measure the extracted approval-line helper and its returned buffer, not
GPUI shaping, all status work, peak/native RSS, or retained request storage.
The pending-reference vector is removed structurally; its heap/peak-memory
cost was not measured. Full transcript counting remains linear, and
whitespace-only summaries can still require scanning.

Reproduce with
`cargo test --offline --locked --release -p flint-app --lib approval_status_perf_probe -- --ignored --nocapture --test-threads=1`.

### Full and native validation

- Offline/locked serial full workspace: ACP **68**, agent **150**, app **150**
  (12 manual probes ignored in the normal suite, then run explicitly), UI
  **234**, terminal **17**; integration and documentation tests pass.
- Explicit composer-refresh **2/2** and compact eight-pane **1/1** pass.
- All twelve retained release probes pass; release app builds in **45.62 s**.
- Sequential native `perf.py --no-live`: **10/10** pass against the retained
  validated Pass 14 app. `queue.py`: **6/6** captures/state checks pass.
- All six captures inspected: working, paused, compact, empty, long editor,
  and compact long editor. Dark-and-ember hierarchy, task/composer separation,
  bounded queue previews, and visible fixed editor footer remain intact.
  Compact queue and welcome content intentionally scroll.
- Measured source snapshots match final code. Retained app/test hashes,
  report/state/capture hashes, and unchanged starting status-path set are
  verified. No new status path, commit, publication, live provider, credential
  access, user-app restart, or second loop was introduced.

| Paired native metric | Before | After |
| --- | ---: | ---: |
| Cold start, window | 264.31 ms | 282.73 ms |
| First painted frame | 167.83 ms | 201.27 ms |
| Idle RSS | 76.05 MB | 76.14 MB |
| 200-turn RSS | 85.51 MB | 85.82 MB |
| Idle CPU | 0.75% | 0.75% |
| 2,000-delta/s CPU | 49.8% | 47.6% |
| Streaming frame interval p95 | 14.00 ms | 14.02 ms |
| Scroll frame interval p95 | 13.76 ms | 13.54 ms |
| Streaming dropped frames | 0 | 0 |
| Release binary | 32.4 MB | 32.4 MB |

These general native scenes do not exercise the oversized approval-summary
fixture. Start/first-frame timing is worse, RSS is slightly higher, and idle
CPU is unchanged; no overall gain is claimed.

### Remaining limits

No typical input, whole-app RSS, idle-CPU, frame-cadence, or approval
interaction gain is inferred. The last actual normal-picker **2.00%**,
failing strict **`<2.0%`**, remains open and was not remeasured. Physical
input, clipboard, hover, VoiceOver, live providers, major-state CPU, and
saved-stream durability are outside this pass.

## Pass 16: bound the sidebar failure-first-line preview

Status: focused/full tests, strict checks, isolated retained-release
measurements, release build, and sequential native validation are complete.
Passes 1–15 were already complete. Baseline PID 69769 and previous validation
PID 77873 were confirmed exited; the original baseline log was reviewed
before measurements. This pass's validation PID 2564 also exited.

Evidence root: `target/failure-preview-u_a20vqp/`. Starting sources/ledger,
status, validated Pass 15 app, intentional baseline bound failure,
benchmark-before sources, retained release apps/test binaries, oversized and
short-line paired probes, native reports, six inspected captures, final
source snapshots/status, pass diffs, `final-integrity.json`, and
`validation-summary.json` are preserved.

### Proven gap and bounded fix

The failed sidebar status formatted the entire trimmed first error line.
An approximately 8 MiB first line still produced an approximately 8 MiB
display buffer, despite the earlier borrowed-source fix. The new baseline
regression fails its scalar bound and the retained release probe reproduces
that capacity.

The projection now passes the already-selected first line through the shared
240-scalar preview before formatting. First-line selection happens before
preview normalization, so a blank first line does not promote later error
details. CRLF, short text, status precedence, danger tone, source ownership,
and the independently owned full `failure()` API stay intact. Oversized
first lines gain an ellipsis; original error text is not changed.

No public signature, persistence, dependency, cache, timer, animation, or
layout-token change was added. Full error-detail surfaces are unchanged,
not globally bounded by this fix.

### Focused verification and isolated measurements

- New bound/ownership regression passes for ASCII/Unicode and both idle/turn
  error sources. Mutating the owned error copy does not affect stored text.
- Independent first-line/prefix oracle passes **105 exact cases**, including
  lengths 239/240/241, Unicode scalars, empty/blank leading lines, CRLF,
  standalone CR, Unicode separator, and prefix-edge whitespace.
- Existing failure borrowing/precedence/first-line regressions pass.
  Focused filter: **5 passed**, two manual probes ignored.
- Real-view failure/status module: **3/3**. The added long-first-line case
  checks both idle/turn sources at **1440×900** and **1000×560**, exact
  bounded Unicode danger status, full retained error, and sidebar width.
- Formatting, strict offline/locked workspace/all-target Clippy, and
  whitespace checks pass.

`hotpath-paired.json` records five alternating retained release binaries,
without concurrent builds, tests, or native scripts. Each fixture calls the
actual `Session::status_line` 100 times; construction and full owned-error
copies are outside timing.

| Error fixture | Before median | After median | Before returned capacity | After |
| --- | ---: | ---: | ---: | ---: |
| Approximately 8 MiB ASCII first line | 35.075 ms | 22.097 ms | 8,388,616 B | 252 B |
| Approximately 8 MiB Unicode first line | 35.073 ms | 21.994 ms | 8,388,612 B | 852 B |
| Blank first line, 8 MiB later body | 0.006 ms | 0.005 ms | 16 B | 16 B |

These measure status projection and its returned buffer, not GPUI shaping,
peak/native RSS, all error rendering, or retained storage. The blank-line
timing difference is not evidence of a meaningful gain. First-line discovery
and whitespace handling can still scan the full input; runtime is not
globally bounded.

Reproduce with
`cargo test --offline --locked --release -p flint-app --lib failure_preview_perf_probe -- --ignored --nocapture --test-threads=1`.

After native validation exited, five additional alternating retained-release
runs of the existing `failure_status_perf_probe` measured the short-line
tradeoff without overlap. `short-line-paired.json` and raw logs preserve it:

| Short Unicode first line, 8 MiB later body | Before, 100 calls | After |
| --- | ---: | ---: |
| Idle error | 0.012 ms | 0.031 ms |
| Turn error | 0.008 ms | 0.018 ms |

The bounded preview adds short-line work: **0.019/0.010 ms per 100 calls**
in these fixtures. This small helper cost is accepted for bounded display
storage. No uniform speedup or typical-input latency gain is claimed.

### Full and native validation

- Offline/locked serial full workspace: ACP **68**, agent **150**, app **152**
  (13 manual probes ignored, then run explicitly), UI **235**, terminal **17**;
  integration and documentation tests pass.
- Explicit composer-refresh **2/2** and compact eight-pane **1/1** pass.
- All thirteen release probes pass, including retained live output and
  terminal-line selection. Release app builds in **45.35 s**.
- Sequential native `perf.py --no-live`: **10/10**, with the validated Pass 15
  baseline. `queue.py`: **6/6** state/capture checks.
- Inspected working, paused, compact, empty, normal long-editor, and compact
  long-editor captures. Dark-and-ember hierarchy, task/composer separation,
  queue/stop controls, and fixed Save/Cancel footer stay readable.
  Compact queue/welcome content intentionally scroll.
- Measured code matches final source snapshots. Retained binary/test,
  report/state/capture hashes and starting status-path set are verified.
  No new status path, commit, publication, live provider, credential access,
  user-app restart, or second loop was introduced.

| Paired native metric | Before | After |
| --- | ---: | ---: |
| Cold start, window | 276.43 ms | 271.92 ms |
| First painted frame | 187.01 ms | 191.46 ms |
| Idle RSS | 75.97 MB | 76.28 MB |
| 200-turn RSS | 85.61 MB | 85.70 MB |
| Idle CPU | 0.75% | 0.75% |
| 2,000-delta/s CPU | 48.8% | 46.8% |
| Streaming frame interval p95 | 13.96 ms | 14.00 ms |
| Scroll frame interval p95 | 13.78 ms | 13.48 ms |
| Streaming dropped frames | 0 | 0 |
| Release binary | 32.4 MB | 32.4 MB |

These general native scenes do not exercise the oversized failure-first-line
fixture. RSS/first-frame samples are slightly worse, idle CPU is unchanged,
and no whole-app improvement is inferred.

### Remaining limits

No typical-input, whole-app RSS, idle-CPU, frame-cadence, or error-detail
interaction gain is inferred. The last actual normal-picker **2.00%** still
fails strict **`<2.0%`** and was not remeasured. Physical input, clipboard,
hover, VoiceOver, live providers, major-state CPU, and saved-stream
durability remain outside this pass.

## Pass 17: bound queued-prompt character metadata work

Status: focused/full tests, strict checks, isolated retained-release
measurements, release build, and sequential native validation are complete.
Passes 1–16 were already complete. Original baseline PID 69769 and previous
validation PID 2564 were confirmed exited; the baseline log was reviewed
before measurements. This pass's validation PID 60579 also exited.

Evidence root: `target/queue-count-nvcnyq76/`. Starting sources/ledger/status,
validated Pass 16 app, equivalent extracted count baseline, intentional
boundary failure, retained release apps/test binaries, raw alternating probes,
native reports, six inspected captures, final source snapshots/status, pass
diffs, `final-integrity.json`, and `validation-summary.json` are preserved.

### Proven gap and bounded fix

The queue row already bounded its preview, but still called
`prompt.text.chars().count()` on every render for the metadata label. The
retained release probe reproduces the cost of scanning approximately 8 MiB
per call. The baseline boundary regression reports `2001 characters` instead
of the new explicitly bounded display label.

Counts remain exact through **2,000 Unicode scalars**. Longer text displays
**“More than 2,000 characters”** rather than computing an exact total.
For at most 2,000 bytes, the existing optimized full scalar count is safe;
otherwise only the first **2,001 scalars** are inspected. The label is
responsive/ellipsized and gains an observation ID.

Full queued text, shown text, frozen context, editing, steering, image
metadata, and applying state stay unchanged. There is no count cache, new
model field/schema, dependency, timer, or extra repainting.

### Focused verification and isolated measurements

- Queue view unit tests: **4 passed**, two manual probes ignored.
  New tests cover **27 exact/boundary labels** (ASCII, CJK, emoji, accented
  scalars, combining marks, LF/CRLF, and whitespace) and complete untouched
  prompt/context data.
- Queue real-view module: **15/15**. Expanded existing long-preview test now
  uses over 4,000 Unicode scalars at **1440×900** and **900×560**. Metadata and
  preview remain inside queue width; keyboard edit/save and full frozen
  steering payload stay exact. Existing pointer footer, compact panes,
  queue/stop/failure/FIFO, images, native/ACP steering tests pass.
- Formatting, strict offline/locked workspace/all-target Clippy, and
  whitespace checks pass.

`hotpath-paired.json` records five alternating retained-release binaries,
without concurrent builds, tests, or native scripts. Each fixture formats
character metadata 100 times; construction, preview/layout, and full prompt
copies are outside timing.

| Prompt fixture | Before median, 100 calls | After |
| --- | ---: | ---: |
| Approximately 8 MiB ASCII | 25.683 ms | 0.092 ms |
| Approximately 8 MiB Unicode | 25.983 ms | 0.156 ms |
| 64 ASCII scalars | 0.006 ms | 0.007 ms |
| 64 Unicode scalars | 0.006 ms | 0.005 ms |

These are character-metadata helper timings only, not whole-row GPUI,
whole-app CPU/latency, or native/peak RSS. The short-input differences do not
establish a meaningful gain. Exact total counts above the cap are deliberately
replaced by a lower-bound label; full model text is still retained.

Reproduce with
`cargo test --offline --locked --release -p flint-app --lib queued_character_detail_perf_probe -- --ignored --nocapture --test-threads=1`.

### Full and native validation

- Offline/locked serial full workspace: ACP **68**, agent **150**, app **154**
  (14 manual probes ignored, then run explicitly), UI **235**, terminal **17**;
  integration and documentation tests pass.
- Explicit composer-refresh **2/2** and compact eight-pane **1/1** pass.
- All fourteen release probes pass, including retained live output and
  terminal-line selection. Release app builds in **45.96 s**.
- Sequential native `perf.py --no-live`: **10/10**, against the validated
  Pass 16 app. `queue.py`: **6/6** state/capture checks.
- Inspected working, paused, compact, empty, normal long-editor, and compact
  long-editor captures. Short metadata remains exact (45/49/573 characters);
  queue/stop controls, dark-and-ember hierarchy, task/composer separation,
  and fixed Save/Cancel footer remain readable. Compact queue/welcome
  content intentionally scroll.
- Measured code matches final source snapshots. Retained apps/tests,
  report/state/capture hashes and the unchanged starting status-path set are
  verified. No new status path, commit, publication, live provider, credential
  access, user-app restart, or second loop was introduced.

| Paired native metric | Before | After |
| --- | ---: | ---: |
| Cold start, window | 281.22 ms | 282.15 ms |
| First painted frame | 191.03 ms | 193.84 ms |
| Idle RSS | 76.28 MB | 76.22 MB |
| 200-turn RSS | 85.79 MB | 85.76 MB |
| Idle CPU | 0.50% | 0.75% |
| 2,000-delta/s CPU | 49.6% | 49.8% |
| Streaming frame interval p95 | 14.04 ms | 14.09 ms |
| Scroll frame interval p95 | 13.61 ms | 13.93 ms |
| Streaming dropped frames | 0 | 0 |
| Release binary | 32.4 MB | 32.4 MB |

Native fixtures contain short prompts, not the oversized counting fixture.
Idle CPU, first-frame, and frame-interval samples are worse; no whole-app gain
is inferred from the helper timings or small RSS differences.

### Remaining limits

Queue preview whitespace scans, full editor content, accepted prompt/context
retention, and all queue-row rendering are not globally bounded by this
helper. No whole-app RSS, idle-CPU, frame-cadence, or typical-input gain is
inferred. The last actual normal-picker **2.00%** still fails strict
**`<2.0%`** and was not remeasured. Physical input, clipboard, hover,
VoiceOver, live providers, major-state CPU, and saved-stream durability remain
outside this pass.

## Pass 18: borrow pending approvals for read-only consumers

Status: focused/full tests, strict checks, pre-fix/current keyboard comparison,
isolated retained-release measurements, release build, and sequential native
validation are complete. Passes 1–17 were already complete. Original baseline
PID 69769 and previous validation PID 60579 were confirmed exited; the
baseline log was reviewed before measurements. This pass's validation PID
10155 also exited.

Evidence root: `target/approval-borrow-0e3dkla4/`. Starting sources/ledger/
status, validated Pass 17 app, equivalent presence-wrapper baseline,
retained release test binaries, raw alternating probes, copied pre-fix
workspace, forced-recompile comparison logs, corrected compile/UI diagnostics,
native reports, six inspected captures, final source snapshots/status, pass
diffs, `final-integrity.json`, and `validation-summary.json` are preserved.

### Measured gap and bounded ownership fix

`pending_approval()` returns independently owned call ID and summary strings.
Presence-only keyboard guards and automation state probes copied that entire
payload before discarding it. Pinned-card rendering copied the same strings
before passing borrowed references, and answer dispatch copied a summary it
did not use. The baseline presence-wrapper probe reproduces the cost with an
approximately 8 MiB summary.

A crate-private borrowed projection now selects the same oldest unanswered
local row and returns original field references. The public owned API maps
that projection into fresh strings, preserving its signature and independent
ownership. Presence checks use the borrowed projection; pinned rendering
borrows directly; answer dispatch owns only the call ID required for mutation
and routing. Main/secondary composer subscriptions and native state reporting
use the same presence predicate.

No cached index/counter shortcut, new model field, schema, persistence,
dependency, layout token, timer, or animation was introduced. Callback call
IDs, full request-detail fields, stored summaries, and the owned API can
still allocate; this is not a globally bounded approval-memory fix.

### Focused verification and corrected diagnostics

- Two unit regressions pass: original field pointer identity/full Unicode
  summary with independently mutable owned copies; exact oldest selection
  through empty/answered/non-approval rows and deliberately mismatched
  aggregate counters.
- New real-view keyboard test passes at **1440×900** and **1000×560**:
  bounded header geometry, ordinary draft typing does not approve, two
  Cmd-Enter presses route the exact oldest request IDs, and the full original
  summary remains on its resolved row.
- Initial missing `ApprovalDecision` import was corrected; compile diagnostic
  preserved. A stale formatted test patch was reread before retry.
- Initial fixture incorrectly expected its typed instruction to stay in the
  composer after all approvals. It actually ends queued. The corrected test
  verifies the complete queued instruction, and passes on both explicitly
  recompiled copied pre-fix sources and the main implementation. This is
  preserved behavior, not a draft-clearing/routing fix. Forced recompilation
  and main non-incremental compilation avoid cross-workspace target-cache
  ambiguity in the comparison.
- Formatting, strict offline/locked workspace/all-target Clippy, and
  whitespace checks pass. No intentional pre-fix assertion failure is
  claimed: the gap is established by source ownership and measured cost.

### Isolated presence measurements

`hotpath-paired.json` records five alternating retained-release binaries,
without concurrent builds, tests, or native scripts. Each fixture checks
pending presence 100 times; construction is outside timing.

| Pending summary | Before median, 100 checks | After |
| --- | ---: | ---: |
| Approximately 8 MiB ASCII | 10.045 ms | <0.001 ms |
| Approximately 8 MiB Unicode | 10.368 ms | <0.001 ms |
| Short request | 0.005 ms | <0.001 ms |

After reports `0.000 ms`, below the **0.001 ms reporting precision**, not
zero runtime. These measure only the presence helper, not pinned shaping,
full keyboard processing, callback/detail allocations, native/peak RSS,
total request retention, or all owned API callers. Full transcript search
remains linear; heap/peak savings were not profiled.

Reproduce with
`cargo test --offline --locked --release -p flint-app --lib pending_approval_presence_perf_probe -- --ignored --nocapture --test-threads=1`.

### Full and native validation

- Offline/locked serial full workspace: ACP **68**, agent **150**, app **156**
  (15 manual probes ignored, then run explicitly), UI **236**, terminal **17**;
  integration and documentation tests pass.
- Explicit composer-refresh **2/2** and compact eight-pane **1/1** pass.
- All fifteen release probes pass, including bounded live output and terminal
  selection. Release app builds in **47.84 s**.
- Sequential native `perf.py --no-live`: **10/10**, against the validated
  Pass 17 app. `queue.py`: **6/6** state/capture checks.
- Inspected working, paused, compact, empty, normal long-editor, and compact
  long-editor captures. Dark-and-ember hierarchy, readable queue metadata,
  task/composer separation, stop controls, and fixed Save/Cancel footer remain
  intact. Compact queue/welcome content intentionally scroll.
- Final code matches measured source snapshots. Retained apps/tests, copied
  pre-fix sources, forced-recompile logs, report/state/capture hashes, and
  unchanged starting status-path set are verified. Production consumers no
  longer invoke the owned selector. No new status path, commit, publication,
  live provider, credential access, user-app restart, or second loop was added.

| Paired native metric | Before | After |
| --- | ---: | ---: |
| Cold start, window | 278.16 ms | 275.55 ms |
| First painted frame | 189.83 ms | 189.09 ms |
| Idle RSS | 76.33 MB | 76.25 MB |
| 200-turn RSS | 85.83 MB | 85.77 MB |
| Idle CPU | 0.50% | 0.75% |
| 2,000-delta/s CPU | 61.8% | 59.0% |
| Streaming frame interval p95 | 13.84 ms | 13.96 ms |
| Scroll frame interval p95 | 14.43 ms | 14.27 ms |
| Streaming dropped frames | 0 | 0 |
| Release binary | 32.4 MB | 32.4 MB |

General native fixtures do not exercise the oversized pending-summary case.
Idle CPU and streaming cadence samples are worse; small RSS/startup
differences and passing budgets do not establish a whole-app improvement.

### Remaining limits

No whole-app RSS, idle-CPU, frame-cadence, or typical-input gain is inferred.
The last actual normal-picker **2.00%** still fails strict **`<2.0%`** and
was not remeasured. Physical input, clipboard, hover, VoiceOver, live
providers, major-state CPU, and saved-stream durability remain outside this
pass.

## Pass 19: avoid temporary strings for merged terminal cells

Status: focused/full tests, strict checks, isolated retained-release
measurements, release build, sequential native validation, and six capture
inspections are complete. Passes 1–18 were already complete. Original
baseline PID 69769 and previous validation PID 10155 were confirmed exited
and the baseline log reviewed before this pass. Validation PID 30864 also
exited.

Evidence root: `target/terminal-cells-us35kg3i/`. Starting sources/ledger/
status, hash-matched validated Pass 18 app, equivalent snapshot probe
baseline, retained release test binaries, raw alternating probes, focused
before/after tests, corrected real-view diagnostics, validation stage
commands, native reports/captures, pass-only diff, final source/status
snapshots, `final-integrity.json`, and `validation-summary.json` are retained.

### Measured gap and bounded change

Every visible non-spacer cell in the terminal snapshot builder created a
temporary owned string. Adjacent cells with the same style immediately
copied that text into an existing run and discarded the temporary.
`TermView` builds this snapshot during painting.

Merged cells now append their character and zero-width marks directly to
the run's owned text. Only a new run constructs its text string. Run
boundaries, widths, colors, styles, trimming, cursor, selection, scrollback,
and full text remain unchanged. No cache, public API/model/schema,
dependency, layout token, timer, or animation was added.

This removes scratch strings structurally on the merged path. New runs,
run-buffer growth, snapshot vectors, and the painter still allocate.
Snapshot construction still scans the visible grid; no global memory bound
or profiled heap/peak-RAM reduction is claimed.

### Focused verification and isolated measurements

- A frozen pre-change builder matches all snapshot fields in **96**
  comparisons: two widths, two heights, six blank/text/Unicode/style/gap
  fixtures, each through initial feed, selection, scrollback, and resize.
  Both the old and new implementation pass.
- An explicit regression verifies coalesced wide/combining text, style
  boundaries/cell endpoints, and independent snapshot ownership.
- A real GPUI detached-terminal fixture passes at **1440×900** and
  **900×560**. Actual viewport resizing and repeated paints retain full
  Unicode text, bold/underline/color fields, and selection.
- Initial real-view fixture failures are retained. `terminal-view` is not
  an observed test element, so the test checks the observed terminal panel
  and actual grid resize. Initial resize clears selection; the corrected
  fixture selects after layout settles. Neither is a production behavior
  fix. A stale formatted unit-test patch was reread before retrying.
- Formatting, strict offline/locked workspace/all-target Clippy, and
  whitespace checks pass.

`paired-results.json` records five alternating retained-release pairs with
no concurrent build, test, or native work. Each case takes 100 actual
`Terminal::snapshot()` calls on a **160-column × 48-row** detached grid:

| Grid text | Before median, 100 snapshots | After |
| --- | ---: | ---: |
| Blank | 17.018 ms | 5.791 ms |
| Uniform ASCII | 16.466 ms | 5.276 ms |
| Wide/combining Unicode | 12.583 ms | 4.980 ms |
| Alternating style per character | 17.008 ms | 17.205 ms |

The alternating-style control is slightly worse and has no claimed gain.
Fixture construction and VT parsing are outside timing; locking, snapshot
projection, and destruction are included. These are snapshot timings, not
GPUI shaping, terminal input, whole-app CPU, native frame cost, or RAM
measurements. No intentional pre-fix assertion failure is claimed.

Reproduce with
`cargo test --offline --locked --release -p flint-term --lib terminal_snapshot_perf_probe -- --ignored --nocapture --test-threads=1`.

### Full and native validation

- Offline/locked serial workspace: ACP **68**, agent **150**, app **156**
  (15 manual probes ignored, then run explicitly), UI **237**, terminal
  **19** (one new manual probe ignored, then run explicitly); integration
  and documentation tests pass.
- All **sixteen** release probes pass. Release terminal tests also pass.
  Explicit composer-refresh **2/2** and compact eight-pane **1/1** pass.
  Release app build completes in **47.02 s**.
- Sequential native `perf.py --no-live` against the validated Pass 18 app:
  **10/10**. `queue.py`: **6/6** state/capture checks, disposable homes and
  workspaces, no live provider.
- Working, paused, compact, empty, normal long-editor, and compact
  long-editor captures were inspected. Dark-and-ember hierarchy, readable
  queue metadata, task/composer separation, stop controls, and fixed
  Save/Cancel footer remain intact. Compact queue/welcome content scrolls.
- Final source/binary/report/capture hashes and starting status entries are
  verified. All **97** tracked modified paths remain; the only new status
  entry is the frozen terminal reference test. The new UI fixture is inside
  the already-untracked UI-test directory. No user-app restart, credential
  access, live provider, commit, publication, or additional loop occurred.

| Paired native metric | Before | After |
| --- | ---: | ---: |
| Cold start, window | 285.70 ms | 278.38 ms |
| First painted frame | 185.30 ms | 191.22 ms |
| Idle RSS | 76.11 MB | 76.28 MB |
| 200-turn RSS | 85.76 MB | 85.88 MB |
| Idle CPU | 0.75% | 0.75% |
| 2,000-delta/s CPU | 46.8% | 47.6% |
| Streaming frame interval p95 | 14.22 ms | 14.21 ms |
| Scroll frame interval p95 | 14.15 ms | 14.04 ms |
| Streaming dropped frames | 0 | 0 |
| Release binary | 32.4 MB | 32.4 MB |

Native RSS, first-frame time, and streaming CPU samples are worse. General
native fixtures do not exercise the large terminal grid. Passing budgets
and small differences do not establish a whole-app gain.

### Remaining limits

The last actual normal-picker **2.00%** still fails strict **`<2.0%`** and
was not remeasured. Native terminal glyph-placement/IME screenshots,
physical input, clipboard, hover, VoiceOver, live providers, major-state
CPU, and saved-stream durability remain outside this pass.

## Pass 20: align terminal link clicks with Unicode cell columns

Status: reproduced regressions, focused/full tests, strict checks, release
probes/build, sequential native validation, and six capture inspections are
complete. Passes 1–19 were already complete. Original baseline PID 69769
and previous validation PID 30864 were confirmed exited; the original
baseline log was reviewed. Validation PID 64223 also exited.

Evidence root: `target/terminal-link-gbq2v0wj/`. Starting sources/ledger/
status, hash-matched validated Pass 19 app, equivalent extracted pre-fix
helper, failing unit/real-view diagnostics, fixed/validated/final source
snapshots, raw test/probe/build logs, validation stage commands, paired
native reports, six inspected captures, pass-only diff, final status,
`final-integrity.json`, and `validation-summary.json` are retained.

### Reproduced gap and bounded fix

Terminal mouse positions use cell columns, but link hit ranges counted
Unicode scalars. A wide character before a URL shifted the range left;
combining marks shifted it right. Wide characters inside the URL also
shortened its clickable range.

The equivalent extracted helper reproduces two unit failures while the
ASCII control passes. The real Cmd-click fixture reproduces a false hit
on the space before a wide-prefixed file link, opening its local preview.
Raw failure logs are retained.

Link start/end columns now sum the same per-character terminal widths used
by the painter, including zero-width marks. A private borrowed helper
supports focused tests; the view still owns the selected URL before
emitting the existing event. The URL regex, schemes, full URL text, local
file-preview routing, ordinary selection, and public API are unchanged.
No dependency, cache, model/schema field, visual token, animation, or idle
repaint was added.

### Focused verification

- Three unit tests pass: wide/combining prefixes and exact outside/inside
  boundaries; wide and combining characters inside URLs; ASCII delimiters,
  multiple links, non-links, unsupported schemes, and out-of-range columns.
- The real GPUI Cmd-click fixture passes four cases: wide and combining
  prefixes at **1440×900** and **900×560**, with a styled URL that fits the
  actual viewport. Prefix/suffix misses do not open a preview; the wide
  link's last cell and combining link's first cell open the exact temporary
  local file, never the external browser.
- Mirrors do not start shells; homes/workspaces are isolated and credential
  sources disabled. No live model or real file outside the fixture is used.
- Formatting, strict offline/locked workspace/all-target Clippy, and
  whitespace checks pass.

Reproduce with
`cargo test --offline --locked -p flint-app --lib terminal_url_hits_ -- --test-threads=1`
and
`cargo test --offline --locked -p flint-app --test blueprint_ui terminal_command_click_uses_unicode_cell_columns_for_file_links -- --test-threads=1`.

### Full and native validation

- Offline/locked serial workspace: ACP **68**, agent **150**, app **159**
  (15 manual probes ignored, then run explicitly), UI **238**, terminal
  **19** (one manual probe ignored, then run explicitly); integration and
  documentation tests pass.
- All **sixteen** release probes pass. Release terminal tests pass.
  Explicit composer-refresh **2/2** and compact eight-pane **1/1** pass.
  Release app build completes in **47.38 s**.
- Sequential native `perf.py --no-live` against the validated Pass 19 app:
  **10/10**. `queue.py`: **6/6**, using disposable homes/workspaces and
  direct retained binaries.
- Inspected working, paused, compact, empty, normal long-editor, and compact
  long-editor captures. Dark-and-ember hierarchy, readable metadata,
  queue/task/composer separation, stop controls, and fixed visible
  Save/Cancel footer remain intact. Compact queue/welcome content scrolls.
- Fixed and validated sources, retained apps, reports/captures, and starting
  status entries are verified. All **97** tracked modified paths remain;
  the only new status entry is `crates/flint-app/src/term_view_tests.rs`.
  No user-app restart, real credential access, live provider, commit,
  publication, or additional loop occurred.

| Paired native metric | Before | After |
| --- | ---: | ---: |
| Cold start, window | 247.72 ms | 216.94 ms |
| First painted frame | 168.81 ms | 161.98 ms |
| Idle RSS | 76.28 MB | 76.39 MB |
| 200-turn RSS | 86.29 MB | 86.23 MB |
| Idle CPU | 0.75% | 0.50% |
| 2,000-delta/s CPU | 47.0% | 48.6% |
| Streaming frame interval p95 | 14.12 ms | 14.00 ms |
| Scroll frame interval p95 | 13.63 ms | 13.84 ms |
| Streaming dropped frames | 0 | 0 |
| Release binary | 32.4 MB | 32.4 MB |

This is a correctness fix, not a measured speedup. Native fixtures do not
exercise Unicode terminal link clicks. Idle RSS, streaming CPU, and scroll
cadence samples are worse; improvements in other samples and passing
budgets do not establish whole-app gains.

### Remaining limits

Full snapshots/text-line copies and regex/prefix scans remain. Wrapped
links and additional URL schemes were not implemented. The last actual
normal-picker **2.00%** still fails strict **`<2.0%`** and was not remeasured.
Physical input, native glyph-placement/IME screenshots, clipboard, hover,
VoiceOver, live providers, major-state CPU, and saved-stream durability
remain outside this pass.

## Pass 21: avoid a second full draft copy for composer queries

Status: focused/full tests, strict checks, isolated retained-release
measurements, release build, sequential native validation, and capture
review/comparison are complete for the copy-removal fix. Passes 1–20 were
already complete. Original baseline PID 69769 and previous validation PID
64223 were confirmed exited and the original baseline log reviewed.
Validation PID 68135 also exited. An unchanged compact native control gap
is recorded below, not claimed fixed.

Evidence root: `target/composer-query-m878drp4/`. Starting sources/ledger/
status, hash-matched validated Pass 20 app, equivalent pre-fix projection,
real-textarea ownership failure/probes, retained release test binaries,
five raw alternating pairs, corrected fixture diagnostics, measured/
validated/final source snapshots, full validation stage commands, native
reports and captures, separate baseline/current scene comparisons,
`initial-capture-review.json`, `compact-comparison.json`, pass-only diff,
final status, `final-integrity.json`, and `validation-summary.json` are
retained.

### Proven copy and bounded change

GPUI's textarea `value()` materializes its rope text into a `SharedString`.
Flint's `composer_text()` then made another full owned `String`. Composer
change/menu parsing and selected-slash lookup only read that text.

Those two consumers now use a private projection returning the original
`SharedString` from `value()`. The owned `composer_text()` API and mutating
mention replacement remain unchanged. Query text, picker ordering,
keyboard routes, sent/queued payloads, and full draft ownership are not
changed. No cache, dependency, public API/model/schema change, layout
token, timer, or animation was added.

The second copy is structurally removed for these read-only consumers.
GPUI still materializes the full value; query parsing/indexing, owned
editing/sending paths, and widget shaping can still allocate or scan the
whole draft. This is not incremental or globally bounded input storage.

### Focused verification and retained diagnostics

- A real textarea ownership regression retains a full multiline Unicode
  projection across an input change. Cloned projections share the same
  payload pointer. The equivalent `String` baseline fails that pointer
  assertion; the implementation passes.
- Real-view preservation test passes at **1440×900** and **900×560**: a
  bulk-seeded large multiline Unicode draft, actual suffix typing and file
  indexing, Enter mention selection, complete owned replacement, full
  annotated send payload, and selected slash command routing.
- Initial fixture compile errors are retained: an entity was incorrectly
  used as a renderable root, a window context was incorrectly annotated as
  `App`, and an integration fixture tried to seed a private file index.
  The corrected tests use a minimal renderable root, real textarea/entity
  APIs, and the actual indexing path. No production visibility was widened.
- Simulating the whole large draft timed out: GPUI Kit's test input renders
  after every character. The corrected test bulk-seeds the draft, focuses
  the input, moves to its end with the real key binding, then types only the
  query suffix. A retained intermediate failure reflects `set_value()`'s
  multiline caret reset, not a production behavior fix. The timed-out test
  process was confirmed absent before continuing.
- Formatting, strict offline/locked workspace/all-target Clippy, and
  whitespace checks pass.

### Isolated extraction measurements

`paired-results.json` records five alternating retained-release pairs
without concurrent builds, tests, or native work. Each case takes 100
actual projection calls on GPUI textarea state:

| Draft | Before median, 100 extractions | After |
| --- | ---: | ---: |
| 8 MiB ASCII | 121.804 ms | 121.420 ms |
| Approximately 8 MiB Unicode | 123.758 ms | 108.235 ms |
| Short ordinary draft | 0.015 ms | 0.013 ms |

ASCII shows no meaningful improvement. The short control is too small for
a useful gain claim. Timing includes window update/entity read, value
extraction, and returned-value destruction. Fixture setup, menu parsing,
indexing, keystroke handling, shaping, native RSS, and peak RAM are excluded.
No whole-app, heap-allocation-count, or RAM gain is inferred.

Reproduce with
`cargo test --offline --locked --release -p flint-app --lib composer_query_perf_probe -- --ignored --nocapture --test-threads=1`.

### Full and native validation

- Offline/locked serial workspace: ACP **68**, agent **150**, app **160**
  (16 manual probes ignored, then run explicitly), UI **239**, terminal
  **19** (one manual probe ignored, then run explicitly); integration and
  documentation tests pass.
- All **seventeen** release probes and release terminal tests pass.
  Original composer-refresh **2/2** and compact eight-pane **1/1** pass.
  Release app build completes in **50.62 s**.
- Sequential native `perf.py --no-live`: **10/10**. Initial `queue.py`:
  **6/6** state/nonblank-capture checks, with disposable homes/workspaces.
- Six initial captures were inspected. Normal running task/Stop controls,
  dark-and-ember hierarchy, readable metadata, queue/composer separation,
  and fixed visible Save/Cancel footer remain intact. Compact queue/editor
  captures omit the task tray and Stop despite a running state.
- Sequential separate retained-baseline/current `queue.py` runs both pass
  **6/6** generic checks. Four additional compact/editor-compact captures
  and their running-state dumps reproduce the same missing controls on both
  apps. This is an unresolved native layout issue, not a verified regression
  or fix in this pass. The generic script does not assert those controls.
- Measured/validated sources, retained apps, reports/captures, and starting
  status entries are verified. All **97** tracked modified paths remain;
  the only new status entry is `crates/flint-app/src/app_input_tests.rs`.
  No user-app restart, real credential access, live provider, commit,
  publication, or additional loop occurred.

| Paired native metric | Before | After |
| --- | ---: | ---: |
| Cold start, window | 285.02 ms | 279.29 ms |
| First painted frame | 190.39 ms | 183.31 ms |
| Idle RSS | 77.50 MB | 87.00 MB |
| 200-turn RSS | 86.74 MB | 86.62 MB |
| Idle CPU | 0.75% | 0.50% |
| 2,000-delta/s CPU | 59.0% | 57.0% |
| Streaming frame interval p95 | 14.22 ms | 14.22 ms |
| Scroll frame interval p95 | 14.29 ms | 14.27 ms |
| Streaming dropped frames | 0 | 0 |
| Release binary | 32.4 MB | 32.4 MB |

Idle RSS worsens by **9.50 MB** in this paired sample; its cause was not
established. One current scroll run drops a frame (>25 ms), versus zero in
the baseline runs. General native scenes do not exercise large draft
extraction. Passing budgets and other sample improvements do not establish
a whole-app performance or memory gain.

### Remaining limits

The compact native task/Stop gap remains open. The last actual normal-picker
**2.00%** still fails strict **`<2.0%`** and was not remeasured. Full draft
materialization, physical input/paste, native glyph-placement/IME
screenshots, clipboard, hover, VoiceOver, live providers, major-state CPU,
and saved-stream durability remain outside this pass.

## Pass 22: require fresh, inactive native composer captures

Status: complete. Focused regressions, strict checks, full workspace tests,
all seventeen release probes, release build, sequential native performance
and capture checks, and final screenshot inspection pass. Validation PID
23039 was confirmed exited. The existing loop remains `318bcc2b`; no second
loop was created.

Evidence root: `target/compact-controls-5_31obu_/`. The validated Pass 21
app, starting status and 232 file hashes, starting sources, rejected
prototypes, retained native state/pixels, focused/validated/final sources,
pass-only diff, validation commands/logs, before/after gate comparison,
release app/test binaries, capture reviews, preservation checks and final
artifact hashes are retained.

### Diagnosis, not a composer layout regression

The previously reported compact task/Stop omission was a stale native
capture. The real demo path, stopped at beat 27 with its queue open, passes
running-state, task-count and on-screen control bounds at both 1440×900 and
900×560 before the capture fix.

The retained centered compact background window still shows its initial
composer at both one and four seconds. GPUI's macOS backend stops its
display link when the window is completely covered. Separately, an instant
background demo can finish and write the state file before the requested
700 ms timed dump or the running composer's first paint. A running model
dump was therefore not proof that the captured frame had caught up.

A popup prototype restored drawing but unexpectedly became key. It was
rejected and removed. The accepted path keeps the ordinary window kind,
`focus=false`, and no app activation. Only explicit capture runs open at
the primary display edge. The actual composer layout, model events,
task/Stop behavior and dark-and-ember tokens are unchanged.

### Bounded capture fix and checks

`FLINT_BP_CAPTURE=1` requires a state path and a valid timed dump. It
suppresses earlier turn-end dumps, requests a fresh frame after the delay,
and writes readiness on the following frame, after paint. Guarded,
non-interactive canvas probes record Send, Stop and task-tray bounds,
content-mask/viewport containment, paint time and whether the window is key.
Outside capture mode no probes or capture callbacks are created.

`queue.py` now requires Send in every scene, the expected running state and
one live command in working scenes, fully visible Stop/task-tray controls,
paint after the settling delay, and an inactive window. Idle scenes must
not report running controls. It still checks queue counts, pause/editor
state, nonblank captures and panics.

- Four Python regressions cover valid running/idle paints and rejection of
  missing legacy provenance, stale state, missing/clipped/key controls,
  early paints and incorrect task counts.
- The stricter gate reports **0/6** on the retained baseline and **6/6** on
  the fixed capture path. The baseline lacks the new paint provenance;
  these are not six production UI bugs. Separate retained compact pixels
  and the inactive edge-window paint establish the freshness problem.
- The final independent six-scene run also passes **6/6**. All six captures
  were inspected: running controls are visible at both sizes, both editor
  footers retain Save/Cancel, and compact queue content scrolls above the
  composer. The empty compact scene still clips lower suggestion content.

### Full validation and honest native samples

- Offline/locked serial workspace: ACP **68**, agent **150**, app **160**,
  UI **240**, terminal **19**; integration and documentation tests pass.
  All **seventeen** ignored release probes are explicitly run and pass.
- Composer regression group **4/4**, including the two original interaction
  checks and the new two-size demo regression; compact eight-pane **1/1**.
- Formatting, strict workspace/all-target Clippy with `-D warnings`, and
  whitespace checks pass. The initial collapsible-if warning was corrected.
- Sequential `perf.py --no-live`: **10/10**, then the stronger native
  `queue.py`: **6/6**, using disposable homes/workspaces. Capture mode is
  not enabled during performance measurement.

| Paired native metric | Before | After |
| --- | ---: | ---: |
| Cold start, window | 267.00 ms | 268.02 ms |
| First painted frame | 171.20 ms | 170.53 ms |
| Idle RSS | 77.23 MB | 77.20 MB |
| 200-turn RSS | 86.94 MB | 86.55 MB |
| Idle CPU | 0.50% | 0.50% |
| 2,000-delta/s CPU | 45.4% | 47.4% |
| Streaming frame interval p95 | 14.23 ms | 14.21 ms |
| Scroll frame interval p95 | 14.21 ms | 14.32 ms |
| Streaming dropped frames | 0 | 0 |
| Release binary | 32.4 MB | 32.5 MB |

Streaming CPU and scroll p95 worsen in this sample; binary size increases.
This is a correctness-only harness fix. No speedup, RAM gain, production
layout correction or causal performance explanation is claimed.

### Preservation and remaining limits

Starting status entries and unrelated starting file contents are preserved.
All **97** tracked modified paths remain; the only new status entry is
`tools/blueprint/test_queue.py`. Validated sources and retained app hashes
match the final sources/build. No user-app restart, live provider, commit,
push or publication occurred.

Display-edge placement cannot guarantee an uncovered window on every
desktop. A covered window fails the completed-paint timeout instead of
passing stale pixels. Paint bounds do not prove all overlay/pixel occlusion,
physical input, hover, clipboard, IME or VoiceOver behavior. The last actual
picker **2.00%** still fails strict **`<2.0%`** and was not remeasured.
Pass 21's idle-RSS increase remains unexplained; this neutral sample does
not establish a memory fix. Full draft materialization, live-provider and
saved-stream durability checks remain outside this pass.

## Pass 23: borrow the composer's running-task projection

Status: complete. Focused compatibility/ownership and real-view regressions,
strict checks, full workspace tests, all eighteen release probes, release
build, five final isolated release pairs, sequential native checks, and six
capture inspections pass. Validation PID 81547 was confirmed exited.
Original baseline PID 69769 and Pass 22 PID 23039 were absent before work;
the original baseline log was reviewed before new measurements. Passes
1–22, including the original composer/output changes, were already complete.

Evidence root: `target/welcome-actions-bb1zca3h/`. The name reflects the
initial declined welcome-screen investigation. It retains the validated
Pass 22 app, starting status and 233 source hashes, starting/observed/
measured/validated/final sources, negative investigation diagnostics,
frozen filter oracle, focused tests, retained release app/test binaries,
preliminary and final raw pairs, validation stage commands/logs, native
reports/captures, pass-only diff, preservation checks and artifact hashes.

### Declined lead

The initial compact welcome screenshot clips the last suggestion at its
initial scroll position. An observed real-view probe found its top at
560 px and height at 40 px, then scrolled the readiness area by -260 px.
The suggestion became fully reachable and pointer activation populated
the draft. This did not establish a layout bug.

Initial missing-observation and implicit-focus-binding diagnostics were
fixture limitations, not proof of broken keyboard activation. Temporary
welcome production/test instrumentation was reverted exactly to preserved
starting content. No welcome layout or keyboard fix is claimed.

### Proven allocation and bounded change

The composer collected a fresh `Vec<&ToolCall>` to obtain the count and
render live command rows on every render. Its equivalent baseline retains
that collection; the final private projection returns the exact count and
a borrowed iterator instead.

Eight optional reference slots keep common small trays on one scan, even
with completed tools before them. Any additional commands remain in a
borrowed tail; none are dropped or reordered. The tail is scanned again
for its exact count when there are more than eight live commands. The
public `running_commands() -> Vec<&ToolCall>` API remains intact and uses
the same filter. Original calls, output, summaries, row IDs, counts,
completion filtering and turn boundaries are retained.

The render path structurally removes the reference-vector allocation.
Baseline capacities for one/eight/512 live commands are **32/64/4096 B**;
these are vector capacities, not profiled peak-memory figures. The eight
inline references have bounded stack storage, but iterator state, GPUI
rows and strings still consume memory. No cache, dependency, model/schema
change, layout token, timer or animation is added.

### Focused and full verification

- Two model regressions compare the frozen pre-change filter across
  **15** live/completed combinations, including the eight/nine boundary,
  exclude a non-command tool, and check exact order and original-call
  pointer identity. The public vector result stays equivalent.
  Interrupted/empty/new turns and missing/invalid current-turn references
  retain their previous empty behavior.
- A real-view regression at **1440×900** and **900×560** renders nine
  commands without dropping the ninth, preserves order, removes a completed
  command, excludes a read tool, queues a follow-up and routes Stop
  to Interrupt. An interrupted turn removes the tray.
  A missing fixture argument was corrected without production changes.
- Offline/locked serial workspace: ACP **68**, agent **150**, app **162**,
  UI **241**, terminal **19**; integration and documentation tests pass.
  All **eighteen** ignored release probes are explicitly run and pass.
- Composer group **5/5**, including the original queue/Stop checks, and
  compact eight-pane **1/1** pass. Release build: **52.86 s**.
  Formatting, strict workspace/all-target Clippy with `-D warnings`, and
  whitespace checks pass.

### Isolated helper measurements and negative control

`final-paired-results.json` retains five alternating release-binary pairs,
without concurrent builds, tests or native work. Each sample projects an
exact count and consumes borrowed call-ID lengths **1,000** times:

| Current-turn fixture | Before median | After |
| --- | ---: | ---: |
| One live command | 0.022 ms | 0.007 ms |
| Eight live commands | 0.061 ms | 0.011 ms |
| 512 live commands | 1.301 ms | 1.407 ms |
| Eight live, 2,048 completed commands | 2.783 ms | 2.744 ms |
| 2,048 completed commands, no live command | 2.697 ms | 2.696 ms |

The small live cases improve. Mixed/completed scans show no meaningful
gain. The **512-live-command stress case worsens**, consistent with the
extra tail scan; that result is retained, not averaged away. Timing includes
count/iterator construction, borrowed ID-length consumption and destruction,
but excludes fixture setup, GPUI rows, output formatting, layout, painting
and native memory. These timings do not establish a whole-app speedup.

Reproduce with
`cargo test --offline --locked --release -p flint-app --lib running_command_projection_perf_probe -- --ignored --nocapture --test-threads=1`.

### Native validation and remaining limits

Sequential `perf.py --no-live`: **10/10**, then native `queue.py`: **6/6**
with disposable homes/workspaces. All six captures were inspected.
Normal/compact running controls, task count, dark-and-ember styling,
queue/composer separation and visible editor Save/Cancel footer remain
intact. Capture windows remain inactive.

| Paired native metric | Before | After |
| --- | ---: | ---: |
| Cold start, window | 352.14 ms | 302.29 ms |
| First painted frame | 229.67 ms | 211.99 ms |
| Idle RSS | 77.36 MB | 77.03 MB |
| 200-turn RSS | 91.55 MB | 86.32 MB |
| Idle CPU | 0.75% | 0.25% |
| 2,000-delta/s CPU | 49.8% | 48.4% |
| Streaming frame interval p95 | 14.04 ms | 14.04 ms |
| Scroll frame interval p95 | 14.12 ms | 14.12 ms |
| Streaming dropped frames | 0 | 0 |
| Release binary | 32.5 MB | 32.5 MB |

These samples do not isolate running-task projection cost; their differences
do not establish whole-app CPU or RAM gains. Current-turn scanning and
over-eight-tail counting remain linear. Public vector consumers, tool
output formatting, widget allocations and full transcript storage remain.
The latest actual picker **2.00%** still fails strict **`<2.0%`** and was
not remeasured. Physical input, hover, clipboard, IME, VoiceOver, live
providers and saved-stream durability remain outside this pass.

Starting git status is unchanged: all **97** tracked modified paths and
all untracked entries remain. Unrelated starting file hashes, measured/
validated final sources and retained app hashes are verified. No user-app
restart, real credential access, live provider, commit, push, publication,
additional loop or overlapping pass occurred.

## Validation checkpoint and Pass 24 — native large-diff frame evidence

**Status: verified complete.** The requested comprehensive offline validation
finished before this next improvement pass. The sole existing loop
`318bcc2b` was retained; no duplicate loop or overlapping pipeline was started.

Checkpoint evidence: `target/validation-checkpoint-u5b4djf4`.
Pass evidence: `target/diff-frames-l9gr9dq3`.

### Comprehensive current-tree checkpoint

- Preserved the validated Pass 23 app, starting git status/diff and **233**
  file hashes. All starting contents and git status matched after validation.
- Checked retained evidence for all **23** earlier passes: **1,459** recorded
  hashes matched their archived source or artifact files.
- Offline/locked serial **debug and release workspace suites** passed:
  ACP **68**, agent **150**, app **162**, UI **241**, terminal **19**, plus
  integration and documentation tests. The explicitly gated live tests did
  not exercise a real provider.
- Formatting, strict workspace/all-target Clippy with `-D warnings`,
  whitespace, **four** Python capture-gate regressions, syntax parsing of
  the blueprint Python scripts, **18** ignored release probes and the release
  build passed.
- Sequential native states **100/100**, persistence/terminal/session workloads
  **71/71**, performance `--no-live` **10/10**, and fresh queue captures
  **6/6** passed. The workloads include saved streaming, mirrored terminal
  output and restoring **10/200/1,000** completed sessions.
- The **10,000-addition diff gate failed 13/16**: all three runs retained the
  additions and exited normally, but each recorded only two startup renders
  and **zero** steady-state frames. Raw failures and resulting `NaN` frame
  metrics remain in the checkpoint. Those samples are invalid performance
  evidence, not a speedup.

The 15-state and five-workload contact sheets, large-diff image and all six
fresh queue captures were inspected. This sweep is executable validation,
not an independent line-by-line audit of every preexisting dirty change.

### Reproduction, cause and bounded fix

An isolated diagnostic ran the **unchanged retained binary** with the existing
capture placement enabled. The diff gate then passed **16/16**, recording
**328/329/329** steady-state frames. A completely covered centered background
window can stop macOS display-link frames, as established during Pass 22.
The failing image stayed at the first diff rows; the diagnostic and corrected
images show scrolling well into the generated patch.

The launcher now uses the existing normal display-edge placement for a
background `--diff-test` window **only when `FLINT_BP_FRAMES` is supplied**.
Foreground launches, ordinary background windows and other performance
workloads keep their existing placement. Window kind, focus/activation
policy, rendering, patch contents, scroll cadence and gate thresholds are
unchanged. The diff frame probe does **not** enable capture mode or its
post-paint readiness instrumentation. The blueprint README documents this
measurement requirement.

A launcher regression covers all **eight** combinations of background,
capture and diff-frame-probe state. It passes in debug and release.
The initial corrected native diff run passed **16/16**, with **337/342/339**
steady-state frames.

### Final verification

The serial final pipeline exited before closeout. Formatting, strict
workspace/all-target Clippy, whitespace and Python capture gates pass.
The full debug workspace repeats ACP **68**, agent **150**, app **162**,
UI **241**, terminal **19**, plus the new **one** launcher regression;
integration and documentation tests pass. The release launcher regression,
all **18** ignored release probes and release build pass.

All final native gates pass without changing their criteria:

| Native gate | Result |
| --- | ---: |
| Fifteen UI states | 100/100 |
| Three large-diff scroll runs | 16/16 |
| Saved-stream, terminal and restored-session workloads | 71/71 |
| Performance, no live provider | 10/10 |
| Fresh inactive queue/composer captures | 6/6 |

The final diff runs record **384/333/332** steady-state frames. Their frame
interval p95 values are **13.96/13.96/14.03 ms**, with all 10,000 additions
retained. The invalid before samples do not support a before/after CPU or
memory comparison. This is a frame-evidence reliability correction,
**not a production rendering optimization or whole-app CPU/RAM gain**.

Final performance samples: window **274.80 ms**, first painted frame
**183.23 ms**, idle RSS **76.97 MB**, 200-turn RSS **86.54 MB**, idle CPU
**0.25%**, streaming CPU **48.4%**, stream/scroll interval p95
**14.24/14.25 ms**, **zero** dropped streaming frames, binary **32.5 MB**.
These are budget checks, not gains attributed to the launcher change.

Both new state sweeps measured the picker at **1.0%**, passing strict
`<2.0%`. The earlier actual **2.00%** failure is retained. No picker fix or
reliable resolution of that intermittent budget risk is claimed.

The final state/workload contact sheets, scrolling diff image and six queue
captures were inspected, including full-size compact task/Stop and editor
footer captures. Dark-and-ember styling and control visibility remain intact.
All **230** unrelated starting file hashes match, the validated launcher/
README and retained app match, and git status remains unchanged with **97**
tracked modified paths and all original untracked work.

Physical input, hover, clipboard, IME, VoiceOver and real-provider checks
remain excluded. Saved-stream workloads verify accepted event count/order
after normal exit, not crash durability or slow-storage backpressure.
No user-app restart, real credential access, commit, push or publication
occurred.

## Pass 25 — borrowed Unicode head/tail excerpts

**Status: verified complete.** The original composer refresh, incremental
bounded tool output and terminal-line selection remain validated. Baseline
PID **69769** and both Pass 24 validation PIDs had exited before this pass;
the original baseline log and current ledger/status were reviewed first.

Evidence: `target/output-ends-hcmsg5ax`. The starting app matches the
validated Pass 24 app. Starting source snapshots, git status and **233**
file hashes were retained before changes.

### Measured gap and bounded change

`flint_agent::tools::head_tail` counted all source scalars, collected two
temporary owned strings for truncated output, and scanned forward across
the source again to find its tail. Command results, fetched text, MCP and
subagent output, and app command/terminal forwarding use this helper.

The truncated branch now finds UTF-8 byte boundaries from the start and
end and borrows both slices when formatting the final owned result.
The exact total-scalar count, 40%/60% split, omission count and marker,
whitespace, public signature, output ownership and short-input branch
are unchanged. The source is not modified or shortened.

This removes the two temporary end strings where they held output and
limits tail selection to the requested tail rather than a forward scan of
the omitted prefix. It adds no dependency, cache, timer, repaint, layout or
model state. Exact total counting still scans the full input; source/caller
storage and the final result allocation remain. This is not a profiled
peak-memory result.

### Focused correctness and isolated measurements

- A frozen pre-change reference matches **5,184** combinations of ASCII,
  CJK, emoji, combining marks, NUL/tab/CR/LF, trailing whitespace and budgets
  including zero, tiny values, scalar boundaries, 8,000 and `usize::MAX`.
- A second regression compares three large outputs to the frozen reference,
  retains exact leading/trailing text and verifies the result stays owned
  after the input is dropped. Together with the existing both-ends test,
  the focused group passes **3/3** before and after the change.
- `paired-results.json` preserves five alternating retained release-binary
  pairs, run without concurrent builds, tests or native scripts. Each sample
  makes **100** calls and drops their owned results. Fixture setup is excluded.

| Helper fixture | Before median | After |
| --- | ---: | ---: |
| Short, untruncated | 0.005 ms | 0.006 ms |
| Exactly 8,000 ASCII scalars | 0.036 ms | 0.039 ms |
| 32,000 ASCII scalars | 0.777 ms | 0.552 ms |
| 32,000 Unicode scalars, 112,000 bytes | 2.158 ms | 1.708 ms |
| 8 MiB ASCII | 52.524 ms | 27.738 ms |
| Approximately 8 MiB Unicode | 53.402 ms | 28.317 ms |
| 3.5 MB Unicode, zero budget | 21.435 ms | 11.321 ms |

The truncated fixtures improve. The short and exactly-at-limit controls
show small regressions and no gain; these results are retained. The
32,000-scalar fixtures are synthetic output classes, not recordings of a
real command capture. The timings cover counting, excerpt selection,
formatting/allocation and destruction, not engine streaming, persistence,
GPUI rendering or native CPU/RAM. No whole-app speedup is claimed.

Reproduce with
`cargo test --offline --locked --release -p flint-agent --lib output_head_tail_perf_probe -- --ignored --nocapture --test-threads=1`.

### Full, release and native verification

The serial pipeline exited before closeout. Offline/locked workspace:
ACP **68**, agent **152**, app **162**, UI **241**, terminal **19**, launcher
**1**, plus integration/documentation tests, all pass. Release agent tests
**152/152**, all **19** explicitly requested ignored release probes,
composer **5/5**, compact eight-pane **1/1**, release build, formatting,
strict workspace/all-target Clippy with `-D warnings`, whitespace and
the **four** Python capture-gate regressions pass.

Sequential isolated native diff **16/16**, performance `--no-live` **10/10**
and fresh queue/composer **6/6** pass. The scrolling diff and all six queue
captures were inspected. Normal/compact task tray and Stop, queue separation,
editor Save/Cancel footer and dark-and-ember styling remain intact.

Current native budget samples: window **284.45 ms**, first painted frame
**174.62 ms**, idle RSS **77.00 MB**, 200-turn RSS **86.14 MB**, idle CPU
**0.25%**, streaming CPU **49.4%**, stream/scroll interval p95
**14.10/14.15 ms**, **zero** dropped streaming frames, binary **32.5 MB**.
These are current budget checks, not paired gains attributed to this helper.
Picker CPU was not remeasured; the earlier **2.00%** failure and Pass 24
**1.0%** samples remain the available evidence.

All **230** unrelated starting file hashes match. Measured and fully
validated final sources match, the retained app matches the release app,
and git status remains unchanged with **97** tracked modified paths and
all original untracked work.

Full-source counting and retained payloads, the over-eight-task extra scan,
persistence/backpressure and intermittent picker budget remain investigation
leads. Physical input, hover, clipboard, IME, VoiceOver, live providers and
crash/slow-storage durability checks remain excluded. No real credential
use, user-app restart, commit, push, publication, duplicate loop or
overlapping pass occurred.

## Pass 26 — borrowed composer text eligibility

**Status: verified complete.** The ledger, current dirty tree, original
baseline log and PID **69769** were checked before benchmarks. That PID
and the Pass 25 pipeline had exited. The original composer refresh,
incremental bounded output and bounded terminal-line selection remain
validated; no overlapping pass or new loop was started.

Evidence: `target/composer-eligibility-mna7hi0s`. The starting app matches
validated Pass 25. Starting snapshots, git status and **233** file hashes
were retained before changes.

### Proven gap and bounded fix

Each composer render called `TextareaState::value()` and trimmed its
materialized full string to decide whether Send/Steer should accept a draft.
The pinned GPUI dependency explicitly materializes the rope on every
`value()` call and exposes `text()` as a borrowed rope. The inspected
methods are recorded in `dependency-evidence.txt`.

The private `composer_has_text` projection now borrows rope chunks and
checks whether any chunk contains non-whitespace text. Rope chunks end on
UTF-8 boundaries, so this preserves the old `!value().trim().is_empty()`
predicate, including Unicode whitespace. It stops after finding a
nonblank chunk and does not flatten the draft into a temporary string.
The surrounding image-only, stopping, pending-paste, permission and
steering guards are unchanged.

Owned editing/sending/query APIs, full draft contents, selections, menus,
layout, accessibility roles, palette and animation/repaint scheduling
are unchanged. No cached flag, dependency or model state is added.
All-whitespace drafts and long whitespace prefixes still require a linear
scan. Textarea layout/painting and other full-value extraction paths remain.
Avoiding this full-value materialization is structural, not a profiled
whole-app or peak-RAM result.

### Focused verification and measured scope

- One actual `TextareaState` regression compares **168** combinations of
  empty/ASCII/Unicode whitespace, CR/LF, chunk-sized prefixes, emoji,
  combining marks, NUL and zero-width space with the original trimmed-value
  predicate, and checks the full stored text is unchanged. It passes before
  and after the change.
- A real-view regression at **1440×900** and **900×560** keeps whitespace-only
  drafts unsent and unmodified, then pointer-sends the complete nonblank
  Unicode/NUL/multichunk draft with the existing submission trimming.
  It also passes before and after.
- The initial fixture's native disabled-flag observation returned `None`,
  not evidence of a production bug. That unsupported assertion was removed;
  the fixture verifies real pointer behavior and complete text instead.
  This is not a native disabled-flag or VoiceOver compliance claim.
- Five alternating retained release-binary pairs run **100** checks each on
  real textarea state through GPUI window updates, with no concurrent
  builds, tests or native scripts. Draft setup, editing, layout, paint and
  engine submission are excluded.

| Eligibility fixture | Before median | After |
| --- | ---: | ---: |
| Empty | 0.011 ms | 0.011 ms |
| Short ordinary draft | 0.013 ms | 0.010 ms |
| 8 MiB nonblank ASCII | 132.204 ms | 0.016 ms |
| Approximately 8 MiB nonblank Unicode | 114.696 ms | 0.017 ms |
| 1 MiB whitespace-only ASCII | 86.488 ms | 80.980 ms |
| Approximately 1 MiB whitespace-only Unicode | 66.070 ms | 60.944 ms |
| 1 MiB spaces followed by text | 86.703 ms | 81.411 ms |

The large nonblank helper checks improve by avoiding materialization and
stopping early. Empty is neutral; the whitespace stress cases remain
linear and relatively costly. These timings are not composer-render,
large-draft frame, typing-latency, native CPU or RSS measurements.

Reproduce with
`cargo test --offline --locked --release -p flint-app --lib composer_eligibility_perf_probe -- --ignored --nocapture --test-threads=1`.

### Full, release and native verification

The serial pipeline exited before closeout. Offline/locked workspace:
ACP **68**, agent **152**, app **163**, UI **242**, terminal **19**, launcher
**1**, plus integration/documentation tests, all pass. All **20** ignored
release probes are explicitly run and pass. Composer group **6/6**,
compact eight-pane **1/1**, release build, formatting, strict workspace/
all-target Clippy with `-D warnings`, whitespace and the **four** Python
capture-gate regressions pass.

Sequential isolated native diff **16/16**, performance `--no-live` **10/10**
and fresh queue/composer **6/6** pass. The scrolling diff and all six queue
captures were inspected. Normal/compact task tray and Stop, queue separation,
editor Save/Cancel footer and dark-and-ember styling remain intact.

Current native samples: window **263.80 ms**, first painted frame
**169.67 ms**, idle RSS **77.08 MB**, 200-turn RSS **100.13 MB**, idle CPU
**0.50%**, streaming CPU **42.2%**, stream/scroll interval p95
**14.09/14.17 ms**, **six** dropped streaming frames, binary **32.5 MB**.
The higher long-session RSS and nonzero dropped frames are retained.
These unpaired samples pass their budgets but do not establish a whole-app
CPU/RAM improvement or a regression caused by this eligibility change.
Picker CPU was not remeasured.

All **228** unrelated starting hashes match; measured and fully validated
final sources match, the retained app matches the release app, and git
status remains unchanged with **97** tracked modified paths and all
original untracked work.

Full-value query/layout materialization, whitespace scans, over-eight-task
projection, persistence/backpressure and the intermittent picker budget
remain gaps. Physical input, hover, clipboard, IME, VoiceOver, live providers
and crash/slow-storage durability remain excluded. No real credential use,
user-app restart, commit, push or publication occurred.

## Pass 27: borrowed raw-empty approval shortcut checks

Status: completed for the bounded input predicate below. Focused before/after
regressions, five retained release pairs, the corrected full/strict/release
pipeline and sequential native captures pass. The user's already-open app
and its release executable were left unchanged.

Evidence root: `target/approval-empty-77on60kr/`. Starting source/status/hashes
and the Pass 26 app are retained, along with both measured test binaries,
source snapshots, ten paired logs, dependency API/lock evidence, validation
logs, nine native screenshots and the pass-only diff.

### Verified cost and fix

Every key event reaching the pending-approval branch materialized the full
composer draft through `TextareaState::value()` just to test exact emptiness.
The resolved `gpui-base` accessor calls `self.text.to_string()`. Its public
`text()` accessor borrows that same rope, and the resolved
`ropey 2.0.0-beta.1` documents `len()` as an O(1) byte-length query.

The private `composer_is_empty` now uses `state.text().len() == 0`.
This is deliberately separate from the trimmed `composer_has_text` predicate:
ASCII/Unicode whitespace, zero-width text and NUL are nonempty drafts and must
continue to block plain Y/A/N approval shortcuts. The key/modifier matching,
oldest-request routing, broad-approval confirmation, full draft and
Command-Enter behavior remain unchanged. No dependency, cache, retained
state, layout, public API or animation was added.

### Predicate and actual-view regressions

The new actual-textarea oracle checks **nine** empty, whitespace, invisible,
NUL, combining/Unicode and multi-chunk cases against the original owned-value
predicate and verifies unchanged full content.

The new real-view fixture checks **30** scenarios: Y/A/N with five drafts at
**1440×900** and **900×560**. Empty drafts route the expected decision without
inserting a character; A still needs explicit broad confirmation. Nonempty
drafts, including whitespace and long Unicode text, remain pending, send no
approval and receive the typed letter without losing their original content.
Both fixtures pass before and after the optimization.

An initial compile attempt used the older rope `len_bytes` API. The retained
failure was corrected to the resolved beta's `len()` API before final
measurements and validation.

### Retained release pairs

Five alternating before/after pairs each run **100** raw-empty checks through
real textarea/window updates. Fixture creation, edit insertion, dispatch,
layout and paint are outside the timer. No builds, tests or native harnesses
ran concurrently with these pairs; the user's app remained running, so this
is not an otherwise idle-host or whole-app benchmark.

| Draft | Before median (ms) | After median (ms) |
| --- | ---: | ---: |
| Empty | 0.012 | 0.008 |
| Short ordinary text | 0.014 | 0.007 |
| 8 MiB ASCII | 127.683 | 0.007 |
| Approximately 8 MiB Unicode | 114.767 | 0.009 |
| 1 MiB ASCII whitespace | 5.041 | 0.008 |
| Approximately 1 MiB Unicode whitespace | 5.070 | 0.007 |

The structural result is removal of the unnecessary full-value copy from this
predicate. Sub-millisecond controls include timer/update overhead. The original
rope and the copies still needed by other query/edit/layout paths remain.
No end-to-end typing latency, peak-RAM or whole-app CPU gain is claimed.

### Full, release and native verification

All **14** corrected serial stages pass and the pipeline exits before
closeout. Offline/locked full workspace: ACP **68**, agent **152**, app **164**,
UI **243**, terminal **19**, launcher **1**, plus integration/documentation
tests. Release app **164**, all **21** ignored release probes, composer group
**7/7**, compact eight-pane **1/1**, formatting, strict workspace/all-target
Clippy with `-D warnings`, whitespace, Python syntax and **four** capture-gate
regressions pass.

The first full pipeline incorrectly applied `FLINT_BP_NO_ACTIVATE` to
headless tests, suppressing their initial composer focus. Its partial failures
and cancellation are retained under `failed-focus-setup/`. Only that owned
pipeline and its descendants were stopped. The corrected harness clears the
flag for cargo tests and sets it only for native captures; the full UI suite,
including launch-focus and approval-input regressions, passes.

To avoid rewriting the user's mapped executable, native validation uses a
separately named scratch launcher that compiles the unchanged real `main.rs`
against the updated app library. Every dependency version matches the
repository lock and the release profile matches the repository. Initial
scratch resolution selected newer cached libc/uuid versions; both were pinned
back to the existing lock before the final build and native runs. No root
manifest or lock changed.

Sequential native approval **2/2**, diff **16/16** and queue/composer **6/6**
pass with disposable homes/workspaces and inactive windows. All nine captures
were inspected, including both approval sizes and the compact editor at full
size. Approval buttons, request details, draft controls, task tray, Stop and
editor Save/Cancel remain readable and separated in the dark-and-ember UI.
Diff evidence has **332/332/332** frames, retains all **10,000** additions and
has valid interval p95 samples **14.07/14.02/13.92 ms**. These samples are
retained evidence, not paired performance gains.

Whole-app performance budgets, the major-state CPU sweep and workload sweep
were not rerun while the user's app was active. The user's process **45687**
kept the same identity and `target/release/flint` kept its starting hash at
every stage and closeout. The new native executable is archived separately;
it was not substituted into the open app.

All **229** unrelated starting hashes match; measured and fully validated
final sources match; native binary and dependency provenance match; git status
remains unchanged with **97** tracked modified paths and all original
untracked work. No physical input, real credentials, provider inference,
user-app restart, commit, push or publication occurred.

Remaining full-value query/layout materialization, whitespace eligibility
scans, large task projection, persistence/backpressure and intermittent picker
CPU remain investigation leads. Clipboard, hover, IME, VoiceOver, live-provider
and crash/slow-storage validation remain excluded.

## Pass 28: bounded slash-query normalization

Status: completed for the matching helper below. Before/after focused
regressions, five retained release pairs, all full/strict/release stages and
sequential native captures pass. The open user app and its executable remain
unchanged.

Evidence root: `target/slash-bound-tzfkoitk/`. Starting source/status/hashes,
the unchanged user executable, the preceding validated native app, both
measured test binaries, ten paired logs, source snapshots, fixture diagnostics,
locked-launcher provenance, full validation logs and nine screenshots are
retained. `pass28-only.diff` records this pass independently of existing work.

### Proven gap and bounded fix

`slash::matches` lowercased the entire query before comparing it with ten
short command names. Large unmatched Unicode queries therefore allocated and
normalized a full string even though no command could match. This helper is
used by input changes, menu rendering, navigation and command selection.

The helper now derives the longest command-name byte length, excluding `/`,
and examines at most that many Unicode scalars plus one before lowercasing.
Lowercasing does not remove scalars, and every scalar needs at least one UTF-8
byte; a query with more scalars than every name's byte length cannot be a
matching prefix. With the current list, the rejection inspects at most **nine**
scalars. Using scalar count rather than input byte length preserves the
Unicode lowercasing behavior even when byte length changes.

Overlong queries return the same empty vector without normalization. Accepted
queries still use the original lowercase matching and exact-name-first sort.
The bound follows the command list rather than a duplicated magic limit.
No dependency, retained cache, public API, draft truncation, layout, animation
or extra repaint was introduced.

### Focused regressions and fixture diagnostics

The frozen-reference oracle covers **369** command prefixes, uppercase
variants, mismatches, boundary sizes, Greek contextual casing, expanding
lowercase, Kelvin sign, combining text, Unicode, whitespace, NUL and invisible
text. A second regression compares large ASCII/Unicode queries with the
reference and verifies that the original query remains unchanged.

The new real-view fixture runs at **1440×900** and **900×560**. It seeds all
but the final scalar, delivers that scalar through the actual input route,
then verifies case-insensitive `/MO` navigation, a **57,345-byte** unmatched
Unicode draft, zero-match selection clamping, Escape, no engine submission
and unchanged full text. Both model regressions and the view fixture pass
before and after the optimization. Existing exact `/mode` selection and
composer queue/Stop fixtures also pass.

The first fixture incorrectly expected programmatic `set_value` to emit an
input-change event. The dependency explicitly documents that it does not.
A second attempt simulated the entire long draft and reached the **180-second**
tool timeout. Both logs are retained; no orphan validation process remained.
The corrected seeded-draft/final-character fixture completes and tests the
intended input path. No production defect or performance result is attributed
to either fixture diagnostic.

The task-projection lead was inspected but not changed: its first eight
references already reuse the initial scan. The over-eight tail is counted and
then consumed; removing that extra traversal needs a broader, separately
measured design, not an alleged missing prefix-iterator reuse fix.

### Retained release measurements

Five alternating before/after pairs each perform **100** matcher calls, with
query creation and the reference result outside the timer. No builds, other
tests or native harnesses overlapped these pairs; the user's app stayed
running. This is a helper comparison, not an idle-host whole-app benchmark.

| Query | Before median (ms) | After median (ms) |
| --- | ---: | ---: |
| Empty | 0.026 | 0.023 |
| Short uppercase `MO` | 0.010 | 0.012 |
| Exact `approval` | 0.006 | 0.006 |
| Nine-character `APPROVALX` | 0.002 | <0.001 |
| 8 MiB ASCII | 24.162 | <0.001 |
| Approximately 8 MiB Unicode | 1400.675 | 0.001 |
| 1 MiB expanding lowercase `İ` | 226.996 | 0.001 |

Values printed as `0.000` are below the probe's millisecond rounding
resolution, not zero elapsed time. The short-prefix slowdown is retained.
The structural outcome is removal of unbounded lowercase work and its
temporary string for overlong queries. Rope materialization, the
`active_query` whitespace scan, keyboard dispatch, text layout and paint are
outside this timer and remain unchanged. No peak-RAM, complete typing-latency
or whole-app CPU improvement is claimed.

### Full, release and native verification

All **14** serial stages pass and the pipeline exits. Offline/locked full
workspace: ACP **68**, agent **152**, app **166**, UI **244**, terminal **19**,
launcher **1**, plus integration/documentation tests. Release app **166**,
all **22** ignored release probes, composer group **8/8**, compact eight-pane
**1/1**, formatting, strict workspace/all-target Clippy with `-D warnings`,
whitespace, Python syntax and the **four** capture-gate regressions pass.

The separately named native launcher compiles the unchanged real `main.rs`
against the updated app, with the repository release profile and every
dependency version matching the existing lock. No root manifest/lock or
`target/release/flint` was rewritten.

Sequential isolated native slash **2/2**, diff **16/16** and queue/composer
**6/6** pass. All nine captures were inspected, including both slash menus
and the compact editor at full size. Command labels, navigation hints and
scrollbars are readable; queue, task tray, Stop and editor footer remain
separated in the dark-and-ember UI. Captures report settled inactive paint
with disposable homes/workspaces and no physical input.

Diff evidence retains all **10,000** additions and records **340/334/334**
frames with interval p95 **14.29/14.07/14.25 ms**. Its unpaired median RSS
**82.89 MB** and CPU **24.0%**, including the higher individual samples, are
retained without attributing differences to this helper. Whole-app performance
budgets, major-state CPU and workload sweeps were not rerun with the user's
app active; the intermittent picker CPU gap remains unmeasured here.

All **230** unrelated starting hashes match. Measured and fully validated
final sources, native binary and locked dependency provenance match.
The original **97** tracked modified paths and all starting untracked work
remain; the only added source path is `crates/flint-app/src/slash_tests.rs`.
User process **45687** keeps its identity and the release executable keeps
its starting hash throughout validation and closeout.

Remaining full-value query/layout copies, whitespace scans, large task tails,
persistence/backpressure and picker CPU remain leads. Physical input,
clipboard, hover, IME, VoiceOver, live providers and crash/slow-storage tests
remain excluded. No real credentials, user-app restart, commit, push or
publication occurred.

## Candidates for later passes

These are investigation leads, not accomplished fixes:

- **Large live-task projection:** the first eight references already reuse
  the initial scan. The over-eight tail is counted and then consumed, and the
  retained 512-command stress case worsens. Measure a broader realistic
  workload before trading extra retained state for one-pass counts.
- **Mention-picker CPU:** reproduce the strict idle-budget failure and profile
  focus/caret, menu rendering, and idle repaint scheduling. The retained Pass 6
  paired diagnostic reproduces it on both before and after binaries.
  The checkpoint and Pass 24 each measured **1.0%**; those passing samples
  do not establish a causal fix for the earlier **2.00%** failure.
- **Saved streaming/persistence pressure:** profile necessary owned records,
  serialization batches, and writer retention under slow storage without
  changing ordering, failure handling, or replay behavior.
- **Tool-event lookup:** `SessionView::tool_index` searches the full transcript
  backwards. Measure parallel-command/subagent updates on long histories before
  deciding whether an index is worth its memory and maintenance cost.
- **Queue bounds:** investigate ACP event-channel backpressure and slow-storage
  retention without dropping accepted events or blocking protocol progress.
