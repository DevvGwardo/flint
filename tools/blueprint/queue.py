#!/usr/bin/env python3
"""Isolated, offline native screenshots of the prompt queue and composer.

Run after building: python3 tools/blueprint/queue.py --bin target/release/flint
Set FLINT_BP_OUTPUT_DIR to a fresh directory to retain independent evidence.
No inference, real session storage, focus activation, or physical input.
"""

from __future__ import annotations

import json
import time

import bp


def check_controls(state: dict, running: bool) -> None:
    controls = state.get("composer_controls")
    assert isinstance(controls, dict), "missing completed composer paint"
    assert controls.get("running") == running, f"stale composer running state: {controls}"
    assert controls.get("tasks") == int(running), f"unexpected live task count: {controls}"
    painted = controls.get("painted", {})
    required = ("send", "stop", "task_tray") if running else ("send",)
    for name in required:
        control = painted.get(name, {})
        assert control.get("fully_visible") is True, f"{name} missing or clipped: {control}"
        assert control.get("window_active") is False, f"capture window became key: {control}"
        assert control.get("painted_at_ms", 0) >= 700, f"capture preceded settling: {control}"
    if not running:
        assert "stop" not in painted and "task_tray" not in painted, controls


def main() -> None:
    args = bp.args_parser(__doc__).parse_args()
    binary = (args.bin or bp.RELEASE_BIN).resolve()
    report = bp.Report("queue", args.tag)
    scenes = [
        ("queue-working", ["--demo-stop", "27"], "1440x900", 2),
        ("queue-paused", [], "1440x900", 2),
        ("queue-compact", ["--demo-stop", "27"], "900x560", 2),
        ("composer-empty", [], "900x560", 0),
        ("queue-editor", ["--demo-stop", "27"], "1440x900", 2),
        ("queue-editor-compact", ["--demo-stop", "27"], "900x560", 2),
    ]
    for name, flags, size, count in scenes:
        state_path = bp.DATA / f"{name}-{args.tag}.json"
        launch = ["--size", size]
        if count:
            launch += ["--demo", "--demo-instant", "--demo-queue", *flags]
        editor = name.startswith("queue-editor")
        app = bp.App(binary, launch, env={
            "FLINT_LIVE": "0", "FLINT_EVAL": "0",
            "FLINT_BP_STATE": str(state_path),
            "FLINT_BP_DUMP_AFTER_MS": "700",
            "FLINT_BP_QUEUE_EDITOR": "1" if editor else "0",
            "FLINT_BP_CAPTURE": "1",
        }, log=bp.LOGS / f"{name}-{args.tag}.log")
        try:
            assert app.wait_window(), f"{name}: window did not open"
            deadline = time.monotonic() + 8
            while not state_path.exists() and time.monotonic() < deadline:
                time.sleep(0.05)
            state = json.loads(state_path.read_text())
            active = next(session for session in state["sessions"] if session["active"])
            assert active["queued_prompts"] == count, (name, active)
            if count:
                assert active["queue_paused"], (name, active)
            assert active.get("queue_editing", False) == editor, (name, active)
            running = bool(flags)
            assert (active["status"] == "running") == running, (name, active)
            check_controls(state, running)
            time.sleep(0.25)
            screenshot = bp.UI / f"{name}-{args.tag}.png"
            assert app.screenshot(screenshot), f"{name}: capture failed"
            contrast, dimensions = bp.image_stats(screenshot)
            assert contrast > 5, f"{name}: screenshot was blank"
            assert app.proc.poll() is None, f"{name}: app exited"
            assert "panicked at" not in app.stderr(), f"{name}: app panicked"
            report.check(f"{name}: correct queue state, painted controls and native capture", True,
                         f"{count} queued, {dimensions}, {screenshot}")
        except (AssertionError, OSError, ValueError, StopIteration) as error:
            report.check(f"{name}: correct queue state, painted controls and native capture", False, error)
        finally:
            app.stop()
    report.finish()


if __name__ == "__main__":
    main()
