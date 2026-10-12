import contextlib
import io
import json
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import bp
import interact


class InteractionEvidenceTests(unittest.TestCase):
    def run_suite(self, stdout, returncode=0, stderr="cargo diagnostic"):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            with contextlib.ExitStack() as stack:
                for name in ("DATA", "UI", "LOGS", "BIN"):
                    stack.enter_context(patch.object(bp, name, root / name.lower()))
                stack.enter_context(patch("sys.argv", ["interact.py", "--tag", "fixture"]))
                run = stack.enter_context(patch.object(
                    interact.subprocess, "run",
                    return_value=subprocess.CompletedProcess([], returncode, stdout, stderr),
                ))
                stack.enter_context(contextlib.redirect_stdout(io.StringIO()))
                with self.assertRaises(SystemExit) as exit:
                    interact.main()
                body = json.loads((bp.DATA / "report-interact.json").read_text())
                log = (bp.LOGS / "interact-fixture.log").read_text()
                self.assertEqual(
                    run.call_args.args[0],
                    ["cargo", "test", "-p", "flint-app", "--test", "blueprint_ui", "--", "--test-threads=1"],
                )
                self.assertEqual(log, stdout + "\n--- stderr ---\n" + stderr)
                self.assertEqual(body["total"], len(body["checks"]))
                return exit.exception.code, body

    def test_process_failure_cannot_hide_behind_passing_test(self):
        code, body = self.run_suite("test partial_success ... ok\n", 101)
        self.assertEqual(code, 1)
        self.assertEqual((body["passed"], body["failed"]), (1, 1))
        self.assertEqual(body["metrics"]["returncode"], 101)
        self.assertEqual(body["checks"][-1]["name"], "partial success")
        self.assertTrue(body["checks"][-1]["ok"])

    def test_failed_test_keeps_assertion_and_process_failure(self):
        stdout = (
            "test ordinary_failure ... FAILED\n"
            "---- ordinary_failure stdout ----\n"
            "thread 'ordinary_failure' panicked at fixture.rs:10\n"
            "assertion failed: wanted success\nfailures:\n"
        )
        code, body = self.run_suite(stdout, 101)
        self.assertEqual(code, 1)
        self.assertEqual(body["failed"], 2)
        self.assertIn("assertion failed", body["checks"][-1]["detail"])

    def test_failed_test_fails_even_if_process_exit_is_zero(self):
        code, body = self.run_suite("test impossible_success ... FAILED\n")
        self.assertEqual(code, 1)
        self.assertEqual(body["failed"], 1)

    def test_no_recognized_output_fails_even_on_zero_exit(self):
        code, body = self.run_suite("running 0 tests\n")
        self.assertEqual(code, 1)
        self.assertEqual(body["metrics"]["tests"], 0)
        self.assertEqual(body["failed"], 1)

    def test_successful_suite_retains_every_test(self):
        code, body = self.run_suite("test first ... ok\ntest second ... ok\n")
        self.assertEqual(code, 0)
        self.assertEqual((body["passed"], body["failed"], body["total"]), (3, 0, 3))
        self.assertEqual(body["metrics"]["tests"], 2)
        self.assertEqual(body["metrics"]["passed"], 2)
        self.assertEqual(body["skipped"], 0)

    def test_all_ignored_tests_are_skipped_not_passed_or_failed(self):
        code, body = self.run_suite("test first ... ignored\ntest second ... ignored\n")
        self.assertEqual(code, 0)
        self.assertEqual((body["passed"], body["failed"], body["skipped"], body["total"]), (1, 0, 2, 3))
        self.assertEqual(body["metrics"]["passed"], 0)
        self.assertEqual(body["metrics"]["skipped"], 2)
        self.assertTrue(all(c["skipped"] and c["ok"] is None for c in body["checks"][1:]))

    def test_mixed_ignored_tests_do_not_fail_successful_suite(self):
        code, body = self.run_suite("test first ... ok\ntest second ... ignored\n")
        self.assertEqual(code, 0)
        self.assertEqual((body["passed"], body["failed"], body["skipped"], body["total"]), (2, 0, 1, 3))
        self.assertEqual(body["metrics"]["tests"], 2)

    def test_ignored_test_with_reason_does_not_disappear(self):
        code, body = self.run_suite("test first ... ok\ntest second ... ignored, platform unavailable\n")
        self.assertEqual(code, 0)
        self.assertEqual(body["metrics"]["tests"], 2)
        self.assertEqual((body["passed"], body["failed"], body["skipped"], body["total"]), (2, 0, 1, 3))
        self.assertEqual(body["checks"][-1]["name"], "second")


if __name__ == "__main__":
    unittest.main()
