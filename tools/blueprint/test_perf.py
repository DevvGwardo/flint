import contextlib
import copy
import io
import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import Mock, patch

import bp
import perf
from test_bp import strict_loads


class PerformanceEvidenceTests(unittest.TestCase):
    def setUp(self):
        self.stack = contextlib.ExitStack()
        self.addCleanup(self.stack.close)
        root = Path(self.stack.enter_context(tempfile.TemporaryDirectory()))
        self.binary = root / "flint"
        self.binary.write_bytes(b"fake binary")
        for name in ("DATA", "UI", "LOGS", "BIN"):
            path = root / name.lower()
            path.mkdir()
            self.stack.enter_context(patch.object(bp, name, path))
        self.stack.enter_context(contextlib.redirect_stdout(io.StringIO()))

    def collect(self, stream, live=False):
        stream = [{"window_opened": True, "returncode": 0, **run} for run in stream]
        with patch.object(perf, "cold_start_once", return_value=(10.0, 20.0)), \
                patch.object(perf, "settled", return_value=(12.0, 0.2)), \
                patch.object(perf, "frame_run", side_effect=copy.deepcopy(stream + [
                    {"frames": 100, "window_opened": True, "returncode": 0, "interval_p95_ms": 13.3, "render_p95_ms": 0.5},
                    {"frames": 100, "window_opened": True, "returncode": 0, "interval_p95_ms": 13.3, "render_p95_ms": 0.5},
                ])), patch.object(perf, "ttft", return_value=15.0) as ttft:
            values = perf.collect({"after": self.binary}, live)["after"]
            self.assertEqual(ttft.call_count, int(live))
            return values

    def valid_values(self):
        return {
            **{key: 0.5 for key in perf.LIMITS},
            "stream": [{"frames": 100, "window_opened": True, "returncode": 0, "cpu_pct": 0.5, "interval_p95_ms": 13.3,
                        "render_p95_ms": 0.5, "dropped_frames": 0} for _ in range(2)],
            "scroll": [{"frames": 100, "window_opened": True, "returncode": 0, "interval_p95_ms": 13.3,
                        "render_p95_ms": 0.5} for _ in range(2)],
        }

    def main_with(self, values, no_live=True):
        args = ["perf.py", "--tag", "after", "--bin", str(self.binary)]
        if no_live:
            args.append("--no-live")
        with patch("sys.argv", args), patch.object(perf, "collect", return_value={"after": values}) as collect, \
                self.assertRaises(SystemExit) as exit:
            perf.main()
        self.assertEqual(collect.call_args.args[1], not no_live)
        raw = strict_loads((bp.DATA / "perf-after.json").read_text())
        body = strict_loads((bp.DATA / "report-perf.json").read_text())
        self.assertEqual(body["total"], len(body["checks"]))
        return exit.exception.code, raw, body

    def test_missing_stream_runs_do_not_synthesize_zero_drops(self):
        values = self.collect([{"cpu_pct": 0.2, "frames": 0}] * 2)
        self.assertIsNone(values["stream_dropped_frames"])
        self.assertIsNone(values["stream_frame_p95_ms"])
        self.assertIsNone(values["stream_cpu_pct"])
        self.assertNotIn("ttft_ms", values)

    def test_one_missing_run_cannot_hide_behind_good_run(self):
        values = self.collect([
            {"cpu_pct": 0.2, "frames": 100, "interval_p95_ms": 13.3, "dropped_frames": 0, "render_p95_ms": 0.5},
            {"cpu_pct": 0.2, "frames": 0},
        ])
        self.assertIsNone(values["stream_dropped_frames"])
        self.assertIsNone(values["stream_frame_p95_ms"])
        self.assertEqual(values["stream"][0]["dropped_frames"], 0)

    def test_dropped_measurement_is_required_even_when_frames_exist(self):
        values = self.collect([
            {"cpu_pct": 0.2, "frames": 100, "interval_p95_ms": 13.3, "render_p95_ms": 0.5},
        ] * 2)
        self.assertIsNone(values["stream_dropped_frames"])

    def test_valid_frame_measurements_and_ttft_are_preserved(self):
        runs = [
            {"cpu_pct": 20, "frames": 100, "interval_p95_ms": 13.2, "dropped_frames": 2, "render_p95_ms": 0.4},
            {"cpu_pct": 22, "frames": 120, "interval_p95_ms": 13.4, "dropped_frames": 3, "render_p95_ms": 0.6},
        ]
        values = self.collect(runs, live=True)
        self.assertEqual(values["stream"], [{"window_opened": True, "returncode": 0, **run} for run in runs])
        self.assertEqual(values["stream_cpu_pct"], 21)
        self.assertEqual(values["stream_frame_p95_ms"], 13.3)
        self.assertEqual(values["stream_dropped_frames"], 3)
        self.assertEqual(values["ttft_ms"], 15)

    def test_frame_run_requires_intervals_not_zero_or_one_frame(self):
        app = Mock()
        app.cpu_percent.return_value = 0.2
        app.wait_exit.return_value = 0
        for frames in ([], [[0, 0.2]], [[0, 0.2], [1000, 0.2]]):
            (bp.DATA / "frames-fixture.json").unlink(missing_ok=True)
            def write_frames(*args):
                (bp.DATA / "frames-fixture.json").write_text(json.dumps({"frames": frames}))
                return True
            app.wait_window.side_effect = write_frames
            with self.subTest(frames=frames), patch.object(perf.bp, "App", return_value=app), \
                    patch.object(perf.time, "sleep"):
                result = perf.frame_run(self.binary, [], "fixture")
            self.assertIsNone(result["dropped_frames"])
            self.assertIsNone(result["seconds"])
            self.assertTrue(result["frames"] <= 1)

    def test_frame_run_missing_file_marks_missing_measurements(self):
        app = Mock()
        app.cpu_percent.return_value = 0.2
        app.wait_exit.return_value = 0
        with patch.object(perf.bp, "App", return_value=app), patch.object(perf.time, "sleep"):
            result = perf.frame_run(self.binary, [], "missing")
        self.assertEqual(result["frames"], 0)
        self.assertIsNone(result["dropped_frames"])
        self.assertIsNone(result["interval_p95_ms"])

    def test_frame_run_valid_samples_are_preserved(self):
        app = Mock()
        app.cpu_percent.return_value = 0.2
        app.wait_exit.return_value = 0
        frames = [[0, 0.1], [1000, 0.2], [1013, 0.3], [1026, 0.4]]
        def write_frames(*args):
            (bp.DATA / "frames-valid.json").write_text(json.dumps({"frames": frames}))
            return True
        app.wait_window.side_effect = write_frames
        with patch.object(perf.bp, "App", return_value=app), patch.object(perf.time, "sleep"):
            result = perf.frame_run(self.binary, [], "valid")
        self.assertEqual(result, {
            "cpu_pct": 0.2, "window_opened": True, "returncode": 0, "frames": 3,
            "interval_p50_ms": 13.0, "interval_p95_ms": 13.0, "dropped_frames": 0,
            "render_p50_ms": 0.3, "render_p95_ms": 0.39, "seconds": 0.03,
        })

    def test_main_dropped_frame_gate_requires_frame_evidence(self):
        values = self.valid_values()
        del values["ttft_ms"]
        values["stream"] = [{"frames": 0, "cpu_pct": 0.2}] * 2
        values["stream_dropped_frames"] = 0
        code, raw, body = self.main_with(values)
        self.assertEqual(code, 1)
        dropped = next(c for c in body["checks"] if c["name"].startswith("dropped frames"))
        self.assertFalse(dropped["ok"])
        self.assertIn("unavailable", dropped["detail"])
        self.assertNotIn("0 dropped", dropped["detail"])
        self.assertEqual(raw["stream"], values["stream"])

    def test_main_valid_measurements_pass_and_no_live_is_a_skip(self):
        values = self.valid_values()
        del values["ttft_ms"]
        code, raw, body = self.main_with(values)
        self.assertEqual(code, 0)
        self.assertEqual((body["passed"], body["failed"], body["skipped"], body["total"]), (10, 0, 1, 11))
        self.assertNotIn("ttft_ms", raw)
        self.assertTrue(body["checks"][-1]["skipped"])

    def test_main_dropped_gate_requires_a_measurement_for_each_run(self):
        values = self.valid_values()
        del values["ttft_ms"]
        del values["stream"][0]["dropped_frames"]
        code, _, body = self.main_with(values)
        self.assertEqual(code, 1)
        dropped = next(c for c in body["checks"] if c["name"].startswith("dropped frames"))
        self.assertFalse(dropped["ok"])
        self.assertIn("unavailable", dropped["detail"])

    def test_main_valid_live_ttft_passes_without_skip(self):
        code, raw, body = self.main_with(self.valid_values(), no_live=False)
        self.assertEqual(code, 0)
        self.assertEqual(raw["ttft_ms"], 0.5)
        self.assertEqual((body["passed"], body["failed"], body["skipped"], body["total"]), (11, 0, 0, 11))

    def test_main_missing_live_ttft_fails_instead_of_skipping(self):
        values = self.valid_values()
        values["ttft_ms"] = None
        code, raw, body = self.main_with(values, no_live=False)
        self.assertEqual(code, 1)
        self.assertIsNone(raw["ttft_ms"])
        self.assertEqual(body["skipped"], 0)
        self.assertEqual(body["failed"], 2)

    def test_main_nonfinite_raw_and_report_are_standard_json(self):
        values = self.valid_values()
        del values["ttft_ms"]
        values["stream"][0]["render_p95_ms"] = float("nan")
        values["idle_cpu_pct"] = -float("inf")
        code, raw, body = self.main_with(values)
        self.assertEqual(code, 1)
        self.assertIsNone(raw["idle_cpu_pct"])
        self.assertIsNone(raw["stream"][0]["render_p95_ms"])
        self.assertEqual(body["failed"], 2)
        self.assertTrue(raw["missing_metrics"])

    def test_historical_before_unknowns_remain_readable(self):
        (bp.DATA / "perf-before.json").write_text('{"numeric": 12.5, "unknown": NaN, "old": "unknown"}')
        values = self.valid_values()
        del values["ttft_ms"]
        code, _, body = self.main_with(values)
        self.assertEqual(code, 0)
        self.assertEqual(body["metrics"]["before"]["numeric"], 12.5)
        self.assertIsNone(body["metrics"]["before"]["unknown"])
        self.assertEqual(body["metrics"]["before"]["old"], "unknown")

    def test_partial_medians_are_missing_not_successful_samples(self):
        for values in ([10.0, None], [10.0, float("nan")], [10.0, float("inf")], []):
            with self.subTest(values=values):
                self.assertIsNone(perf.med(values))
        self.assertEqual(perf.med([10.0, 20.0]), 15.0)

    def test_collect_retains_every_cold_and_settled_sample(self):
        values = self.collect([
            {"cpu_pct": 0.2, "frames": 100, "interval_p95_ms": 13.3,
             "dropped_frames": 0, "render_p95_ms": 0.5},
        ] * 2)
        self.assertEqual(values["cold_start"], [{"window_ms": 10.0, "first_frame_ms": 20.0}] * 7)
        self.assertEqual(values["idle"], [{"rss_mb": 12.0, "cpu_pct": 0.2}] * 3)
        self.assertEqual(values["demo"], [{"rss_mb": 12.0, "cpu_pct": 0.2}] * 2)
        self.assertEqual(values["long_session"], [{"rss_mb": 12.0, "cpu_pct": 0.2}] * 2)

    def test_interleaved_run_labels_do_not_overwrite_other_binary_evidence(self):
        labels = []
        def measure(binary, label):
            labels.append((binary, label))
            return label
        result = perf.interleaved({"before": Path("/before"), "after": Path("/after")}, 2, measure)
        self.assertEqual(labels, [
            (Path("/before"), "before-0"), (Path("/after"), "after-0"),
            (Path("/before"), "before-1"), (Path("/after"), "after-1"),
        ])
        self.assertEqual(result, {"before": ["before-0", "before-1"], "after": ["after-0", "after-1"]})

    def test_cold_timing_path_includes_run_label(self):
        app = Mock(window_ms=10)
        app.wait_window.return_value = True
        with patch.object(perf.bp, "App", return_value=app) as launch, patch.object(perf.time, "sleep"):
            perf.cold_start_once(self.binary, "before-0")
            perf.cold_start_once(self.binary, "after-0")
        paths = [call.kwargs["env"]["FLINT_BP_TIMING"] for call in launch.call_args_list]
        self.assertEqual(paths, [str(bp.DATA / "timing-before-0.json"), str(bp.DATA / "timing-after-0.json")])

    def test_cold_start_closes_process_when_timing_is_malformed(self):
        app = Mock()
        def write_timing(*args):
            for path in (bp.DATA / "timing.json", bp.DATA / "timing-fixture.json"):
                path.write_text("{")
        app.wait_window.side_effect = write_timing
        with patch.object(perf.bp, "App", return_value=app), patch.object(perf.time, "sleep"), \
                self.assertRaises(json.JSONDecodeError):
            perf.cold_start_once(self.binary, "fixture")
        app.stop.assert_called_once()

    def test_settled_closes_process_when_probe_raises(self):
        app = Mock()
        app.rss_mb.side_effect = OSError("probe unavailable")
        with patch.object(perf.bp, "App", return_value=app), patch.object(perf.time, "sleep"), \
                self.assertRaises(OSError):
            perf.settled(self.binary, [], 1, "fixture")
        app.stop.assert_called_once()

    def test_settled_missing_window_invalidates_other_samples(self):
        app = Mock()
        app.wait_window.return_value = False
        app.rss_mb.return_value = 12.0
        app.cpu_percent.return_value = 0.2
        with patch.object(perf.bp, "App", return_value=app), patch.object(perf.time, "sleep"):
            rss, cpu = perf.settled(self.binary, [], 1, "fixture")
        self.assertFalse(perf.finite(rss))
        self.assertFalse(perf.finite(cpu))
        app.stop.assert_called_once()

    def test_frame_run_always_closes_process_log(self):
        app = Mock()
        app.cpu_percent.return_value = 0.2
        app.wait_exit.return_value = 0
        with patch.object(perf.bp, "App", return_value=app), patch.object(perf.time, "sleep"):
            perf.frame_run(self.binary, [], "fixture")
        app.stop.assert_called_once()

    def test_failed_or_timed_out_frame_run_cannot_pass_with_valid_frames(self):
        for returncode in (101, None):
            app = Mock()
            app.cpu_percent.return_value = 0.2
            app.wait_exit.return_value = returncode
            def write_frames(*args):
                (bp.DATA / "frames-failed.json").write_text(json.dumps({
                    "frames": [[0, 0.1], [1000, 0.2], [1013, 0.3], [1026, 0.4]],
                }))
            app.wait_window.side_effect = write_frames
            with self.subTest(returncode=returncode), patch.object(perf.bp, "App", return_value=app), \
                    patch.object(perf.time, "sleep"):
                run = perf.frame_run(self.binary, [], "failed")
            self.assertEqual(run["returncode"], returncode)
            self.assertEqual(run["frames"], 3)
            self.assertIsNone(perf.frame_metric([run], "interval_p95_ms"))
            app.stop.assert_called_once()

    def test_frame_probe_exception_still_closes_process(self):
        app = Mock()
        app.cpu_percent.side_effect = OSError("probe unavailable")
        with patch.object(perf.bp, "App", return_value=app), patch.object(perf.time, "sleep"), \
                self.assertRaises(OSError):
            perf.frame_run(self.binary, [], "fixture")
        app.stop.assert_called_once()

    def test_missing_frame_window_invalidates_valid_frame_data(self):
        run = {"frames": 100, "window_opened": False, "returncode": 0, "interval_p95_ms": 13.3}
        self.assertIsNone(perf.frame_metric([run], "interval_p95_ms"))

    def test_missing_probe_value_fails_report_as_null_not_zero(self):
        values = self.valid_values()
        del values["ttft_ms"]
        values["rss_idle_mb"] = float("nan")
        values["idle_cpu_pct"] = float("nan")
        code, raw, body = self.main_with(values)
        self.assertEqual(code, 1)
        self.assertIsNone(raw["rss_idle_mb"])
        self.assertIsNone(raw["idle_cpu_pct"])
        self.assertEqual(body["failed"], 3)

    def test_partial_cold_sample_fails_collect_and_report(self):
        frames = [{"frames": 100, "window_opened": True, "returncode": 0, "cpu_pct": 0.2, "interval_p95_ms": 13.3,
                   "dropped_frames": 0, "render_p95_ms": 0.5}] * 4
        with patch.object(perf, "cold_start_once", side_effect=[(10.0, 20.0)] * 6 + [(None, None)]), \
                patch.object(perf, "settled", return_value=(12.0, 0.2)), \
                patch.object(perf, "frame_run", side_effect=frames):
            values = perf.collect({"after": self.binary}, False)["after"]
        self.assertIsNone(values["cold_start_window_ms"])
        self.assertIsNone(values["cold_start_first_frame_ms"])
        code, raw, body = self.main_with(values)
        self.assertEqual(code, 1)
        self.assertEqual(raw["cold_start"][-1], {"window_ms": None, "first_frame_ms": None})
        self.assertEqual(body["failed"], 3)

    def test_live_probe_paths_are_distinct_and_processes_are_closed(self):
        for name in ("before", "after"):
            app = Mock()
            app.wait_exit.return_value = 0
            def write_state(*args):
                (bp.DATA / f"perf-{name}-ttft-state.json").write_text(json.dumps({
                    "sessions": [{"active": True, "first_text_ms": 15.0}],
                }))
                return True
            app.wait_window.side_effect = write_state
            with patch.object(perf.bp, "App", return_value=app) as launch:
                self.assertEqual(perf.ttft(self.binary, name), 15.0)
            app.stop.assert_called_once()
            self.assertEqual(launch.call_args.kwargs["log"], bp.LOGS / f"perf-{name}-ttft.log")
            workspace = Path(launch.call_args.args[1][1])
            self.assertFalse(workspace.exists())
        self.assertTrue((bp.DATA / "perf-before-ttft-state.json").exists())
        self.assertTrue((bp.DATA / "perf-after-ttft-state.json").exists())

    def test_live_probe_failure_keeps_cleanup_and_does_not_pass(self):
        for returncode in (101, None):
            app = Mock()
            app.wait_exit.return_value = returncode
            def write_state(*args):
                (bp.DATA / "perf-fixture-ttft-state.json").write_text(json.dumps({
                    "sessions": [{"active": True, "first_text_ms": 15.0}],
                }))
                return True
            app.wait_window.side_effect = write_state
            with self.subTest(returncode=returncode), patch.object(perf.bp, "App", return_value=app) as launch:
                self.assertIsNone(perf.ttft(self.binary, "fixture"))
            app.stop.assert_called_once()
            self.assertFalse(Path(launch.call_args.args[1][1]).exists())

    def test_live_probe_exception_keeps_cleanup(self):
        app = Mock()
        app.wait_window.side_effect = OSError("window probe unavailable")
        with patch.object(perf.bp, "App", return_value=app) as launch, self.assertRaises(OSError):
            perf.ttft(self.binary, "fixture")
        app.stop.assert_called_once()
        self.assertFalse(Path(launch.call_args.args[1][1]).exists())

    def test_missing_frame_process_status_is_not_success(self):
        valid = {"frames": 100, "window_opened": True, "returncode": 0, "interval_p95_ms": 13.3}
        for key in ("window_opened", "returncode"):
            run = dict(valid)
            del run[key]
            with self.subTest(missing=key):
                self.assertIsNone(perf.frame_metric([run], "interval_p95_ms"))

    def test_paired_comparison_refuses_colliding_before_tag(self):
        values = self.valid_values()
        del values["ttft_ms"]
        args = ["perf.py", "--bin", str(self.binary), "--baseline-bin", str(self.binary), "--tag", "before", "--no-live"]
        with patch("sys.argv", args), patch.object(perf, "collect", return_value={"before": values}) as collect, \
                contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit) as exit:
            perf.main()
        self.assertEqual(exit.exception.code, 2)
        collect.assert_not_called()


if __name__ == "__main__":
    unittest.main()
