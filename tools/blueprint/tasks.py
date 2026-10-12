#!/usr/bin/env python3
"""Sequential, isolated native task-detail and queue-review captures.

Synthetic scopes and results exercise rendering only. Interaction, engine and
persistence promises are verified separately by the serial Rust regressions.
"""

from __future__ import annotations

import math

import bp
from sweep import wait_capture_state


OUTCOMES = {
    "partial": "partial", "review": "needs_review", "blocked": "blocked",
    "external": "unknown", "accepted": "accepted", "stale": "partial",
    "editing": "needs_review", "confirmation": "needs_review", "queue": "partial",
}


def check_state(state: dict, scene: str) -> None:
    capture = state.get("capture", {})
    assert capture.get("fully_visible") is True, f"root not fully painted: {capture}"
    assert capture.get("window_active") is False, f"active capture: {capture}"
    at = capture.get("painted_at_ms")
    assert isinstance(at, (int, float)) and math.isfinite(at) and at >= 700, (
        f"unsettled root paint: {capture}"
    )
    active = next(session for session in state["sessions"] if session["active"])
    assert state["task_details_open"] is (scene != "queue"), state
    assert state["task_scope_editing"] is (scene == "editing"), state
    assert state["task_accept_confirming"] is (scene == "confirmation"), state
    if scene != "queue":
        assert state["task_details_turn_id"] == 1, state
    if scene != "historical":
        task = active["task"]
        assert task["assessment"]["outcome"] == OUTCOMES[scene], task
        if scene == "external":
            assert task["source"] == "external" and task["contract"]["mode"] == "unspecified", task
            assert not task["checks"] and task["acceptance"] is None, task
        if scene == "accepted":
            assert task["acceptance"] is not None, task
    if scene == "queue":
        assert active["queued_prompts"] == 1 and active["queue_paused"], active
        assert active["queue_review_hold"], active
        for name in ("queue-review", "queue-confirmation"):
            painted = state["task_controls"].get(name, {})
            assert painted.get("fully_visible") is True, f"{name}: {painted}"
            assert painted.get("window_active") is False, f"{name}: {painted}"
            assert painted.get("painted_at_ms", 0) >= 700, f"early {name}: {painted}"
    if scene == "historical":
        assert active["task"]["source"] == "legacy", active
        assert active["task"]["assessment"]["outcome"] == "unknown", active
    if scene != "queue":
        controls = state["task_controls"]
        for name in ("sheet", "header", "footer"):
            painted = controls.get(name, {})
            assert painted.get("fully_visible") is True, f"{name}: {painted}"
            assert painted.get("window_active") is False, f"{name}: {painted}"
            assert painted.get("painted_at_ms", 0) >= 700, f"early {name}: {painted}"
        sheet, header, footer = (controls[name] for name in ("sheet", "header", "footer"))
        assert header["y"] >= sheet["y"], controls
        assert footer["y"] + footer["height"] <= sheet["y"] + sheet["height"] + 1, controls
        assert footer["y"] > header["y"] + header["height"], controls


def main() -> None:
    args = bp.args_parser(__doc__).parse_args()
    binary = (args.bin or bp.RELEASE_BIN).resolve()
    reports = (bp.DATA / "report-tasks.json", bp.DATA / f"report-tasks-{args.tag}.json")
    if any(path.exists() or path.is_symlink() for path in reports):
        raise SystemExit("Task evidence already exists. Choose a fresh FLINT_BP_OUTPUT_DIR.")
    report = bp.Report("tasks", args.tag)
    scenes = [*OUTCOMES, "historical"]
    for scene in scenes:
        for size in ("1440x900", "900x560"):
            name = f"task-{scene}-{size}-{args.tag}"
            state_path = bp.DATA / f"{name}.json"
            screenshot = bp.UI / f"{name}.png"
            log = bp.LOGS / f"{name}.log"
            if any(path.exists() or path.is_symlink() for path in (state_path, screenshot, log)):
                report.check(
                    f"{scene} at {size}: current-launch evidence is fresh", False,
                    "Capture artifacts already exist; use a fresh output directory.",
                )
                continue
            app = bp.App(binary, ["--demo", "--demo-instant", "--size", size], env={
                "FLINT_LIVE": "0", "FLINT_EVAL": "0",
                "FLINT_BP_STATE": str(state_path), "FLINT_BP_DUMP_AFTER_MS": "700",
                "FLINT_BP_CAPTURE": "1", "FLINT_BP_TASK_SCENE": scene,
            }, log=log)
            try:
                assert app.wait_window(), "window did not open"
                state = wait_capture_state(state_path)
                assert app.screenshot(screenshot), "capture failed"
                contrast, dimensions = bp.image_stats(screenshot)
                check_state(state, scene)
                report.check(f"{scene} at {size}: native capture is nonblank", contrast > 5,
                             f"contrast {contrast}, {dimensions}, {screenshot}")
                report.check(f"{scene} at {size}: fixture remains running", app.proc.poll() is None)
                report.check(f"{scene} at {size}: no native panic", "panicked at" not in app.stderr())
                report.check(
                    f"{scene} at {size}: honest outcome and visible native controls", True,
                    f"{dimensions}, {screenshot}",
                )
            except (AssertionError, OSError, ValueError, StopIteration, KeyError) as error:
                report.check(f"{scene} at {size}: honest outcome and visible native controls", False, error)
                if not screenshot.exists():
                    app.screenshot(screenshot)
            finally:
                app.stop()
    report.finish()


if __name__ == "__main__":
    main()
