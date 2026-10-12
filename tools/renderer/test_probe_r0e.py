#!/usr/bin/env python3
"""Future authorized mocked tests only. No real helper/control/network spawn."""
import copy
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

import probe_r0e as r


HEALTHY = {"healthy": True, "closed": True, "hits": 0}


def records(stage="socket", error=errno.EPERM):
    values = [{"protocol": 1, "event": "main"}]
    for operation in ("read", "write", "network"):
        values.append({"event": "probe", "operation": operation,
                       "stage": stage if operation == "network" else "open",
                       "allowed": False, "errno": error})
    values.append({"event": "done"})
    return b"".join(r.canonical(value).encode() + b"\n" for value in values).replace(
        b'{"event":"main","protocol":1}\n', r.MAIN_RECORD, 1)


def seal_object():
    return {"schemaVersion": 1, "experiment": "R0E", "authorizationId": "R0E-mocked",
            "executionAuthorized": True,
            "sourceHashes": {str(p): r.PROFILE_SHA if p == r.SOURCES[2] else "a" * 64 for p in r.SOURCES},
            "helperIdentity": {"path": str(r.HELPER), "sha256": r.HELPER_SHA,
                               "uid": os.getuid(), "device": 1, "inode": 2, "mode": 0o755, "bytes": 3},
            "preservationHashes": {"/synthetic/preserved": "b" * 64},
            "policy": copy.deepcopy(r.POLICY)}


class SealTests(unittest.TestCase):
    def test_only_exact_cli(self):
        self.assertEqual(r.parse_cli(["--seal-sha256", "a" * 64]), "a" * 64)

    def test_missing_invalid_or_alternative_cli(self):
        for argv in ([], ["--seal-sha256", "x"], ["--seal-sha256", "A" * 64],
                     ["--seal-sha256", "a" * 64, "--seal", "/tmp/other"],
                     ["--seal-sha256", "a" * 64, "--profile", "other"],
                     ["--seal-sha256", "a" * 64, "--seal-sha256", "b" * 64],
                     ["--seal-sha", "a" * 64], ["--help"]):
            with mock.patch("sys.stderr", io.StringIO()), self.assertRaises(SystemExit):
                r.parse_cli(argv)

    def test_exact_injected_schema(self):
        seal = seal_object()
        self.assertEqual(r.validate_seal_object(seal, seal["preservationHashes"]), seal)

    def test_no_false_boolean_or_numeric_authorization(self):
        for key, value in (("executionAuthorized", False), ("executionAuthorized", 1),
                           ("schemaVersion", True), ("experiment", "R0D"),
                           ("authorizationId", "R0E-../other")):
            seal = seal_object()
            seal[key] = value
            with self.assertRaises(r.Stop):
                r.validate_seal_object(seal, seal["preservationHashes"])

    def test_extra_fields_or_unsealed_sources(self):
        seal = seal_object()
        seal["alternativeProfile"] = "/tmp/profile"
        with self.assertRaises(r.Stop):
            r.validate_seal_object(seal, seal["preservationHashes"])
        seal = seal_object()
        seal["sourceHashes"][str(r.SOURCES[2])] = "a" * 64
        with self.assertRaises(r.Stop):
            r.validate_seal_object(seal, seal["preservationHashes"])

    def test_limits_grants_stage_and_counter_changes_rejected(self):
        for key, value in (("maximumR0ELaunches", 3), ("maximumAfterLockSeconds", 301),
                           ("maximumCaseSeconds", 31), ("firstFailureStops", False),
                           ("nonblockingStageStops", False), ("rendererCapabilityAuthorized", True),
                           ("historicalCounts", {"R0": 0, "R0D": 0})):
            seal = seal_object()
            seal["policy"][key] = value
            with self.assertRaises(r.Stop):
                r.validate_seal_object(seal, seal["preservationHashes"])

    def test_preservation_closure_not_arbitrary_user_files(self):
        seal = seal_object()
        with self.assertRaises(r.Stop):
            r.validate_seal_object(seal, {"/another/path": "b" * 64})

    def test_wrong_binary_identity_path_or_digest(self):
        for key, value in (("path", "/tmp/helper"), ("sha256", "a" * 64),
                           ("mode", 0o777), ("inode", 0), ("uid", os.getuid() + 1)):
            seal = seal_object()
            seal["helperIdentity"][key] = value
            with self.assertRaises(r.Stop):
                r.validate_seal_object(seal, seal["preservationHashes"])

    def test_hash_failure_before_proof_or_spawn(self):
        with mock.patch.object(r, "sha", return_value="b" * 64), \
             mock.patch.object(r, "preservation_expectations") as proof, \
             mock.patch.object(r, "spawn") as spawn, self.assertRaises(r.Stop):
            r.load_seal("a" * 64)
        proof.assert_not_called()
        spawn.assert_not_called()

    def test_cli_failure_before_seal_io(self):
        with mock.patch.object(r, "load_seal") as load, mock.patch("sys.stderr", io.StringIO()), \
             self.assertRaises(SystemExit):
            r.main([])
        load.assert_not_called()

    def test_duplicate_keys_and_nan_rejected(self):
        for raw in (b'{"a":1,"a":2}', b'{"a":NaN}', b'\xff'):
            with self.assertRaises(r.Stop):
                r.strict_json(raw)


class ProfileTests(unittest.TestCase):
    def test_exact_byte_delta(self):
        # Reading source bytes is not a parser/profile launch.
        baseline = r.BASELINE.read_bytes()
        candidate = (Path(__file__).parent / "diagnostic-root-01.sb").read_bytes()
        r.check_profile(candidate, baseline)
        self.assertEqual(len(candidate), 1014)
        self.assertEqual(candidate, baseline + r.ROOT_RULE)

    def test_all_extra_grants_and_version_changes_rejected(self):
        baseline = r.BASELINE.read_bytes()
        good = baseline + r.ROOT_RULE
        bad = (good + b"(allow default)\n", good.replace(b"(version 1)", b"(version 3)"),
               good.replace(b'(literal "/")', b'(subpath "/")'),
               good.replace(b"(vnode-type DIRECTORY)", b"(vnode-type REGULAR-FILE)"),
               good + b'(import "dyld-support.sb")\n', good + b"(allow network-outbound)\n",
               good + b"(allow file-read-metadata)\n",
               good + b'(allow mach-lookup (global-name "synthetic"))\n')
        for candidate in bad:
            with self.assertRaises(r.Stop):
                r.check_profile(candidate, baseline)

    def test_changed_baseline_rejected(self):
        with self.assertRaises(r.Stop):
            r.check_profile(b"other", b"other")


class BudgetTests(unittest.TestCase):
    def test_exact_two_slots_separate_cumulative_arguments(self):
        budget = r.Budget(lambda: 1000)
        store = mock.Mock()
        first = budget.consume("E-A", store)
        second = budget.consume("E-B", store, eligible_previous=True)
        self.assertEqual((first["helperArgument"], second["helperArgument"]), (1, 2))
        self.assertEqual((first["cumulativeConsumed"], second["cumulativeConsumed"]), (43, 44))
        self.assertEqual(budget.deadline, 1300)
        with self.assertRaises(r.Stop):
            budget.consume("E-A", store, True)

    def test_replay_out_of_order_and_ineligible_second(self):
        budget = r.Budget(lambda: 0)
        with self.assertRaises(r.Stop):
            budget.consume("E-B", mock.Mock(), True)
        budget.consume("E-A", mock.Mock())
        with self.assertRaises(r.Stop):
            budget.consume("E-A", mock.Mock(), True)
        with self.assertRaises(r.Stop):
            budget.consume("E-B", mock.Mock(), False)
        self.assertEqual(budget.consumed, 1)

    def test_counter_reserved_even_when_persistence_fails(self):
        budget = r.Budget(lambda: 0)
        store = mock.Mock()
        store.json.side_effect = OSError("synthetic")
        with self.assertRaises(OSError):
            budget.consume("E-A", store)
        self.assertEqual(budget.consumed, 1)

    def test_deadline_reserves_case_and_finalization(self):
        now = [1000]
        budget = r.Budget(lambda: now[0])
        now[0] = 1250
        with self.assertRaises(r.Stop):
            budget.consume("E-A", mock.Mock())
        self.assertEqual(budget.consumed, 0)

    def test_existing_root_blocks_changed_authorization(self):
        with mock.patch.object(Path, "glob", return_value=[Path("/synthetic/r0d-root-e-1")]), \
             self.assertRaises(r.Stop):
            r.reject_existing_run()


class PriorProofTests(unittest.TestCase):
    def prior(self):
        old = {"cases": [{"profile": profile, "repetition": rep, "launchNumber": number,
                          "process": {"pid": pid, "exit": code}}
                         for profile, rep, number, pid, code in
                         ((1, "A", 1, 160, -6), (1, "B", 2, 161, -6), (2, "A", 3, 162, 71))],
               "launchCount": 3, "perProfileCount": [2, 1, 0, 0], "guardsUnchanged": True}
        current = {"cases": [{"slot": slot, "launchNumber": number, "process": {"pid": pid, "exit": 71}}
                             for slot, number, pid in
                             (("D2-B", 4, 10427), ("D3-A", 5, 10428), ("D3-B", 6, 10429),
                              ("D4-A", 7, 10430), ("D4-B", 8, 10431))],
                   "additionalLaunchCount": 5, "additionalPerProfileCount": [0, 1, 2, 2],
                   "cumulativeLaunchCount": 8, "cumulativePerProfileCount": [2, 2, 2, 2],
                   "guardsUnchanged": True}
        return old, current

    def test_exact_prior_identities_and_counts(self):
        r.check_prior_counts(*self.prior())

    def test_missing_reset_replayed_identity_or_guard_failure_stops(self):
        for key, value in (("cumulativeLaunchCount", 0), ("cases", []), ("guardsUnchanged", False)):
            old, current = self.prior()
            current[key] = value
            with self.assertRaises(r.Stop):
                r.check_prior_counts(old, current)
        old, current = self.prior()
        old["cases"][-1]["repetition"] = "B"
        with self.assertRaises(r.Stop):
            r.check_prior_counts(old, current)

    def test_index_identity_failure_before_payload_read(self):
        with mock.patch.object(r, "sha", return_value="wrong"), \
             mock.patch.object(r, "read_json") as read, self.assertRaises(r.Stop):
            r.indexed_proofs(Path("/synthetic/owned"), "a" * 64)
        read.assert_not_called()


class ClassificationTests(unittest.TestCase):
    def classify(self, raw, stderr=b"", code=0, fixtures=True, canary=None):
        return r.classify(raw, stderr, code, fixtures, HEALTHY if canary is None else canary)

    def test_complete_socket_and_connect_stages(self):
        for stage in ("socket", "connect"):
            for error in (errno.EPERM, errno.EACCES):
                self.assertEqual(self.classify(records(stage, error)),
                                 "complete_native_open_and_" + stage + "_denials")

    def test_nonblocking_stops_not_connect_denial(self):
        self.assertEqual(self.classify(records("nonblocking")), "nonblocking_stage_stop")

    def test_both_cases_require_protocol_one(self):
        self.assertTrue(records().startswith(r.MAIN_RECORD))
        self.assertEqual(self.classify(records().replace(b'"protocol":1', b'"protocol":2')),
                         "malformed_output_stop")

    def test_launcher_permissions_stop_and_are_not_native(self):
        prefix = b"sandbox-exec: execvp() of '" + str(r.HELPER).encode() + b"' failed: "
        for detail in (b"Operation not permitted\n", b"Permission denied\n"):
            self.assertEqual(self.classify(b"", prefix + detail, 71), "launcher_exec_permission_stop")
            self.assertEqual(self.classify(records(), prefix + detail, 71), "unknown_diagnostic_stop")

    def test_launcher_wrong_path_status_extra_line_or_errno_unknown(self):
        prefix = b"sandbox-exec: execvp() of '" + str(r.HELPER).encode() + b"' failed: "
        known = prefix + b"Operation not permitted\n"
        for value, code in ((known, 0), (known + b"extra\n", 71),
                            (known.replace(str(r.HELPER).encode(), b"/other"), 71),
                            (prefix + b"No such file or directory\n", 71)):
            self.assertEqual(self.classify(b"", value, code), "unknown_diagnostic_stop")

    def test_actual_profile_errors_and_unknown_diagnostics_stop(self):
        self.assertEqual(self.classify(b"", b"sandbox-exec: syntax error\n", 1),
                         "profile_syntax_load_stop")
        self.assertEqual(self.classify(b"", b"sandbox-exec: failed to load profile\n", 1),
                         "profile_syntax_load_stop")
        self.assertEqual(self.classify(records(), b"unknown\n"), "unknown_diagnostic_stop")

    def test_missing_main_sigabrt_and_complete_nonzero_exit_stop(self):
        self.assertEqual(self.classify(b"", code=-6), "startup_abort_stop")
        self.assertEqual(self.classify(b""), "missing_main_stop")
        self.assertEqual(self.classify(records(), code=64), "incomplete_process_stop")

    def test_malformed_truncated_extra_scalar_and_duplicate_records(self):
        raw = records()
        lines = raw.splitlines(keepends=True)
        cases = (raw[:-1], raw + b"{}\n", r.MAIN_RECORD, b"garbage\n",
                 b"".join(lines[:1] + [b"null\n"] + lines[2:]),
                 raw.replace(b'"errno":1', b'"errno":1,"errno":13', 1),
                 raw.replace(b'"errno":1', b'"errno":true', 1))
        for output in cases:
            self.assertEqual(self.classify(output), "malformed_output_stop")

    def test_allowed_access_and_nonpermission_errno_stop(self):
        self.assertEqual(self.classify(records().replace(b'"allowed":false', b'"allowed":true', 1)),
                         "unexpected_access_stop")
        for error in (errno.ENOENT, errno.EINVAL, errno.EINPROGRESS, errno.ECONNREFUSED):
            self.assertEqual(self.classify(records(error=error)), "nonpermission_errno_stop")

    def test_invalid_stage_and_extra_schema_stop(self):
        self.assertEqual(self.classify(records("invented")), "invalid_syscall_stage_stop")
        self.assertEqual(self.classify(records().replace(b'"event":"probe"', b'"extra":0,"event":"probe"', 1)),
                         "malformed_output_stop")

    def test_canary_health_hit_and_fixture_failures_take_precedence(self):
        self.assertEqual(self.classify(records(), fixtures=False), "fixture_failure")
        self.assertEqual(self.classify(records(), canary={"healthy": True, "closed": True, "hits": 1}),
                         "canary_hit")
        self.assertEqual(self.classify(records(), canary={"healthy": False, "closed": True, "hits": 0}),
                         "canary_health_or_cleanup")

    def test_eligibility_not_classification_alone(self):
        case = {"classification": "complete_native_open_and_socket_denials",
                "process": {"exit": 0, "pipeCleanup": True, "pipeFailure": False, "outputOverflow": False},
                "fixturesUnchanged": True, "caseDeadlineExceeded": False, "canary": dict(HEALTHY)}
        self.assertTrue(r.eligible(case))
        for key, value in (("classification", "nonblocking_stage_stop"), ("fixturesUnchanged", False),
                           ("caseDeadlineExceeded", True), ("errorCode", "synthetic")):
            changed = copy.deepcopy(case)
            changed[key] = value
            self.assertFalse(r.eligible(changed))
        case["process"]["timedOut"] = True
        self.assertFalse(r.eligible(case))


class OwnedTemporary(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        base = Path(self.temporary.name).resolve()
        self.evidence = base / "renderer-evidence"
        self.evidence.mkdir(mode=0o700)
        self.patch = mock.patch.object(r, "EVIDENCE", self.evidence)
        self.patch.start()
        self.run = self.evidence / "r0d-root-e-123"
        r.private_directory(self.run, self.run)

    def tearDown(self):
        self.patch.stop()
        self.temporary.cleanup()


class StoreAndPathTests(OwnedTemporary):
    def test_new_root_only_and_no_alias_traversal(self):
        self.assertEqual(r.owned_run_path(self.run / "E-A", self.run), self.run / "E-A")
        for path in (self.run / ".." / "other", Path("/tmp/other"), self.run / 'bad"name'):
            with self.assertRaises(r.Stop):
                r.owned_run_path(path, self.run)
        with self.assertRaises(r.Stop):
            r.owned_run_path(self.evidence / "r0d-123/E-A", self.evidence / "r0d-123")

    def test_private_directory_mode(self):
        self.assertEqual(self.run.stat().st_mode & 0o777, 0o700)

    def test_write_once_and_sealed_inventory_exclusions(self):
        store = r.Store(self.run)
        path = store.bytes("synthetic.txt", b"synthetic")
        with self.assertRaises(FileExistsError):
            store.bytes("synthetic.txt", b"overwrite")
        digest = store.seal(r.Budget())
        inventory = r.read_json(self.run / "artifact-index.json")
        self.assertEqual(set(inventory), {str(path)})
        store.completion({"artifactIndexSha256": digest})
        self.assertEqual(r.sha(self.run / "artifact-index.json"), digest)
        with self.assertRaises(r.Stop):
            store.bytes("later.txt", b"later")
        with self.assertRaises(r.Stop):
            store.seal(r.Budget())
        with self.assertRaises(FileExistsError):
            store.completion({})

    def test_unclosed_unregistered_or_mutated_artifact_stops_seal(self):
        store = r.Store(self.run)
        path, stream = store.open("open.txt")
        with self.assertRaises(r.Stop):
            store.seal(r.Budget())
        store.close(path, stream)
        path.write_bytes(b"mutated")
        with self.assertRaises(r.Stop):
            store.seal(r.Budget())

    def test_unknown_file_or_symlink_not_sealed(self):
        store = r.Store(self.run)
        (self.run / "unknown").write_bytes(b"synthetic")
        with self.assertRaises(r.Stop):
            store.seal(r.Budget())

    def test_completion_requires_index(self):
        with self.assertRaises(r.Stop):
            r.Store(self.run).completion({})

    def test_fixture_provenance_mutation_symlink_and_missing(self):
        store = r.Store(self.run)
        path = store.bytes("sentinel.txt", r.SENTINEL)
        first = r.fixture(path, self.run)
        self.assertEqual(first["mode"], 0o600)
        link = self.run / "link"
        link.symlink_to(path)
        with self.assertRaises(r.Stop):
            r.fixture(link, self.run)
        path.write_bytes(b"changed")
        with self.assertRaises(r.Stop):
            r.fixture(path, self.run)
        with self.assertRaises(FileNotFoundError):
            r.fixture(self.run / "missing", self.run)

    def test_fixture_permissions_and_hardlinks_rejected(self):
        store = r.Store(self.run)
        path = store.bytes("sentinel.txt", r.SENTINEL)
        path.chmod(0o644)
        with self.assertRaises(r.Stop):
            r.fixture(path, self.run)
        path.chmod(0o600)
        os.link(path, self.run / "hardlink")
        with self.assertRaises(r.Stop):
            r.fixture(path, self.run)

    def test_helper_arguments_bind_prefix_protocol_and_outside_scratch(self):
        scratch = self.run / "scratch/E-A"
        read = self.run / "E-A/read-sentinel.txt"
        write = self.run / "E-A/write-sentinel.txt"
        profile = self.run / "inputs/diagnostic-root-01.sb"
        for ordinal in (1, 2):
            command = r.helper_command(profile, scratch, read, write, 12345, ordinal, self.run)
            self.assertEqual(command[7], str(r.HELPER))
            self.assertEqual(command[-1], str(ordinal))
        for ordinal in (0, 3, 43, 44, True):
            with self.assertRaises(r.Stop):
                r.helper_command(profile, scratch, read, write, 12345, ordinal, self.run)
        with self.assertRaises(r.Stop):
            r.helper_command(profile, scratch, scratch / "read", write, 12345, 1, self.run)


class IsolationCleanupTests(unittest.TestCase):
    def test_five_variable_environment_no_ambient_inheritance(self):
        with mock.patch.dict(os.environ, {"DUMMY_PROVIDER_TOKEN": "synthetic",
                                        "DYLD_INSERT_LIBRARIES": "synthetic", "SSH_AUTH_SOCK": "synthetic"}, clear=True):
            env = r.environment(Path("/private/synthetic"))
        self.assertEqual(set(env), {"HOME", "TMPDIR", "PATH", "LANG", "LC_ALL"})
        self.assertEqual(env["HOME"], "/private/synthetic")
        self.assertEqual(env["TMPDIR"], "/private/synthetic/")

    def test_spawn_is_only_a_mock(self):
        factory = mock.Mock()
        r.spawn(["synthetic"], Path("/private/synthetic"), popen=factory)
        _args, kwargs = factory.call_args
        self.assertEqual(kwargs["cwd"], Path("/private/synthetic"))
        self.assertEqual(kwargs["stdin"], subprocess.PIPE)
        self.assertEqual(kwargs["stdout"], subprocess.PIPE)
        self.assertEqual(kwargs["stderr"], subprocess.PIPE)
        self.assertTrue(kwargs["close_fds"])
        self.assertTrue(kwargs["start_new_session"])
        self.assertNotIn("pass_fds", kwargs)

    def test_reaped_child_never_signalled(self):
        process = mock.Mock(pid=100, returncode=-6)
        process.poll.return_value = -6
        killer = mock.Mock()
        r.cleanup(process, 30, [], clock=lambda: 0, killpg=killer)
        killer.assert_not_called()

    def test_foreign_group_or_elapsed_deadline_never_signalled(self):
        for group, clock in ((99, lambda: 0), (100, lambda: 31)):
            process = mock.Mock(pid=100)
            process.poll.return_value = None
            killer = mock.Mock()
            with self.assertRaises(r.Stop):
                r.cleanup(process, 30, [], clock=clock, getpgid=lambda _pid: group, killpg=killer)
            killer.assert_not_called()

    def test_owned_term_then_kill_are_mocked(self):
        process = mock.Mock(pid=100)
        process.poll.return_value = None
        process.wait.side_effect = [subprocess.TimeoutExpired("synthetic", 0.25), 0]
        killer = mock.Mock()
        r.cleanup(process, 30, [], clock=lambda: 0, getpgid=lambda _pid: 100, killpg=killer)
        self.assertEqual(killer.call_args_list, [mock.call(100, signal.SIGTERM), mock.call(100, signal.SIGKILL)])

    def test_reused_group_blocks_escalation(self):
        process = mock.Mock(pid=100)
        process.poll.return_value = None
        process.wait.side_effect = subprocess.TimeoutExpired("synthetic", 0.25)
        groups = iter((100, 99))
        killer = mock.Mock()
        with self.assertRaises(r.Stop):
            r.cleanup(process, 30, [], clock=lambda: 0, getpgid=lambda _pid: next(groups), killpg=killer)
        killer.assert_called_once_with(100, signal.SIGTERM)

    def test_failed_reap_propagates(self):
        process = mock.Mock(pid=100)
        process.poll.return_value = None
        process.wait.side_effect = subprocess.TimeoutExpired("synthetic", 0.25)
        with self.assertRaises(subprocess.TimeoutExpired):
            r.cleanup(process, 30, [], clock=lambda: 0, getpgid=lambda _pid: 100, killpg=mock.Mock())

    def test_listener_factory_and_thread_are_mocked(self):
        sock = mock.Mock()
        sock.getsockname.return_value = ("127.0.0.1", 12345)
        thread = mock.Mock()
        thread.is_alive.side_effect = [True, False]
        with mock.patch.object(r.threading, "Thread", return_value=thread):
            canary = r.Canary(factory=mock.Mock(return_value=sock))
            result = canary.close()
        self.assertTrue(result["healthy"])
        sock.bind.assert_called_once_with(("127.0.0.1", 0))

    def test_listener_error_or_failed_thread_close_is_unhealthy(self):
        sock = mock.Mock()
        sock.getsockname.return_value = ("127.0.0.1", 12345)
        thread = mock.Mock()
        thread.is_alive.return_value = True
        with mock.patch.object(r.threading, "Thread", return_value=thread):
            canary = r.Canary(factory=mock.Mock(return_value=sock))
            canary.errors.append(errno.EINVAL)
            result = canary.close()
        self.assertFalse(result["healthy"])

    def test_listener_bind_failure_closes_socket(self):
        sock = mock.Mock()
        sock.bind.side_effect = OSError("synthetic")
        with self.assertRaises(OSError):
            r.Canary(factory=mock.Mock(return_value=sock))
        sock.close.assert_called_once()


class ExecutionTests(OwnedTemporary):
    def fake_process(self, output=b"", stderr=b""):
        process = mock.Mock(pid=100, returncode=-6)
        process.stdin = io.BytesIO()
        process.stdout = io.BytesIO(output)
        process.stderr = io.BytesIO(stderr)
        process.poll.return_value = -6
        process.wait.return_value = -6
        return process

    def execute_fake(self, process, out_limit=4096):
        store = r.Store(self.run)
        r.private_directory(self.run / "E-A", self.run)
        with mock.patch.object(r, "spawn", return_value=process), \
             mock.patch.object(r.os, "getpgid", return_value=100):
            result = r.execute(store, "E-A", ["synthetic"], self.run,
                               r.time.monotonic() + 25, r.time.monotonic() + 30, out_limit, 16384)
        return result

    def test_retained_abort_and_closed_drains(self):
        result = self.execute_fake(self.fake_process())
        self.assertEqual(result["exit"], -6)
        self.assertTrue(result["pipeCleanup"])
        self.assertFalse(r.process_ok(result))
        self.assertTrue((self.run / "E-A/process-result.json").exists())

    def test_output_flood_stops_and_is_bounded(self):
        result = self.execute_fake(self.fake_process(output=b"abcdefgh"), out_limit=4)
        self.assertTrue(result["outputOverflow"])
        self.assertEqual((self.run / "E-A/stdout.raw").read_bytes(), b"abcd")
        self.assertFalse(r.process_ok(result))

    def test_broken_pipe_is_retained_failure(self):
        process = self.fake_process()
        process.stdin = mock.Mock()
        process.stdin.close.side_effect = BrokenPipeError()
        result = self.execute_fake(process)
        self.assertEqual(result["errorCode"], "BrokenPipeError")
        self.assertFalse(r.process_ok(result))

    def test_ordinary_deadline_no_native_spawn(self):
        store = r.Store(self.run)
        r.private_directory(self.run / "E-A", self.run)
        with mock.patch.object(r, "spawn") as spawn:
            result = r.execute(store, "E-A", ["synthetic"], self.run, 0, 0, 4096, 16384)
        spawn.assert_not_called()
        self.assertEqual(result["errorCode"], "ordinary_deadline_before_spawn")


if __name__ == "__main__":
    # This entry point may run only after the primary explicitly authorizes
    # the sealed controller's one locked invocation.
    unittest.main(verbosity=2)
