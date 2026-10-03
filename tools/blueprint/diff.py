#!/usr/bin/env python3
"""Offline 10,000-line Changes panel workload, with an off-screen long line."""

from __future__ import annotations

import json
import time

import bp


def main() -> None:
    args = bp.args_parser(__doc__).parse_args()
    binary = bp.resolve_bin(args.bin)
    report = bp.Report("diff", args.tag)
    runs = []
    for n in range(3):
        name = f"diff-{args.tag}-{n}"
        frames_path = bp.DATA / f"{name}-frames.json"
        state_path = bp.DATA / f"{name}-state.json"
        app = bp.App(
            binary,
            ["--diff-test", "10000", "--size", "1100x800"],
            env={
                "FLINT_BP_FRAMES": str(frames_path),
                "FLINT_BP_STATE": str(state_path),
                "FLINT_BP_DUMP_AFTER_MS": "1200",
            },
            log=bp.LOGS / f"{name}.log",
        )
        try:
            report.check(f"run {n}: large patch opens a window", app.wait_window())
            time.sleep(1.5)
            rss = app.rss_mb()
            if n == 0:
                shot = bp.UI / f"{args.tag}-large-diff.png"
                report.check("large patch screenshot is captured", app.screenshot(shot), shot)
            cpu = app.cpu_percent(2.0)
            report.check(f"run {n}: scroll workload exits", app.wait_exit(45) == 0)
        finally:
            app.stop()
        report.check(f"run {n}: no panic", "panicked" not in app.stderr())
        state = json.loads(state_path.read_text()) if state_path.exists() else {}
        changes = next((s["changes"] for s in state.get("sessions", []) if s["active"]), [])
        report.check(
            f"run {n}: all 10,000 additions are retained",
            len(changes) == 1 and changes[0]["added"] == 10000,
            changes,
        )
        frames = json.loads(frames_path.read_text())["frames"] if frames_path.exists() else []
        steady = [f for f in frames if f[0] >= 1000]
        intervals = [b[0] - a[0] for a, b in zip(steady, steady[1:])]
        runs.append({
            "rss_mb": round(rss, 2),
            "cpu_pct": round(cpu, 1),
            "frames": len(steady),
            "interval_p95_ms": round(bp.percentile(intervals, 95), 2),
            "root_render_p95_ms": round(bp.percentile([f[1] for f in steady], 95), 3),
        })
        report.check(f"run {n}: frame evidence is present", len(steady) > 10, runs[-1])
    metrics = {
        key: round(bp.median([run[key] for run in runs]), 3)
        for key in ("rss_mb", "cpu_pct", "interval_p95_ms", "root_render_p95_ms")
    }
    metrics["runs"] = runs
    (bp.DATA / f"diff-{args.tag}.json").write_text(json.dumps(metrics, indent=2))
    before_path = bp.DATA / "diff-before.json"
    before = json.loads(before_path.read_text()) if args.tag != "before" and before_path.exists() else None
    report.metrics = {"current": metrics, "before": before}
    report.finish()


if __name__ == "__main__":
    main()
