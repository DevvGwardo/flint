#!/usr/bin/env python3
"""Performance: cold start, memory, idle/streaming CPU, frame cost while
streaming 2k deltas/s and while scrolling a 200-turn transcript, binary size,
and time to first token on a live run. Release build only.

Writes `blueprint-out/data/perf-<tag>.json`; `report-perf.json` carries the
regression checks plus before -> after for every metric when a `before` run
exists.

    python3 tools/blueprint/perf.py [--bin PATH] [--tag before|after] [--no-live]
"""

from __future__ import annotations

import json
import math
import tempfile
import time
from pathlib import Path

import bp

# Regression thresholds, calibrated from the measured runs (see README).
LIMITS = {
    "cold_start_window_ms": 1500,
    "cold_start_first_frame_ms": 1500,
    "rss_idle_mb": 250,
    "rss_long_session_mb": 600,
    "idle_cpu_pct": 1.0,
    "stream_cpu_pct": 120.0,
    "stream_frame_p95_ms": 16.7,
    "stream_dropped_frames": 30,
    "scroll_frame_p95_ms": 16.7,
    "binary_mb": 80,
    "ttft_ms": 20000,
}
STREAM_RATE = 2000


def cold_start_once(binary: Path, n: str) -> tuple[float | None, float | None]:
    timing = bp.DATA / f"timing-{n}.json"
    timing.unlink(missing_ok=True)
    launched_ms = time.time() * 1000
    app = bp.App(binary, [], env={"FLINT_BP_TIMING": str(timing)}, log=bp.LOGS / f"perf-cold-{n}.log")
    app.launched = launched_ms / 1000
    try:
        opened = app.wait_window()
        time.sleep(1.0)
        frame = json.loads(timing.read_text())["first_frame_epoch_ms"] - launched_ms if timing.exists() else None
        if not opened:
            return None, None
    finally:
        app.stop()
    time.sleep(0.5)
    return app.window_ms, frame


def settled(binary: Path, args: list[str], settle: float, name: str, cpu_window: float = 4.0) -> tuple[float, float]:
    """RSS (MB) after settling, then CPU % over the next `cpu_window` seconds."""
    app = bp.App(binary, args, log=bp.LOGS / f"perf-{name}.log")
    try:
        if not app.wait_window():
            return float("nan"), float("nan")
        time.sleep(settle)
        return app.rss_mb(), app.cpu_percent(cpu_window)
    finally:
        app.stop()


def frame_run(binary: Path, args: list[str], name: str) -> dict:
    frames_path = bp.DATA / f"frames-{name}.json"
    frames_path.unlink(missing_ok=True)
    app = bp.App(binary, args, env={"FLINT_BP_FRAMES": str(frames_path)}, log=bp.LOGS / f"perf-{name}.log")
    try:
        opened = app.wait_window()
        time.sleep(1.5)
        cpu = app.cpu_percent(2.5)
        returncode = app.wait_exit(30)
    finally:
        app.stop()
    frames = json.loads(frames_path.read_text())["frames"] if frames_path.exists() else []
    # Skip the first second (startup) and use the steady state.
    start = frames[0][0] + 1000 if frames else 0
    steady = [f for f in frames if f[0] >= start]
    intervals = [b[0] - a[0] for a, b in zip(steady, steady[1:])]
    costs = [f[1] for f in steady]
    return {
        "cpu_pct": round(cpu, 1),
        "window_opened": opened,
        "returncode": returncode,
        "frames": len(steady),
        "interval_p50_ms": round(bp.percentile(intervals, 50), 2) if intervals else None,
        "interval_p95_ms": round(bp.percentile(intervals, 95), 2) if intervals else None,
        "dropped_frames": sum(1 for i in intervals if i > 25.0) if intervals else None,
        "render_p50_ms": round(bp.percentile(costs, 50), 3) if costs else None,
        "render_p95_ms": round(bp.percentile(costs, 95), 3) if costs else None,
        "seconds": round((steady[-1][0] - steady[0][0]) / 1000, 2) if intervals else None,
    }


def ttft(binary: Path, name: str = "after") -> float | None:
    with tempfile.TemporaryDirectory(prefix="flint-bp-ttft-") as workspace:
        state = bp.DATA / f"perf-{name}-ttft-state.json"
        state.unlink(missing_ok=True)
        app = bp.App(
            binary,
            ["--workspace", workspace, "--prompt", "Reply with exactly the word: ok", "--exit-after-turn"],
            env={"FLINT_BP_STATE": str(state)},
            log=bp.LOGS / f"perf-{name}-ttft.log",
        )
        try:
            if not app.wait_window():
                return None
            deadline = time.time() + 120
            while not state.exists() and time.time() < deadline and app.proc.poll() is None:
                time.sleep(0.3)
            if app.wait_exit(10) != 0 or not state.exists():
                return None
            session = next(s for s in json.loads(state.read_text())["sessions"] if s["active"])
            return session.get("first_text_ms")
        finally:
            app.stop()


def interleaved(binaries: dict[str, Path], runs: int, measure) -> dict[str, list]:
    """Run `measure(binary, label)` alternately, with a unique binary/run label."""
    results: dict[str, list] = {name: [] for name in binaries}
    for n in range(runs):
        for name, binary in binaries.items():
            results[name].append(measure(binary, f"{name}-{n}"))
    return results


def med(values) -> float | None:
    return round(bp.median(values), 2) if values and all(finite(v) for v in values) else None


def frame_evidence(runs: list[dict]) -> bool:
    """Every run must contain at least one measured steady-state interval."""
    return bool(runs) and all(
        isinstance(r.get("frames"), int) and r["frames"] > 1
        and r.get("window_opened") is True and r.get("returncode") == 0
        for r in runs
    )


def finite(value) -> bool:
    return isinstance(value, (int, float)) and math.isfinite(value)


def frame_metric(runs: list[dict], key: str, worst: bool = False) -> float | None:
    values = [r.get(key) for r in runs]
    if not frame_evidence(runs) or not all(finite(v) for v in values):
        return None
    return max(values) if worst else med(values)


def collect(binaries: dict[str, Path], live: bool) -> dict[str, dict]:
    m: dict[str, dict] = {name: {} for name in binaries}
    for name, binary in binaries.items():
        m[name]["binary_mb"] = round(binary.stat().st_size / 1e6, 1)
    cold = interleaved(binaries, 7, cold_start_once)
    for name, runs in cold.items():
        m[name]["cold_start"] = [{"window_ms": w, "first_frame_ms": f} for w, f in runs]
        m[name]["cold_start_window_ms"] = med([w for w, _ in runs])
        m[name]["cold_start_first_frame_ms"] = med([f for _, f in runs])
    idle = interleaved(binaries, 3, lambda b, n: settled(b, [], 3.0, f"idle-{n}"))
    demo = interleaved(binaries, 2, lambda b, n: settled(b, ["--demo", "--demo-instant"], 4.0, f"demo-{n}", 3.0))
    long = interleaved(binaries, 2, lambda b, n: settled(b, ["--demo-long", "200"], 6.0, f"long-{n}", 3.0))
    for name in binaries:
        for key, runs in (("idle", idle[name]), ("demo", demo[name]), ("long_session", long[name])):
            m[name][key] = [{"rss_mb": r, "cpu_pct": c} for r, c in runs]
        m[name]["rss_idle_mb"] = med([r for r, _ in idle[name]])
        m[name]["idle_cpu_pct"] = med([c for _, c in idle[name]])
        m[name]["rss_demo_done_mb"] = med([r for r, _ in demo[name]])
        m[name]["rss_long_session_mb"] = med([r for r, _ in long[name]])
        m[name]["long_session_cpu_pct"] = med([c for _, c in long[name]])
    stream = interleaved(binaries, 2, lambda b, n: frame_run(b, ["--stream-test", str(STREAM_RATE)], f"stream-{n}"))
    scroll = interleaved(binaries, 2, lambda b, n: frame_run(b, ["--scroll-test"], f"scroll-{n}"))
    for name in binaries:
        m[name]["stream"] = stream[name]
        m[name]["stream_cpu_pct"] = frame_metric(stream[name], "cpu_pct")
        m[name]["stream_frame_p95_ms"] = frame_metric(stream[name], "interval_p95_ms")
        m[name]["stream_dropped_frames"] = frame_metric(stream[name], "dropped_frames", worst=True)
        m[name]["stream_render_p95_ms"] = frame_metric(stream[name], "render_p95_ms")
        m[name]["scroll"] = scroll[name]
        m[name]["scroll_frame_p95_ms"] = frame_metric(scroll[name], "interval_p95_ms")
        m[name]["scroll_render_p95_ms"] = frame_metric(scroll[name], "render_p95_ms")
        if live:
            m[name]["ttft_ms"] = ttft(binaries[name], name)
    return m


def main() -> None:
    parser = bp.args_parser(__doc__)
    parser.add_argument("--no-live", action="store_true", help="skip the live time-to-first-token run")
    parser.add_argument("--baseline-bin", type=Path, help="also measure this binary, interleaved, as `before`")
    args = parser.parse_args()
    if args.baseline_bin and args.tag == "before":
        parser.error("--baseline-bin requires a tag other than 'before'")
    binary = bp.resolve_bin(args.bin)
    report = bp.Report("perf", args.tag)
    binaries = {args.tag: binary}
    if args.baseline_bin:
        binaries = {"before": args.baseline_bin.resolve(), args.tag: binary}
    measured = collect(binaries, not args.no_live)
    for name, values in measured.items():
        (bp.DATA / f"perf-{name}.json").write_text(bp.strict_json(values))
    m = measured[args.tag]
    stream = {"dropped_frames": m["stream_dropped_frames"], "frames": sum(r.get("frames", 0) for r in m["stream"])}

    def at_most(key: str, label: str, unit: str) -> None:
        value, limit = m.get(key), LIMITS[key]
        report.check(f"{label} stays under {limit}{unit}", finite(value) and value < limit, f"{value}{unit}")

    at_most("cold_start_window_ms", "cold start to window (median of 7)", " ms")
    at_most("cold_start_first_frame_ms", "cold start to first painted frame (median of 7)", " ms")
    at_most("rss_idle_mb", "idle memory", " MB")
    at_most("rss_long_session_mb", "memory after a 200-turn session", " MB")
    at_most("idle_cpu_pct", "idle CPU with nothing running (median of 3)", "%")
    at_most("stream_cpu_pct", f"CPU while streaming {STREAM_RATE} deltas/s", "%")
    at_most("stream_frame_p95_ms", f"p95 frame interval while streaming {STREAM_RATE} deltas/s", " ms")
    drops_available = frame_metric(m["stream"], "dropped_frames", worst=True) is not None and finite(stream["dropped_frames"])
    report.check(
        "dropped frames (>25 ms) while streaming stays under "
        f"{LIMITS['stream_dropped_frames']}",
        drops_available and stream["dropped_frames"] < LIMITS["stream_dropped_frames"],
        f"worst run: {stream['dropped_frames']} dropped ({stream['frames']} frames over 2 runs)"
        if drops_available else "unavailable: frame intervals and dropped-frame measurements are required for every run",
    )
    at_most("scroll_frame_p95_ms", "p95 frame interval scrolling a 200-turn transcript", " ms")
    at_most("binary_mb", "release binary size", " MB")
    if not args.no_live:
        at_most("ttft_ms", "time to first visible text on a live run", " ms")
    else:
        report.skip("time to first visible text on a live run", "--no-live")
    before_path = bp.DATA / "perf-before.json"
    before = json.loads(before_path.read_text()) if before_path.exists() and args.tag != "before" else None
    report.metrics = {"current": m, "before": before, "limits": LIMITS, "interleaved": bool(args.baseline_bin)}
    report.finish()


if __name__ == "__main__":
    main()
