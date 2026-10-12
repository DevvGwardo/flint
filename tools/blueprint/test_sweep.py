from copy import deepcopy
import contextlib
import io
import json
import math
from pathlib import Path
import tempfile
import unittest
from unittest.mock import Mock, patch

import bp
import sweep


def painted_running_state():
    return {
        "sessions": [{"active": True, "status": "running"}],
        "capture": {
            "fully_visible": True,
            "window_active": False,
            "painted_at_ms": sweep.SETTLE_S * 1000 + 1,
        },
    }


class CaptureReadinessTests(unittest.TestCase):
    def test_completed_inactive_paint_of_expected_scene_is_ready(self):
        sweep.check_capture_state(painted_running_state(), "active_running")

    def test_model_state_without_completed_paint_is_not_ready(self):
        state = painted_running_state()
        del state["capture"]
        with self.assertRaises(AssertionError):
            sweep.check_capture_state(state, "active_running")

    def test_clipped_active_early_or_missing_paint_is_not_ready(self):
        for key, value in (
            ("fully_visible", False), ("fully_visible", None),
            ("window_active", True), ("window_active", None),
            ("painted_at_ms", sweep.SETTLE_S * 1000 - 1),
            ("painted_at_ms", None), ("painted_at_ms", math.nan),
        ):
            with self.subTest(key=key, value=value):
                state = painted_running_state()
                state["capture"][key] = value
                with self.assertRaises(AssertionError):
                    sweep.check_capture_state(state, "active_running")

    def test_painted_wrong_scene_cannot_be_accepted(self):
        state = painted_running_state()
        state["sessions"][0]["status"] = "idle"
        with self.assertRaises(AssertionError):
            sweep.check_capture_state(state, "active_running")

    def test_every_sweep_scene_has_an_explicit_expected_state(self):
        self.assertTrue(all(expected for *_, expected in sweep.STATES))

    def test_wait_retries_partial_dump_before_returning_complete_state(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "state.json"
            expected = painted_running_state()
            with patch.object(Path, "read_text", side_effect=[
                FileNotFoundError(), '{"sessions":', json.dumps(expected),
            ]), patch.object(sweep.time, "sleep") as sleep:
                self.assertEqual(sweep.wait_capture_state(path), expected)
            self.assertEqual(sleep.call_count, 2)

    def test_missing_dump_times_out_instead_of_proving_readiness(self):
        with tempfile.TemporaryDirectory() as tmp, self.assertRaises(TimeoutError):
            sweep.wait_capture_state(Path(tmp) / "missing.json", timeout=0)


class SweepCaptureOrderTests(unittest.TestCase):
    def setUp(self):
        self.stack = contextlib.ExitStack()
        self.addCleanup(self.stack.close)
        root = Path(self.stack.enter_context(tempfile.TemporaryDirectory()))
        self.stack.enter_context(patch.object(bp, "OUT", root))
        for name in ("DATA", "UI", "LOGS", "BIN"):
            self.stack.enter_context(patch.object(bp, name, root / name.lower()))
        self.stack.enter_context(patch.object(bp, "resolve_bin", return_value=Path("/fake/flint")))
        self.stack.enter_context(patch.object(bp, "main_screen", return_value=(1920, 1080)))
        self.stack.enter_context(patch.object(sweep, "STATES", [sweep.STATES[1]]))
        self.stack.enter_context(patch.object(sweep.time, "sleep"))
        self.stack.enter_context(patch("sys.argv", ["sweep.py", "--tag", "capture"]))
        self.stack.enter_context(contextlib.redirect_stdout(io.StringIO()))
        self.app = Mock(window_ms=10, window=(1, 0, 0, 1440, 900))
        self.app.wait_window.return_value = True
        self.app.rss_mb.return_value = 100
        self.app.cpu_percent.return_value = 1
        self.app.stderr.return_value = ""
        self.launch = self.stack.enter_context(patch.object(bp, "App", return_value=self.app))
        self.events = []
        self.wait = self.stack.enter_context(patch.object(
            sweep, "wait_capture_state", create=True,
            side_effect=lambda *_: self.record_state(),
        ))
        self.app.screenshot.side_effect = self.capture
        self.stack.enter_context(patch.object(bp, "image_stats", return_value=(10, (1440, 900))))

    def record_state(self):
        self.events.append("paint")
        return painted_running_state()

    def capture(self, path):
        self.events.append("screenshot")
        path.touch()
        return True

    def run_sweep(self):
        with self.assertRaises(SystemExit) as exit:
            sweep.main()
        body = json.loads((bp.DATA / "report-sweep.json").read_text())
        return exit.exception.code, body

    def test_capture_mode_is_explicit_and_paint_precedes_screenshot(self):
        code, body = self.run_sweep()
        self.assertEqual(self.launch.call_args.kwargs["env"].get("FLINT_BP_CAPTURE"), "1")
        self.assertEqual(self.events, ["paint", "screenshot"])
        self.assertEqual(code, 0, body)
        self.app.stop.assert_called_once()

    def test_unpainted_model_dump_fails_but_keeps_screenshot_and_stops(self):
        state = painted_running_state()
        del state["capture"]
        self.wait.side_effect = None
        self.wait.return_value = state
        code, body = self.run_sweep()
        self.assertEqual(code, 1)
        self.assertFalse(next(check for check in body["checks"] if "completed inactive paint" in check["name"])["ok"])
        self.assertTrue((bp.UI / "capture-running.png").exists())
        self.app.stop.assert_called_once()

    def test_capture_failure_still_stops_the_owned_fixture(self):
        self.app.screenshot.side_effect = OSError("fixture capture failed")
        code, body = self.run_sweep()
        self.assertEqual(code, 1)
        self.assertTrue(any(not check["ok"] for check in body["checks"]))
        self.app.stop.assert_called_once()


class LegacyCompletionTests(unittest.TestCase):
    def setUp(self):
        self.active = {
            "active": True,
            "status": "needs_review",
            "item_kinds": ["user", "assistant", "summary"],
            "turns": [{"end_reason": "Completed", "outcome": "unknown"}],
            "task": {
                "source": "legacy",
                "assessment": {"outcome": "unknown"},
                "acceptance": None,
            },
        }

    def shown(self, active):
        return sweep.ui_flag({"sessions": [active]}, "active_stopped_unverified")

    def test_completed_legacy_fixture_stays_unverified(self):
        self.assertTrue(self.shown(self.active))

    def test_running_transport_is_not_stopped(self):
        active = deepcopy(self.active)
        active["status"] = "running"
        active["turns"][-1]["end_reason"] = None
        self.assertFalse(self.shown(active))

    def test_failed_transport_does_not_match_completed_fixture(self):
        active = deepcopy(self.active)
        active["turns"][-1]["end_reason"] = {"Failed": "fixture failure"}
        self.assertFalse(self.shown(active))

    def test_missing_summary_is_not_rendered_completion(self):
        active = deepcopy(self.active)
        active["item_kinds"].remove("summary")
        self.assertFalse(self.shown(active))

    def test_missing_projection_is_not_honest_unknown(self):
        active = deepcopy(self.active)
        active["task"] = None
        self.assertFalse(self.shown(active))

    def test_missing_finished_turn_is_not_rendered_completion(self):
        active = deepcopy(self.active)
        active["turns"] = []
        self.assertFalse(self.shown(active))

    def test_turn_outcome_cannot_disagree_with_unknown_projection(self):
        active = deepcopy(self.active)
        active["turns"][-1]["outcome"] = "accepted"
        self.assertFalse(self.shown(active))

    def test_missing_acceptance_field_is_not_verified_absence(self):
        active = deepcopy(self.active)
        del active["task"]["acceptance"]
        self.assertFalse(self.shown(active))

    def test_accepted_status_cannot_mask_legacy_unknown(self):
        active = deepcopy(self.active)
        active["status"] = "done"
        self.assertFalse(self.shown(active))

    def test_fabricated_assessment_cannot_mask_legacy_unknown(self):
        active = deepcopy(self.active)
        active["task"]["assessment"]["outcome"] = "accepted"
        self.assertFalse(self.shown(active))

    def test_user_acceptance_cannot_mask_unreviewed_fixture(self):
        active = deepcopy(self.active)
        active["task"]["acceptance"] = {"reviewed": True}
        self.assertFalse(self.shown(active))


if __name__ == "__main__":
    unittest.main()
