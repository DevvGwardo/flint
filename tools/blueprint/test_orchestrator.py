"""Offline evaluator regressions. No credential access or external network."""

import contextlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
from pathlib import Path
import sqlite3
import sys
import tempfile
import threading
import time
import unittest
from unittest import mock
import urllib.error
import urllib.request

import orchestrator as evaluation
from orchestrator_cases import CASES, scripted_plan
from orchestrator_proxy import (
    Budget, BudgetError, Proxy, RoutingError, local_url, omniroute_credentials,
    receipt_attestation, response_usage, sse_usage,
)


class BudgetSeedTests(unittest.TestCase):
    def test_resumed_budget_debits_previous_spending(self):
        budget = Budget(5, 100, 60, spent_calls=4, spent_tokens=80)
        budget.reserve(20)
        budget.settle(20, 20)
        self.assertEqual(budget.snapshot()["calls"], 5)
        self.assertEqual(budget.snapshot()["charged_tokens"], 100)
        with self.assertRaises(BudgetError):
            budget.reserve(1)
        with self.assertRaises(ValueError):
            Budget(5, 100, 60, spent_calls=5)


class ResumeTests(unittest.TestCase):
    def report(self, args):
        return {"live": True, "model": args.model, "fixture_sha256": evaluation.fixture_digest(),
                "bridge_sha256": "bridge", "seed": args.seed,
                "python_executable": str(Path(sys.executable).resolve()),
                "budget_limits": {name: getattr(args, name) for name in
                    ("run_calls", "run_tokens", "run_seconds", "reply_tokens")},
                "budget": {"calls": 10, "charged_tokens": 1000, "pending_reserved_tokens": 0,
                           "token_bound_breached": False, "safety_halted": False},
                "checks": [{"name": "Relevant source stayed unchanged", "passed": True}],
                "gateway": {"url": args.gateway, "key_name": args.gateway_key_name},
                "planned_scenarios": len(CASES) * 3 * args.trials, "results": [
                    {"id": "parallel-changes-0-single", "task": "parallel-changes",
                     "trial": 0, "variant": "single", "score": {"passed": False},
                     "proxy": {"requests": [{"kind": "agent", "status": "ok"}]}},
                    {"id": "parallel-changes-1-single", "task": "parallel-changes",
                     "trial": 1, "variant": "single", "proxy": {"requests": []}}]}

    def test_resume_retains_executed_failure_but_retries_no_response(self):
        args = evaluation.parser().parse_args(["--live"])
        report = self.report(args)
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "report.json"
            evaluation.write_json(path, report)
            _, rows = evaluation.resume_results(path, args, "bridge")
            self.assertEqual(len(rows), 1)
            self.assertFalse(rows[0]["score"]["passed"])

    def test_resume_rejects_changed_execution_or_unsafe_evidence(self):
        args = evaluation.parser().parse_args(["--live"])
        for field, value in (("bridge_sha256", "different"), ("model", "other"),
                             ("fixture_sha256", "different")):
            report = self.report(args)
            report[field] = value
            with tempfile.TemporaryDirectory() as directory:
                path = Path(directory) / "report.json"
                evaluation.write_json(path, report)
                with self.assertRaises(ValueError):
                    evaluation.resume_results(path, args, "bridge")
        report = self.report(args)
        report["budget"]["safety_halted"] = True
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "report.json"
            evaluation.write_json(path, report)
            with self.assertRaises(ValueError):
                evaluation.resume_results(path, args, "bridge")


class BudgetTests(unittest.TestCase):
    def test_parallel_requests_share_one_hard_call_limit(self):
        budget = Budget(3, 1000, 60)
        accepted = []
        def attempt():
            try:
                budget.reserve(100)
                accepted.append(True)
            except BudgetError:
                pass
        threads = [threading.Thread(target=attempt) for _ in range(20)]
        for thread in threads:
            thread.start()
        for thread in threads:
            thread.join()
        self.assertEqual(len(accepted), 3)
        self.assertEqual(budget.snapshot()["pending_reserved_tokens"], 300)

    def test_unknown_usage_keeps_full_reservation(self):
        budget = Budget(5, 100, 60)
        budget.reserve(80)
        budget.settle(80, None)
        self.assertEqual(budget.charged, 80)
        with self.assertRaises(BudgetError):
            budget.reserve(21)

    def test_reported_overrun_halts_further_calls(self):
        budget = Budget(5, 1000, 60)
        budget.reserve(80)
        budget.settle(80, 81)
        self.assertTrue(budget.breach)
        with self.assertRaises(BudgetError):
            budget.reserve(1)

    def test_wall_deadline_is_independent_of_received_events(self):
        budget = Budget(10, 1000, -1)
        with self.assertRaises(BudgetError):
            budget.reserve(1)

    def test_usage_does_not_double_count_reasoning_or_cache(self):
        self.assertEqual(response_usage({"usage": {
            "prompt_tokens": 100, "completion_tokens": 20,
            "completion_tokens_details": {"reasoning_tokens": 10},
            "prompt_tokens_details": {"cached_tokens": 80}}}), 120)
        self.assertIsNone(response_usage({"usage": {"prompt_tokens": True, "completion_tokens": 2}}))
        self.assertIsNone(response_usage({}))

    def test_non_object_usage_payloads_remain_unknown(self):
        for payload in (None, [], "keepalive", 1, True):
            with self.subTest(payload=payload):
                self.assertIsNone(response_usage(payload))

    def test_non_object_stream_events_do_not_discard_valid_usage(self):
        body = (b'data: {"usage":{"prompt_tokens":12,"completion_tokens":3}}\n\n'
                b'data: null\n\ndata: []\n\ndata: "keepalive"\n\n'
                b'data: [DONE]\n\n')
        self.assertEqual(sse_usage(body), 15)
        self.assertIsNone(sse_usage(b"data: null\n\ndata: []\n\n"))


class ScoringTests(unittest.TestCase):
    def metrics(self):
        return {"outcome": "needs_review", "current_verification": True,
                "turn_reason": "Completed", "counts": {"SessionStopped": 1},
                "blockers": [], "accepted": False, "commands": []}

    def test_completed_turn_is_not_a_passing_deliverable(self):
        result = evaluation.score(CASES[0], self.metrics(), {"passed": False},
                                  {"exit_code": 0, "timed_out": False})
        self.assertFalse(result["passed"])
        self.assertTrue(result["false_completion"])

    def test_current_verification_and_clean_shutdown_are_required(self):
        for key, value in (("current_verification", False), ("accepted", True),
                           ("outcome", "partial")):
            metrics = self.metrics()
            metrics[key] = value
            self.assertFalse(evaluation.score(CASES[0], metrics, {"passed": True},
                                             {"exit_code": 0, "timed_out": False})["passed"])

    def test_cumulative_parent_usage_is_not_summed_with_children(self):
        events = [{"Usage": {"input_tokens": 20}}, {"SubagentEvent": {
            "call_id": "spawn", "event": {"Usage": {"input_tokens": 10}}}},
            {"Usage": {"input_tokens": 40}}]
        self.assertEqual(evaluation.event_metrics(events)["usage"]["input_tokens"], 40)

    def test_stale_or_failed_evidence_does_not_count(self):
        task = {"work_revision": 2, "contract": {"revision": 1},
                "assessment": {"outcome": "needs_review"},
                "checks": [{"verification": True, "success": True, "exit_code": 0,
                            "contract_revision": 1, "started_revision": 1, "work_revision": 2}]}
        self.assertFalse(evaluation.event_metrics([{"TaskAssessed": {"task": task}}])[
            "current_verification"])
        task["checks"][0]["started_revision"] = 2
        self.assertTrue(evaluation.event_metrics([{"TaskAssessed": {"task": task}}])[
            "current_verification"])

    def test_blocked_dependency_is_not_a_verified_deliverable(self):
        metrics = self.metrics()
        metrics.update(outcome="blocked", current_verification=False,
                       blockers=[{"text": "Missing source.bin."}])
        self.assertTrue(evaluation.score(CASES[3], metrics, {"passed": True},
                                        {"exit_code": 0, "timed_out": False})["passed"])
        metrics["blockers"] = []
        self.assertFalse(evaluation.score(CASES[3], metrics, {"passed": True},
                                         {"exit_code": 0, "timed_out": False})["passed"])

    def test_failed_check_must_precede_the_observed_passing_retry(self):
        metrics = self.metrics()
        metrics["commands"] = [{"command": "python3 check.py", "exit_code": 0}]
        self.assertFalse(evaluation.score(CASES[2], metrics, {"passed": True},
                                         {"exit_code": 0, "timed_out": False})["passed"])
        metrics["commands"].insert(0, {"command": "python3 check.py", "exit_code": 23})
        self.assertTrue(evaluation.score(CASES[2], metrics, {"passed": True},
                                        {"exit_code": 0, "timed_out": False})["passed"])

    def test_analysis_requires_read_only_zero_edits_and_findings_not_verification(self):
        case = next(case for case in CASES if case.analysis)
        metrics = self.metrics()
        metrics.update(mode="read_only", observed_edits=0, current_verification=False,
                       final_text="Recommend trim and uppercase, with edge-case tests.")
        process = {"exit_code": 0, "timed_out": False}
        self.assertTrue(evaluation.score(case, metrics, {"passed": True}, process)["passed"])
        for key, value in (("mode", "work"), ("observed_edits", 1), ("final_text", "Done")):
            changed = dict(metrics, **{key: value})
            self.assertFalse(evaluation.score(case, changed, {"passed": True}, process)["passed"])

    def test_bun_requires_both_current_checks_and_uncredited_masked_failure(self):
        case = next(case for case in CASES if case.name == "bun-mask-recovery")
        metrics = self.metrics()
        metrics.update(current_commands=["bun test", "bun run typecheck"],
                       uncredited_commands=["bun test 2>&1 | tail -5"],
                       commands=[{"scope": "parent", "command": command, "exit_code": code}
                                 for command, code in [("bun test 2>&1 | tail -5", 0),
                                                       ("bun test", 1), ("bun test", 0)]])
        process = {"exit_code": 0, "timed_out": False}
        self.assertTrue(evaluation.score(case, metrics, {"passed": True}, process)["passed"])
        for key, value in (("current_commands", ["bun test"]), ("uncredited_commands", [])):
            self.assertFalse(evaluation.score(case, dict(metrics, **{key: value}),
                                              {"passed": True}, process)["passed"])


@unittest.skipUnless(sys.platform == "darwin", "requires macOS process containment")
class ContainmentAndGraderTests(unittest.TestCase):
    def test_directory_discovery_never_grants_home_contents(self):
        home = Path.home()
        policy = evaluation.profile([], [], directory_paths=[home, home / "private"])
        self.assertNotIn(f'(literal "{home}")', policy)
        self.assertNotIn(f'(literal "{home / "private"}")', policy)

    def test_shell_checks_use_the_same_python_as_the_grader(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            home, tmp = root / "home", root / "tmp"
            home.mkdir()
            tmp.mkdir()
            result = evaluation.execute(
                ["/usr/bin/sandbox-exec", "-p", evaluation.profile([root], [root]),
                 "/bin/sh", "-c", "python3 -c 'import sys; print(sys.executable)'"],
                root, evaluation.clean_environment(home, tmp), "",
                root / "out", root / "err", 5)
            self.assertEqual(result["exit_code"], 0)
            self.assertEqual(Path((root / "out").read_text().strip()).resolve(),
                             Path(sys.executable).resolve())

    def test_read_write_and_network_boundary(self):
        with tempfile.TemporaryDirectory() as temp:
            result = evaluation.containment_check(Path(temp))
        self.assertTrue(result["passed"], result)

    def test_starting_work_fails_and_reference_work_passes(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            for case in CASES:
                if case.runtime == "bun":
                    continue  # Actual Bun/compiler grading has its own optional-runtime test below.
                workspace = root / case.name
                case.seed(workspace)
                before = root / (case.name + "-before")
                before.mkdir()
                self.assertEqual(evaluation.grade(case, workspace, before)["passed"],
                                 case.blocked or case.analysis)
                for name, content in case.solution.items():
                    (workspace / name).write_text(content)
                after = root / (case.name + "-after")
                after.mkdir()
                result = evaluation.grade(case, workspace, after)
                self.assertTrue(result["passed"], (case.name, result))

    @unittest.skipUnless(evaluation.BUN.is_file() and (evaluation.TYPESCRIPT_LIB / "tsc.js").is_file(),
                         "requires Bun and HOOTER_EVAL_TYPESCRIPT_LIB")
    def test_bun_reference_work_and_protected_nonempty_tests(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            case = next(case for case in CASES if case.name == "bun-typescript")
            workspace = root / "workspace"
            case.seed(workspace)
            for index, state in enumerate(("broken", "fixed", "empty", "removed")):
                if state == "fixed":
                    for name, content in case.solution.items():
                        (workspace / name).write_text(content)
                elif state == "empty":
                    (workspace / "labels.test.ts").write_text("")
                elif state == "removed":
                    (workspace / "labels.test.ts").unlink()
                target = root / state
                target.mkdir()
                result = evaluation.grade(case, workspace, target)
                self.assertEqual(result["passed"], state == "fixed", result)
                if state == "fixed":
                    self.assertGreaterEqual(result["tests_run"], 4)

    def test_analysis_grader_rejects_edits_and_new_files(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            case = next(case for case in CASES if case.analysis)
            workspace = root / "workspace"
            case.seed(workspace)
            self.assertTrue(evaluation.grade(case, workspace, root)["passed"])
            (workspace / "new.txt").write_text("unrequested")
            self.assertFalse(evaluation.grade(case, workspace, root)["passed"])
            (workspace / "new.txt").unlink()
            (workspace / "labels.ts").write_text("changed")
            self.assertFalse(evaluation.grade(case, workspace, root)["passed"])

    def test_removed_empty_or_changed_test_suites_fail(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            case = CASES[0]
            workspace = root / "workspace"
            case.seed(workspace)
            for name, content in case.solution.items():
                (workspace / name).write_text(content)
            for index, action in enumerate(("empty", "removed", "changed")):
                test = workspace / "test_pages.py"
                if action == "removed":
                    test.unlink()
                else:
                    test.write_text("" if action == "empty" else "raise SystemExit(0)\n")
                target = root / f"grader-{index}"
                target.mkdir()
                self.assertFalse(evaluation.grade(case, workspace, target)["passed"])

    def test_hung_grader_is_killed_with_a_real_wall_deadline(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            home, tmp = root / "home", root / "tmp"
            home.mkdir()
            tmp.mkdir()
            result = evaluation.execute(
                ["/usr/bin/sandbox-exec", "-p", evaluation.profile([root], [root]),
                 sys.executable, "-I", "-"], root, evaluation.clean_environment(home, tmp),
                "import time\ntime.sleep(60)\n", root / "out", root / "err", .2)
            self.assertTrue(result["timed_out"])
            self.assertLess(result["seconds"], 3)

    def test_python_grader_output_overflow_cannot_pass(self):
        case = CASES[0]
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            workspace = root / "workspace"
            case.seed(workspace)
            directory = root / "grader"
            directory.mkdir()

            def completed_with_overflow(*args):
                args[4].write_text(json.dumps({
                    "grader": "hooter-v1", "tests_run": case.min_tests,
                    "successful": True, "failures": 0, "errors": 0,
                }) + "\n")
                args[5].write_text("")
                return {"exit_code": 0, "timed_out": False,
                        "output_limit_exceeded": True, "seconds": .1}

            with mock.patch.object(evaluation, "execute", completed_with_overflow):
                result = evaluation.grade(case, workspace, directory)
            self.assertTrue(result["protected_files_intact"])
            self.assertTrue(result["process"]["output_limit_exceeded"])
            self.assertFalse(result["passed"])


class GatewayTests(unittest.TestCase):
    def test_streaming_receipt_must_match_provider_model_connection_and_client(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "receipt.sqlite"
            with contextlib.closing(sqlite3.connect(path)) as db:
                db.executescript("""
                create table call_logs(id,correlation_id,provider,model,requested_model,
                                      connection_id,status,api_key_name);
                insert into call_logs values(1,'own-request','antigravity','gemini-fixture',
                  'antigravity/gemini-fixture','pinned',200,'fixture');
                """)
                db.commit()
            before = path.read_bytes()
            receipt = receipt_attestation(path, "own-request", "antigravity/gemini-fixture",
                                           "pinned", "fixture")
            self.assertEqual(receipt["attestation"], "read-only-correlation-receipt")
            self.assertEqual(path.read_bytes(), before)
            self.assertIsNone(receipt_attestation(
                path, "another-request", "antigravity/gemini-fixture", "pinned", "fixture"))
            for field, value in (("provider", "openrouter"), ("connection_id", "other"),
                                 ("requested_model", "antigravity/other"), ("api_key_name", "other")):
                with contextlib.closing(sqlite3.connect(path)) as db:
                    db.execute(f"update call_logs set {field}=?", (value,))
                    db.commit()
                with self.assertRaises(RoutingError):
                    receipt_attestation(path, "own-request", "antigravity/gemini-fixture",
                                        "pinned", "fixture")
                with contextlib.closing(sqlite3.connect(path)) as db:
                    original = {"provider": "antigravity", "connection_id": "pinned",
                                "requested_model": "antigravity/gemini-fixture",
                                "api_key_name": "fixture"}[field]
                    db.execute(f"update call_logs set {field}=?", (original,))
                    db.commit()

    def test_upstream_slow_stream_has_an_absolute_deadline(self):
        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *_):
                pass
            def do_POST(self):
                self.rfile.read(int(self.headers["Content-Length"]))
                self.send_response(200)
                self.send_header("X-OmniRoute-Provider", "antigravity")
                self.send_header("X-OmniRoute-Model", "gemini-fixture")
                self.end_headers()
                try:
                    for _ in range(50):
                        self.wfile.write(b"data: ")
                        self.wfile.flush()
                        time.sleep(.05)
                except (BrokenPipeError, ConnectionResetError):
                    pass
        server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        proxy = Proxy(CASES[0], "single", Budget(5, 100_000, 10), seconds=.2,
                      upstream=f"http://127.0.0.1:{server.server_port}/v1",
                      api_key="not-a-real-key", connection="fixture")
        proxy.model = "antigravity/gemini-fixture"
        started = time.monotonic()
        try:
            with self.assertRaises(TimeoutError):
                proxy.dispatch({"model": proxy.model, "stream": True, "messages": []})
            self.assertLess(time.monotonic() - started, 1)
            self.assertEqual(proxy.global_budget.pending, 0)
        finally:
            proxy.close()
            server.shutdown()
            server.server_close()
            thread.join()

    def test_content_length_response_keeps_usage_and_route_evidence(self):
        body = ('data: {"choices":[{"delta":{"content":"OK"}}]}\n\n'
                'data: {"usage":{"prompt_tokens":12,"completion_tokens":3}}\n\n'
                'data: [DONE]\n\n').encode()
        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *_):
                pass
            def do_POST(self):
                self.rfile.read(int(self.headers["Content-Length"]))
                self.send_response(200)
                self.send_header("X-OmniRoute-Provider", "antigravity")
                self.send_header("X-OmniRoute-Model", "gemini-fixture")
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)
        server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        proxy = Proxy(CASES[0], "single", Budget(5, 100_000, 10),
                      upstream=f"http://127.0.0.1:{server.server_port}/v1",
                      api_key="not-a-real-key", connection="fixture")
        proxy.model = "antigravity/gemini-fixture"
        try:
            # Even an explicitly configured environment proxy cannot receive
            # the gateway Authorization header.
            with mock.patch("urllib.request.getproxies",
                            return_value={"http": "http://unapproved.invalid:9"}):
                output, _ = proxy.dispatch({"model": proxy.model, "stream": True, "messages": []})
            self.assertEqual(output, body)
            self.assertEqual(proxy.global_budget.charged, 15)
            self.assertEqual(proxy.requests[0]["attestation"], "response-headers")
        finally:
            proxy.close()
            server.shutdown()
            server.server_close()
            thread.join()

    def test_remote_urls_credentials_and_non_subscription_models_are_rejected(self):
        for value in ("https://api.openai.com/v1", "http://localhost:20128/v1",
                      "http://user:key@127.0.0.1:20128/v1", "http://127.0.0.1/v1?key=secret"):
            with self.assertRaises(ValueError):
                local_url(value)
        with self.assertRaises(ValueError):
            omniroute_credentials(Path("/unused"), "unused", "openrouter/google/gemini-2.5-flash")

    def test_client_key_use_is_read_only_and_routing_is_pinned(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "gateway.sqlite"
            with contextlib.closing(sqlite3.connect(path)) as db:
                db.executescript("""
                create table api_keys(name,key,is_active,revoked_at,expires_at);
                create table provider_connections(id,provider,is_active,priority);
                create table domain_fallback_chains(model,chain);
                create table model_combo_mappings(pattern,enabled);
                insert into api_keys values('fixture','not-a-real-key',1,null,null);
                insert into provider_connections values('fixture-connection','antigravity',1,1);
                """)
                db.commit()
            before = path.read_bytes()
            self.assertEqual(omniroute_credentials(path, "fixture", "antigravity/gemini-fixture"),
                             ("not-a-real-key", "fixture-connection"))
            self.assertEqual(path.read_bytes(), before)
            with contextlib.closing(sqlite3.connect(path)) as db:
                db.execute("insert into domain_fallback_chains values('x','[]')")
                db.commit()
            with self.assertRaises(ValueError):
                omniroute_credentials(path, "fixture", "antigravity/gemini-fixture")

    def test_proxy_requires_auth_and_rejects_model_substitution(self):
        proxy = Proxy(CASES[0], "single", Budget(10, 100_000, 30))
        try:
            with self.assertRaises(urllib.error.HTTPError) as error:
                urllib.request.urlopen(proxy.base_url + "/models", timeout=2)
            self.assertEqual(error.exception.code, 401)
            error.exception.close()
            with self.assertRaises(ValueError):
                proxy.dispatch({"model": "another-model", "messages": []})
            self.assertEqual(proxy.global_budget.calls, 0)
        finally:
            proxy.close()

    def test_billed_provider_response_halts_all_further_requests(self):
        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *_):
                pass
            def do_POST(self):
                self.rfile.read(int(self.headers["Content-Length"]))
                self.send_response(200)
                self.send_header("X-OmniRoute-Provider", "openrouter")
                self.send_header("X-OmniRoute-Model", "google/gemini-fixture")
                self.end_headers()
                self.wfile.write(b"{}")
        server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        proxy = Proxy(CASES[0], "single", Budget(10, 100_000, 30),
                      upstream=f"http://127.0.0.1:{server.server_port}/v1",
                      api_key="not-a-real-key", connection="fixture")
        proxy.model = "antigravity/gemini-fixture"
        try:
            with self.assertRaises(RoutingError):
                proxy.dispatch({"model": proxy.model, "messages": []})
            self.assertTrue(proxy.global_budget.halted)
            with self.assertRaises(BudgetError):
                proxy.dispatch({"model": proxy.model, "messages": []})
            self.assertEqual(proxy.global_budget.calls, 1)
            self.assertNotIn("not-a-real-key", json.dumps(proxy.snapshot()))
        finally:
            proxy.close()
            server.shutdown()
            server.server_close()
            thread.join()

    def test_scripted_solutions_obey_the_native_read_before_write_gate(self):
        plan = scripted_plan(CASES[0])
        self.assertEqual(plan[0][0][0], "read_file")
        self.assertEqual(plan[1][0][0], "write_file")

    def test_existing_output_or_symlink_is_never_overwritten(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            keep = root / "keep"
            keep.mkdir()
            marker = keep / "marker"
            marker.write_text("user work")
            link = root / "link"
            link.symlink_to(keep)
            for path in (keep, link):
                with self.assertRaises(ValueError):
                    evaluation.fresh_output(path)
            self.assertEqual(marker.read_text(), "user work")


if __name__ == "__main__":
    unittest.main()
