import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock

import eval_report


class EvidencePageTests(unittest.TestCase):
    def render(self, directory, valid, workflow=False, ui=False,
               validation_log=None, ui_log_text=None):
        root = Path(directory) / "orchestrator-1791662692-fixture"
        root.mkdir()
        row = {"variant": "<script>", "id": "censored-fixture",
               "proxy": {"budget": {"calls": 2}, "requests": [{}]},
               "score": {"passed": True}, "task": "<fixture>",
               "metrics": {"outcome": "needs_review", "observed_edits": 0, "counts": {}},
               "grader": {"tests_run": None, "passed": True}}
        report = {"results": [row], "summary": {"<script>": {
            "scenario_passed": 1, "scenarios": 1, "artifacts_passed": 1,
            "deliverable_tasks": 1, "artifacts_held_by_host": 0, "provider_calls": 1}},
            "budget": {"calls": 3, "charged_tokens": 1234},
            "budget_limits": {"max_calls": 120, "max_tokens": 300000}}
        data = root / "report.json"
        data.write_text(json.dumps(report))
        log = root / "validation.log"
        if validation_log is None:
            validation_log = (
                "test result: ok. 304 passed; 0 failed; 1 ignored\n"
                "Ran 12 tests\n\nOK\nRan 125 tests\n\nOK\n"
                "Finished `release` profile\n" if valid else "error: failed")
        log.write_text(validation_log)
        out = root / "page.html"
        argv = ["eval_report.py", "--baseline", str(data), "--post", str(data),
                "--offline", str(data), "--validation", str(log), "--out", str(out)]
        if workflow:
            argv.extend(["--workflow", str(data)])
        if ui:
            ui_log = root / "ui.log"
            ui_log.write_text(ui_log_text if ui_log_text is not None else
                              "test result: ok. 45 passed; 0 failed; 0 ignored\n")
            argv.extend(["--ui-validation", str(ui_log)])
        with mock.patch.object(sys, "argv", argv), mock.patch("builtins.print"):
            eval_report.main()
        return out

    def test_page_escapes_data_and_discloses_censored_trials(self):
        with tempfile.TemporaryDirectory() as directory:
            page = self.render(directory, True).read_text()
            self.assertIn("&lt;script&gt;", page)
            self.assertNotIn("<script>", page)
            self.assertIn("censored-fixture", page)
            self.assertIn("2026-10-10", page)
            self.assertIn("1,234", page)
            self.assertIn("304 + 125", page)

    def test_failed_validation_cannot_render_a_passing_page(self):
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaises(ValueError):
                self.render(directory, False)

    def test_failed_python_run_cannot_borrow_a_later_passing_status(self):
        log = ("test result: ok. 304 passed; 0 failed; 1 ignored\n"
               "Ran 12 tests in 0.1s\n\nFAILED (failures=1)\n"
               "Ran 125 tests in 1.0s\n\nOK (skipped=1)\n"
               "Finished `release` profile\n")
        with tempfile.TemporaryDirectory() as directory:
            page = self.render(directory, True, validation_log=log).read_text()
            self.assertIn("304 + 125", page)

    def test_latest_failed_test_run_blocks_the_evidence_page(self):
        passing = ("test result: ok. 304 passed; 0 failed; 1 ignored\n"
                   "Ran 125 tests\n\nOK\n")
        for failure in ("Ran 12 tests\n\nFAILED (failures=1)\n",
                        "test result: FAILED. 2 passed; 1 failed; 0 ignored\n",
                        "Ran 12 tests\n",
                        "running 12 tests\n"):
            with self.subTest(failure=failure), tempfile.TemporaryDirectory() as directory:
                with self.assertRaises(ValueError):
                    self.render(directory, True, validation_log=
                                passing + failure + "Finished `release` profile\n")

    def test_latest_rust_test_count_is_used(self):
        log = ("test result: ok. 304 passed; 0 failed; 1 ignored\n"
               "test result: ok. 306 passed; 0 failed; 1 ignored\n"
               "Ran 125 tests\n\nOK\nFinished `release` profile\n")
        with tempfile.TemporaryDirectory() as directory:
            page = self.render(directory, True, validation_log=log).read_text()
            self.assertIn("306 + 125", page)

    def test_python_run_started_without_a_summary_cannot_borrow_earlier_pass(self):
        passing = ("test result: ok. 304 passed; 0 failed; 1 ignored\n"
                   "Ran 125 tests\n\nOK\nFinished `release` profile\n")
        for started in (
                "Report-renderer regression sweep:\n",
                ".",
                ".sEFxu\n",
                "test_page (test_eval_report.EvidencePageTests) ... ",
                "runTest (unittest.case.FunctionTestCase) ... ok\n"):
            with self.subTest(started=started), tempfile.TemporaryDirectory() as directory:
                with self.assertRaises(ValueError):
                    self.render(directory, True, validation_log=passing + started)

    def test_completed_python_rerun_uses_its_own_summary(self):
        log = ("test result: ok. 304 passed; 0 failed; 1 ignored\n"
               "Ran 125 tests\n\nOK\nFinished `release` profile\n"
               "Report-renderer regression sweep:\n.........\n"
               "Ran 9 tests in 0.1s\n\nOK\n")
        with tempfile.TemporaryDirectory() as directory:
            page = self.render(directory, True, validation_log=log).read_text()
            self.assertIn("304 + 9", page)

    def test_later_rust_output_does_not_look_like_a_python_rerun(self):
        log = ("Ran 125 tests\n\nOK\n"
               "running 2 tests\n"
               "test session::one ... ok\n"
               "test session::two ... ok\n"
               "test result: ok. 2 passed; 0 failed; 0 ignored\n"
               "Finished `release` profile\n")
        with tempfile.TemporaryDirectory() as directory:
            page = self.render(directory, True, validation_log=log).read_text()
            self.assertIn("2 + 125", page)

    def test_latest_failed_ui_run_blocks_the_evidence_page(self):
        for latest in ("test result: FAILED. 44 passed; 1 failed; 0 ignored\n",
                       "running 45 tests\n"):
            log = "test result: ok. 45 passed; 0 failed; 0 ignored\n" + latest
            with self.subTest(latest=latest), tempfile.TemporaryDirectory() as directory:
                with self.assertRaises(ValueError):
                    self.render(directory, True, workflow=True, ui=True, ui_log_text=log)

    def test_fresh_workflow_evidence_has_separate_budget_and_escaped_rows(self):
        with tempfile.TemporaryDirectory() as directory:
            page = self.render(directory, True, workflow=True).read_text()
            self.assertIn("1/1 raw live records passed", page)
            self.assertIn("3/120 calls", page)
            self.assertIn("1,234/300,000 tokens", page)
            self.assertIn("&lt;fixture&gt;", page)
            self.assertNotIn("<fixture>", page)
            self.assertIn("not a pre-fix Bun A/B", page)

    def test_local_token_reservation_denial_is_censored_even_without_extra_call_count(self):
        row = {"proxy": {"budget": {"calls": 1}, "requests": [{}]},
               "metrics": {"turn_reason": {"Failed": "conservative token budget exhausted"}}}
        self.assertTrue(eval_report.budget_censored(row))

    def test_scoped_ui_validation_does_not_claim_a_full_ui_suite_pass(self):
        with tempfile.TemporaryDirectory() as directory:
            page = self.render(directory, True, workflow=True, ui=True).read_text()
            self.assertIn("45 scoped headless task-outcome UI tests passed", page)
            self.assertIn("no full-suite pass", page)
