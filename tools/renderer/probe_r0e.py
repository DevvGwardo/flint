#!/usr/bin/env python3
"""R0E only. No execution without the primary-owned external authorization seal."""
import argparse
import errno
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import resource
import signal
import socket
import stat
import subprocess
import sys
import threading
import time

REPO = Path("/Volumes/T7 Shield/mac-offload/Projects/flint")
ROOT = Path("/Volumes/T7 Shield/mac-offload/flint-t3-implementation-glct94hg")
EVIDENCE = ROOT / "renderer-evidence"
SEAL = ROOT / "evidence/renderer-r0e-execution-seal.json"
LOCK = ROOT / "evidence/build.lock"
PRIOR = EVIDENCE / "r0d-1791256686399009000"
CONTINUATION = EVIDENCE / "r0d-1791257653286289000"
HELPER = PRIOR / "DyldStartupProbe"
BASELINE = REPO / "renderer/macos/diagnostic-dyld-01-base.sb"
SOURCES = (REPO / "tools/renderer/probe_r0e.py",
           REPO / "tools/renderer/test_probe_r0e.py",
           REPO / "renderer/macos/diagnostic-root-01.sb")
HELPER_SHA = "fb428c9cf88327901b24f78efdad9de1a44c89c9c1b7019dee514a187f206364"
BASELINE_SHA = "1cec28403b16936fcb57052d66bf2f3001c1480f6017c930408952ce67261ae4"
PROFILE_SHA = "d1d448bc00a24139dfb135c70006c9727bdcd2e59162f14038e58afa7b3ffed0"
PRIOR_INDEX_SHA = "1984a87a2bd48b7fd26cc04da24d6ae0fc205c685ea42568062d5d779d6c72e3"
CONT_INDEX_SHA = "609510e2ad9a2e62edb730019fa0d7124ce99577df0d0531f2ce2157ebb8c064"
CONT_RECEIPT_SHA = "a4949a8e0c4e5a703ab71c81d634652350572c6b55c56af6a6cb08c6f2b5e2f8"
ROOT_RULE = (b'(allow file-read-data file-read-metadata file-test-existence\n'
             b'  (require-all (literal "/") (vnode-type DIRECTORY)))\n')
MAIN_RECORD = b'{"protocol":1,"event":"main"}\n'
SENTINEL = b"R0D_SYNTHETIC_SENTINEL_NOT_A_CREDENTIAL\n"
POLICY = {
    "historicalCounts": {"R0": 34, "R0D": 8},
    "maximumR0ELaunches": 2, "maximumAfterLockSeconds": 300,
    "maximumCaseSeconds": 30, "ordinaryCaseSeconds": 25,
    "finalizationReserveSeconds": 20,
    "slots": ["E-A", "E-B"], "helperArguments": [1, 2],
    "eligibleNetworkStages": ["socket", "connect"],
    "nonblockingStageStops": True, "firstFailureStops": True,
    "environmentNames": ["HOME", "TMPDIR", "PATH", "LANG", "LC_ALL"],
    "privatePipes": True, "closeExtraFds": True, "ownedSessionCleanup": True,
    "compileAuthorized": False, "controlAuthorized": False,
    "webKitAuthorized": False, "rendererCapabilityAuthorized": False,
}
AUTHORITY = {
    REPO / "docs/T3-SLICE-RENDERER-00E.md": "4971918fdccb0305e5bc85e0231939b8c6f210823729f4a4b388c885646224ba",
    REPO / "docs/T3-RENDERER-R0E-RULINGS.md": "5e52abd0d5b4feb23652b9ed9e64cdf7067f663dd4ae6c5842fb492a7042beeb",
    ROOT / "specs/renderer-r0e-phase0.md": "fe5efc341e3db62a3898aa13009725965d7f6b0ec272fd0920331ee0ace595d7",
    ROOT / "specs/renderer-r0d-results.md": "970fc2d48a0bc0c820089889146e28823f29d0a5bf8be74415d121be6a71f30d",
    REPO / "docs/T3-RENDERER-R0D-VERDICT.md": "c2bcc0b022db3ca7d12042d2758ab92bad88fedae016389a976144d54f8bd060",
    ROOT / "evidence/contracts-frozen-renderer-r0e.json": "d011ade400d5629b2df7cdbc2c7befcec30cf845ac33033745c9c564e478d788",
    BASELINE: BASELINE_SHA, HELPER: HELPER_SHA,
    REPO / "tools/renderer/probe_r0d.py": "c50f2fa0153f469f1d8438310bc381a14d234705a328ca2e14b9348368f3fee7",
    REPO / "tools/renderer/test_probe_r0d.py": "f20658dbc3937275289526f14f4cc81ccab80be3b33f3221eebf3e6e2222b5eb",
}
EXTRA_PROOFS = (
    ROOT / "evidence/contracts-frozen-renderer-r0e-rulings.json",
    ROOT / "specs/renderer-phase0.md",
    ROOT / "specs/renderer-r0-results.md",
    ROOT / "specs/renderer-r0d-continuation-results.md",
    ROOT / "evidence/renderer-r0d-independent-1791258102167676000-result.json",
    ROOT / "specs/renderer-r0e-implementation.md",
)


class Stop(Exception):
    """Stable diagnostic code only; do not stringify arbitrary exception data."""


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"), allow_nan=False)


def strict_json(raw):
    def pairs(items):
        result = {}
        for key, value in items:
            if key in result:
                raise Stop("duplicate_json_key")
            result[key] = value
        return result
    def invalid_constant(_value):
        raise Stop("invalid_json_constant")
    try:
        return json.loads(raw.decode("utf-8"), object_pairs_hook=pairs,
                          parse_constant=invalid_constant)
    except (UnicodeError, ValueError, TypeError):
        raise Stop("invalid_json")


def valid_digest(value):
    return type(value) is str and re.fullmatch(r"[0-9a-f]{64}", value) is not None


def identity(info):
    return {"uid": info.st_uid, "device": info.st_dev, "inode": info.st_ino,
            "mode": stat.S_IMODE(info.st_mode), "bytes": info.st_size}


def regular(path, owned=False, single_link=False):
    path = Path(path)
    if not path.is_absolute() or path.resolve() != path:
        raise Stop("path_alias")
    info = path.lstat()
    if not stat.S_ISREG(info.st_mode) or (single_link and info.st_nlink != 1):
        raise Stop("nonregular_or_linked_file")
    if owned and info.st_uid != os.getuid():
        raise Stop("file_owner")
    return info


def file_bytes(path, budget=None, limit=2 * 1024 * 1024, reserve=0):
    before = regular(path)
    if before.st_size > limit:
        raise Stop("file_size_limit")
    flags = os.O_RDONLY | os.O_NOFOLLOW
    fd = os.open(str(path), flags)
    chunks = []
    try:
        if identity(os.fstat(fd)) != identity(before):
            raise Stop("file_identity_race")
        total = 0
        while True:
            if budget is not None:
                budget.check(reserve)
            block = os.read(fd, min(1024 * 1024, limit + 1 - total))
            if not block:
                break
            chunks.append(block)
            total += len(block)
            if total > limit:
                raise Stop("file_size_limit")
        if identity(os.fstat(fd)) != identity(before):
            raise Stop("file_changed")
    finally:
        os.close(fd)
    if identity(regular(path)) != identity(before):
        raise Stop("file_replaced")
    return b"".join(chunks)


def sha(path, budget=None, reserve=0):
    # Stream large preserved binaries, never import or execute them.
    before = regular(path)
    if before.st_size > 512 * 1024 * 1024:
        raise Stop("hash_size_limit")
    digest = hashlib.sha256()
    count = 0
    with Path(path).open("rb") as stream:
        if identity(os.fstat(stream.fileno())) != identity(before):
            raise Stop("hash_identity_race")
        while True:
            if budget is not None:
                budget.check(reserve)
            block = stream.read(1024 * 1024)
            if not block:
                break
            count += len(block)
            if count > before.st_size:
                raise Stop("hash_file_grew")
            digest.update(block)
        if count != before.st_size or identity(os.fstat(stream.fileno())) != identity(before):
            raise Stop("hash_changed")
    if identity(regular(path)) != identity(before):
        raise Stop("hash_replaced")
    return digest.hexdigest()


def read_json(path, budget=None, reserve=0):
    return strict_json(file_bytes(path, budget, reserve=reserve))


def check_profile(candidate, baseline):
    if hashlib.sha256(baseline).hexdigest() != BASELINE_SHA:
        raise Stop("baseline_hash")
    if candidate != baseline + ROOT_RULE or len(candidate) != 1014:
        raise Stop("profile_delta")
    if hashlib.sha256(candidate).hexdigest() != PROFILE_SHA:
        raise Stop("profile_hash")


def indexed_proofs(folder, index_digest, excluded=(), budget=None):
    index = folder / "artifact-index.json"
    if sha(index, budget) != index_digest:
        raise Stop("prior_index_hash")
    records = read_json(index, budget)
    if type(records) is not dict:
        raise Stop("prior_index_schema")
    expected = {str(index): index_digest}
    for name, item in records.items():
        path = Path(name)
        if not path.is_relative_to(folder) or path == index:
            raise Stop("prior_index_scope")
        if type(item) is not dict or set(item) != {"bytes", "sha256"}:
            raise Stop("prior_index_schema")
        if type(item["bytes"]) is not int or item["bytes"] < 0 or not valid_digest(item["sha256"]):
            raise Stop("prior_index_schema")
        if regular(path).st_size != item["bytes"] or sha(path, budget) != item["sha256"]:
            raise Stop("prior_payload_hash")
        expected[name] = item["sha256"]
    for path, digest in excluded:
        if sha(path, budget) != digest:
            raise Stop("prior_receipt_hash")
        expected[str(path)] = digest
    actual = {str(p) for p in folder.rglob("*") if p.is_file()}
    if actual != set(expected) or any(p.is_symlink() for p in folder.rglob("*")):
        raise Stop("prior_file_set")
    return expected


def preservation_expectations(budget=None):
    """Exact external preservationHashes map; read-only, no subprocess."""
    original = indexed_proofs(PRIOR, PRIOR_INDEX_SHA, budget=budget)
    continuation = indexed_proofs(
        CONTINUATION, CONT_INDEX_SHA,
        ((CONTINUATION / "completion-receipt.json", CONT_RECEIPT_SHA),), budget)
    if len(original) != 55 or len(continuation) != 70:
        raise Stop("prior_artifact_counts")
    guards = read_json(CONTINUATION / "guards-after.json", budget)
    if type(guards) is not dict or len(guards) != 702:
        raise Stop("prior_guard_schema")
    expected = {}
    for mapping in (guards, original, continuation,
                    {str(path): value for path, value in AUTHORITY.items()}):
        for path, digest in mapping.items():
            if not valid_digest(digest) or (path in expected and expected[path] != digest):
                raise Stop("preservation_conflict")
            expected[path] = digest
    historical_inputs = read_json(PRIOR / "input-hashes.json", budget)
    repaired_inputs = read_json(CONTINUATION / "repaired-source-hashes.json", budget)
    for mapping in (historical_inputs, repaired_inputs):
        for path, digest in mapping.items():
            # Historical runner/test copies remain guarded in their artifact
            # sets; the current two files retain their separately reviewed repair.
            if path in repaired_inputs and mapping is historical_inputs:
                continue
            if not valid_digest(digest) or (path in expected and expected[path] != digest):
                raise Stop("historical_source_conflict")
            expected[path] = digest
    for path in EXTRA_PROOFS:
        digest = sha(path, budget)
        if str(path) in expected and expected[str(path)] != digest:
            raise Stop("extra_proof_conflict")
        expected[str(path)] = digest
    old = read_json(PRIOR / "summary.json", budget)
    current = read_json(CONTINUATION / "summary.json", budget)
    check_prior_counts(old, current)
    for summary, folder, slots in (
            (old, PRIOR, ("D1-A", "D1-B", "D2-A")),
            (current, CONTINUATION, ("D2-B", "D3-A", "D3-B", "D4-A", "D4-B"))):
        for case, slot in zip(summary["cases"], slots):
            if canonical(case) != canonical(read_json(folder / slot / "case-result.json", budget)):
                raise Stop("prior_case_conflict")
            if canonical(case["process"]) != canonical(read_json(folder / slot / "process-result.json", budget)):
                raise Stop("prior_process_conflict")
    ruled = read_json(ROOT / "evidence/contracts-frozen-renderer-r0e-rulings.json", budget)
    if (ruled.get("testsAndNativeExecutionAuthorized") is not False
            or ruled.get("externalExecutionSealRequired") is not True
            or ruled.get("frozenImplementationContracts") != {
                "docs/T3-RENDERER-R0E-RULINGS.md": AUTHORITY[REPO / "docs/T3-RENDERER-R0E-RULINGS.md"]}
            or ruled.get("frozenProposal") != {
                str(ROOT / "specs/renderer-r0e-phase0.md"): AUTHORITY[ROOT / "specs/renderer-r0e-phase0.md"]}):
        raise Stop("implementation_ruling_conflict")
    return expected


def check_prior_counts(old, current):
    old_ids = [(c["profile"], c["repetition"], c["launchNumber"], c["process"]["pid"], c["process"]["exit"])
               for c in old.get("cases", [])]
    current_ids = [(c["slot"], c["launchNumber"], c["process"]["pid"], c["process"]["exit"])
                   for c in current.get("cases", [])]
    if (old_ids != [(1, "A", 1, 160, -6), (1, "B", 2, 161, -6), (2, "A", 3, 162, 71)]
            or old.get("launchCount") != 3 or old.get("perProfileCount") != [2, 1, 0, 0]
            or current_ids != [("D2-B", 4, 10427, 71), ("D3-A", 5, 10428, 71),
                               ("D3-B", 6, 10429, 71), ("D4-A", 7, 10430, 71),
                               ("D4-B", 8, 10431, 71)]
            or current.get("additionalLaunchCount") != 5
            or current.get("additionalPerProfileCount") != [0, 1, 2, 2]
            or current.get("cumulativeLaunchCount") != 8
            or current.get("cumulativePerProfileCount") != [2, 2, 2, 2]
            or old.get("guardsUnchanged") is not True
            or current.get("guardsUnchanged") is not True):
        raise Stop("historical_accounting_conflict")


def validate_seal_object(seal, expected_preservation):
    keys = {"schemaVersion", "experiment", "authorizationId", "executionAuthorized",
            "sourceHashes", "helperIdentity", "preservationHashes", "policy"}
    if type(seal) is not dict or set(seal) != keys:
        raise Stop("seal_schema")
    for key, value in (("schemaVersion", 1), ("experiment", "R0E"),
                       ("executionAuthorized", True), ("policy", POLICY)):
        if canonical(seal[key]) != canonical(value):
            raise Stop("seal_authorization")
    if type(seal["authorizationId"]) is not str or not re.fullmatch(r"R0E-[A-Za-z0-9.-]{1,64}", seal["authorizationId"]):
        raise Stop("seal_authorization_id")
    hashes = seal["sourceHashes"]
    if (type(hashes) is not dict or set(hashes) != {str(p) for p in SOURCES}
            or any(not valid_digest(v) for v in hashes.values())
            or hashes[str(SOURCES[2])] != PROFILE_SHA):
        raise Stop("seal_source_schema")
    if canonical(seal["preservationHashes"]) != canonical(expected_preservation):
        raise Stop("seal_preservation_closure")
    helper = seal["helperIdentity"]
    if type(helper) is not dict or set(helper) != {"path", "sha256", "uid", "device", "inode", "mode", "bytes"}:
        raise Stop("seal_helper_schema")
    if helper["path"] != str(HELPER) or helper["sha256"] != HELPER_SHA:
        raise Stop("seal_helper_path_hash")
    for key in ("uid", "device", "inode", "mode", "bytes"):
        if type(helper[key]) is not int or helper[key] < 0:
            raise Stop("seal_helper_identity")
    if helper["uid"] != os.getuid() or helper["inode"] == 0 or helper["bytes"] == 0 or helper["mode"] & 0o022 or not helper["mode"] & 0o100:
        raise Stop("seal_helper_identity")
    return seal


def verify_inputs(seal, seal_digest, budget=None, reserve=0):
    if sha(SEAL, budget, reserve) != seal_digest:
        raise Stop("execution_seal_drift")
    expected = preservation_expectations(budget)
    validate_seal_object(seal, expected)
    for path, digest in list(seal["preservationHashes"].items()) + list(seal["sourceHashes"].items()):
        if sha(Path(path), budget, reserve) != digest:
            raise Stop("preserved_or_input_hash_drift")
    helper = {"path": str(HELPER), "sha256": sha(HELPER, budget, reserve),
              **identity(regular(HELPER, owned=True))}
    if helper != seal["helperIdentity"]:
        raise Stop("helper_identity_drift")
    check_profile(file_bytes(SOURCES[2], budget, reserve=reserve),
                  file_bytes(BASELINE, budget, reserve=reserve))
    return {"preservedFiles": len(expected), "helperIdentity": helper,
            "sealSha256": seal_digest, "sourceHashes": seal["sourceHashes"]}


def load_seal(seal_digest):
    if not valid_digest(seal_digest):
        raise Stop("seal_digest_argument")
    if sha(SEAL) != seal_digest:
        raise Stop("execution_seal_hash")
    seal = read_json(SEAL)
    validate_seal_object(seal, preservation_expectations())
    verify_inputs(seal, seal_digest)
    return seal


def owned_run_path(path, run):
    path, run = Path(path), Path(run)
    if (run.parent != EVIDENCE or not re.fullmatch(r"r0d-root-e-[0-9]+", run.name)
            or path.resolve() != path or not path.is_relative_to(run)
            or any(not re.fullmatch(r"[A-Za-z0-9._-]+", part) or part in (".", "..")
                   for part in path.relative_to(run).parts)):
        raise Stop("r0e_owned_path")
    return path


def private_directory(path, run):
    path = owned_run_path(path, run)
    path.mkdir(mode=0o700)
    info = path.lstat()
    if not stat.S_ISDIR(info.st_mode) or info.st_uid != os.getuid() or stat.S_IMODE(info.st_mode) != 0o700:
        raise Stop("private_directory")
    return path


class Budget:
    def __init__(self, clock=time.monotonic):
        self.clock = clock
        self.start = clock()
        self.deadline = self.start + 300
        self.consumed = 0

    def check(self, reserve=0):
        if self.clock() >= self.deadline - reserve:
            raise Stop("execution_deadline")

    def require_case(self):
        self.check(50)

    def consume(self, slot, store, eligible_previous=False):
        if self.consumed >= 2 or slot != ("E-A", "E-B")[self.consumed]:
            raise Stop("slot_limit_or_replay")
        if slot == "E-B" and not eligible_previous:
            raise Stop("e_b_ineligible")
        self.require_case()
        ordinal = self.consumed + 1
        record = {"slot": slot, "r0eConsumed": ordinal, "r0": 34, "r0d": 8,
                  "cumulativeConsumed": 42 + ordinal, "helperArgument": ordinal}
        self.consumed = ordinal
        store.json("attempt-" + slot + ".json", record)
        return record


class Store:
    """Closed-file ledger. All artifacts, including raw streams, are exclusive."""
    def __init__(self, run):
        self.run = run
        self.closed = {}
        self.opened = {}
        self.sealed = False

    def open(self, name):
        if self.sealed:
            raise Stop("store_sealed")
        path = owned_run_path(self.run / name, self.run)
        stream = path.open("xb")
        os.fchmod(stream.fileno(), 0o600)
        self.opened[str(path)] = identity(os.fstat(stream.fileno()))
        return path, stream

    def close(self, path, stream):
        stream.flush()
        os.fsync(stream.fileno())
        info = os.fstat(stream.fileno())
        stream.close()
        original = self.opened.pop(str(path))
        actual = regular(path, owned=True, single_link=True)
        for key in ("uid", "device", "inode", "mode"):
            if identity(actual)[key] != original[key] or identity(info)[key] != original[key]:
                raise Stop("artifact_identity_drift")
        self.closed[str(path)] = {"bytes": actual.st_size, "sha256": sha(path),
                                  "identity": identity(actual)}

    def bytes(self, name, value):
        path, stream = self.open(name)
        try:
            stream.write(value)
        finally:
            self.close(path, stream)
        return path

    def json(self, name, value):
        return self.bytes(name, (json.dumps(value, indent=2, sort_keys=True, allow_nan=False) + "\n").encode())

    def seal(self, budget):
        if self.sealed or self.opened:
            raise Stop("inventory_state")
        budget.check()
        actual = {str(p) for p in self.run.rglob("*") if p.is_file()}
        if actual != set(self.closed) or any(p.is_symlink() for p in self.run.rglob("*")):
            raise Stop("inventory_file_set")
        for name, record in self.closed.items():
            if identity(regular(Path(name), owned=True, single_link=True)) != record["identity"] or sha(Path(name), budget) != record["sha256"]:
                raise Stop("inventory_drift")
        payloads = {name: {"bytes": value["bytes"], "sha256": value["sha256"]}
                    for name, value in sorted(self.closed.items())}
        path = self.json("artifact-index.json", payloads)
        self.sealed = True
        return sha(path, budget)

    def completion(self, value):
        if not self.sealed or self.opened:
            raise Stop("completion_before_seal")
        path = self.run / "completion-receipt.json"
        with path.open("xb") as stream:
            os.fchmod(stream.fileno(), 0o600)
            stream.write((json.dumps(value, indent=2, sort_keys=True, allow_nan=False) + "\n").encode())
            stream.flush()
            os.fsync(stream.fileno())
        return sha(path)


def environment(scratch):
    return {"HOME": str(scratch), "TMPDIR": str(scratch) + "/",
            "PATH": "/usr/bin:/bin:/usr/sbin:/sbin", "LANG": "en_US.UTF-8", "LC_ALL": "en_US.UTF-8"}


def fixture(path, run):
    owned_run_path(path, run)
    info = regular(path, owned=True, single_link=True)
    if stat.S_IMODE(info.st_mode) != 0o600 or file_bytes(path, limit=128) != SENTINEL:
        raise Stop("fixture_provenance")
    return {"path": str(path), "sha256": sha(path), **identity(info)}


class Canary:
    def __init__(self, factory=socket.socket):
        self.sock = factory(socket.AF_INET, socket.SOCK_STREAM)
        self.active = False
        self.hits = 0
        self.errors = []
        self.thread = None
        try:
            self.sock.bind(("127.0.0.1", 0))
            self.sock.listen(4)
            self.sock.settimeout(0.02)
            self.address = self.sock.getsockname()
            if self.address[0] != "127.0.0.1" or not 0 < self.address[1] <= 65535:
                raise Stop("canary_binding")
            self.active = True
            self.started = time.monotonic()
            self.thread = threading.Thread(target=self.accept, daemon=True)
            self.thread.start()
        except BaseException:
            self.sock.close()
            raise

    def accept(self):
        while self.active:
            try:
                connection, _address = self.sock.accept()
            except socket.timeout:
                continue
            except OSError as error:
                if self.active:
                    self.errors.append(error.errno)
                return
            self.hits += 1
            connection.close()

    def close(self):
        was_healthy = self.thread is not None and self.thread.is_alive() and not self.errors
        self.active = False
        self.sock.close()
        if self.thread is not None:
            self.thread.join(timeout=0.2)
        closed = self.thread is not None and not self.thread.is_alive()
        return {"bound": list(self.address), "hits": self.hits, "acceptErrors": self.errors,
                "startedMonotonic": self.started, "closedMonotonic": time.monotonic(),
                "healthyBeforeClose": was_healthy, "closed": closed,
                "healthy": was_healthy and closed and not self.errors}


def spawn(command, scratch, popen=None):
    popen = popen or subprocess.Popen
    return popen(command, cwd=scratch, env=environment(scratch), stdin=subprocess.PIPE,
                 stdout=subprocess.PIPE, stderr=subprocess.PIPE, close_fds=True, start_new_session=True)


def cleanup(process, deadline, events, clock=time.monotonic, getpgid=None, killpg=None):
    getpgid = getpgid or os.getpgid
    killpg = killpg or os.killpg
    if process.poll() is not None:
        events.append({"event": "already_reaped", "exit": process.returncode})
        return
    if clock() >= deadline or getpgid(process.pid) != process.pid:
        raise Stop("cleanup_ownership_or_deadline")
    events.append({"event": "owned_term", "pid": process.pid, "pgid": process.pid})
    killpg(process.pid, signal.SIGTERM)
    try:
        process.wait(timeout=min(0.25, max(0.001, deadline - clock())))
    except subprocess.TimeoutExpired:
        if process.poll() is None:
            if clock() >= deadline or getpgid(process.pid) != process.pid:
                raise Stop("cleanup_ownership_or_deadline")
            events.append({"event": "owned_kill", "pid": process.pid, "pgid": process.pid})
            killpg(process.pid, signal.SIGKILL)
        process.wait(timeout=min(1.0, max(0.001, deadline - clock())))
    events.append({"event": "reaped", "exit": process.returncode})


def execute(store, prefix, command, scratch, ordinary_end, hard_end, out_limit, err_limit):
    info = {"command": command, "cwd": str(scratch), "environmentNames": sorted(environment(scratch)),
            "startMonotonic": time.monotonic(), "ordinaryDeadline": ordinary_end,
            "cleanupDeadline": hard_end, "events": [], "timedOut": False}
    process = None
    drains = []
    destinations = []
    overflow = threading.Event()
    pipe_error = threading.Event()
    def drain(pipe, destination, limit):
        retained = 0
        try:
            while True:
                block = pipe.read(4096)
                if not block:
                    return
                take = min(len(block), max(0, limit - retained))
                destination.write(block[:take])
                retained += take
                if take < len(block):
                    overflow.set()
        except Exception:
            pipe_error.set()
        finally:
            pipe.close()
    try:
        if time.monotonic() >= ordinary_end:
            raise Stop("ordinary_deadline_before_spawn")
        for name in ("stdout.raw", "stderr.raw"):
            destinations.append(store.open(prefix + "/" + name))
        process = spawn(command, scratch)
        info["pid"] = process.pid
        info["pgid"] = os.getpgid(process.pid)
        if info["pgid"] != process.pid:
            raise Stop("spawn_ownership")
        process.stdin.close()
        for pipe, destination, limit in ((process.stdout, destinations[0][1], out_limit),
                                         (process.stderr, destinations[1][1], err_limit)):
            thread = threading.Thread(target=drain, args=(pipe, destination, limit), daemon=True)
            drains.append(thread)
            thread.start()
        while process.poll() is None and time.monotonic() < ordinary_end:
            if overflow.is_set() or pipe_error.is_set():
                raise Stop("pipe_failure")
            time.sleep(0.01)
        info["timedOut"] = process.poll() is None
        cleanup(process, hard_end, info["events"])
        info["exit"] = process.wait(timeout=min(0.2, max(0.001, hard_end - time.monotonic())))
    except BaseException as error:
        info["errorCode"] = str(error) if isinstance(error, Stop) else type(error).__name__
    finally:
        try:
            if process is not None:
                cleanup(process, hard_end, info["events"])
                info["exit"] = process.returncode
        except BaseException as error:
            info["cleanupFailure"] = str(error) if isinstance(error, Stop) else type(error).__name__
        for thread in drains:
            thread.join(timeout=min(0.2, max(0, hard_end - time.monotonic())))
        info["pipeCleanup"] = not any(thread.is_alive() for thread in drains)
        if info["pipeCleanup"]:
            for path, stream in destinations:
                store.close(path, stream)
            if process is not None:
                for pipe in (process.stdin, process.stdout, process.stderr):
                    try:
                        pipe.close()
                    except Exception as error:
                        info["pipeCloseError"] = type(error).__name__
                        pipe_error.set()
        else:
            info["cleanupFailure"] = "unclosed_pipe_drain"
        info["outputOverflow"] = overflow.is_set()
        info["pipeFailure"] = pipe_error.is_set()
        info["finishedMonotonic"] = time.monotonic()
        if info["finishedMonotonic"] >= hard_end:
            info["cleanupFailure"] = "process_deadline"
        store.json(prefix + "/process-result.json", info)
    return info


def process_ok(info):
    return (info.get("exit") == 0 and not info.get("errorCode") and not info.get("cleanupFailure")
            and not info.get("timedOut") and info.get("pipeCleanup") is True
            and info.get("pipeFailure") is False and info.get("outputOverflow") is False)


def classify(raw, stderr, code, fixtures_valid, canary):
    if not fixtures_valid:
        return "fixture_failure"
    if canary.get("hits", 0):
        return "canary_hit"
    if canary.get("healthy") is not True or canary.get("closed") is not True:
        return "canary_health_or_cleanup"
    if stderr:
        prefix = b"sandbox-exec: execvp() of '" + str(HELPER).encode() + b"' failed: "
        if not raw and code == 71 and stderr in (
                prefix + b"Operation not permitted\n", prefix + b"Permission denied\n"):
            return "launcher_exec_permission_stop"
        if re.match(rb"^sandbox-exec: (?:syntax error\b|sandbox_compile:|failed to (?:load|read) profile\b|error in (?:profile|load)\b)", stderr):
            return "profile_syntax_load_stop"
        return "unknown_diagnostic_stop"
    if not raw:
        return "startup_abort_stop" if code == -signal.SIGABRT else "missing_main_stop"
    if not raw.startswith(MAIN_RECORD) or not raw.endswith(b"\n") or len(raw) > 4096:
        return "malformed_output_stop"
    try:
        records = [strict_json(line) for line in raw.splitlines()]
    except Stop:
        return "malformed_output_stop"
    if (len(records) != 5 or canonical(records[0]) != canonical({"protocol": 1, "event": "main"})
            or records[-1] != {"event": "done"}):
        return "malformed_output_stop"
    for value, operation in zip(records[1:4], ("read", "write", "network")):
        if type(value) is not dict or set(value) != {"event", "operation", "stage", "allowed", "errno"}:
            return "malformed_output_stop"
        if (value["event"] != "probe" or value["operation"] != operation
                or type(value["allowed"]) is not bool or type(value["errno"]) is not int
                or type(value["stage"]) is not str):
            return "malformed_output_stop"
        if value["stage"] not in (("socket", "nonblocking", "connect") if operation == "network" else ("open",)):
            return "invalid_syscall_stage_stop"
        if value["allowed"]:
            return "unexpected_access_stop"
        if value["errno"] not in (errno.EPERM, errno.EACCES):
            return "nonpermission_errno_stop"
    if code != 0:
        return "incomplete_process_stop"
    if records[3]["stage"] == "nonblocking":
        return "nonblocking_stage_stop"
    return "complete_native_open_and_" + records[3]["stage"] + "_denials"


def eligible(case):
    return (case.get("classification") in (
        "complete_native_open_and_socket_denials", "complete_native_open_and_connect_denials")
        and process_ok(case.get("process", {})) and case.get("fixturesUnchanged") is True
        and case.get("caseDeadlineExceeded") is False and not case.get("errorCode")
        and case.get("canary", {}).get("healthy") is True
        and case.get("canary", {}).get("closed") is True
        and case.get("canary", {}).get("hits") == 0)


def helper_command(profile, scratch, read_path, write_path, port, ordinal, run):
    for path in (profile, scratch, read_path, write_path):
        owned_run_path(path, run)
    if (type(ordinal) is not int or ordinal not in (1, 2)
            or type(port) is not int or not 0 < port <= 65535
            or read_path.is_relative_to(scratch) or write_path.is_relative_to(scratch)
            or len(str(read_path).encode()) > 1024 or len(str(write_path).encode()) > 1024):
        raise Stop("helper_arguments")
    return ["/usr/bin/sandbox-exec", "-f", str(profile), "-D", "HELPER=" + str(HELPER),
            "-D", "SCRATCH=" + str(scratch), str(HELPER), str(read_path), str(write_path),
            str(port), str(ordinal)]


def case_run(store, budget, slot, profile, used_ports, eligible_previous):
    budget.require_case()
    start = budget.clock()
    ordinary_end = start + 25
    hard_end = min(start + 30, budget.deadline - 20)
    folder = private_directory(store.run / slot, store.run)
    scratch = private_directory(store.run / "scratch" / slot, store.run)
    case = {"slot": slot, "startMonotonic": start, "caseDeadline": hard_end,
            "process": {}, "fixturesUnchanged": False}
    canary = None
    sentinels = [folder / "read-sentinel.txt", folder / "write-sentinel.txt"]
    before = None
    try:
        for path in sentinels:
            store.bytes(str(path.relative_to(store.run)), SENTINEL)
        before = [fixture(path, store.run) for path in sentinels]
        store.json(slot + "/fixtures-before.json", before)
        canary = Canary()
        port = canary.address[1]
        if port in used_ports:
            raise Stop("canary_port_reuse")
        used_ports.add(port)
        if budget.clock() >= ordinary_end:
            raise Stop("case_setup_deadline")
        ordinal = budget.consumed + 1
        command = helper_command(profile, scratch, *sentinels, port, ordinal, store.run)
        case["attempt"] = budget.consume(slot, store, eligible_previous)
        case["process"] = execute(store, slot, command, scratch, ordinary_end, hard_end - 1,
                                  4096, 16384)
    except BaseException as error:
        case["errorCode"] = str(error) if isinstance(error, Stop) else type(error).__name__
    finally:
        case["canary"] = canary.close() if canary is not None else {
            "healthy": False, "closed": True, "hits": 0, "notCreated": True}
        try:
            after = [fixture(path, store.run) for path in sentinels]
            case["fixturesUnchanged"] = before is not None and after == before
            store.json(slot + "/fixtures-after.json", after)
        except BaseException:
            case["fixturesUnchanged"] = False
        raw_path, err_path = folder / "stdout.raw", folder / "stderr.raw"
        raw = file_bytes(raw_path, limit=4096) if raw_path.is_file() else b""
        err = file_bytes(err_path, limit=16384) if err_path.is_file() else b""
        case["mainMarkerObserved"] = raw.startswith(MAIN_RECORD)
        case["classification"] = classify(raw, err, case["process"].get("exit"),
                                           case["fixturesUnchanged"], case["canary"])
        case["caseDeadlineExceeded"] = budget.clock() >= hard_end
        case["throughChecksSeconds"] = budget.clock() - start
        store.json(slot + "/case-result.json", case)
        closed = budget.clock()
        # Record the boundary AFTER serializing the case receipt, without
        # retroactively overwriting that receipt.
        store.json(slot + "/case-closed.json", {
            "receiptClosedMonotonic": closed, "cleanupInclusiveSeconds": closed - start,
            "deadlineExceeded": closed >= hard_end,
            "excludesOnly": "this case-closed receipt serialization"})
        if budget.clock() >= hard_end:
            case["caseDeadlineExceeded"] = True
    return case


def reject_existing_run():
    # Any prior R0E root is a consumed experiment/uncertain interrupted claim.
    # Changing authorizationId or seal bytes does not reset it.
    if any(EVIDENCE.glob("r0d-root-e-*")):
        raise Stop("r0e_run_already_exists")


def parse_cli(argv=None):
    parser = argparse.ArgumentParser(add_help=False, allow_abbrev=False)
    parser.add_argument("--seal-sha256", required=True)
    argv = sys.argv[1:] if argv is None else argv
    if len(argv) != 2 or argv[0] != "--seal-sha256":
        parser.error("only --seal-sha256 followed by one digest is accepted")
    args = parser.parse_args(argv)
    if not valid_digest(args.seal_sha256):
        parser.error("--seal-sha256 requires exactly 64 lowercase hexadecimal characters")
    return args.seal_sha256


def main(argv=None):
    digest = parse_cli(argv)
    # No run directory, claim, tests or native spawn exists before this gate.
    seal = load_seal(digest)
    if Path(__file__).resolve() != SOURCES[0]:
        raise Stop("controller_source_location")
    def interrupted(_number, _frame):
        raise Stop("controller_signal")
    signal.signal(signal.SIGTERM, interrupted)
    signal.signal(signal.SIGINT, interrupted)
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    lock_fd = os.open(str(LOCK), os.O_RDWR | os.O_NOFOLLOW)
    acquired = False
    store = None
    status = 1
    try:
        lock_stat = os.fstat(lock_fd)
        if not stat.S_ISREG(lock_stat.st_mode) or (lock_stat.st_dev, lock_stat.st_ino) != (16777239, 20581567):
            raise Stop("shared_lock_identity")
        fcntl.flock(lock_fd, fcntl.LOCK_EX)
        acquired = True
        budget = Budget()
        acquisition_wall = time.time()
        reject_existing_run()
        verify_inputs(seal, digest, budget, 20)
        run = EVIDENCE / ("r0d-root-e-" + str(time.time_ns()))
        private_directory(run, run)
        store = Store(run)
        store.json("claim.json", {"authorizationId": seal["authorizationId"],
                                 "sealSha256": digest, "controllerPid": os.getpid(),
                                 "historicalCounts": {"R0": 34, "R0D": 8}, "r0eConsumed": 0})
        store.json("lock-acquired.json", {"path": str(LOCK), "device": lock_stat.st_dev,
                                         "inode": lock_stat.st_ino, "wallTime": acquisition_wall,
                                         "monotonic": budget.start, "deadline": budget.deadline})
        stop_reason = None
        cases = []
        try:
            private_directory(run / "inputs", run)
            private_directory(run / "scratch", run)
            store.bytes("inputs/execution-seal.json", file_bytes(SEAL, budget, reserve=20))
            for source in SOURCES:
                data = file_bytes(source, budget, reserve=20)
                if hashlib.sha256(data).hexdigest() != seal["sourceHashes"][str(source)]:
                    raise Stop("input_copy_hash")
                store.bytes("inputs/" + source.name, data)
            before = verify_inputs(seal, digest, budget, 20)
            store.json("guards-before.json", before)
            tests = private_directory(run / "mocked-tests", run)
            scratch = private_directory(run / "scratch" / "tests", run)
            budget.require_case()
            test_start = budget.clock()
            tested = execute(store, tests.name, ["/usr/bin/python3", "-B",
                             str(run / "inputs/test_probe_r0e.py")], scratch,
                             test_start + 25, min(test_start + 30, budget.deadline - 20),
                             65536, 65536)
            if not process_ok(tested):
                raise Stop("mocked_tests_failure")
            prior_ports = set()
            for folder in (PRIOR, CONTINUATION):
                for case in read_json(folder / "summary.json", budget)["cases"]:
                    prior_ports.add(case["canaryPort"])
            profile = run / "inputs/diagnostic-root-01.sb"
            for slot in ("E-A", "E-B"):
                verify_inputs(seal, digest, budget, 20)
                if sha(profile, budget, 20) != PROFILE_SHA:
                    raise Stop("retained_profile_drift")
                previous = bool(cases) and eligible(cases[-1])
                case = case_run(store, budget, slot, profile, prior_ports, previous)
                cases.append(case)
                if not eligible(case):
                    raise Stop(case.get("errorCode") or case["classification"])
            stop_reason = "two_complete_native_observations"
        except BaseException as error:
            stop_reason = str(error) if isinstance(error, Stop) else type(error).__name__
        finally:
            final_guards = False
            try:
                final = verify_inputs(seal, digest, budget)
                store.json("guards-after.json", final)
                final_guards = True
            except BaseException as error:
                store.json("guard-failure.json", {"code": str(error) if isinstance(error, Stop) else type(error).__name__})
                stop_reason = "final_guard_failure"
            status = 0 if stop_reason == "two_complete_native_observations" and final_guards else 1
            store.json("summary.json", {"stopReason": stop_reason, "plannedControllerExit": status,
                "r0": 34, "r0d": 8, "r0eConsumed": budget.consumed,
                "cumulativeConsumed": 42 + budget.consumed,
                "mainMarkersObserved": sum(bool(c.get("mainMarkerObserved")) for c in cases),
                "caseClassifications": [c["classification"] for c in cases],
                "preservationGuardsChecked": final_guards, "rendererCapability": False,
                "controllerExitIsObservedOnlyByForegroundObserver": True})
            release_before = budget.clock()
            fcntl.flock(lock_fd, fcntl.LOCK_UN)
            acquired = False
            release_after = budget.clock()
            os.close(lock_fd)
            lock_fd = None
            store.json("lock-released.json", {"releaseRequestedMonotonic": release_before,
                "releasedMonotonic": release_after, "releasedWallTime": time.time(),
                "afterLockThroughUnlockSeconds": release_after - budget.start})
            index_digest = store.seal(budget)
            boundary = budget.clock()
            completion_digest = store.completion({
                "artifactIndexSha256": index_digest, "r0eConsumed": budget.consumed,
                "cumulativeConsumed": 42 + budget.consumed,
                "afterLockThroughFinalInventorySeconds": boundary - budget.start,
                "deadlineExceededAtBoundary": boundary >= budget.deadline,
                "inventoryExcludes": ["artifact-index.json", "completion-receipt.json"],
                "boundaryExcludes": "completion receipt serialization, verification and final output",
                "plannedControllerExit": status})
            final_elapsed = budget.clock() - budget.start
            if final_elapsed >= 300:
                status = 1
            print(canonical({"run": str(run), "artifactIndexSha256": index_digest,
                "completionReceiptSha256": completion_digest,
                "afterLockThroughCompletionReceiptSeconds": final_elapsed,
                "elapsedExcludes": "this foreground output serialization and process return",
                "r0eConsumed": budget.consumed, "cumulativeConsumed": 42 + budget.consumed,
                "plannedControllerExit": status}), flush=True)
            if budget.clock() >= budget.deadline:
                status = 1
    finally:
        if lock_fd is not None:
            if acquired:
                fcntl.flock(lock_fd, fcntl.LOCK_UN)
            os.close(lock_fd)
    return status


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Stop as error:
        print(canonical({"controllerFailure": str(error), "plannedControllerExit": 1}), flush=True)
        raise SystemExit(1)
