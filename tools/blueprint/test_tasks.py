import copy
import contextlib
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import Mock, patch

import bp
import tasks
from tasks import check_state


class TaskCaptureTests(unittest.TestCase):
    def fixture(self, scene="partial"):
        task = {
            "source": "native", "contract": {"mode": "work"},
            "assessment": {"outcome": scene},
            "checks": [{}], "acceptance": None,
        }
        if scene == "external":
            task.update(source="external", checks=[])
            task["contract"]["mode"] = "unspecified"
            task["assessment"]["outcome"] = "unknown"
        if scene == "accepted":
            task["acceptance"] = {"work_revision": 7}
        if scene == "historical":
            task["source"] = "legacy"
            task["assessment"]["outcome"] = "unknown"
        if scene == "queue":
            task["assessment"]["outcome"] = "partial"
        if scene in ("review", "editing", "confirmation"):
            task["assessment"]["outcome"] = "needs_review"
        if scene == "stale":
            task["assessment"]["outcome"] = "partial"
        return {
            "capture": {
                "fully_visible": True, "window_active": False, "painted_at_ms": 701,
            },
            "sessions": [{
                "active": True, "task": task, "queued_prompts": 1,
                "queue_paused": True, "queue_review_hold": "Partial. Review first.",
            }],
            "task_details_open": scene != "queue",
            "task_details_turn_id": 1 if scene != "queue" else None,
            "task_scope_editing": scene == "editing",
            "task_accept_confirming": scene == "confirmation",
            "task_controls": {
                name: {
                    "fully_visible": True, "window_active": False, "painted_at_ms": 701,
                    "y": y, "height": height,
                }
                for name, y, height in (("sheet", 10, 500), ("header", 10, 60), ("footer", 460, 50),
                                       ("queue-review", 70, 60), ("queue-confirmation", 130, 100))
            },
        }

    def test_each_intended_scene_has_honest_state(self):
        for scene in ("partial", "review", "blocked", "external", "accepted", "stale",
                      "editing", "confirmation", "queue", "historical"):
            with self.subTest(scene=scene):
                check_state(self.fixture(scene), scene)

    def test_wrong_outcome_or_invented_external_evidence_fails(self):
        state = self.fixture()
        state["sessions"][0]["task"]["assessment"]["outcome"] = "accepted"
        with self.assertRaises(AssertionError):
            check_state(state, "partial")
        state = self.fixture("external")
        state["sessions"][0]["task"]["checks"] = [{}]
        with self.assertRaises(AssertionError):
            check_state(state, "external")

    def test_missing_clipped_active_and_early_paints_fail(self):
        for name in ("sheet", "header", "footer"):
            for property_name, value in (("fully_visible", False), ("window_active", True),
                                         ("painted_at_ms", 0)):
                state = copy.deepcopy(self.fixture())
                state["task_controls"][name][property_name] = value
                with self.subTest(name=name, property_name=property_name):
                    with self.assertRaises(AssertionError):
                        check_state(state, "partial")

    def test_footer_overlap_or_overflow_fails(self):
        for y in (20, 490):
            state = self.fixture()
            state["task_controls"]["footer"]["y"] = y
            with self.assertRaises(AssertionError):
                check_state(state, "partial")

    def test_unpainted_root_cannot_certify_task_controls(self):
        state = self.fixture()
        del state["capture"]
        with self.assertRaises(AssertionError):
            check_state(state, "partial")


class TaskCaptureLaunchTests(unittest.TestCase):
    def test_old_dump_cannot_certify_a_launch_that_does_not_write_current_state(self):
        with tempfile.TemporaryDirectory() as tmp, contextlib.ExitStack() as stack:
            root = Path(tmp)
            stack.enter_context(patch.object(bp, "OUT", root))
            for name in ("DATA", "UI", "LOGS", "BIN"):
                directory = root / name.lower()
                directory.mkdir()
                stack.enter_context(patch.object(bp, name, directory))
            stack.enter_context(patch.object(tasks, "OUTCOMES", {"partial": "partial"}))
            stack.enter_context(patch("sys.argv", ["tasks.py", "--tag", "same"]))
            stack.enter_context(contextlib.redirect_stdout(io.StringIO()))
            stale = TaskCaptureTests().fixture()
            stale["capture"] = {
                "fully_visible": True, "window_active": False, "painted_at_ms": 701,
            }
            state_path = bp.DATA / "task-partial-1440x900-same.json"
            state_path.write_text(json.dumps(stale))
            app = Mock()
            app.wait_window.return_value = True
            app.proc.poll.return_value = None
            app.stderr.return_value = ""
            app.screenshot.return_value = True

            def inspect_launch(*_, **kwargs):
                # The retained report stays inspectable; this launch must
                # nevertheless refuse a reused state path before accepting it.
                self.assertFalse(
                    Path(kwargs["env"]["FLINT_BP_STATE"]).exists(),
                    "the new fixture inherited a completed dump from another launch",
                )
                return app

            stack.enter_context(patch.object(bp, "App", side_effect=inspect_launch))
            stack.enter_context(patch.object(bp, "image_stats", return_value=(10, (1440, 900))))
            stack.enter_context(patch.object(tasks, "wait_capture_state", side_effect=TimeoutError()))
            with self.assertRaises(SystemExit) as exit:
                tasks.main()
            self.assertEqual(exit.exception.code, 1)
            report = json.loads((bp.DATA / "report-tasks.json").read_text())
            self.assertTrue(all(not check["ok"] for check in report["checks"]))


if __name__ == "__main__":
    unittest.main()
