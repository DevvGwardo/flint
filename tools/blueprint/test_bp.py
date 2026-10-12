import contextlib
import io
import json
import math
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import bp
import diff
import sweep


def strict_loads(text):
    def reject(value):
        raise ValueError(f"non-standard JSON constant: {value}")
    return json.loads(text, parse_constant=reject)


class MetricSerializationTests(unittest.TestCase):
    def test_nested_nonfinite_and_missing_values_have_paths_and_reasons(self):
        values = {"runs": [{"finite": 1.25, "missing": float("nan")},
                           [float("inf"), -float("inf"), None, 0, 3.5]]}
        body = strict_loads(bp.strict_json(values))
        self.assertEqual(body["runs"], [{"finite": 1.25, "missing": None},
                                       [None, None, None, 0, 3.5]])
        self.assertEqual(
            {item["path"] for item in body["missing_metrics"]},
            {"$.runs[0].missing", "$.runs[1][0]", "$.runs[1][1]", "$.runs[1][2]"},
        )
        self.assertTrue(all(item["reason"] for item in body["missing_metrics"]))
        self.assertTrue(math.isnan(values["runs"][0]["missing"]))
        self.assertEqual(values["runs"][1][0], float("inf"))

    def test_finite_samples_are_unchanged(self):
        values = {"samples": [0, -2, 1.23456789, 1e100], "nested": {"ok": True}, "label": "unknown"}
        self.assertEqual(strict_loads(bp.strict_json(values)), values)

    def test_empty_statistics_are_missing_not_zero(self):
        body = strict_loads(bp.strict_json({"median": bp.median([]), "p95": bp.percentile([], 95)}))
        self.assertIsNone(body["median"])
        self.assertIsNone(body["p95"])
        self.assertEqual(len(body["missing_metrics"]), 2)

    def test_finite_statistics_are_unchanged(self):
        self.assertEqual(bp.median([1, 3, 5]), 3)
        self.assertEqual(bp.percentile([0, 10], 95), 9.5)


class ReportDirectories(unittest.TestCase):
    def setUp(self):
        self.stack = contextlib.ExitStack()
        self.addCleanup(self.stack.close)
        root = Path(self.stack.enter_context(tempfile.TemporaryDirectory()))
        for name in ("DATA", "UI", "LOGS", "BIN"):
            path = root / name.lower()
            path.mkdir()
            self.stack.enter_context(patch.object(bp, name, path))
        self.stack.enter_context(contextlib.redirect_stdout(io.StringIO()))

    def finish(self, report):
        with self.assertRaises(SystemExit) as exit:
            report.finish()
        path = bp.DATA / f"report-{report.script}.json"
        text = path.read_text()
        self.assertEqual(text, (bp.DATA / f"report-{report.script}-{report.tag}.json").read_text())
        body = strict_loads(text)
        self.assertEqual(body["total"], len(body["checks"]))
        return exit.exception.code, body


class ReportEvidenceTests(ReportDirectories):
    def test_missing_required_metrics_cannot_claim_all_checks_passed(self):
        report = bp.Report("fixture", "after")
        report.check("existing success", True)
        report.metrics = {"samples": [1.5, float("nan"), None]}
        code, body = self.finish(report)
        self.assertEqual(code, 1)
        self.assertEqual((body["passed"], body["failed"], body["total"]), (1, 1, 2))
        self.assertEqual(body["metrics"]["samples"], [1.5, None, None])
        self.assertIn("$.metrics.samples[1]", body["checks"][-1]["detail"])

    def test_finite_report_and_historical_unknowns_do_not_fail(self):
        report = bp.Report("fixture", "after")
        report.check("existing success", True)
        report.metrics = {"current": {"value": 2.5}, "before": {"value": float("nan"), "other": "unknown"}}
        code, body = self.finish(report)
        self.assertEqual(code, 0)
        self.assertEqual((body["passed"], body["failed"], body["total"]), (1, 0, 1))
        self.assertEqual(body["metrics"]["current"]["value"], 2.5)
        self.assertIsNone(body["metrics"]["before"]["value"])
        self.assertEqual(body["metrics"]["before"]["other"], "unknown")

    def test_absent_baseline_is_not_a_required_measurement(self):
        report = bp.Report("fixture", "after")
        report.metrics = {"current": {"value": 2.5}, "before": None}
        code, body = self.finish(report)
        self.assertEqual(code, 0)
        self.assertEqual(body["failed"], 0)

    def test_existing_failures_are_not_erased_by_missing_metric_failure(self):
        report = bp.Report("fixture", "after")
        report.check("existing failure", False, "keep this")
        report.metrics = {"value": float("inf")}
        code, body = self.finish(report)
        self.assertEqual(code, 1)
        self.assertEqual(body["failed"], 2)
        self.assertEqual(body["checks"][0]["detail"], "keep this")

    def test_explicit_skip_does_not_count_as_pass_or_failure(self):
        report = bp.Report("fixture", "after")
        report.check("success", True)
        report.skip("optional live measurement", "disabled")
        code, body = self.finish(report)
        self.assertEqual(code, 0)
        self.assertEqual((body["passed"], body["failed"], body["skipped"], body["total"]), (1, 0, 1, 2))
        self.assertIsNone(body["checks"][1]["ok"])


class DiffEvidenceTests(ReportDirectories):
    def test_missing_frames_use_strict_raw_and_report_json(self):
        app = unittest.mock.Mock()
        app.wait_window.return_value = True
        app.screenshot.return_value = True
        app.rss_mb.return_value = 12.5
        app.cpu_percent.return_value = 0.2
        app.wait_exit.return_value = 0
        app.stderr.return_value = ""
        for n in range(3):
            (bp.DATA / f"diff-after-{n}-state.json").write_text(json.dumps({
                "sessions": [{"active": True, "changes": [{"added": 10000}]}],
            }))
        with patch.object(diff.bp, "resolve_bin", return_value=Path("/fake/flint")), \
                patch.object(diff.bp, "App", return_value=app), \
                patch.object(diff.time, "sleep"), \
                patch("sys.argv", ["diff.py", "--tag", "after"]), \
                self.assertRaises(SystemExit) as exit:
            diff.main()
        self.assertEqual(exit.exception.code, 1)
        raw = strict_loads((bp.DATA / "diff-after.json").read_text())
        body = strict_loads((bp.DATA / "report-diff.json").read_text())
        self.assertIsNone(raw["interval_p95_ms"])
        self.assertIsNone(raw["root_render_p95_ms"])
        self.assertEqual(len(raw["runs"]), 3)
        self.assertTrue(all(run["frames"] == 0 for run in raw["runs"]))
        self.assertEqual(body["failed"], 4)
        self.assertEqual(body["passed"], 13)
        self.assertEqual(body["total"], 17)


class ProcessProbeTests(unittest.TestCase):
    def app(self):
        app = bp.App.__new__(bp.App)
        app.proc = unittest.mock.Mock(pid=123)
        return app

    def test_missing_or_failed_rss_is_not_zero(self):
        app = self.app()
        for code, output in ((1, ""), (0, ""), (1, "1024"), (0, "bad"), (0, "0"), (0, "-1")):
            with self.subTest(code=code, output=output), patch.object(
                bp.subprocess, "run", return_value=subprocess.CompletedProcess([], code, output, "")
            ):
                self.assertTrue(math.isnan(app.rss_mb()))

    def test_valid_rss_keeps_megabyte_units(self):
        with patch.object(bp.subprocess, "run", return_value=subprocess.CompletedProcess([], 0, "2048\n", "")):
            self.assertEqual(self.app().rss_mb(), 2.0)

    def test_missing_or_failed_cpu_is_not_zero(self):
        app = self.app()
        for code, output in ((1, ""), (0, ""), (1, "00:01.25"), (0, "bad"), (0, "-1"), (0, "NaN")):
            with self.subTest(code=code, output=output), patch.object(
                bp.subprocess, "run", return_value=subprocess.CompletedProcess([], code, output, "")
            ):
                self.assertTrue(math.isnan(app.cpu_seconds()))

    def test_cpu_time_formats_and_real_zero_are_preserved(self):
        for text, expected in (("00:00.00", 0), ("01:02.50", 62.5),
                               ("02:03:04.25", 7384.25), ("1-02:03:04.25", 93784.25)):
            with self.subTest(text=text):
                self.assertEqual(bp.parse_cpu_time(text), expected)

    def test_invalid_cpu_time_is_missing(self):
        for text in ("", "bad", "NaN", "inf", "-1", "00:60.00"):
            with self.subTest(text=text):
                self.assertTrue(math.isnan(bp.parse_cpu_time(text)))

    def test_cpu_counter_reset_cannot_pass_as_negative_cpu(self):
        app = self.app()
        app.cpu_seconds = unittest.mock.Mock(side_effect=[2.0, 1.0])
        with patch.object(bp.time, "sleep"):
            self.assertTrue(math.isnan(app.cpu_percent(2.0)))

    def test_valid_cpu_percent_is_preserved(self):
        app = self.app()
        app.cpu_seconds = unittest.mock.Mock(side_effect=[1.0, 1.5])
        with patch.object(bp.time, "sleep"):
            self.assertEqual(app.cpu_percent(2.0), 25.0)

    def test_forced_stop_waits_after_kill_and_closes_log(self):
        app = self.app()
        app.log = io.StringIO()
        app.proc.poll.return_value = None
        app.proc.wait.side_effect = [subprocess.TimeoutExpired("fixture", 5), -9]
        app.proc.returncode = -9
        self.assertEqual(app.stop(), -9)
        self.assertEqual(app.proc.wait.call_args_list, [unittest.mock.call(5), unittest.mock.call()])
        app.proc.kill.assert_called_once()
        self.assertTrue(app.log.closed)

    def test_real_owned_process_probes_and_stop(self):
        with tempfile.TemporaryDirectory() as tmp:
            app = bp.App.__new__(bp.App)
            app.log_path = Path(tmp) / "probe.log"
            app.log = app.log_path.open("w")
            app.proc = subprocess.Popen(
                [sys.executable, "-c", "import time; time.sleep(30)"],
                stdout=subprocess.DEVNULL, stderr=app.log,
            )
            try:
                self.assertGreater(app.rss_mb(), 0)
                self.assertTrue(math.isfinite(app.cpu_seconds()))
                self.assertIsNone(app.wait_exit(0.01))
            finally:
                app.stop()
            self.assertTrue(app.log.closed)
            self.assertIsNotNone(app.proc.returncode)
            self.assertTrue(math.isnan(app.rss_mb()))
            self.assertTrue(math.isnan(app.cpu_seconds()))

    def test_real_forced_stop_reaps_owned_process(self):
        with tempfile.TemporaryDirectory() as tmp:
            app = bp.App.__new__(bp.App)
            app.log_path = Path(tmp) / "forced.log"
            app.log = app.log_path.open("w")
            app.proc = subprocess.Popen([
                sys.executable, "-c",
                "import signal, time; signal.signal(signal.SIGTERM, signal.SIG_IGN); "
                "print('ready', flush=True); time.sleep(30)",
            ], stdout=app.log, stderr=app.log)
            try:
                deadline = bp.time.monotonic() + 5
                while "ready" not in app.log_path.read_text() and bp.time.monotonic() < deadline:
                    bp.time.sleep(0.01)
                self.assertIn("ready", app.log_path.read_text())
            finally:
                code = app.stop()
            self.assertEqual(code, -9)
            self.assertTrue(app.log.closed)
            self.assertEqual(app.proc.poll(), -9)


class FreshDirectoryTests(unittest.TestCase):
    def test_existing_directory_and_evidence_are_preserved(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "run"
            path.mkdir()
            evidence = path / "evidence.txt"
            evidence.write_text("keep me")
            with self.assertRaises(FileExistsError):
                bp.fresh_dir(path)
            self.assertEqual(evidence.read_text(), "keep me")

    def test_existing_symlink_is_refused_without_touching_target(self):
        with tempfile.TemporaryDirectory() as tmp:
            target = Path(tmp) / "target"
            target.mkdir()
            path = Path(tmp) / "run"
            path.symlink_to(target, target_is_directory=True)
            with self.assertRaises(FileExistsError):
                bp.fresh_dir(path)
            self.assertTrue(path.is_symlink())
            self.assertTrue(target.is_dir())

    def test_new_directory_can_create_parents(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "parent" / "run"
            self.assertEqual(bp.fresh_dir(path), path)
            self.assertTrue(path.is_dir())


class SweepProbeTests(ReportDirectories):
    def test_missing_rss_and_cpu_fail_without_crashing_report(self):
        app = unittest.mock.Mock(window_ms=10, window=(1, 0, 0, 1440, 900))
        app.wait_window.return_value = True
        app.rss_mb.return_value = float("nan")
        app.cpu_percent.return_value = float("nan")
        app.stderr.return_value = ""
        with patch.object(bp, "OUT", bp.DATA.parent), \
                patch.object(bp, "resolve_bin", return_value=Path("/fake/flint")), \
                patch.object(bp, "App", return_value=app), \
                patch.object(bp, "main_screen", return_value=(1920, 1080)), \
                patch.object(sweep, "STATES", [sweep.STATES[0]]), \
                patch.object(sweep, "wait_capture_state", return_value={}), \
                patch.object(sweep.time, "sleep"), \
                patch("sys.argv", ["sweep.py", "--tag", "missing"]), \
                self.assertRaises(SystemExit) as exit:
            sweep.main()
        self.assertEqual(exit.exception.code, 1)
        body = strict_loads((bp.DATA / "report-sweep.json").read_text())
        self.assertIsNone(body["metrics"]["empty"]["rss_mb"])
        self.assertIsNone(body["metrics"]["empty"]["cpu_pct"])
        self.assertFalse(next(c for c in body["checks"] if "memory settles" in c["name"])["ok"])


if __name__ == "__main__":
    unittest.main()
