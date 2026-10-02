# flint blueprint harness

Evidence for what flint promises its users. Every script records one
`check(name, ok, detail)` per promise (named as the behaviour a user relies on,
with the measured value in `detail`), writes
`blueprint-out/data/report-<script>.json` (plus a per-tag copy
`report-<script>-<tag>.json`), and exits non-zero if any check failed.
Raw evidence (screenshots, logs, state dumps, frame logs) goes under
`blueprint-out/`, which is gitignored.

Run the scripts **one after another, never in parallel** (they drive real
windows and measure CPU). They never steal keyboard focus
(`FLINT_BP_NO_ACTIVATE`) and never touch your real `~/.flint`
(each launch gets a fresh `FLINT_HOME` under `blueprint-out/home/`).

```sh
python3 tools/blueprint/sweep.py      # UI states: launch, size, pixels, memory, CPU
python3 tools/blueprint/interact.py   # headless UI tests, one check per test
python3 tools/blueprint/live.py       # real turns on deepseek-v4.1-flash via Surplus
python3 tools/blueprint/perf.py       # cold start, memory, CPU, frame cost, size, TTFT
```

Common flags: `--bin PATH` tests an existing binary instead of building the
release binary; `--tag before|after` labels the run (screenshots are
`blueprint-out/ui/<tag>-<state>.png`). `interact.py` takes `--src DIR` and
`--target-dir DIR` to test another checkout (used for the baseline worktree).

## Scripts

### `sweep.py` — UI states
Launches each state through CLI flags: empty, running, done, expanded,
changes, palette, approval, settings, @-mention picker, /-command menu, and
1100×800, 1000×700 and 1920×1200 windows. Per state it checks: window within
3 s of launch; the requested size (or, if larger than the display, clamped to
it); a non-blank screenshot at the display's native scale (2x on Retina, 1x on
1080p displays); the expected UI is showing (from the automation state dump);
settled memory; CPU over 3 s (idle states must not repaint, animating states
are measured while the demo animates, states with a background session only
pay for its spinner); and no panic on stderr. `winid.swift` is compiled into
`blueprint-out/bin/winid` and reports a process's window id and bounds.

### `interact.py` — interactions
Runs `crates/flint-app/tests/blueprint_ui.rs` (gpui-kit `test-support`): the
real `FlintApp` views in a headless window, driven by real key presses and
clicks, with scripted engine events attached through `FlintApp::attach_engine`.
Each test is one promise (Enter sends, Shift+Tab cycles approval, ⌘K/⌘J/⌘N,
Stop sends Interrupt, approval buttons and Y/A/N keys send the right `Op`,
"Worked for" expands, Review opens the file, background sessions keep running,
the transcript follows output until you scroll up, the @ picker respects
`.gitignore` and attaches capped files, / commands, settings persist, sessions
survive a restart, rename/delete, effort chip, guard nudges, combined diffs,
error cards with fix actions, …).

### `live.py` — real runs
For each fixture in `fixtures/<name>/` (`repo/`, `task.txt`, hidden
`check.sh`): copies `repo/` to a fresh temp dir (never the flint repo),
`git init`s it, runs `flint --workspace <dir> --prompt <task>
--exit-after-turn` with `FLINT_BP_STATE`, screenshots the final state, then
independently runs `check.sh <dir> <fixture>` and compares the files-changed
card with `git diff --numstat`. Needs the local Surplus shim on
`127.0.0.1:18433` and a key at `~/.fx/surplus.key`.

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
`LIMITS`) were set from the measured before/after runs with headroom.

## Automation hooks in the app (automation-only)

| Hook | Effect |
| --- | --- |
| `FLINT_BP_STATE=<path>` | JSON dump of sessions, statuses, item kinds, changes, usage, guard events and time to first token when a turn ends |
| `FLINT_BP_DUMP_AFTER_MS=<ms>` | also dump the state once the UI has settled (sweep) |
| `FLINT_BP_FRAMES=<path>` | per-frame timing log, written on exit |
| `FLINT_BP_TIMING=<path>` | first-frame timestamp |
| `FLINT_BP_NO_ACTIVATE=1` | open without activating the app or taking focus |
| `FLINT_HOME=<dir>` | settings and saved sessions location |
| `--exit-after-turn`, `--demo-long N`, `--stream-test RATE`, `--scroll-test` | automation workloads |
| `--demo*`, `--palette`, `--settings`, `--mention`, `--slash`, `--changes`, `--select-change`, `--size WxH` | UI states |

## Baseline

The `before` run was recorded from a worktree of commit `f081b77` (the UI and
engine as committed) with only these automation hooks added, before any UX or
performance change.
