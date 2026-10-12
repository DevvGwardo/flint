#!/usr/bin/env python3
"""Mocked controller tests. Never launch a native or unconfined helper."""
import errno
import io
import json
import os
from pathlib import Path
import signal
import subprocess
import tempfile
import unittest
from unittest import mock

import probe_r0d as r


def denied_output(error=errno.EPERM):
    values = [{"protocol": 1, "event": "main"}]
    for operation, stage in (("read", "open"), ("write", "open"), ("network", "socket")):
        values.append({"event": "probe", "operation": operation, "stage": stage,
                       "allowed": False, "errno": error})
    values.append({"event": "done"})
    return b"".join(json.dumps(value, separators=(",", ":")).encode() + b"\n" for value in values)


class BudgetTests(unittest.TestCase):
    def test_exact_four_profiles(self):
        self.assertEqual(len(r.PROFILE_NAMES), 4)
        self.assertFalse(any("05" in name for name in r.PROFILE_NAMES))
        with self.assertRaises(r.Stop):
            r.template(4)

    def test_eight_launches_count_before_spawn(self):
        budget = r.Budget(lambda: 0)
        for number, (index, repetition) in enumerate(r.REMAINING_SLOTS, 4):
            self.assertEqual(budget.consume(index, repetition), number)
        with self.assertRaises(r.Stop):
            budget.consume(0)
        self.assertEqual(budget.count, 8)
        self.assertEqual(budget.additional, 5)
        self.assertEqual(budget.per_profile, [2, 2, 2, 2])

    def test_per_profile_limit(self):
        budget = r.Budget(lambda: 0)
        with self.assertRaises(r.Stop):
            budget.consume(0)
        budget.consume(1, "B")
        with self.assertRaises(r.Stop):
            budget.consume(1, "B")

    def test_unknown_profile_never_consumes(self):
        budget = r.Budget(lambda: 0)
        for value in (-1, 4, 5):
            with self.assertRaises(r.Stop):
                budget.consume(value)
        self.assertEqual(budget.count, 3)
        self.assertEqual(budget.additional, 0)

    def test_global_deadline_reserves_cleanup(self):
        now = [1000.0]
        budget = r.Budget(lambda: now[0])
        now[0] = 1536
        with self.assertRaises(r.Stop):
            budget.consume(1, "B")
        self.assertEqual(budget.count, 3)
        self.assertEqual(budget.deadline, 1570)

    def test_lock_wait_excluded_from_new_budget(self):
        budget = r.Budget(lambda: 50000)
        self.assertEqual(budget.remaining(), 570)

    def test_replayed_and_out_of_order_slots_rejected(self):
        for index, repetition in ((0, "A"), (0, "B"), (1, "A"), (2, "A"), (3, "B")):
            budget = r.Budget(lambda: 0)
            with self.assertRaises(r.Stop):
                budget.consume(index, repetition)
            self.assertEqual((budget.count, budget.additional), (3, 0))

    def test_prior_counters_cannot_be_reset(self):
        for counts in ([0, 0, 0, 0], [2, 0, 0, 0], [2, 2, 0, 0]):
            with self.assertRaises(r.Stop):
                r.Budget(lambda: 0, counts)


class ProfileTests(unittest.TestCase):
    helper = r.ROOT / "renderer-evidence/r0d-123/DyldStartupProbe"
    scratch = r.ROOT / "renderer-evidence/r0d-123/scratch/D3-A"

    def test_templates_match_owned_files(self):
        # Tests run against the retained copies beside this test module.
        for index, name in enumerate(r.PROFILE_NAMES):
            self.assertEqual((Path(__file__).parent / name).read_text(), r.template(index))

    def test_version_delta_only(self):
        self.assertEqual(r.template(0).replace("(version 1)", "(version 3)"), r.template(1))

    def test_exact_approved_roots(self):
        self.assertEqual(r.LIBRARIES, (
            "/usr/lib", "/System/Library/dyld",
            "/System/Volumes/Preboot/Cryptexes/OS/System/Library/dyld",
            "/System/Volumes/Preboot/Cryptexes/OS/usr/lib",
            "/System/Cryptexes/OS/System/Library/dyld", "/System/Cryptexes/OS/usr/lib"))

    def test_ancestor_expansion_is_literals(self):
        policy, paths = r.profile(2, r.template(2), self.helper, self.scratch)
        self.assertNotIn("@@", policy)
        self.assertIn('(literal "/")', policy)
        self.assertIn(str(self.helper.parent), paths)
        self.assertEqual(len(paths), len(set(paths)))
        self.assertNotIn("path-ancestors", policy)
        self.assertNotIn("mach-", policy)

    def test_strict_template_rejection(self):
        changes = [
            '(import "system.sb")\n', "(allow default)\n",
            '(allow file-read-data (literal "/"))\n',
            '(allow file-read* (subpath "/Volumes"))\n',
            "(allow file-read-metadata)\n",
            '(allow mach-lookup (global-name "com.apple.dyld"))\n',
            "(allow network-outbound)\n", "(allow syscall*)\n",
        ]
        for delta in changes:
            with self.subTest(delta=delta), self.assertRaises(r.Stop):
                r.profile(3, r.template(3) + delta, self.helper, self.scratch)

    def test_malicious_parameters(self):
        for path in (Path("/tmp/helper"), self.helper.parent / 'bad"name',
                     self.helper.parent / ".." / "elsewhere"):
            with self.subTest(path=path), self.assertRaises(r.Stop):
                r.profile(0, r.template(0), path, self.scratch)

    def test_mismatched_run(self):
        other = r.ROOT / "renderer-evidence/r0d-124/scratch/D1-A"
        with self.assertRaises(r.Stop):
            r.profile(0, r.template(0), self.helper, other)

    def test_only_retained_binary_cross_run_exception(self):
        other = r.ROOT / "renderer-evidence/r0d-124/scratch/D3-A"
        policy, literals = r.profile(2, r.template(2), r.PRIOR_HELPER, other)
        self.assertIn(str(r.PRIOR_HELPER.parent), literals)
        self.assertIn(str(other.parent), literals)
        self.assertNotIn("file-read-data", policy)
        for wrong in (r.PRIOR_HELPER.with_name("OtherProbe"), self.helper):
            with self.assertRaises(r.Stop):
                r.profile(2, r.template(2), wrong, other)

    def test_operation_bundle_exact(self):
        self.assertIn("SYS_getfsstat SYS_getfsstat64 SYS___mac_syscall SYS_map_with_linking_np", r.OPERATIONS)
        self.assertIn('mac-policy-name "Sandbox"', r.OPERATIONS)
        self.assertIn("mac-syscall-number 2", r.OPERATIONS)
        self.assertNotIn("mach-lookup", r.OPERATIONS)
        self.assertNotIn("syscall*", r.OPERATIONS)


class IsolationTests(unittest.TestCase):
    def test_environment_no_ambient_inheritance(self):
        with mock.patch.dict(os.environ, {"DUMMY_PROVIDER_TOKEN": "synthetic", "DYLD_INSERT_LIBRARIES": "dummy",
                                        "SSH_AUTH_SOCK": "dummy", "HOME": "/not-scratch"}, clear=True):
            value = r.environment(Path("/private/test-scratch"))
        self.assertEqual(set(value), {"HOME", "TMPDIR", "PATH", "LANG", "LC_ALL"})
        self.assertEqual(value["HOME"], "/private/test-scratch")
        self.assertEqual(value["TMPDIR"], "/private/test-scratch/")

    def test_spawn_cwd_pipes_descriptors_session(self):
        fake = mock.Mock()
        r.spawn(["sandbox-exec", "synthetic"], Path("/private/test-scratch"), popen=fake)
        args, kwargs = fake.call_args
        self.assertEqual(kwargs["cwd"], Path("/private/test-scratch"))
        self.assertEqual(kwargs["stdin"], subprocess.PIPE)
        self.assertEqual(kwargs["stdout"], subprocess.PIPE)
        self.assertEqual(kwargs["stderr"], subprocess.PIPE)
        self.assertTrue(kwargs["close_fds"])
        self.assertTrue(kwargs["start_new_session"])
        self.assertNotIn("pass_fds", kwargs)
        self.assertEqual(set(kwargs["env"]), {"HOME", "TMPDIR", "PATH", "LANG", "LC_ALL"})

    def test_sentinel_provenance_and_mutation(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "sentinel"
            path.write_bytes(r.SENTINEL_BYTES)
            path.chmod(0o600)
            first = r.preflight(path)
            self.assertEqual(first["mode"], 0o600)
            path.write_bytes(b"changed")
            with self.assertRaises(r.Stop):
                r.preflight(path)

    def test_symlink_fixture_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / "target"
            target.write_bytes(r.SENTINEL_BYTES)
            target.chmod(0o600)
            link = Path(directory) / "link"
            link.symlink_to(target)
            with self.assertRaises(r.Stop):
                r.preflight(link)

    def test_missing_fixture_rejected(self):
        with tempfile.TemporaryDirectory() as directory, self.assertRaises(FileNotFoundError):
            r.preflight(Path(directory) / "missing")


class ClassificationTests(unittest.TestCase):
    def test_executed_denials_only(self):
        for error in (errno.EPERM, errno.EACCES):
            self.assertEqual(r.classify(denied_output(error), b"", 0, 0, True), "native_denials")

    def test_no_main_zero_canary_inconclusive(self):
        self.assertEqual(r.classify(b"", b"", -6, 0, True), "startup_abort")
        self.assertEqual(r.classify(b"", b"", 0, 0, True), "missing_main")

    def test_main_marker_alone_not_success(self):
        self.assertEqual(r.classify(r.MAIN_RECORD, b"", 0, 0, True), "malformed_output")

    def test_misleading_errnos(self):
        for error in (errno.ENOENT, errno.EINVAL, errno.ECONNREFUSED, errno.EINPROGRESS):
            self.assertEqual(r.classify(denied_output(error), b"", 0, 0, True), "misleading_errno")

    def test_canary_hit_stops(self):
        self.assertEqual(r.classify(denied_output(), b"", 0, 1, True), "canary_hit")

    def test_fixture_failure_stops(self):
        self.assertEqual(r.classify(denied_output(), b"", 0, 0, False), "fixture_unverified")

    def test_parse_error_stops(self):
        for diagnostic in (b"sandbox-exec: syntax error", b"sandbox-exec: sandbox_compile: invalid\n",
                           b"sandbox-exec: failed to load profile: synthetic\n",
                           b"sandbox-exec: failed to read profile: synthetic\n"):
            self.assertEqual(r.classify(b"", diagnostic, 1, 0, True), "profile_syntax_load_error")

    def launcher_error(self, detail=b"Operation not permitted"):
        return b"sandbox-exec: execvp() of '" + str(r.PRIOR_HELPER).encode() + b"' failed: " + detail + b"\n"

    def test_exact_launcher_permission_errors_not_native_denials(self):
        for detail in (b"Operation not permitted", b"Permission denied"):
            self.assertEqual(r.classify(b"", self.launcher_error(detail), 71, 0, True, r.PRIOR_HELPER),
                             "launcher_exec_permission")

    def test_launcher_requires_exact_binary_and_exit(self):
        err = self.launcher_error()
        for helper, code in ((None, 71), (Path("/tmp/not-the-helper"), 71), (r.PRIOR_HELPER, 1),
                             (r.PRIOR_HELPER, 0), (r.PRIOR_HELPER, -6)):
            self.assertEqual(r.classify(b"", err, code, 0, True, helper), "unknown_diagnostic")

    def test_launcher_output_conflict_stops(self):
        for raw in (r.MAIN_RECORD, denied_output(), b"garbage"):
            self.assertEqual(r.classify(raw, self.launcher_error(), 71, 0, True, r.PRIOR_HELPER),
                             "unknown_diagnostic")

    def test_unknown_diagnostics_and_prefix_suffix_stop(self):
        err = self.launcher_error()
        diagnostics = (err[:-1], err + b"extra\n", b"extra\n" + err,
                       self.launcher_error(b"No such file or directory"),
                       b"sandbox-exec: unknown failure\n", b"dyld: synthetic\n")
        for value in diagnostics:
            self.assertEqual(r.classify(b"", value, 71, 0, True, r.PRIOR_HELPER), "unknown_diagnostic")

    def test_launcher_never_overrides_canary_or_fixture_failure(self):
        self.assertEqual(r.classify(b"", self.launcher_error(), 71, 1, True, r.PRIOR_HELPER), "canary_hit")
        self.assertEqual(r.classify(b"", self.launcher_error(), 71, 0, False, r.PRIOR_HELPER), "fixture_unverified")

    def test_valid_native_output_with_unknown_stderr_stops(self):
        self.assertEqual(r.classify(denied_output(), b"unexpected diagnostic\n", 0, 0, True), "unknown_diagnostic")

    def test_truncated_and_extra_output(self):
        for raw in (denied_output()[:-1], b"garbage", denied_output() + b"{}\n"):
            self.assertEqual(r.classify(raw, b"", 0, 0, True), "malformed_output")

    def test_permitted_access_stops(self):
        raw = denied_output().replace(b'"allowed":false', b'"allowed":true', 1)
        self.assertEqual(r.classify(raw, b"", 0, 0, True), "permitted_access")

    def test_invalid_schema(self):
        raw = denied_output().replace(b'"errno":1', b'"errno":true', 1)
        self.assertEqual(r.classify(raw, b"", 0, 0, True), "malformed_output")


class PriorEvidenceTests(unittest.TestCase):
    def prior_summary(self):
        return {"cases": [{"profile": profile, "repetition": rep, "launchNumber": number,
                           "process": {"pid": pid, "exit": code}}
                          for profile, rep, number, pid, code in
                          ((1, "A", 1, 160, -6), (1, "B", 2, 161, -6), (2, "A", 3, 162, 71))],
                "launchCount": 3, "perProfileCount": [2, 1, 0, 0],
                "guardsUnchanged": True, "sourcesUnchanged": True}

    def test_exact_prior_identities(self):
        r.check_prior_summary(self.prior_summary())

    def test_conflicting_missing_replayed_prior_identities_stop(self):
        for key, value in (("launchCount", 0), ("perProfileCount", [0, 0, 0, 0]),
                           ("guardsUnchanged", False), ("cases", [])):
            summary = self.prior_summary()
            summary[key] = value
            with self.assertRaises(r.Stop):
                r.check_prior_summary(summary)
        summary = self.prior_summary()
        summary["cases"][-1]["repetition"] = "B"
        with self.assertRaises(r.Stop):
            r.check_prior_summary(summary)

    def test_prior_index_hash_drift_stops_before_reading(self):
        with mock.patch.object(r, "sha", return_value="wrong"), self.assertRaises(r.Stop):
            r.validate_prior()

    def test_existing_continuation_receipt_prevents_replay(self):
        prior_receipt = r.ROOT / "renderer-evidence/r0d-123/continuation-owner.json"
        with mock.patch.object(Path, "glob", return_value=[prior_receipt]), \
             mock.patch.object(Path, "read_text", return_value=json.dumps({"priorEvidence": str(r.PRIOR)})), \
             self.assertRaises(r.Stop):
            r.check_no_continuation_replay(r.ROOT / "renderer-evidence/r0d-456")


class CleanupTests(unittest.TestCase):
    def test_reaped_child_never_signalled(self):
        process = mock.Mock(pid=100, returncode=-6)
        process.poll.return_value = -6
        killer = mock.Mock()
        r.cleanup(process, [], killpg=killer)
        killer.assert_not_called()

    def test_unknown_group_never_signalled(self):
        process = mock.Mock(pid=100)
        process.poll.return_value = None
        killer = mock.Mock()
        with self.assertRaises(r.Stop):
            r.cleanup(process, [], getpgid=lambda _: 99, killpg=killer)
        killer.assert_not_called()

    def test_owned_child_sigterm(self):
        process = mock.Mock(pid=100)
        process.poll.return_value = None
        killer = mock.Mock()
        events = []
        r.cleanup(process, events, getpgid=lambda _: 100, killpg=killer)
        killer.assert_called_once_with(100, signal.SIGTERM)
        process.wait.assert_called_once_with(timeout=0.25)

    def test_owned_kill_after_timeout(self):
        process = mock.Mock(pid=100)
        process.poll.return_value = None
        process.wait.side_effect = [subprocess.TimeoutExpired("synthetic", 0.25), 0]
        killer = mock.Mock()
        r.cleanup(process, [], getpgid=lambda _: 100, killpg=killer)
        self.assertEqual(killer.call_args_list, [mock.call(100, signal.SIGTERM), mock.call(100, signal.SIGKILL)])

    def test_reused_identity_blocks_kill(self):
        process = mock.Mock(pid=100)
        process.poll.return_value = None
        process.wait.side_effect = subprocess.TimeoutExpired("synthetic", 0.25)
        groups = iter((100, 99))
        killer = mock.Mock()
        with self.assertRaises(r.Stop):
            r.cleanup(process, [], getpgid=lambda _: next(groups), killpg=killer)
        killer.assert_called_once_with(100, signal.SIGTERM)

    def test_failed_reap_is_not_success(self):
        process = mock.Mock(pid=100)
        process.poll.return_value = None
        process.wait.side_effect = subprocess.TimeoutExpired("synthetic", 0.25)
        with self.assertRaises(subprocess.TimeoutExpired):
            r.cleanup(process, [], getpgid=lambda _: 100, killpg=mock.Mock())


class ExecutionTests(unittest.TestCase):
    def fake_process(self, output=b"", stderr=b""):
        process = mock.Mock(pid=100, returncode=-6)
        process.stdin = io.BytesIO()
        process.stdout = io.BytesIO(output)
        process.stderr = io.BytesIO(stderr)
        process.poll.return_value = -6
        process.wait.return_value = -6
        return process

    def test_retains_aborted_case_and_cleanup(self):
        with tempfile.TemporaryDirectory() as directory:
            folder = Path(directory)
            process = self.fake_process()
            with mock.patch.object(r, "spawn", return_value=process), mock.patch.object(r.os, "getpgid", return_value=100):
                result = r.execute(folder, ["synthetic"], folder, r.Budget(), 25, 4096, 16384)
            self.assertEqual(result["exit"], -6)
            self.assertTrue(result["pipeCleanup"])
            self.assertEqual(result["cleanupInclusiveLimitSeconds"], 27)
            self.assertTrue((folder / "process-result.json").is_file())

    def test_retains_broken_pipe_failure(self):
        with tempfile.TemporaryDirectory() as directory:
            folder = Path(directory)
            process = self.fake_process()
            process.stdin = mock.Mock()
            process.stdin.close.side_effect = BrokenPipeError()
            with mock.patch.object(r, "spawn", return_value=process), mock.patch.object(r.os, "getpgid", return_value=100):
                with self.assertRaises(r.Stop):
                    r.execute(folder, ["synthetic"], folder, r.Budget(), 25, 4096, 16384)
            value = json.loads((folder / "process-result.json").read_text())
            self.assertEqual(value["errorCode"], "BrokenPipeError")
            self.assertEqual(value["exit"], -6)

    def test_output_flood_is_retained_and_stops(self):
        with tempfile.TemporaryDirectory() as directory:
            folder = Path(directory)
            process = self.fake_process(output=b"abcdefgh")
            with mock.patch.object(r, "spawn", return_value=process), mock.patch.object(r.os, "getpgid", return_value=100):
                with self.assertRaises(r.Stop):
                    r.execute(folder, ["synthetic"], folder, r.Budget(), 25, 4, 4)
            self.assertEqual((folder / "stdout.raw").read_bytes(), b"abcd")
            self.assertTrue(json.loads((folder / "process-result.json").read_text())["outputOverflow"])

    def test_expired_budget_never_spawns(self):
        with tempfile.TemporaryDirectory() as directory:
            folder = Path(directory)
            budget = r.Budget()
            budget.deadline = 0
            with mock.patch.object(r, "spawn") as spawned, self.assertRaises(r.Stop):
                r.execute(folder, ["synthetic"], folder, budget, 25, 4096, 16384)
            spawned.assert_not_called()
            self.assertTrue((folder / "process-result.json").is_file())


if __name__ == "__main__":
    unittest.main(verbosity=2)
