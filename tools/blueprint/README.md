# flint blueprint harness

Evidence for what flint promises its users. The shared `bp.Report` scripts record one
`check(name, ok, detail)` per promise (named as the behaviour a user relies on,
with the measured value in `detail`), writes
`blueprint-out/data/report-<script>.json` (plus a per-tag copy
`report-<script>-<tag>.json`), and exits non-zero if any check failed.
Raw evidence (screenshots, logs, state dumps, frame logs) goes under
`blueprint-out/`, which is gitignored.
Set `FLINT_BP_OUTPUT_DIR` to a fresh directory to keep an existing run's
reports and screenshots untouched.

Run the scripts **one after another, never in parallel** (they drive real
windows and measure CPU). They never steal keyboard focus
(`FLINT_BP_NO_ACTIVATE`) and never touch your real `~/.flint`
(each launch gets a fresh `FLINT_HOME` under `blueprint-out/home/`).
Standard launches seed an existing Ask configuration and use a disposable
workspace when one is not supplied. First-run tests use genuinely empty homes.

```sh
python3 tools/blueprint/sweep.py      # UI states: launch, size, pixels, memory, CPU
python3 tools/blueprint/interact.py   # headless UI tests, one check per test
python3 tools/blueprint/queue.py --bin target/release/flint # offline queue/composer captures
python3 tools/blueprint/live.py       # real turns against your configured model endpoint
python3 tools/blueprint/perf.py       # cold start, memory, CPU, frame cost, size, TTFT
```

For an offline usability pass, skip `live.py` and run `perf.py --no-live`.
Never run provider-backed scripts with real credentials without permission.

Common flags: `--bin PATH` tests an existing binary instead of building the
release binary; `--tag before|after` labels the run (screenshots are
`blueprint-out/ui/<tag>-<state>.png`). `interact.py` takes `--src DIR` and
`--target-dir DIR` to test another checkout (used for the baseline worktree).

## Scripts

### `acp.py` — isolated native ACP execution and selection

```sh
python3 tools/blueprint/acp.py --bin target/release/flint --out target/acp-native-fresh
```

Requires macOS, Node/npm and a running Cua Driver service with Accessibility
and Screen Recording permissions already granted. The output directory must
not exist. This script has its own `report.json`, `peer.jsonl`, native PNG
captures and Cua operation records under `--out`.

It launches the compiled app through Cua Driver in a registered, disposable
app bundle. Only that instance's PATH points `droid` at `acp_fixture.py`, an
offline ACP peer. A fresh HOME/FLINT_HOME and removed credential variables keep
the user's account and sessions untouched. It checks foreground echo/npm/
heredocs, a symlinked cwd, real missing-binary diagnostics, native command
expansion and model/provider picking, new session, `/clear`, compact approval/
running states, and an isolated restart into another workspace. Every UI action
uses a fresh Cua snapshot and is verified from native state or the peer trace.
Successful runs close the fixture; failed runs retain it for diagnosis.

These are native integration checks, not real Droid inference, a reproduction
of a historical failing request, or permission to restart the user's active app.

### `queue.py` — prompt queue and composer
Uses an already-built binary to capture working, paused, compact-popover and
empty-composer states. Checks isolated queue counts, paused fixtures, nonblank
native captures and absence of panics. It also requires a completed composer
paint: Send must fit; running scenes must show one live task and an unclipped
Stop control; the capture window must not become key. `--demo-queue` adds two frozen demo
prompts and opens the queue popover in short demo windows. This is offline,
with disposable homes/workspaces and no activation or physical input. These
captures do not prove VoiceOver or live ACP steering.

Queue captures use `FLINT_BP_CAPTURE=1` and a timed state dump. The normal
background window opens at the primary display's edge, avoiding a completely
covered centered window whose macOS frame loop can stop. The dump waits for
a completed paint instead of accepting an earlier background-demo completion.
If the window is still covered, the script fails rather than accepting stale
pixels. This mode is not used for performance measurements.
Run the capture-check regressions with `python3 tools/blueprint/test_queue.py`.

### `sweep.py` — UI states
Launches each state through CLI flags: empty, running, done, expanded,
changes, palette, approval, settings, @-mention picker, /-command menu, and
1100×800, 1000×700 and 1920×1200 windows. Per state it checks: window within
3 s of launch; the requested size within one point of macOS rounding (or, if
larger than the main display's usable area, clamped to it); a non-blank screenshot at the display's native scale (2x on Retina, 1x on
1080p displays); the expected UI is showing (from the automation state dump);
settled memory; CPU over 3 s (idle states must not repaint, animating states
are measured while the demo animates, states with a background session only
pay for its spinner); and no panic on stderr. `winid.swift` is compiled into
`blueprint-out/bin/winid` and reports a process's window id and bounds.

### `interact.py` — interactions
Runs `crates/flint-app/tests/blueprint_ui.rs` (gpui-kit `test-support`): the
real `FlintApp` views in a headless window, driven by real key presses and
clicks, with scripted engine events attached through `FlintApp::attach_engine`.
Archive/undo tests also run the actual native engine against a loopback-only
provider, with disposable workspaces, dummy credentials, shutdown acknowledgments,
failed persistence, queued messages, restart recovery, and resumed history.
Connection-test fixtures issue only `GET /models`. Keyboard tests cover Tab,
Shift+Tab, full keydown/keyup activation, modal containment and focus restoration.
These headless checks are not a physical-keyboard or VoiceOver compliance check.
Each test is one promise (Enter sends, Shift+Tab cycles approval, ⌘K/⌘J/⌘N,
Stop sends Interrupt, approval buttons and Y/A/N keys send the right `Op`,
"Worked for" expands, Review opens the file, background sessions keep running,
the transcript follows output until you scroll up, the @ picker respects
`.gitignore` and attaches capped files, / commands, settings persist, sessions
survive a restart, rename/delete, effort chip, guard nudges, combined diffs,
error cards with fix actions, …).

### `diff.py` — large patch
Runs three offline scroll workloads through the real Changes panel with
10,000 additions and an off-screen long line. Checks retained additions,
normal exit, no panic, a native screenshot, and steady-state frame evidence.
Background `--diff-test` launches with `FLINT_BP_FRAMES` open at the primary
display's edge without activation. This avoids a fully covered centered
window whose macOS display-link frames can stop. The frame probe does not
enable capture mode or its post-paint readiness instrumentation.
CPU and frame results require sufficient recorded frames; missing evidence
fails the gate and is not a valid performance sample.

### `live.py` — real runs
For each fixture in `fixtures/<name>/` (`repo/`, `task.txt`, hidden
`check.sh`): copies `repo/` to a fresh temp dir (never the flint repo),
`git init`s it, runs `flint --workspace <dir> --prompt <task>
--exit-after-turn` with `FLINT_BP_STATE`, screenshots the final state, then
independently runs `check.sh <dir> <fixture>` and compares the files-changed
card with `git diff --numstat`. Needs a reachable OpenAI-compatible endpoint
and a key (`FLINT_API_KEY`, plus `FLINT_BASE_URL` / `FLINT_MODEL` if you are
not using the defaults).

### `perf.py` — performance (release build)
Cold start to window and to first painted frame (median of 5), RSS idle /
after the demo / after a 200-turn synthetic session (`--demo-long 200`), idle
CPU, CPU and frame intervals while streaming 2,000 deltas/s through the real
engine path (`--stream-test 2000`), frame intervals while scrolling a 200-turn
transcript (`--scroll-test`), binary size, and time to first visible text on a
live run. Writes `data/perf-<tag>.json`; `report-perf.json` carries the
regression checks and, once a `before` run exists, before → after for every
metric.

Frame intervals are display-bound: on this harness's 75 Hz display a perfect
frame interval is ~13.3 ms, so the 16.7 ms p95 threshold catches dropped
frames; CPU while streaming is the cost signal. Thresholds (in `perf.py`
`LIMITS`) were set from the measured before/after runs with headroom:
idle CPU 1.0% (after measures a stable 0.50% — background launches skip
the composer's initial focus so the caret never blinks while the window
isn't key; the remaining ~0.5% is GPUI's always-on display link, the floor
for any GPUI app); stream p95 16.7 ms with dropped frames under 30
(baseline dropped 67 per run at p95 ~40 ms, after drops 8–14 at p95
~14 ms — the residual at 2000 deltas/s is macOS timer coalescing for a
never-key window, not render cost, so p95 is the smoothness signal; the
scroll test, the realistic interaction, drops 0).

## Automation hooks in the app (automation-only)

| Hook | Effect |
| --- | --- |
| `FLINT_BP_STATE=<path>` | JSON dump of sessions, statuses, item kinds, changes, usage, guard events and time to first token when a turn ends |
| `FLINT_BP_DUMP_AFTER_MS=<ms>` | also dump the state once the UI has settled (sweep) |
| `FLINT_BP_CAPTURE=1` | with a timed state dump, position the normal background window at the primary display's edge and write painted composer control bounds after a completed frame; suppress earlier turn-end dumps |
| `FLINT_BP_FRAMES=<path>` | per-frame timing log, written on exit |
| `FLINT_BP_TIMING=<path>` | first-frame timestamp |
| `FLINT_BP_NO_ACTIVATE=1` | open without activating the app or taking focus; the composer also stays unfocused (no caret blink) until the window is key |
| `FLINT_HOME=<dir>` | settings and saved sessions location |
| `--exit-after-turn`, `--demo-long N`, `--stream-test RATE`, `--scroll-test` | automation workloads |
| `--demo*`, `--palette`, `--settings`, `--mention`, `--slash`, `--changes`, `--select-change`, `--size WxH` | UI states |

## Baseline

The `before` run was recorded from a worktree of commit `f081b77` (the UI and
engine as committed) with only these automation hooks added, before any UX or
performance change.
