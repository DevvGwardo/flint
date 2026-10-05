import copy
import importlib.util
import unittest
from pathlib import Path

spec = importlib.util.spec_from_file_location("queue_capture", Path(__file__).with_name("queue.py"))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
check_controls = module.check_controls


class CaptureControlsTests(unittest.TestCase):
    def fixture(self, running=True):
        names = ("send", "stop", "task_tray") if running else ("send",)
        return {
            "composer_controls": {
                "running": running,
                "tasks": int(running),
                "painted": {
                    name: {"fully_visible": True, "window_active": False, "painted_at_ms": 701}
                    for name in names
                },
            }
        }

    def test_running_and_idle_paints_pass(self):
        for running in (True, False):
            with self.subTest(running=running):
                check_controls(self.fixture(running), running)

    def test_absent_legacy_and_stale_controls_fail(self):
        for state in ({}, {"composer_controls": None}, self.fixture(False)):
            with self.subTest(state=state):
                with self.assertRaises(AssertionError):
                    check_controls(state, True)

    def test_each_missing_clipped_or_key_control_fails(self):
        for name in ("send", "stop", "task_tray"):
            for change in ("missing", "clipped", "key", "early"):
                state = copy.deepcopy(self.fixture())
                painted = state["composer_controls"]["painted"]
                if change == "missing":
                    del painted[name]
                elif change == "clipped":
                    painted[name]["fully_visible"] = False
                elif change == "key":
                    painted[name]["window_active"] = True
                else:
                    painted[name]["painted_at_ms"] = 0
                with self.subTest(name=name, change=change):
                    with self.assertRaises(AssertionError):
                        check_controls(state, True)

    def test_wrong_task_count_and_idle_running_controls_fail(self):
        state = self.fixture()
        state["composer_controls"]["tasks"] = 0
        with self.assertRaises(AssertionError):
            check_controls(state, True)
        state = self.fixture(False)
        state["composer_controls"]["painted"]["stop"] = {}
        with self.assertRaises(AssertionError):
            check_controls(state, False)


if __name__ == "__main__":
    unittest.main()
