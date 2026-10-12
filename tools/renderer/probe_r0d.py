#!/usr/bin/env python3
"""Remaining-slot R0D continuation; immutable helper/profiles, no compilation."""
import difflib
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
import threading
import time

REPO = Path("/Volumes/T7 Shield/mac-offload/Projects/flint")
ROOT = Path("/Volumes/T7 Shield/mac-offload/flint-t3-implementation-glct94hg")
LOCK = ROOT / "evidence/build.lock"
SDK = Path("/Applications/Xcode.app/Contents/Developer/Platforms/MacOSX.platform/Developer/SDKs/MacOSX26.2.sdk")
PROFILE_NAMES = ("diagnostic-dyld-01-base.sb", "diagnostic-dyld-02-version3.sb",
                 "diagnostic-dyld-03-ancestors.sb", "diagnostic-dyld-04-loader-ops.sb")
LIBRARIES = ("/usr/lib", "/System/Library/dyld",
             "/System/Volumes/Preboot/Cryptexes/OS/System/Library/dyld",
             "/System/Volumes/Preboot/Cryptexes/OS/usr/lib",
             "/System/Cryptexes/OS/System/Library/dyld", "/System/Cryptexes/OS/usr/lib")
METADATA = ("/", "/usr", "/System", "/System/Library", "/System/Volumes",
            "/System/Volumes/Preboot", "/System/Volumes/Preboot/Cryptexes",
            "/System/Volumes/Preboot/Cryptexes/OS",
            "/System/Volumes/Preboot/Cryptexes/OS/System",
            "/System/Volumes/Preboot/Cryptexes/OS/System/Library",
            "/System/Cryptexes", "/System/Cryptexes/OS", "/System/Cryptexes/OS/System",
            "/System/Cryptexes/OS/System/Library", "/System/Cryptexes/OS/usr",
            "/System/Volumes/Preboot/Cryptexes/OS/usr")
OPERATIONS = """(allow syscall-unix
  (syscall-number SYS_open SYS_openat SYS_fstatat SYS_fstatat64 SYS_dup
    SYS_getfsstat SYS_getfsstat64 SYS___mac_syscall SYS_map_with_linking_np))
(allow system-fcntl (fcntl-command F_GETPATH F_ADDFILESIGS_RETURN F_CHECK_LV))
(allow system-mac-syscall
  (require-all (mac-policy-name "Sandbox") (mac-syscall-number 2)))
"""
AUTHORITY = {
    REPO / "docs/T3-SLICE-RENDERER-00D.md": "66abdce4a1bb1ea65d6704e309d2f75010e3b9004ceb7b884422aae7f67e8b1b",
    REPO / "docs/T3-RENDERER-R0D-RULINGS.md": "9498b0dd0d48601f927429a7463c3cfef96bab5660815b1be41f32eea5ea0d30",
    ROOT / "specs/renderer-r0d-phase0.md": "f67db8aef719c1110cec7fd76e211bb636544464cb8e5ad1f21db7ed6609a492",
    REPO / "docs/T3-RENDERER-R0D-CONTINUATION.md": "53c7b16f9bc4ed194db735d70f90d69572e939a6c672f52b02dd3cea1fa2f077",
    ROOT / "specs/renderer-r0d-results.md": "970fc2d48a0bc0c820089889146e28823f29d0a5bf8be74415d121be6a71f30d",
}
PRIOR = ROOT / "renderer-evidence/r0d-1791256686399009000"
PRIOR_HELPER = PRIOR / "DyldStartupProbe"
PRIOR_BINARY_HASH = "fb428c9cf88327901b24f78efdad9de1a44c89c9c1b7019dee514a187f206364"
PRIOR_INDEX_HASH = "1984a87a2bd48b7fd26cc04da24d6ae0fc205c685ea42568062d5d779d6c72e3"
PRIOR_INPUT_HASH = "774f2ce38289bf3ca428c69da69037dd95bcec4c0a796d67809841352ea4f221"
REMAINING_SLOTS = ((1, "B"), (2, "A"), (2, "B"), (3, "A"), (3, "B"))
PRIOR_COUNTS = (2, 1, 0, 0)
ORIGINAL_BINARIES = {
    REPO / "target/release/flint": "26858d8f7a6e0137a6b977d3f46ceeb92b45095289eb2acfbe62d81b408ecdff",
    REPO / "target/release/jevdex": "9414c98470ebf3846c1e143ae3e5942ba8ccb38f0236f0244a8376cf09c9e5e3",
}
SENTINEL_BYTES = b"R0D_SYNTHETIC_SENTINEL_NOT_A_CREDENTIAL\n"
MAIN_RECORD = b'{"protocol":1,"event":"main"}\n'


class Stop(Exception):
    pass


def sha(path):
    with Path(path).open("rb") as stream:
        digest = hashlib.sha256()
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
        return digest.hexdigest()


def save(path, value):
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n")


def environment(scratch):
    return {"HOME": str(scratch), "TMPDIR": str(scratch) + "/",
            "PATH": "/usr/bin:/bin:/usr/sbin:/sbin", "LANG": "en_US.UTF-8", "LC_ALL": "en_US.UTF-8"}


def template(index):
    if index not in range(4):
        raise Stop("profile_not_authorized")
    roots = "".join('  (subpath "' + path + '")\n' for path in LIBRARIES)
    roots = roots.rstrip("\n") + ")\n"
    result = "(version " + ("1" if index == 0 else "3") + ")\n(deny default)\n"
    result += '(allow process-exec (literal (param "HELPER")))\n'
    result += '(allow file-read* file-test-existence (literal (param "HELPER")) (literal "/usr/lib/dyld")\n' + roots
    result += '(allow file-map-executable (literal (param "HELPER")) (literal "/usr/lib/dyld")\n' + roots
    result += '(allow file-read* file-write* file-test-existence (subpath (param "SCRATCH")))\n'
    if index >= 2:
        result += "@@ANCESTOR_METADATA@@\n"
    if index == 3:
        result += OPERATIONS
    return result


def owned_path(path):
    path = Path(path)
    prefix = ROOT / "renderer-evidence"
    if not path.is_absolute() or path.resolve() != path or not path.is_relative_to(prefix):
        raise Stop("unowned_path")
    relative = path.relative_to(prefix)
    if not relative.parts or not re.fullmatch(r"r0d-[0-9]+", relative.parts[0]):
        raise Stop("unowned_run")
    if any(not re.fullmatch(r"[A-Za-z0-9._-]+", part) for part in relative.parts):
        raise Stop("path_parameter")
    return path


def profile(index, source, helper, scratch):
    if source != template(index):
        raise Stop("profile_scope")
    helper, scratch = owned_path(helper), owned_path(scratch)
    if helper.parents[0] not in scratch.parents and helper != PRIOR_HELPER:
        raise Stop("different_run")
    expanded = sorted(set(METADATA) | {str(p) for p in helper.parents} | {str(p) for p in scratch.parents})
    metadata = "(allow file-read-metadata file-test-existence\n"
    metadata += "".join("  (literal " + json.dumps(path) + ")\n" for path in expanded)
    metadata = metadata.rstrip("\n") + ")"
    result = source.replace("@@ANCESTOR_METADATA@@", metadata)
    return result, expanded if index >= 2 else []


def validate_prior():
    if sha(PRIOR / "artifact-index.json") != PRIOR_INDEX_HASH or sha(PRIOR / "input-hashes.json") != PRIOR_INPUT_HASH:
        raise Stop("prior_manifest_drift")
    indexed = json.loads((PRIOR / "artifact-index.json").read_text())
    actual_paths = {str(path) for path in PRIOR.rglob("*") if path.is_file()}
    if actual_paths != set(indexed) | {str(PRIOR / "artifact-index.json")}:
        raise Stop("prior_artifact_identity")
    guarded = {str(PRIOR / "artifact-index.json"): PRIOR_INDEX_HASH}
    for name, value in indexed.items():
        path = Path(name)
        if path.is_symlink() or not path.is_relative_to(PRIOR) or path.stat().st_size != value["bytes"] or sha(path) != value["sha256"]:
            raise Stop("prior_artifact_drift")
        guarded[name] = value["sha256"]
    inputs = json.loads((PRIOR / "input-hashes.json").read_text())
    repaired = {str(REPO / "tools/renderer/probe_r0d.py"), str(REPO / "tools/renderer/test_probe_r0d.py")}
    for name, value in inputs.items():
        retained = PRIOR / "inputs" / Path(name).name
        if sha(retained) != value:
            raise Stop("prior_input_drift")
        if name not in repaired and sha(Path(name)) != value:
            raise Stop("immutable_source_drift")
    for index, name in enumerate(PROFILE_NAMES):
        if (PRIOR / "inputs" / name).read_text() != template(index):
            raise Stop("prior_template_scope")
    if sha(PRIOR_HELPER) != PRIOR_BINARY_HASH or json.loads((PRIOR / "binary.json").read_text())["sha256"] != PRIOR_BINARY_HASH:
        raise Stop("prior_binary_drift")
    summary = json.loads((PRIOR / "summary.json").read_text())
    check_prior_summary(summary)
    for case, slot in zip(summary["cases"], ("D1-A", "D1-B", "D2-A")):
        retained = json.loads((PRIOR / slot / "case-result.json").read_text())
        if case != retained or case["process"] != json.loads((PRIOR / slot / "process-result.json").read_text()):
            raise Stop("prior_case_conflict")
        command = case["process"]["command"]
        if command[0] != "/usr/bin/sandbox-exec" or command[7] != str(PRIOR_HELPER):
            raise Stop("prior_launcher_identity")
    first_main = [(PRIOR / slot / "stdout.raw").read_bytes() for slot in ("D1-A", "D1-B", "D2-A")]
    if any(first_main) or (PRIOR / "D1-A/stderr.raw").stat().st_size or (PRIOR / "D1-B/stderr.raw").stat().st_size:
        raise Stop("prior_output_conflict")
    changed = classify(b"", (PRIOR / "D2-A/stderr.raw").read_bytes(), 71, 0, True, PRIOR_HELPER)
    if changed != "launcher_exec_permission":
        raise Stop("prior_launcher_diagnostic")
    return {"guardedArtifacts": guarded, "priorCounts": list(PRIOR_COUNTS),
            "priorTotal": 3, "priorD2AOriginalClassification": "profile_error",
            "priorD2AReclassified": changed, "originalInputHashes": inputs,
            "binarySha256": PRIOR_BINARY_HASH}


def check_prior_summary(summary):
    identities = [(case["profile"], case["repetition"], case["launchNumber"], case["process"]["pid"],
                   case["process"]["exit"]) for case in summary.get("cases", [])]
    if identities != [(1, "A", 1, 160, -6), (1, "B", 2, 161, -6), (2, "A", 3, 162, 71)]:
        raise Stop("prior_case_identity")
    if summary.get("launchCount") != 3 or summary.get("perProfileCount") != list(PRIOR_COUNTS):
        raise Stop("prior_count_conflict")
    if not summary.get("guardsUnchanged") or not summary.get("sourcesUnchanged"):
        raise Stop("prior_guard_failure")


def guard_snapshot():
    expected = {**AUTHORITY, **ORIGINAL_BINARIES}
    contract = ROOT / "evidence/contracts-frozen.json"
    old = json.loads(contract.read_text())
    for group in ("historicalFrozenFiles", "frozenImplementationContracts"):
        expected.update({REPO / name: digest for name, digest in old[group].items()})
    manifests = (ROOT / "evidence/contracts-frozen-renderer-r0d.json",
                 ROOT / "evidence/contracts-frozen-renderer-r0d-rulings.json",
                 ROOT / "evidence/contracts-frozen-renderer-r0d-continuation.json")
    ruled = json.loads(manifests[1].read_text())
    if ruled.get("approvedProfiles") != ["D1", "D2", "D3", "D4"] or ruled.get("maximumNativeLaunches") != 8:
        raise Stop("ruling_matrix")
    if ruled.get("rootDirectoryDataReadAuthorized") is not False or ruled.get("rendererCapabilityEnabled") is not False:
        raise Stop("ruling_scope")
    if ruled.get("proposalSha256") != AUTHORITY[ROOT / "specs/renderer-r0d-phase0.md"]:
        raise Stop("ruling_proposal")
    continued = json.loads(manifests[2].read_text())
    if (continued.get("priorReportSha256") != AUTHORITY[ROOT / "specs/renderer-r0d-results.md"]
            or continued.get("priorEvidence") != str(PRIOR) or continued.get("priorLaunches") != 3
            or continued.get("remainingSlots") != ["D2-B", "D3-A", "D3-B", "D4-A", "D4-B"]
            or continued.get("maximumAdditionalLaunches") != 5
            or continued.get("maximumAdditionalExecutionSeconds") != 570
            or continued.get("permissionsChanged") is not False
            or continued.get("rendererCapabilityEnabled") is not False):
        raise Stop("continuation_authority")
    for manifest in manifests:
        data = json.loads(manifest.read_text())
        for relative, digest in data["frozenImplementationContracts"].items():
            if expected.get(REPO / relative) != digest:
                raise Stop("frozen_manifest")
    # Preservation of every indexed R0 artifact, not just the final binary.
    for directory in sorted((ROOT / "renderer-evidence").glob("r0-*")):
        index = directory / "artifact-index.json"
        if not index.is_file():
            raise Stop("missing_r0_index")
        for item in json.loads(index.read_text())["files"]:
            path = Path(item["path"])
            if not path.is_relative_to(directory):
                raise Stop("r0_index_scope")
            expected[path] = item["sha256"]
        expected[index] = sha(index)
    final_r0 = ROOT / "renderer-evidence/r0-1791255085834627000/hashes.json"
    expected.update({Path(path): value for path, value in json.loads(final_r0.read_text()).items()})
    expected.update({Path(path): value for path, value in validate_prior()["guardedArtifacts"].items()})
    for path, digest in expected.items():
        if not path.is_file() or sha(path) != digest:
            raise Stop("guard_mismatch:" + str(path))
    for path in (contract, *manifests):
        expected[path] = sha(path)
    return {str(path): digest for path, digest in expected.items()}


class Budget:
    def __init__(self, clock=time.monotonic, prior_counts=PRIOR_COUNTS):
        if tuple(prior_counts) != PRIOR_COUNTS:
            raise Stop("prior_count_conflict")
        self.clock = clock
        self.start = clock()
        self.deadline = self.start + 570
        self.count = 3
        self.additional = 0
        self.per_profile = list(prior_counts)

    def remaining(self):
        return self.deadline - self.clock()

    def consume(self, index, repetition=None):
        if self.additional >= 5 or index not in range(4) or self.count >= 8 or self.per_profile[index] >= 2:
            raise Stop("launch_limit")
        expected = REMAINING_SLOTS[self.additional]
        if index != expected[0] or (repetition is not None and repetition != expected[1]):
            raise Stop("slot_replay_or_order")
        if self.remaining() < 35:
            raise Stop("execution_deadline")
        self.count += 1
        self.additional += 1
        self.per_profile[index] += 1
        return self.count


def preflight(path):
    info = path.lstat()
    if not stat.S_ISREG(info.st_mode) or info.st_uid != os.getuid() or stat.S_IMODE(info.st_mode) != 0o600:
        raise Stop("fixture_provenance")
    if path.read_bytes() != SENTINEL_BYTES:
        raise Stop("fixture_content")
    return {"path": str(path), "sha256": sha(path), "mode": stat.S_IMODE(info.st_mode),
            "inode": info.st_ino, "device": info.st_dev, "uid": info.st_uid}


class Canary:
    def __init__(self):
        self.sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self.sock.bind(("127.0.0.1", 0))
        self.sock.listen(4)
        self.sock.settimeout(0.05)
        self.port = self.sock.getsockname()[1]
        self.count = 0
        self.active = True
        self.thread = threading.Thread(target=self.accept, daemon=True)
        self.thread.start()

    def accept(self):
        while self.active:
            try:
                connection, _ = self.sock.accept()
            except socket.timeout:
                continue
            except OSError:
                return
            self.count += 1
            connection.close()

    def close(self):
        self.active = False
        self.sock.close()
        self.thread.join(timeout=0.2)
        if self.thread.is_alive():
            raise Stop("canary_cleanup")


def spawn(command, scratch, popen=None):
    popen = popen or subprocess.Popen
    return popen(command, cwd=scratch, env=environment(scratch), stdin=subprocess.PIPE,
                 stdout=subprocess.PIPE, stderr=subprocess.PIPE, close_fds=True, start_new_session=True)


def cleanup(process, events, getpgid=os.getpgid, killpg=os.killpg):
    if process.poll() is not None:
        events.append({"event": "already_reaped", "exit": process.returncode})
        return
    if getpgid(process.pid) != process.pid:
        raise Stop("ownership_uncertain")
    events.append({"event": "owned_sigterm", "pid": process.pid, "pgid": process.pid})
    killpg(process.pid, signal.SIGTERM)
    try:
        process.wait(timeout=0.25)
    except subprocess.TimeoutExpired:
        if process.poll() is None:
            if getpgid(process.pid) != process.pid:
                raise Stop("ownership_uncertain")
            events.append({"event": "owned_sigkill", "pid": process.pid, "pgid": process.pid})
            killpg(process.pid, signal.SIGKILL)
        process.wait(timeout=1)
    events.append({"event": "reaped", "exit": process.returncode})


def execute(folder, command, scratch, budget, ordinary_seconds, out_limit, err_limit):
    start = time.monotonic()
    end = min(start + ordinary_seconds, budget.deadline - 2)
    info = {"command": command, "cwd": str(scratch), "environmentNames": sorted(environment(scratch)),
            "startMonotonic": start, "ordinaryDeadline": end, "cleanupInclusiveLimitSeconds": ordinary_seconds + 2,
            "nativeCapability": False, "events": []}
    process = None
    drains = []
    files = []
    overflow = threading.Event()
    pipe_failure = threading.Event()
    try:
        if end <= start:
            raise Stop("execution_deadline")
        process = spawn(command, scratch)
        info["pid"] = process.pid
        info["pgid"] = os.getpgid(process.pid)
        if info["pgid"] != process.pid:
            raise Stop("spawn_ownership")
        process.stdin.close()
        def drain(pipe, destination, limit):
            try:
                retained = 0
                while True:
                    block = pipe.read(4096)
                    if not block:
                        return
                    take = min(len(block), max(0, limit - retained))
                    destination.write(block[:take])
                    retained += take
                    if take != len(block):
                        overflow.set()
            except Exception:
                pipe_failure.set()
            finally:
                pipe.close()
        for pipe, name, limit in [(process.stdout, "stdout.raw", out_limit), (process.stderr, "stderr.raw", err_limit)]:
            destination = (folder / name).open("xb")
            files.append(destination)
            thread = threading.Thread(target=drain, args=(pipe, destination, limit), daemon=True)
            drains.append(thread)
            thread.start()
        while process.poll() is None and time.monotonic() < end:
            if overflow.is_set() or pipe_failure.is_set():
                raise Stop("output_failure")
            time.sleep(0.01)
        info["timedOut"] = process.poll() is None
        if info["timedOut"]:
            cleanup(process, info["events"])
        info["exit"] = process.wait(timeout=0.2)
    except BaseException as error:
        info["errorCode"] = str(error) if isinstance(error, Stop) else type(error).__name__
    finally:
        try:
            if process is not None:
                cleanup(process, info["events"])
                info["exit"] = process.returncode
        except BaseException as error:
            info["cleanupFailure"] = str(error) if isinstance(error, Stop) else type(error).__name__
        for thread in drains:
            thread.join(timeout=0.3)
        info["pipeCleanup"] = not any(thread.is_alive() for thread in drains)
        if info["pipeCleanup"]:
            for destination in files:
                destination.close()
        else:
            info["cleanupFailure"] = "pipe_drain"
        info["outputOverflow"] = overflow.is_set()
        info["pipeFailure"] = pipe_failure.is_set()
        info["seconds"] = time.monotonic() - start
        save(folder / "process-result.json", info)
    if info.get("cleanupFailure") or info.get("errorCode") or info["outputOverflow"] or info["pipeFailure"]:
        raise Stop("process_failure")
    return info


def classify(raw, stderr, code, hits, verified, expected_helper=None):
    if hits:
        return "canary_hit"
    if not verified:
        return "fixture_unverified"
    if stderr:
        # Only exact diagnostics for this verified executable, with no helper
        # output and the retained launcher status, are eligible to continue.
        if expected_helper is not None and not raw and code == 71:
            prefix = b"sandbox-exec: execvp() of '" + str(expected_helper).encode() + b"' failed: "
            if stderr in (prefix + b"Operation not permitted\n", prefix + b"Permission denied\n"):
                return "launcher_exec_permission"
        if re.match(rb"^sandbox-exec: (?:syntax error\b|sandbox_compile:|failed to (?:load|read) profile\b|error in (?:profile|load)\b)", stderr):
            return "profile_syntax_load_error"
        return "unknown_diagnostic"
    if not raw:
        return "startup_abort" if code == -signal.SIGABRT else "missing_main"
    if not raw.startswith(MAIN_RECORD) or not raw.endswith(b"\n") or len(raw) > 4096:
        return "malformed_output"
    try:
        values = [json.loads(line) for line in raw.splitlines()]
    except (ValueError, UnicodeDecodeError):
        return "malformed_output"
    if len(values) != 5 or values[0] != {"protocol": 1, "event": "main"} or values[-1] != {"event": "done"}:
        return "malformed_output"
    for value, operation in zip(values[1:4], ("read", "write", "network")):
        if type(value) is not dict or set(value) != {"event", "operation", "stage", "allowed", "errno"}:
            return "malformed_output"
        if value["event"] != "probe" or value["operation"] != operation or type(value["allowed"]) is not bool or type(value["errno"]) is not int:
            return "malformed_output"
        if value["stage"] not in (("socket", "connect") if operation == "network" else ("open",)):
            return "misleading_errno"
        if value["allowed"]:
            return "permitted_access"
        if value["errno"] not in (errno.EPERM, errno.EACCES):
            return "misleading_errno"
    return "native_denials" if code == 0 else "incomplete_process"


def new_scratch(run, name):
    path = run / "scratch" / name
    path.mkdir(parents=True, mode=0o700)
    return path


def run_case(run, inputs, index, repetition, budget, used_ports):
    name = PROFILE_NAMES[index]
    folder = run / ("D" + str(index + 1) + "-" + repetition)
    folder.mkdir(mode=0o700)
    scratch = new_scratch(run, folder.name)
    text, literals = profile(index, (inputs / name).read_text(), PRIOR_HELPER, scratch)
    policy = folder / name
    policy.write_text(text)
    save(folder / "profile.json", {"sha256": sha(policy), "ancestorMetadataLiterals": literals,
                                  "rootDataRead": False, "machLookups": [],
                                  "templateSha256": sha(inputs / name)})
    sentinels = []
    for label in ("read", "write"):
        path = folder / (label + "-sentinel.txt")
        with path.open("xb") as stream:
            stream.write(SENTINEL_BYTES)
        os.chmod(path, 0o600)
        sentinels.append(path)
    fixtures = [preflight(path) for path in sentinels]
    save(folder / "fixtures-before.json", fixtures)
    canary = Canary()
    case_start = time.monotonic()
    case = {"slot": folder.name, "profile": index + 1, "repetition": repetition,
            "launchNumber": None, "additionalLaunchNumber": None, "canaryPort": canary.port,
            "canaryBound": "127.0.0.1", "fixturesVerified": True}
    try:
        if canary.port in used_ports:
            raise Stop("canary_port_reuse")
        used_ports.add(canary.port)
        number = budget.consume(index, repetition)
        case["launchNumber"] = number
        case["additionalLaunchNumber"] = budget.additional
        command = ["/usr/bin/sandbox-exec", "-f", str(policy),
                   "-D", "HELPER=" + str(PRIOR_HELPER), "-D", "SCRATCH=" + str(scratch),
                   str(PRIOR_HELPER), str(sentinels[0]), str(sentinels[1]), str(canary.port), str(number)]
        case["process"] = execute(folder, command, scratch, budget, 25, 4096, 16 * 1024)
    except BaseException as error:
        case["errorCode"] = str(error) if isinstance(error, Stop) else type(error).__name__
    finally:
        try:
            canary.close()
        except Stop as error:
            case["cleanupFailure"] = str(error)
        case["canaryHits"] = canary.count
        try:
            after = [preflight(path) for path in sentinels]
            case["fixturesUnchanged"] = after == fixtures
            save(folder / "fixtures-after.json", after)
        except Exception:
            case["fixturesUnchanged"] = False
        if (folder / "process-result.json").is_file():
            case["process"] = json.loads((folder / "process-result.json").read_text())
        raw = (folder / "stdout.raw").read_bytes() if (folder / "stdout.raw").exists() else b""
        err = (folder / "stderr.raw").read_bytes() if (folder / "stderr.raw").exists() else b""
        case["classification"] = classify(raw, err, case.get("process", {}).get("exit"),
                                          case["canaryHits"], case["fixturesUnchanged"], PRIOR_HELPER)
        case["mainMarkerObserved"] = raw.startswith(MAIN_RECORD)
        case["nativeDenialsObserved"] = case["classification"] == "native_denials"
        case["cleanupInclusiveSeconds"] = time.monotonic() - case_start
        if case["cleanupInclusiveSeconds"] > 30:
            case["cleanupFailure"] = "case_deadline"
        save(folder / "case-result.json", case)
    return case


def inventory(run):
    return {str(path): {"bytes": path.stat().st_size, "sha256": sha(path)}
            for path in sorted(run.rglob("*")) if path.is_file() and path.name != "artifact-index.json"}


def check_no_continuation_replay(run):
    for receipt in (ROOT / "renderer-evidence").glob("r0d-*/continuation-owner.json"):
        if receipt.parent != run and json.loads(receipt.read_text()).get("priorEvidence") == str(PRIOR):
            raise Stop("continuation_already_started")


def main():
    def interrupt(_number, _frame):
        raise Stop("controller_signal")
    signal.signal(signal.SIGTERM, interrupt)
    signal.signal(signal.SIGINT, interrupt)
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    # Validate immutable identities before creating any continuation run state.
    prior = validate_prior()
    initial_guards = guard_snapshot()
    run = ROOT / "renderer-evidence" / ("r0d-" + str(time.time_ns()))
    run.mkdir(mode=0o700)
    print("Evidence:", run, flush=True)
    save(run / "prior-validation.json", prior)
    save(run / "guards-before.json", initial_guards)
    sources = [REPO / "tools/renderer/probe_r0d.py", REPO / "tools/renderer/test_probe_r0d.py"]
    source_hashes = {str(path): sha(path) for path in sources}
    save(run / "repaired-source-hashes.json", source_hashes)
    save(run / "original-input-hashes.json", prior["originalInputHashes"])
    inputs = run / "inputs"
    inputs.mkdir(mode=0o700)
    for path in sources:
        if path.is_symlink():
            raise Stop("source_symlink")
        (inputs / path.name).write_bytes(path.read_bytes())
    for name in PROFILE_NAMES:
        (inputs / name).write_bytes((PRIOR / "inputs" / name).read_bytes())
    save(run / "binary.json", {"path": str(PRIOR_HELPER), "sha256": PRIOR_BINARY_HASH,
                               "reused": True, "compiled": False})
    summary = {"cases": [], "rendererCapability": False, "interactiveCapability": False,
               "priorLaunchCount": 3, "priorPerProfileCount": list(PRIOR_COUNTS),
               "maximumAdditionalLaunches": 5, "maximumCumulativeLaunches": 8,
               "reservedPriorExecutionSeconds": 30, "maximumAdditionalExecutionSeconds": 570,
               "stopReason": None}
    print("Waiting for shared build lock:", LOCK, flush=True)
    with LOCK.open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        budget = Budget()
        lock_info = os.fstat(lock.fileno())
        locked = {"path": str(LOCK), "inode": lock_info.st_ino, "device": lock_info.st_dev,
                  "acquiredTime": time.time(), "acquiredMonotonic": budget.start,
                  "budgetSeconds": 570}
        save(run / "lock.json", locked)
        try:
            if (lock_info.st_ino, lock_info.st_dev) != (20581567, 16777239):
                raise Stop("lock_identity")
            check_no_continuation_replay(run)
            save(run / "continuation-owner.json", {"priorEvidence": str(PRIOR), "controllerPid": os.getpid(),
                                                   "remainingSlots": ["D2-B", "D3-A", "D3-B", "D4-A", "D4-B"]})
            if guard_snapshot() != initial_guards or validate_prior() != prior:
                raise Stop("guard_drift")
            print("Acquired shared lock; 570-second continuation budget started.", flush=True)
            tests = run / "mocked-tests"
            tests.mkdir(mode=0o700)
            command = ["/usr/bin/python3", "-B", str(inputs / "test_probe_r0d.py")]
            tested = execute(tests, command, new_scratch(run, "tests"), budget, 28, 64 * 1024, 64 * 1024)
            summary["mockedTests"] = tested
            print("Mocked tests exit:", tested["exit"], flush=True)
            if tested["exit"] != 0 or tested["timedOut"]:
                raise Stop("mocked_tests_failed")
            old_summary = json.loads((PRIOR / "summary.json").read_text())
            used_ports = {case["canaryPort"] for case in old_summary["cases"]}
            for index, repetition in REMAINING_SLOTS:
                if {str(path): sha(path) for path in sources} != source_hashes or guard_snapshot() != initial_guards:
                    raise Stop("guard_drift")
                case = run_case(run, inputs, index, repetition, budget, used_ports)
                summary["cases"].append(case)
                save(run / "summary.json", summary)
                print("Case:", case["slot"], "exit:", case.get("process", {}).get("exit"),
                      "classification:", case["classification"], flush=True)
                if case.get("errorCode") or case.get("cleanupFailure") or case.get("process", {}).get("timedOut"):
                    raise Stop("case_failure")
                if case["classification"] not in ("startup_abort", "native_denials", "launcher_exec_permission"):
                    raise Stop(case["classification"])
                if repetition == "B" and len(summary["cases"]) >= 2:
                    pair = summary["cases"][-2:]
                    if all(c["profile"] == index + 1 and c["classification"] == "native_denials" for c in pair):
                        summary["stopReason"] = "two_native_observations"
                        break
            if summary["stopReason"] is None:
                summary["stopReason"] = "remaining_slots_exhausted"
        except BaseException as error:
            summary["stopReason"] = str(error) if isinstance(error, Stop) else type(error).__name__
        finally:
            summary["additionalLaunchCount"] = budget.additional
            summary["additionalPerProfileCount"] = [now - old for now, old in zip(budget.per_profile, PRIOR_COUNTS)]
            summary["cumulativeLaunchCount"] = budget.count
            summary["cumulativePerProfileCount"] = budget.per_profile
            try:
                after = guard_snapshot()
                save(run / "guards-after.json", after)
                summary["guardsUnchanged"] = after == initial_guards
                summary["repairedSourcesUnchanged"] = {str(path): sha(path) for path in sources} == source_hashes
                summary["priorEvidenceUnchanged"] = validate_prior() == prior
                if not all(summary[k] for k in ("guardsUnchanged", "repairedSourcesUnchanged", "priorEvidenceUnchanged")):
                    raise Stop("final_guard_drift")
            except BaseException as error:
                summary["guardFailure"] = str(error) if isinstance(error, Stop) else type(error).__name__
                summary["stopReason"] = "guard_failure"
            save(run / "summary.json", summary)
            save(run / "artifact-index.json", inventory(run))
            # Every owned process/canary is closed, and final guards/inventory are
            # complete before releasing the actual flock. Record both boundaries.
            locked["releaseRequestedMonotonic"] = time.monotonic()
            fcntl.flock(lock, fcntl.LOCK_UN)
            locked["releasedMonotonic"] = time.monotonic()
            locked["releasedTime"] = time.time()
            locked["executionThroughLockReleaseSeconds"] = locked["releasedMonotonic"] - budget.start
            summary["executionThroughLockReleaseSeconds"] = locked["executionThroughLockReleaseSeconds"]
            summary["deadlineExceeded"] = locked["releasedMonotonic"] > budget.deadline
            if summary["deadlineExceeded"]:
                summary["stopReason"] = "execution_deadline"
            save(run / "lock.json", locked)
            save(run / "summary.json", summary)
            save(run / "artifact-index.json", inventory(run))
            print("Finished:", run, "additional launches:", budget.additional,
                  "cumulative:", budget.count, "stop:", summary["stopReason"], flush=True)
    # This boundary is AFTER inventory serialization, final output and closing
    # the lock FD. The receipt itself is intentionally outside the inventory.
    completed = time.monotonic()
    save(run / "completion-receipt.json", {
        "finalAccountingCompletedMonotonic": completed,
        "afterLockThroughFinalAccountingSeconds": completed - budget.start,
        "reservedPriorSecondsPlusAccounting": 30 + completed - budget.start,
        "deadlineExceeded": completed > budget.deadline,
        "artifactIndexSha256": sha(run / "artifact-index.json"),
        "inventoryExcludes": ["artifact-index.json", "completion-receipt.json"],
        "measurementExcludesOnly": "this completion receipt serialization and function return",
        "additionalLaunchCount": budget.additional, "cumulativeLaunchCount": budget.count})
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
