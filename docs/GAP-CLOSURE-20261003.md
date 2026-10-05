# Current-checkout gap closure, 2026-10-03

This is the current implementation pass, not a replacement for historical
evidence. Source fixes and the first integrated/native release checks are
verified. Compact-pane and accessibility fixes found during visual review also
pass focused/full Cargo gates and a separate retained-binary native rerun.
No comprehensive all-clear is claimed.

## Preservation and isolation

Evidence root: `target/gap-closure-20261003.n8SJNS/`.
Initial checkout: 65 modified tracked files and 14 untracked files.
`initial-working.patch`, `initial-staged.patch`, status/diff inventories, and
`initial-source/` preserve the starting point. `initial-files.json` records
source hashes; `coverage.json` enumerates every first-party source/test/example
and blueprint script. `coverage-closeout.json` appends the latest validation
references without replacing that earlier inventory. An inventory entry is
not an inspection.

No commits, pushes, uploads, live inference, real credentials, or changes to
the user's running processes. Validation uses an environment allowlist,
`FLINT_LIVE=0`, `FLINT_EVAL=0`, and a disposable HOME inside ignored `target/`.
Only the existing Cargo/rustup locations are retained for the toolchain.

## Baseline

- Formatting, strict locked workspace/all-target Clippy, release build, and
  whitespace checks passed.
- The first workspace/serial UI runs failed because the validation runner
  mistakenly set native automation's `FLINT_BP_NO_ACTIVATE=1` for headless
  tests. This disables initial composer focus and breaks keyboard tests.
  Logs remain as `baseline-{workspace,ui}.log`; the corrected, unchanged-source
  rerun passed. These failures are not attributed to implementation.
- Corrected unchanged-source workspace and serial UI runs pass (165 UI tests,
  100.10 s for the separate serial run). The pre-change release performance
  baseline passes 10/10: streaming 2,000 deltas/s uses 50.0% CPU, frame p95
  14.26 ms; scrolling 200 turns has frame p95 14.11 ms. This standalone run is
  not the comparison baseline: use the interleaved before/after measurements
  below. Logs: `baseline-corrected-*`, `before-perf/`.
- Foreground command execution returns an infrastructure error; background
  execution works. All executable checks use the working background route.

## Framework evidence and constraints

Manifests use edition 2024 and toolchain 1.95.0. Cargo.lock resolves gpui-kit,
gpui-base, and gpui-component 0.7.0, ACP 2.2.0, and Alacritty 0.26.0.
Installed implementation, not latest upstream APIs, governs changes.
Keep variable-height GPUI lists, native input, selection, and 8 ms coalescing.
Primary upstream research and exact dependency source anchors are recorded
alongside rendering decisions. No major dependency upgrades were made.

Exact framework implementation:

- gpui-kit/gpui-base/gpui-component 0.7.0 depend on **gpui-pre 0.3.7**, not
  the separately cached gpui 0.2.2. All are Apache-2.0 in their manifests.
- `gpui-pre-0.3.7/src/elements/list.rs`, `ListState::remeasure_items`
  (lines 406–472), preserves the absolute scroll anchor and replaces only
  invalidated measurements. `FollowMode::Tail`, `is_following_tail`, and
  `set_follow_mode` exist in this exact version.
- `gpui-base-0.7.0/src/text/state.rs` (lines 232, 346–386) already parses
  asynchronously and recognizes unchanged content. No replacement Markdown
  parser or speculative rendering cache is needed for this pass.
- `gpui-base-0.7.0/src/text/text_view.rs` (lines 570–685) keeps keyed native
  text/selection state. Preserve that implementation and stable item IDs.
- Primary upstream references consulted through documentation search:
  <https://gpui.rs/examples/> and Zed's
  <https://zed.dev/blog/gpui-ownership>. Installed implementation is the
  stronger compatibility evidence; search snippets alone do not prove APIs.
- `gpui-pre-0.3.7/src/_accessibility.rs` requires both stable element IDs and
  explicit roles. An `aria_label` alone does not export a custom Div.
  `on_click` automatically registers AccessKit's Click action. Primary custom
  controls now set Button roles and names without replacing native handlers.
  A second read-only AX query after 500 ms is needed to observe the activated
  tree; the first cold query is not a complete semantic inventory.

Rationale: limit each ordered pump batch to 256 events while retaining the
8 ms interval. Fold all events in order, then splice appended rows once and
remeasure each changed existing row once, grouping adjacent rows. Do not
invalidate newly appended rows before inserting them. This reduces redundant
sum-tree work and guarantees a flooded producer yields between batches.
“Jump to latest” explicitly re-engages the existing native tail mode, addressed
by session UID; new output never overrides intentional scrolling.

## Findings ledger

IDs identify this pass only. Source-proven findings still require regression
validation. “Suspected” is not a confirmed vulnerability. Each entry below has
an explicit current disposition; implemented changes are recorded after tests.

| ID | Severity / kind | Component / files | Impact and concrete evidence | Root cause / proposed fix | Coverage / result / status |
| --- | --- | --- | --- | --- | --- |
| G01 | High / defect | agent provider, session, harness args | Clean EOF with partial tool arguments can be accepted and repaired into executable mutation | Require supported terminal finish_reason; never invent missing string contents | EOF/length/filter/no-mutation regressions fail before/pass after; fixed |
| G02 | High / defect | agent harness args, tools files | JSON-valued file content strings and MCP null/string arguments are changed before dispatch | Preserve valid JSON argument values; schema-specific repair only | File-content and nested null/string regressions fail before/pass after; fixed |
| G03 | High / permission | agent approvals | Absolute-path or quoted dangerous executable can receive broad prefix approval | Bare-cargo subcommand allowlist; unknown/nontrivial executables exact-only | Executable spelling/unknown-command regressions fail before/pass after; fixed |
| G04 | High / permission | agent tools tracker | Undo follows a replacement symlink with matching bytes outside workspace | Workspace confinement, canonical identity, locked descriptor restoration | Outside-symlink regression fails before/passes after; ordinary undo passes; fixed within G18 limits |
| G05 | High / resource | agent tools files | Whole-file read loads huge files; FIFO read cannot cancel | Regular-file checks, 8 MiB input bound, 30-second preflight deadline and cancellation | Oversize/FIFO/cancellation regressions fail before/pass after; fixed, mutation paths additionally covered by G27 |
| G06 | Medium / permission | agent fetch, approvals | Approved host may redirect to an unapproved host | Refuse all implicit redirects; destination needs a separate fetch/approval | Two-server fixture fails before/passes after; destination receives no connection; fixed |
| G07 | Medium / lifecycle | agent MCP | Cancelled requests leave pending entries and remote work | RAII pending cleanup and best-effort cooperative cancellation notification | Controlled MCP drop fixture fails before/passes after; fixed locally, not a remote-stop guarantee |
| G08 | Medium / resource | agent provider/MCP | Unterminated frames and continually arriving content have unbounded retention | Frame/completion/inventory caps and total deadlines; overflows fail closed | SSE/completion/MCP/schema inventory regressions fail before/pass after; fixed for these dimensions |
| G09 | Medium / correctness | agent session/harness | Denied/failed writes count as completed edits | Track mutation only after successful outcome | Failed-edit accounting regression fails before/passes after; successful-edit verification retained; fixed |
| G10 | Medium / correctness | agent tools/guard | Unchecked range arithmetic on maximum model-supplied offsets | Saturating bounded calculations | Both maximum-range regressions fail before/pass after; fixed |
| G11 | Medium / lifecycle | ACP runner/live | Handshake/options and uncooperative Interrupt can wait forever | 30-second RPC deadlines and non-extendable two-second cancellation grace | Six regressions fail before/pass after; fixed, integrated gates pass |
| G12 | High / lifecycle | ACP terminals/lib | Direct-child kill leaves descendants; inherited pipes delay exit indefinitely | Owned Unix process groups, bounded pipe drain, escalation/reaping even after leader exit | Three regressions fail before/pass after; additional graceful-leader test passes; fixed on this macOS host |
| G13 | Medium / fidelity | ACP terminals | Per-read lossy UTF-8 corrupts split Unicode | Independent incremental UTF-8 decoding per pipe | Split-stream regression fails before/passes after; every-split/invalid-EOF helper passes; fixed |
| G14 | Medium / resource | ACP lib/mapper/terminals; term events | Caps apply after newline accumulation or only to final output; metadata floods queue | Fixed-size stderr reads, 1 MiB hard output caps, terminal metadata coalescing | Three regressions fail before/pass after; partially fixed. ACP event-channel backpressure deferred below |
| G15 | High / input integrity | term keys | Removing one bracketed-paste delimiter can assemble another | Strip remaining payload ESC bytes after compatible marker removal | Nested-delimiter regression fails before/passes after; fixed |
| G16 | Medium / correctness | term snapshot | Text export uses character count instead of terminal cell endpoints | Preserve run column endpoints across wide/combining glyphs | Styled Unicode regression fails before/passes after; fixed |
| G17 | Medium / defect | app engine/session/view-model | Continuous producers monopolize one pump update; newline-free output bypasses 2,000-line limit | 256 ordered events per batch; grouped/deduplicated list invalidation; UTF-8-safe 256 KiB live tail | Both regressions fail before/pass after; latest app units 100/100 and serial UI 173/173 pass; fixed |
| G18 | Low / documented limitation | native/ACP permissions | Canonical pathname checks do not sandbox arbitrary commands or eliminate check/use races | Keep distinction explicit; descriptor-relative confinement is separate work | Intentional limitation, not a full OS sandbox claim |
| G19 | Verification gap | native UI | Historical screenshot, VoiceOver, physical clipboard/keyboard, multipane performance checks remain incomplete | Inspect new isolated final binary and screenshots; native accessibility probe | Fresh major-state/narrow/pane screenshots inspected; AX activation investigated. Physical input, VoiceOver and concurrent native multipane streaming remain unverified |
| G20 | High / session isolation | app term_panel | Selecting session B then forwarding A's terminal sends contents to B | Resolve destination from terminal's owner; refuse missing/archived owner | Controlled terminal-routing regression and integrated gates pass; fixed |
| G21 | Medium / misleading state | app session_options | Chip changes before acknowledgement and remains false on closed channel | Retain confirmed value until SessionOptions; validate reported choices and surface failed delivery | ACP acknowledgement/closed-channel regression and integrated gates pass; fixed |
| G22 | Medium / draft loss | app session_workspace/project_menu | Ordinary switching and project-created sessions share draft/attachments | Session-local swapping in ordinary mode and project transitions; avoid last-split double swap | Ordinary/project switching, 1/2/4/8 stress and all pane tests pass; fixed |
| G23 | Medium / responsiveness | app mention/app_input | First mention synchronously traverses workspace; attachment cap is after whole-file read | Workspace-guarded background indexing, bounded traversal, at most 24,005 input bytes per regular text attachment | Six mention units, picker/stale-workspace and integrated tests pass; fixed. Slow filesystem operations are not a hard-latency guarantee |
| G24 | Low / capability contract | app engine/connection_test | Keyless local model-list probe can succeed while native startup requires nonempty key | Retain documented nonempty-key contract; startup error explicitly explains dummy local keys | Intentional contract clarified; no paid/live tests |
| G25 | Medium / UX, partly confirmed | app composer/header/popover/term_paint | Compact restored eight-pane layout crushes the input under wrapped options; other header/popover/combining concerns remain unproven | Reserve a 44px input and send/stop row; make wrapped options scroll within remaining card space | Minimum-input regression fails before/passes after; latest compact-pane screenshot inspected with input/send visible. Very small popovers and styled combining paint are not declared fully verified |
| G26 | Medium / attachment confinement | app mention | A selected relative file replaced by an outside symlink can attach outside-workspace bytes | Reject parent/absolute paths, canonical escapes and nonregular files before bounded reading | Outside-symlink regression fails before/passes after; static confinement fixed, concurrent filesystem races remain G18 |
| G27 | High / mutation safety | agent tools files | Write/edit read entire targets; write treats every read error as nonexistent | Shared bounded regular-file preflight; creation only on NotFound using create_new; cancellable lock/read waits | Seven regressions fail before/pass after, compatibility case passes both; fixed. Commit itself is awaited, not claimed cancellable/transactional |
| G28 | Medium / resource | agent tools files | replace_all expands a small input into an arbitrarily large result before diffing/writing | Checked result-size calculation before allocation; refuse results above 8 MiB without mutation | Controlled expansion regression fails before/passes after; fixed |
| G29 | Medium / accessibility | app custom controls | Named custom Div controls lack roles and disappear from GPUI's exported semantic tree | Explicit Button roles and names for primary navigation, session, composer, option, permission and file controls; preserve native Click handlers | Composer role regression fails before/passes after. Activated native AX sees 37 elements/28 named and primary AXPress actions; actions not invoked, no full VoiceOver/transcript/terminal reading claim |

ACP/terminal evidence: `acp-term-validation.md` in the evidence root records all
15 baseline-compatible failures and fixed-source 47 ACP + 17 terminal passes,
strict all-target Clippy, formatting and diff checks. No tests were suppressed.
These results do not substitute for final app/workspace gates.

Native evidence: `native-worker.md` and
`native-phase2-per-finding-results.json` record the sealed initial-source run
(119 existing passes, 20 expected failures) and green G01–G10/canonical-alias
tests. G27 has seven further red failures and eight green compatibility/
regression cases; the native library then passes 147 tests. G28 adds another
verified red/green case. Four offline grader tests pass after correcting hidden
Python assertion and CLI exit masking. Live eval/inference were not enabled.
An early shared-cache red attempt reused a fixed artifact and is explicitly
excluded; the accepted red proof uses a sealed copy and separate target.

Initial integrated gates exposed five stale test expectations/fixtures:
three loopback providers omitted terminal completion, a clipboard test expected
shared drafts, and a dropdown test expected unacknowledged optimistic settings.
Fixtures now provide supported completion/acknowledgement and verify restored
session-local drafts. All five focused reruns and final3 full reruns pass.

Visual-review regressions: `compact-red6.log` reproduces a 30.5px input region
in an accepted compact restored layout, below the required 44px minimum.
`compact-green.log` passes with readable input and lower options reachable by
scrolling. Earlier compact fixtures passed because they did not reproduce this
geometry or assert input height; initial fixture compilation errors are not red
proof. `a11y-red2.log` shows the missing Button role, and `a11y-green.log`
passes after explicit roles/names. A macro-import compilation error in the
first attempt is likewise excluded from red proof.

G14 deferral: synchronous ACP mapping and the shared unbounded `AgentEvent`
contract currently cannot apply lossless bounded backpressure. A bounded
`try_send` replacement would silently lose permission/tool/exit events.
Safe closure requires an end-to-end asynchronous writer/consumer contract or
explicit rolling-output events, plus persistence/order/lifecycle regressions.
Per-output storage and terminal metadata are bounded now; channel/SDK argument
allocations and total call count are not claimed globally bounded.

### Concurrent editing

Additional app isolation tests and a duplicate byte-cap implementation appeared
during this pass. The read-only app reviewer explicitly reported no writes.
Main removed only its redundant constant/trim block, preserving the other
equivalent cap. Three additional tests were retained and their behavior verified.
Attribution of that concurrent work remains unresolved; do not label all
post-snapshot additions as authored by this session. README also changed
externally during review, so it must be reread before any targeted corrections.

## Coverage and connection matrix

The native engine (44 Rust files plus manifest) and ACP/terminal (24 Rust
files plus two manifests) have been fully source-read by independent reviewers.
Their source findings above are not runtime verification. App and harness
coverage is now source-read completely, including the immutable baseline UI
suite; later concurrent additions require separate review. Blueprint scripts
and their disposable live fixture source/checks have been read, not live-run.
Runtime coverage is tracked separately in `coverage.json`.

| Feature | Entry → state/operation → events → feedback/persistence | Backend distinctions / verification |
| --- | --- | --- |
| Native turn | composer → submit/config/spawn_session → provider/tools → view fold/list + saved events/history | Flint harness/tools/subagents; offline local-provider fixtures and integration pass, no live provider verification |
| ACP turn | agent selection → spawn_adapter/runner/live → ACP mapper → same view fold | Agent-provided mode/model options; no Flint harness/native undo |
| Approval | pinned card/keys → Op::Approval → native inbox or ACP permission response → scoped card resolution | Native broad choice and ACP per-request behavior must remain distinct |
| Delegation | native spawn_agent → child session → SubagentEvent → child activity + history | Four concurrent, 32 retained, no grandchildren; external agents own delegation |
| Terminal | open shell/input or command events → Alacritty/client terminal → snapshots/output/exit → terminal view | Interactive PTY versus read-only piped/adapter command tabs |
| Persistence | user/events → ordered UI writer + native snapshot writer → flush/shutdown → archive/restart | Stalled-storage queues/barriers need separate review |
| Panes | sidebar drag/focus/mode → layout/drafts/session routing → independent lists/terminal entity → saved placement | Layout/modes survive restart, drafts/processes intentionally do not |

## Integrated verification

`final3-results.json`: all six gates pass using the isolated environment and
offline/locked Cargo, with no lint suppressions or ignored new regressions:

- Formatting and strict workspace/all-target Clippy.
- Workspace tests: ACP 47, native agent 148, app 99, UI 172, terminal 17.
- Separate serial UI: 172/172, 128.98 seconds including Cargo invocation.
- Release build and whitespace check.

Four offline grader cases run. Live-provider, model-eval and opt-in benchmark
entry points return without running their optional workloads, not live passes.
The 1/2/4/8-session chat/terminal isolation fixture is automated headless GPUI
coverage, not a native multi-stream performance measurement.

Final release binary: SHA-256
`3f0e80e80acd84d69efa73789e668d96ed6918a753453c696bddaa826b4a7dd7`,
31,971,728 bytes. It is preserved as `flint-after`; `flint-before` remains
unchanged. `final-source-hashes.json` records source state at final validation.
Worker-owned and app-versus-initial deltas are retained separately from HEAD
diffs; the latter include substantial original user changes.

## First release native verification

`after-native-results.json` records sequential checks against `flint-after`
without heavy Cargo overlap: sweep **100/100** (95.95 s), interleaved performance
**10/10** (185.33 s), diff **16/16** (18.90 s), workloads **71/71** (77.17 s).
The fresh output roots are `after-{sweep,perf,diff,workloads}/`.

Interleaved performance, not the earlier standalone run:

| Metric | Initial binary | First fixed binary |
| --- | ---: | ---: |
| Window / first frame, ms | 316.33 / 197.83 | 310.93 / 194.35 |
| Idle / 200-turn RSS, MB | 74.50 / 84.58 | 74.72 / 84.99 |
| Idle CPU, % | 0.5 | 0.5 |
| Streaming CPU, % | 53.4 | 53.6 |
| Streaming frame / render p95, ms | 15.27 / 0.23 | 15.23 / 0.23 |
| Worst streaming dropped frames | 9 | 10 |
| Scrolling frame / render p95, ms | 15.09 / 0.34 | 15.16 / 0.39 |
| Binary size, MB | 31.7 | 32.0 |

Budgets pass, but these results show **no meaningful streaming speedup**.
The small startup improvement does not establish a causal speedup; memory,
CPU and some scrolling/dropped-frame measures increase slightly.

`native-panes.json` records restored 1/2/4/8-session idle geometry: RSS
76.92/80.94/83.55/88.75 MB, CPU 0–1%, no panic. The eight-pane fixture
uses accepted equal nested splits, not uniform grid weights, intentionally
exercising compact tiles. The single-pane launch shows a fresh session, not
its seeded transcript. These are not concurrent-stream measurements.

Fresh screenshots inspected include approval, expanded/running transcript,
palette, slash/mention pickers, large diff, terminal output, 900x560 welcome
and settings, and restored panes. The settings footer remains visible while
the form scrolls; normal-state headers/popovers show no obvious overlap.
The compact-pane input clipping is addressed by G25, not waved through.
These observations do not prove every resized, selected or Unicode state.

Initial AX query: trusted, no window error, six elements/one named.
`accessibility-recheck.json` repeats the read after 500 ms and sees 13 elements,
four named, including a text area and field. Activation explains some sparsity,
but missing custom roles remain a source-proven gap (G29). This read-only probe
does not exercise VoiceOver or accessible actions.

## Completed closeout and remaining limits

The completed `closeout` phase reruns all gates and native checks against a
separately retained binary after G25/G29. Earlier binaries and reports are retained.
`closeout-results.json` passes all six Cargo/whitespace gates:
ACP 47, native agent 148, app 100, UI 173, terminal 17; separate serial UI
173/173, 117.40 s including Cargo; strict offline/locked all-target Clippy and
release build. `closeout-native-results.json` records sequential native sweep
**100/100** (93.50 s), performance **10/10** (183.94 s), diff **16/16** (16.51 s)
and workloads **71/71** (77.51 s), all exit 0.
`closeout-native-verification.json` confirms budget totals, named primary
controls/AXPress, pane counts/no panics and no source drift since validation.

The diff fixture measures RSS 81.97 MB, CPU 28%, frame interval p95 16.54 ms
and root-render p95 0.183 ms. No comparable diff baseline was collected in
this phase. Saved streaming accepts approximately 2,000 events/s and preserves
10,001–10,003 records in order; event-batch p95 is 0.339–0.378 ms and maximum
0.430–0.597 ms. These are fixture results, not global storage or queue bounds.
Terminal frame p95 spans 16.64–16.73 ms; its behavioral checks do not impose
a 16.7 ms performance budget. Saved-session launch checks cover 10/200/1,000
sessions, not every storage size or filesystem.

`accessibility-closeout.json` records a trusted, error-free, activated native
tree with **37 elements, 28 named**, including 31 buttons, three groups, a text
area, a text field and the window. New agent, Send message, agent/model choice
and other named primary buttons advertise **AXPress**. The cold first query
still sees six elements/one named. Actions were enumerated, not invoked;
VoiceOver and complete transcript/terminal semantics remain unverified.

`closeout-panes.json` records correct restored 1/2/4/8-session counts, no panics,
RSS **77.125/80.297/84.203/88.750 MB** and idle CPU **0–1%**. All four fresh pane
screenshots and the AX demo screenshot were inspected. The compact eight-pane
composer now shows readable input and a visible Send button; only the first
wrapped option row is visible, with lower options scroll-reachable in the
headless regression rather than exercised by this native probe. Two/four-pane
composers show both option chips without overlap. Single-pane launch
intentionally opens a fresh session; its saved session remains in the sidebar.
These are idle geometry observations, not native concurrent-stream measurements.

Latest interleaved performance (`closeout-perf/data/report-perf.json`):

| Metric | Initial binary, rerun | Latest fixed binary |
| --- | ---: | ---: |
| Window / first frame, ms | 309.23 / 195.63 | 301.37 / 206.16 |
| Idle / 200-turn RSS, MB | 74.72 / 84.04 | 74.92 / 78.50 |
| Idle CPU, % | 0.5 | 0.5 |
| Streaming CPU, % | 75.4 | 79.4 |
| Streaming frame / render p95, ms | 13.37 / 0.23 | 13.23 / 0.22 |
| Worst streaming dropped frames | 10 | 10 |
| Scrolling frame / render p95, ms | 15.50 / 0.24 | 15.22 / 0.25 |
| Binary size, MB | 31.7 | 32.0 |

The latest paired comparison is authoritative for the latest binary.
Streaming CPU and first-frame time increase; small frame-percentile changes
do not establish a meaningful speedup. The CPU samples vary substantially
between repeats. Both binaries now have approximately 8.3ms median frame
intervals, versus 13.3ms in the first release comparison; do not compare CPU
across these different frame cadences. The lower retained-session RSS is a
fixture measurement, not a proven general memory reduction.

New retained binary: `flint-closeout`, 31,988,240 bytes, SHA-256
`999a0d1a2eb1c519af5d8e78e01769e1fbe821a7fccaca3b19c43d6aa1635b44`.
`closeout-source-hashes.json` covers the validated source.
`closeout-preservation.json` verifies all 158 immutable initial file copies,
unchanged prior binaries, and unchanged root manifest/lock/toolchain metadata.
Exactly 12 shared source files differ from the `final3` checkpoint, all in the
compact-composer/semantic-control follow-up. Inventory scope differences are
listed separately, not mistaken for file modifications.
`verify_closeout.py` seals the earlier documentation state in
`closeout-final-verification.json`: whitespace, validated source hashes,
158 original copies, all three retained binaries and completed gate results.
`verify_postdocs.py` records the subsequent documentation additions separately
in `postdocs-final-verification.json`, retaining the earlier seal. Neither check
reruns or extends the security audit.

- ACP's shared event channel lacks end-to-end backpressure (G14). Per-output
  storage bounds do not bound total queued events or SDK argument allocations.
- Canonical path checks retain filesystem check/use races. Arbitrary commands
  are not confined by those checks. Mutation commits are awaited/tracked, not
  transactional or hard-deadlined once they begin.
- Unix cleanup targets owned groups; descendants that escape the group and
  non-Unix cleanup are not covered by the macOS fixtures.
- Physical keyboard/clipboard, VoiceOver, full transcript/terminal semantic
  reading and concurrent native multipane streaming remain unverified.
- Slow filesystem/indexing operations and durable-storage queue growth are
  not claimed globally bounded. Live inference/eval remain disabled.
