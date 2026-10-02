#!/usr/bin/env python3
"""UI state sweep: launch every UI state through its CLI flags, one at a time,
and check launch time, window size, non-blank screenshot (2x), no panic,
settled memory, and CPU (idle states must not repaint; animating states are
measured while the demo animates).

    python3 tools/blueprint/sweep.py [--bin PATH] [--tag before|after]
"""

from __future__ import annotations

import json
import time

import bp

# name, flags, (width, height), CPU mode, expected UI state key in the dump
STATES = [
    ("empty", [], (1440, 900), "idle", None),
    ("running", ["--demo", "--demo-instant", "--demo-stop", "27"], (1440, 900), "animating", "active_running"),
    ("done", ["--demo", "--demo-instant"], (1440, 900), "background", "active_done"),
    ("expanded", ["--demo", "--demo-instant", "--demo-expand"], (1440, 900), "background", "work_expanded"),
    ("changes", ["--demo", "--demo-instant", "--select-change"], (1440, 900), "background", "changes_open"),
    ("palette", ["--palette"], (1440, 900), "idle", "palette_open"),
    (
        "approval",
        ["--demo", "--demo-instant", "--demo-approval", "--demo-stop", "80"],
        (1440, 900),
        "animating",
        "pinned_approval",
    ),
    ("settings", ["--settings"], (1440, 900), "idle", "settings_open"),
    ("mention", ["--mention"], (1440, 900), "idle", "mention_open"),
    ("slash", ["--slash"], (1440, 900), "idle", "slash_open"),
    ("size-1100x800", ["--demo", "--demo-instant", "--size", "1100x800"], (1100, 800), "background", None),
    ("size-1000x700", ["--size", "1000x700"], (1000, 700), "idle", None),
    ("size-1920x1200", ["--demo", "--demo-instant", "--size", "1920x1200"], (1920, 1200), "background", None),
]

SETTLE_S = 2.5
IDLE_CPU_MAX = 2.0  # %, over 3 s with nothing running
ANIM_CPU_MAX = 40.0  # %, while the status line and spinners animate
BACKGROUND_CPU_MAX = 10.0  # %, a background session's spinner only
RSS_MAX_MB = 400
LAUNCH_MAX_MS = 3000


def ui_flag(state: dict, key: str) -> bool:
    """Reads an expected UI condition from the automation dump."""
    sessions = state.get("sessions", [])
    active = next((s for s in sessions if s.get("active")), {})
    match key:
        case "active_running":
            return active.get("status") == "running"
        case "active_done":
            return active.get("status") == "done" and "summary" in active.get("item_kinds", [])
        case "work_expanded":
            return bool(state.get("work_expanded"))
        case "pinned_approval":
            return state.get("pinned_approval") is True
        case _:
            return state.get(key) is True


def main() -> None:
    args = bp.args_parser(__doc__).parse_args()
    binary = bp.resolve_bin(args.bin)
    report = bp.Report("sweep", args.tag)
    workspace = bp.ROOT  # demo states only display it; the agent never runs here
    perf = {}
    screen = bp.main_screen()
    for name, flags, (w, h), mode, expect in STATES:
        print(f"state {name}", flush=True)
        dump = bp.DATA / f"state-{args.tag}-{name}.json"
        dump.unlink(missing_ok=True)
        app = bp.App(
            binary,
            [str(workspace), *flags],
            env={"FLINT_BP_STATE": str(dump), "FLINT_BP_DUMP_AFTER_MS": str(int(SETTLE_S * 1000))},
            log=bp.LOGS / f"sweep-{args.tag}-{name}.log",
        )
        opened = app.wait_window()
        report.check(
            f"{name}: window opens within {LAUNCH_MAX_MS} ms",
            opened and app.window_ms < LAUNCH_MAX_MS,
            f"{app.window_ms:.0f} ms" if opened else "no window",
        )
        if not opened:
            app.stop()
            continue
        time.sleep(SETTLE_S)
        app.refresh_window()
        _, _, _, aw, ah = app.window
        sw, sh = screen
        if w <= sw and h <= sh - 40:
            report.check(f"{name}: window is {w}x{h}", (aw, ah) == (w, h), f"{aw}x{ah}")
        else:
            # Larger than this display: macOS must clamp it to the screen.
            report.check(
                f"{name}: oversized window is clamped to the {sw}x{sh} screen",
                aw <= sw and ah <= sh and aw >= min(w, sw) - 1,
                f"{aw}x{ah} (requested {w}x{h})",
            )
        shot = bp.UI / f"{args.tag}-{name}.png"
        app.screenshot(shot)
        stddev, size = bp.image_stats(shot) if shot.exists() else (0, (0, 0))
        # Captures use the display's backing scale: 2x on Retina, 1x on this
        # harness's 1080p displays (screencapture cannot upsample honestly).
        scale = size[0] / aw if aw else 0
        report.check(
            f"{name}: screenshot is not blank and at native scale",
            stddev > 6 and scale in (1.0, 2.0) and size[1] == round(ah * scale),
            f"stddev {stddev:.1f}, {size[0]}x{size[1]} px ({scale:g}x)",
        )
        if expect:
            state = json.loads(dump.read_text()) if dump.exists() else {}
            report.check(f"{name}: shows {expect.replace('_', ' ')}", ui_flag(state, expect), f"dump keys: {sorted(state)[:8]}")
        rss = app.rss_mb()
        report.check(f"{name}: memory settles under {RSS_MAX_MB} MB", rss < RSS_MAX_MB, f"{rss:.0f} MB")
        cpu = app.cpu_percent(3.0)
        limit = {"idle": IDLE_CPU_MAX, "animating": ANIM_CPU_MAX, "background": BACKGROUND_CPU_MAX}[mode]
        label = {"idle": "idle CPU", "animating": "CPU while animating", "background": "CPU with a background session"}[mode]
        report.check(f"{name}: {label} under {limit}% over 3 s", cpu < limit, f"{cpu:.1f}%")
        app.stop()
        err = app.stderr()
        report.check(f"{name}: no panic on stderr", "panicked" not in err, f"{len(err)} bytes stderr")
        perf[name] = {"window_ms": round(app.window_ms), "rss_mb": round(rss), "cpu_pct": round(cpu, 1)}
    report.metrics = perf
    report.finish()


if __name__ == "__main__":
    main()
