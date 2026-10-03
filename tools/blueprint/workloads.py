#!/usr/bin/env python3
"""Provider-free saved streaming, mirrored terminal, and many-session workloads."""

from __future__ import annotations

import json
import time
from pathlib import Path

import bp


def frame_metrics(path: Path) -> dict:
    body = json.loads(path.read_text()) if path.exists() else {}
    frames = [frame for frame in body.get("frames", []) if frame[0] >= 1000]
    batches = body.get("event_batches", [])
    return {
        "frames": len(frames),
        "interval_p95_ms": bp.percentile(
            [b[0] - a[0] for a, b in zip(frames, frames[1:])], 95
        ),
        "root_render_p95_ms": bp.percentile([frame[1] for frame in frames], 95),
        "event_batch_p95_ms": bp.percentile([batch[1] for batch in batches], 95),
        "event_batch_max_ms": max((batch[1] for batch in batches), default=0),
        "event_count": sum(batch[0] for batch in batches),
        "rate": body.get("deltas_per_second"),
    }


def stream(binary: Path, report: bp.Report, name: str, flags: list[str], run: int) -> dict:
    label = f"{report.tag}-{name}-{run}"
    frames = bp.DATA / f"{label}-frames.json"
    home = bp.OUT / "home" / label
    app = bp.App(
        binary,
        flags,
        env={"FLINT_HOME": str(home), "FLINT_BP_FRAMES": str(frames)},
        log=bp.LOGS / f"{label}.log",
    )
    try:
        opened = app.wait_window()
        report.check(f"{label}: workload opens", opened)
        time.sleep(1.5)
        rss = app.rss_mb()
        cpu = app.cpu_percent(2.5)
        if run == 0:
            shot = bp.UI / f"{label}.png"
            report.check(f"{name}: screenshot captured", app.screenshot(shot), shot)
        report.check(f"{label}: workload exits normally", app.wait_exit(30) == 0)
    finally:
        app.stop()
    report.check(f"{label}: no panic", "panicked" not in app.stderr())
    metrics = frame_metrics(frames)
    metrics.update(rss_mb=rss, cpu_pct=cpu)
    report.check(f"{label}: frame evidence exists", metrics["frames"] > 10, metrics)
    if "--save-stream" in flags:
        path = home / "sessions" / "stream-fixture" / "events.jsonl"
        records = [json.loads(line) for line in path.read_text().splitlines()] if path.exists() else []
        events = [record["Event"] for record in records if "Event" in record]
        deltas = [
            event for event in events
            if isinstance(event, dict) and ("TextDelta" in event or "ToolOutputDelta" in event)
        ]
        report.check(f"{label}: accepted stream records are saved",
                     len(events) == metrics["event_count"] and len(deltas) > 9000,
                     f"{len(events)} events, {len(deltas)} deltas")
        report.check(f"{label}: stream order is preserved",
                     bool(events) and "TurnStarted" in events[0] and "TurnFinished" in events[-1],
                     "turn start/finish bracket the persisted stream")
    return metrics


def sessions(binary: Path, report: bp.Report, count: int, run: int) -> dict:
    label = f"{report.tag}-sessions-{count}-{run}"
    home = bp.OUT / "home" / label
    home.mkdir(parents=True, exist_ok=False)
    workspace = bp.OUT / "workspace" / label
    workspace.mkdir(parents=True, exist_ok=False)
    for n in range(count):
        directory = home / "sessions" / f"fixture-{n:05}"
        directory.mkdir(parents=True)
        (directory / "meta.json").write_text(json.dumps({
            "id": directory.name, "title": f"Offline session {n:05}",
            "workspace": str(workspace / f"project-{n % 12:02}"),
            "created_at": n, "updated_at": n, "agent": "flint",
        }))
        (directory / "events.jsonl").write_text(
            json.dumps({"User": f"Offline session {n:05}"}) + "\n"
            + json.dumps({"Event": {"TurnStarted": {"turn_id": 1}}}) + "\n"
            + json.dumps({"Event": {"TextDelta": "Offline answer."}}) + "\n"
            + json.dumps({"Event": {"TurnFinished": {"turn_id": 1, "reason": "Completed"}}}) + "\n"
        )
    timing = bp.DATA / f"{label}-timing.json"
    state = bp.DATA / f"{label}-state.json"
    app = bp.App(
        binary, [],
        env={"FLINT_HOME": str(home), "FLINT_BP_TIMING": str(timing),
             "FLINT_BP_STATE": str(state), "FLINT_BP_DUMP_AFTER_MS": "1500"},
        log=bp.LOGS / f"{label}.log",
    )
    try:
        report.check(f"{label}: window opens", app.wait_window())
        time.sleep(2)
        metrics = {"window_ms": app.window_ms, "rss_mb": app.rss_mb(),
                   "idle_cpu_pct": app.cpu_percent(2)}
        body = json.loads(timing.read_text()) if timing.exists() else {}
        metrics["main_to_first_frame_ms"] = body.get("main_to_first_frame_ms")
        restored = json.loads(state.read_text()).get("sessions", []) if state.exists() else []
        report.check(f"{label}: every saved session is restored",
                     len(restored) == count + 1, f"{len(restored)} sessions including fresh composer")
        report.check(f"{label}: restored turns are complete",
                     sum(session["status"] == "done" for session in restored) == count)
        if run == 0:
            shot = bp.UI / f"{label}.png"
            report.check(f"{label}: screenshot captured", app.screenshot(shot), shot)
    finally:
        app.stop()
    report.check(f"{label}: no panic", "panicked" not in app.stderr())
    return metrics


def main() -> None:
    args = bp.args_parser(__doc__).parse_args()
    binary = bp.resolve_bin(args.bin)
    report = bp.Report("workloads", args.tag)
    results = {}
    for name, flags in [
        ("saved-stream", ["--stream-test", "2000", "--save-stream"]),
        ("terminal", ["--terminal-test", "20000"]),
    ]:
        results[name] = [stream(binary, report, name, flags, n) for n in range(3)]
    for count in (10, 200, 1000):
        results[f"sessions-{count}"] = [sessions(binary, report, count, n) for n in range(3)]
    report.metrics = results
    report.finish()


if __name__ == "__main__":
    main()
