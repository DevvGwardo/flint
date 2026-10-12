#!/usr/bin/env python3
"""Evaluate the real native loop with independent graders and bounded providers."""

import argparse
from collections import Counter
import hashlib
import html
import json
import math
import os
from pathlib import Path
import random
import resource
import signal
import shutil
import socket
import statistics
import subprocess
import sys
import threading
import time
import uuid

from orchestrator_cases import CASES, fixture_digest
from orchestrator_proxy import Budget, Proxy, local_url, omniroute_credentials

REPO = Path(__file__).resolve().parents[2]
VARIANTS = ("single", "delegated", "delegated-gemini-judge")
SANDBOX = Path("/usr/bin/sandbox-exec")
BUN = Path(shutil.which("bun") or "/missing-bun").resolve()
TYPESCRIPT_LIB = Path(os.environ.get("HOOTER_EVAL_TYPESCRIPT_LIB", "/missing-typescript")).resolve()


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2, allow_nan=False) + "\n")


def fresh_output(path):
    path = Path(path).absolute()
    if path.exists() or path.is_symlink() or any(parent.is_symlink() for parent in path.parents):
        raise ValueError("output must be a fresh path without symlink ancestors")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.mkdir(mode=0o700)
    return path.resolve()


def source_fingerprint():
    files = [REPO / "Cargo.toml", REPO / "Cargo.lock"]
    files.append(REPO / "crates/flint-agent/tests/fixtures/recovered-python-check.json")
    for root in ("crates/flint-agent", "crates/flint-platform"):
        files.extend(sorted((REPO / root).rglob("*.rs")))
        files.append(REPO / root / "Cargo.toml")
    files.extend(sorted((REPO / "tools/blueprint").glob("*orchestrator*.py")))
    digest = hashlib.sha256()
    for path in sorted(set(files)):
        if path.is_file():
            digest.update(str(path.relative_to(REPO)).encode() + b"\0" + path.read_bytes())
    return digest.hexdigest()

def runtime_identity():
    """Pin installed public runtimes; never install or expose the surrounding project."""
    if not BUN.is_file() or not (TYPESCRIPT_LIB / "tsc.js").is_file():
        raise ValueError("Bun and HOOTER_EVAL_TYPESCRIPT_LIB must point to installed runtimes")
    digest = hashlib.sha256()
    for path in sorted(TYPESCRIPT_LIB.rglob("*.js")) + sorted(TYPESCRIPT_LIB.rglob("*.ts")):
        digest.update(str(path.relative_to(TYPESCRIPT_LIB)).encode() + b"\0" + path.read_bytes())
    return {"bun_sha256": hashlib.sha256(BUN.read_bytes()).hexdigest(),
            "typescript_sha256": digest.hexdigest()}


def profile(read_paths, write_paths, port=None, directory_paths=()):
    """Stronger than the engine's command sandbox: no host-home reads/network."""
    def quoted(path):
        return '"' + str(Path(path).resolve()).replace("\\", "\\\\").replace('"', '\\"') + '"'
    common = ["/System", "/usr", "/bin", "/sbin", "/Library/Developer",
              "/Library/Frameworks", "/opt/homebrew", "/dev", "/private/etc",
              "/private/var/db", "/private/var/run"]
    reads = "".join(f"(subpath {quoted(path)})" for path in common + list(read_paths))
    writes = "".join(f"(subpath {quoted(path)})" for path in write_paths)
    network = f'(allow network-outbound (remote ip "localhost:{port}"))' if port else ""
    # Bun's package discovery lists ancestor directories. Literal rules grant
    # directory names only, never descendant file contents or user-home reads.
    directories = "".join(f"(literal {quoted(path)})" for path in directory_paths
                          if not Path(path).resolve().is_relative_to(Path.home()))
    return ("(version 1)(allow default)(deny file-read-data)(deny file-write*)"
            f"(allow file-read-data {reads}{directories})(allow file-write* {writes})"
            # macOS process initialization reads the root directory itself.
            # A literal allowance does not grant reads of its descendants.
            "(allow file-read-data (literal \"/\"))"
            "(allow file-write* (subpath \"/dev\"))"
            f"(deny network*){network}")


def clean_environment(home, tmp):
    return {"HOME": str(home), "TMPDIR": str(tmp), "LANG": "en_US.UTF-8",
            "PATH": f"{Path(sys.executable).resolve().parent}:{BUN.parent}:/opt/homebrew/bin:"
                    "/usr/bin:/bin:/usr/sbin:/sbin",
            "PYTHONDONTWRITEBYTECODE": "1", "PYTHONHASHSEED": "0",
            "FLINT_HOME": str(home / ".flint")}


def process_limits():
    resource.setrlimit(resource.RLIMIT_CPU, (60, 65))
    resource.setrlimit(resource.RLIMIT_FSIZE, (16_000_000, 16_000_000))
    resource.setrlimit(resource.RLIMIT_NOFILE, (256, 256))


def process_tree(root):
    listing = subprocess.run(["/bin/ps", "-axo", "pid=,ppid=,lstart="],
                             capture_output=True, text=True, timeout=3)
    rows = {}
    for line in listing.stdout.splitlines():
        values = line.split(None, 2)
        if len(values) == 3:
            rows[int(values[0])] = (int(values[1]), values[2])
    children, frontier = {}, {root}
    while frontier:
        found = {pid for pid, (parent, _) in rows.items()
                 if parent in frontier and pid not in children and pid != root}
        children.update({pid: rows[pid][1] for pid in found})
        frontier = found
    return children, rows


def stop_process(process, children):
    # Native commands use separate process groups. Stop the observed subtree,
    # not just the bridge's group, and refuse reused PIDs.
    try:
        current_children, rows = process_tree(process.pid)
        children.update(current_children)
    except (subprocess.SubprocessError, OSError):
        rows = {}
    for sig in (signal.SIGTERM, signal.SIGKILL):
        for pid, started in children.items():
            if pid in rows and rows[pid][1] == started:
                try:
                    os.kill(pid, sig)
                except ProcessLookupError:
                    pass
        if process.poll() is None:
            try:
                os.killpg(process.pid, sig)
            except ProcessLookupError:
                pass
        if sig == signal.SIGTERM:
            time.sleep(.1)
    process.wait(timeout=5)


def execute(command, cwd, environment, input_text, stdout, stderr, seconds):
    started = time.monotonic()
    children = {}
    overflow = threading.Event()
    def drain(pipe, destination):
        size = 0
        try:
            while chunk := pipe.read(8192):
                size += len(chunk)
                if size > 16_000_000:
                    overflow.set()
                    break
                destination.write(chunk)
        except (OSError, ValueError):
            overflow.set()
    with stdout.open("wb") as out, stderr.open("wb") as err:
        process = subprocess.Popen(command, cwd=cwd, env=environment, stdin=subprocess.PIPE,
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE, start_new_session=True,
                                   preexec_fn=process_limits)
        readers = [threading.Thread(target=drain, args=pair, daemon=True)
                   for pair in ((process.stdout, out), (process.stderr, err))]
        for reader in readers:
            reader.start()
        try:
            process.stdin.write(input_text.encode())
            process.stdin.close()
            while (process.poll() is None and not overflow.is_set() and
                   time.monotonic() - started < seconds):
                observed, _ = process_tree(process.pid)
                children.update(observed)
                time.sleep(.1)
            timed_out = process.poll() is None
        finally:
            stop_process(process, children)
            for reader in readers:
                reader.join(timeout=2)
            process.stdout.close()
            process.stderr.close()
    return {"exit_code": process.returncode, "timed_out": timed_out,
            "output_limit_exceeded": overflow.is_set(), "seconds": time.monotonic() - started}


def containment_check(out):
    if sys.platform != "darwin" or not SANDBOX.is_file():
        raise ValueError("this harness requires macOS sandbox-exec; no unsandboxed fallback")
    root = out / "containment"
    root.mkdir()
    workspace, home, tmp = [root / name for name in ("workspace", "home", "tmp")]
    for path in (workspace, home, tmp):
        path.mkdir()
    canary = root / "private-canary"
    canary.write_text("synthetic canary, not a user secret")
    listener = socket.socket()
    listener.bind(("127.0.0.1", 0))
    listener.listen()
    port = listener.getsockname()[1]
    approved = socket.socket()
    approved.bind(("127.0.0.1", 0))
    approved.listen()
    approved_port = approved.getsockname()[1]
    script = f"""
import json, pathlib, socket
checks = {{}}
try: pathlib.Path({str(canary)!r}).read_text(); checks['host_read_denied']=False
except PermissionError: checks['host_read_denied']=True
try: pathlib.Path({str(canary)!r}).write_text('changed'); checks['host_write_denied']=False
except PermissionError: checks['host_write_denied']=True
try:
 s=socket.create_connection(('127.0.0.1',{port}),timeout=.5)
 s.close(); checks['unapproved_network_denied']=False
except (PermissionError, OSError): checks['unapproved_network_denied']=True
s=socket.create_connection(('127.0.0.1',{approved_port}),timeout=.5)
s.close(); checks['approved_proxy_network_allowed']=True
pathlib.Path('allowed.txt').write_text('ok')
checks['workspace_write_allowed']=True
print(json.dumps(checks))
"""
    try:
        result = execute(
            [str(SANDBOX), "-p", profile([workspace, home, tmp], [workspace, home, tmp], approved_port,
                                       directory_paths=workspace.parents),
             sys.executable, "-I", "-"],
            workspace, clean_environment(home, tmp), script,
            root / "stdout.txt", root / "stderr.txt", 5)
    finally:
        listener.close()
        approved.close()
    try:
        checks = json.loads((root / "stdout.txt").read_text())
    except ValueError:
        checks = {}
    return {"passed": result["exit_code"] == 0 and len(checks) == 5 and all(checks.values()),
            "checks": checks, "process": result}


def flatten_events(events):
    for event in events:
        if not isinstance(event, dict) or len(event) != 1:
            continue
        name, value = next(iter(event.items()))
        yield "parent", name, value
        if name == "SubagentEvent":
            for _, child_name, child_value in flatten_events([value["event"]]):
                yield value["call_id"], child_name, child_value


def event_metrics(events):
    counts = Counter()
    commands, starts, task, reason, usage, text = [], {}, None, None, None, []
    for scope, name, value in flatten_events(events):
        counts[name] += 1
        if name == "ToolCallStarted":
            starts[(scope, value["call_id"])] = value
        elif name == "ToolCallFinished":
            start = starts.get((scope, value["call_id"]), {})
            if start.get("name") == "run_command":
                commands.append({"scope": scope, "command": start.get("args", {}).get("command"),
                                 "exit_code": value.get("exit_code"),
                                 "success": value.get("success")})
        elif scope == "parent":
            if name == "TextDelta":
                text.append(value if isinstance(value, str) else value.get("text", ""))
            if name == "TaskAssessed":
                task = value["task"]
            elif name == "TurnFinished":
                reason = value["reason"]
            elif name == "Usage":
                usage = value  # Cumulative parent usage already contains child usage.
    assessment = (task or {}).get("assessment") or {}
    current_commands = [check.get("command") for check in (task or {}).get("checks", [])
                  if check.get("verification") and check.get("success") is True and
                  check.get("exit_code") == 0 and
                  check.get("started_revision") == check.get("work_revision") and
                  check.get("work_revision") == task.get("work_revision") and
                  check.get("contract_revision") == task.get("contract", {}).get("revision")]
    return {"turn_reason": reason, "outcome": assessment.get("outcome"),
            "current_verification": bool(current_commands),
            "current_commands": current_commands,
            "uncredited_commands": [check.get("command") for check in (task or {}).get("checks", [])
                                    if not check.get("verification")],
            "mode": (task or {}).get("contract", {}).get("mode"),
            "observed_edits": (task or {}).get("observed_edits", 0),
            "final_text": "".join(text),
            "issues": assessment.get("issues", []),
            "accepted": (task or {}).get("acceptance") is not None,
            "blockers": (task or {}).get("blockers", []),
            "usage": usage, "counts": dict(counts), "commands": commands}


def integrity(case, workspace):
    if workspace.is_symlink():
        return False
    for name, expected in case.protected().items():
        path = workspace / name
        if path.is_symlink() or not path.is_file() or hashlib.sha256(path.read_bytes()).hexdigest() != expected:
            return False
    return True


def grade(case, workspace, directory):
    protected = integrity(case, workspace)
    if case.analysis:
        return {"passed": protected and {p.name for p in workspace.iterdir()} == set(case.files),
                "protected_files_intact": protected, "tests_run": None, "kind": "analysis-no-writes"}
    if case.runtime == "bun":
        hidden = directory / "hidden.test.ts"
        hidden.write_text("import {test,expect} from 'bun:test';\n"
                          f"import {{label}} from {json.dumps(str(workspace / 'labels.ts'))};\n"
                          + case.hidden)
        home, tmp = directory / "home", directory / "tmp"
        home.mkdir()
        tmp.mkdir()
        policy = profile([workspace, directory, BUN, TYPESCRIPT_LIB], [home, tmp],
                         directory_paths=workspace.parents)
        env = clean_environment(home, tmp)
        checked = execute([str(SANDBOX), "-p", policy, str(BUN),
                           str(TYPESCRIPT_LIB / "tsc.js"), "--noEmit"],
                          workspace, env, "", directory / "types.out", directory / "types.err", 15)
        tested = execute([str(SANDBOX), "-p", policy, str(BUN), "test",
                          str(workspace / "labels.test.ts"), str(hidden)],
                         workspace, env, "", directory / "stdout.txt", directory / "stderr.txt", 15)
        import re
        output = (directory / "stdout.txt").read_text() + (directory / "stderr.txt").read_text()
        counts = re.findall(r"Ran (\d+) tests", output)
        tests = int(counts[-1]) if counts else 0
        return {"passed": protected and checked["exit_code"] == tested["exit_code"] == 0
                and not checked["timed_out"] and not tested["timed_out"]
                and not checked["output_limit_exceeded"] and not tested["output_limit_exceeded"]
                and tests >= case.min_tests,
                "protected_files_intact": protected, "tests_run": tests,
                "process": tested, "typecheck_process": checked,
                "grader_sha256": hashlib.sha256(hidden.read_bytes()).hexdigest()}
    if case.blocked:
        expected_files = set(case.files)
        observed = {path.name for path in workspace.iterdir()}
        return {"passed": protected and observed == expected_files,
                "protected_files_intact": protected, "tests_run": None,
                "kind": "missing-dependency-no-output"}
    grader = directory / "hidden.py"
    grader.write_text(case.grader(workspace))
    home, tmp = directory / "home", directory / "tmp"
    home.mkdir()
    tmp.mkdir()
    result = execute(
        [str(SANDBOX), "-p", profile([workspace, grader, home, tmp], [home, tmp]),
         sys.executable, "-I", str(grader)],
        workspace, clean_environment(home, tmp), "",
        directory / "stdout.txt", directory / "stderr.txt", 10)
    payload = None
    try:
        payload = json.loads((directory / "stdout.txt").read_text().splitlines()[-1])
    except (ValueError, IndexError):
        pass
    passed = (protected and result["exit_code"] == 0 and not result["timed_out"] and
              not result["output_limit_exceeded"] and
              isinstance(payload, dict) and payload.get("grader") == "hooter-v1" and
              payload.get("successful") is True and
              payload.get("tests_run", 0) >= case.min_tests and
              payload.get("failures") == 0 and payload.get("errors") == 0)
    return {"passed": passed, "protected_files_intact": protected,
            "tests_run": payload.get("tests_run") if isinstance(payload, dict) else None,
            "process": result, "grader_sha256": hashlib.sha256(grader.read_bytes()).hexdigest()}


def score(case, metrics, grader, process):
    orderly = (process["exit_code"] == 0 and not process["timed_out"] and
               not process.get("output_limit_exceeded") and
               metrics["counts"].get("SessionStopped", 0) == 1 and
               metrics["turn_reason"] == "Completed")
    if case.analysis:
        text = metrics.get("final_text", "").lower()
        correct = (metrics.get("mode") == "read_only" and metrics["outcome"] == "needs_review"
                   and metrics.get("observed_edits") == 0
                   and all(word in text for word in ("trim", "upper", "test")))
    elif case.blocked:
        correct = metrics["outcome"] == "blocked" and any(
            "source.bin" in json.dumps(blocker).lower() for blocker in metrics["blockers"])
    else:
        correct = metrics["current_verification"] and metrics["outcome"] == "needs_review"
    recovery = True
    if case.name == "check-recovery":
        checks = [entry for entry in metrics["commands"]
                  if entry["command"] and "check.py" in entry["command"]]
        recovery = (len(checks) >= 2 and checks[0]["exit_code"] == 23 and
                    any(entry["exit_code"] == 0 for entry in checks[1:]))
    if case.runtime == "bun":
        commands = [entry for entry in metrics["commands"] if entry["scope"] == "parent"]
        correct &= all(command in metrics.get("current_commands", [])
                       for command in ("bun run typecheck", "bun test"))
        if case.name == "bun-mask-recovery":
            bare = [entry for entry in commands if entry["command"] == "bun test"]
            recovery = (any("| tail" in entry["command"] and entry["exit_code"] == 0
                            for entry in commands) and len(bare) >= 2
                        and bare[0]["exit_code"] != 0 and bare[-1]["exit_code"] == 0
                        and "bun test 2>&1 | tail -5" in metrics.get("uncredited_commands", []))
    passed = orderly and correct and grader["passed"] and recovery and not metrics["accepted"]
    # A verified host assessment contradicted by independent artifact checks,
    # not merely a normally terminated transport turn.
    false_completion = (not case.blocked and not case.analysis and metrics["outcome"] == "needs_review" and
                        not grader["passed"])
    return {"passed": passed, "orderly_shutdown": orderly, "expected_outcome": correct,
            "recovery_observed": recovery, "false_completion": false_completion}


def run_case(case, variant, trial, out, bridge, budget, args, credentials):
    run_id = f"{case.name}-{trial}-{variant}"
    directory = out / "runs" / run_id
    directory.mkdir(parents=True)
    workspace, home, tmp = [directory / name for name in ("workspace", "home", "tmp")]
    case.seed(workspace)
    runtime_reads = []
    if case.runtime == "bun":
        (workspace / ".eval-runtime").symlink_to(TYPESCRIPT_LIB, target_is_directory=True)
        runtime_reads = [BUN, TYPESCRIPT_LIB]
    home.mkdir()
    tmp.mkdir()
    proxy = Proxy(case, variant, budget, call_limit=args.run_calls,
                  token_limit=args.run_tokens, seconds=args.run_seconds,
                  output_limit=args.reply_tokens,
                  upstream=args.gateway if args.live else None,
                  api_key=credentials[0] if credentials else None,
                  connection=credentials[1] if credentials else None,
                  gateway_database=args.gateway_db if args.live else None,
                  gateway_key_name=args.gateway_key_name if args.live else None)
    proxy.model = args.model if args.live else "offline-scripted"
    spec = {"workspace": str(workspace), "base_url": proxy.base_url,
            "model": proxy.model, "proxy_key": proxy.key, "prompt": case.prompt,
            "allow_subagents": variant != "single",
            "judge": variant == "delegated-gemini-judge", "timeout_secs": args.run_seconds,
            "denied_read_canary": str(out / "containment/private-canary")}
    try:
        process = execute(
            [str(SANDBOX), "-p",
             profile([bridge, workspace, home, tmp] + runtime_reads, [workspace, home, tmp],
                     proxy.server.server_port,
                     directory_paths=workspace.parents if runtime_reads else ()), str(bridge)],
            workspace, clean_environment(home, tmp), json.dumps(spec) + "\n",
            directory / "events.jsonl", directory / "stderr.txt", args.run_seconds + 8)
    finally:
        proxy.close()
    events, parse_errors = [], 0
    for line in (directory / "events.jsonl").read_text().splitlines():
        try:
            events.append(json.loads(line))
        except ValueError:
            parse_errors += 1
    metrics = event_metrics(events)
    grader_dir = out / "verifiers" / run_id
    grader_dir.mkdir(parents=True)
    grader = grade(case, workspace, grader_dir)
    result = {"id": run_id, "task": case.name, "variant": variant, "trial": trial,
              "process": process, "metrics": metrics, "grader": grader,
              "score": score(case, metrics, grader, process), "proxy": proxy.snapshot(),
              "parse_errors": parse_errors, "events": str(directory / "events.jsonl")}
    if parse_errors or result["proxy"]["budget"]["token_bound_breached"]:
        result["score"]["passed"] = False
    if variant == "single" and metrics["counts"].get("SubagentStarted", 0):
        result["score"]["passed"] = False
    result["delegation_observed"] = metrics["counts"].get("SubagentStarted", 0)
    write_json(directory / "result.json", result)
    return result


def percentile(values, fraction):
    return sorted(values)[max(0, math.ceil(len(values) * fraction) - 1)] if values else None


def resume_results(path, args, bridge_hash):
    """Retain executed failures too; only budget-denied/no-response runs retry."""
    previous = json.loads(Path(path).resolve(strict=True).read_text())
    expected = {
        "live": args.live, "model": args.model if args.live else "offline-scripted",
        "fixture_sha256": fixture_digest(), "bridge_sha256": bridge_hash,
        "seed": args.seed, "python_executable": str(Path(sys.executable).resolve()),
    }
    if any(previous.get(key) != value for key, value in expected.items()):
        raise ValueError("resume requires identical model, fixtures, bridge, seed and Python")
    if previous.get("runtime_receipt") and previous["runtime_receipt"] != runtime_identity():
        raise ValueError("resume refuses changed Bun or TypeScript runtimes")
    for name in ("run_calls", "run_tokens", "run_seconds", "reply_tokens"):
        if previous["budget_limits"].get(name) != getattr(args, name):
            raise ValueError("resume cannot change per-scenario budgets")
    if not any(check["name"] == "Relevant source stayed unchanged" and check["passed"]
               for check in previous["checks"]):
        raise ValueError("cannot resume evidence with source drift")
    budget = previous["budget"]
    if budget["pending_reserved_tokens"] or budget["token_bound_breached"] or budget["safety_halted"]:
        raise ValueError("cannot resume a provider safety halt or pending reservations")
    # Controller changes are disclosed, never presented as identical source.
    # The actual executed bridge and frozen independent fixtures must match.
    rows = [row for row in previous["results"] if not args.live or any(
        request["kind"] == "agent" and request["status"] == "ok"
        for request in row["proxy"]["requests"])]
    ids = {f"{case.name}-{trial}-{variant}" for case in CASES
           if case.name in args.tasks.split(",") for trial in range(args.trials)
           for variant in args.variants.split(",")}
    if (previous.get("planned_scenarios") != len(ids) or
            len({row["id"] for row in rows}) != len(rows) or
            any(row["id"] not in ids or row["id"] !=
                f"{row['task']}-{row['trial']}-{row['variant']}" for row in rows)):
        raise ValueError("resume requires the same scenario set without duplicate IDs")
    if args.live and (previous["gateway"]["url"] != args.gateway or
                      previous["gateway"]["key_name"] != args.gateway_key_name):
        raise ValueError("resume requires the same gateway and client identity")
    return previous, rows


def summarize(results):
    summary = {}
    for variant in VARIANTS:
        rows = [row for row in results if row["variant"] == variant]
        delivered = [row for row in rows if row["task"] not in ("blocked-dependency", "analysis-gaps")]
        times = [row["process"]["seconds"] for row in rows]
        summary[variant] = {
            "scenario_passed": sum(row["score"]["passed"] for row in rows), "scenarios": len(rows),
            "deliverables_verified": sum(row["score"]["passed"] for row in delivered),
            "artifacts_passed": sum(row["grader"]["passed"] for row in delivered),
            "artifacts_held_by_host": sum(row["grader"]["passed"] and
                row["metrics"]["outcome"] != "needs_review" for row in delivered),
            "deliverable_tasks": len(delivered),
            "false_completions": sum(row["score"]["false_completion"] for row in rows),
            "provider_calls": sum(len(row["proxy"]["requests"]) for row in rows),
            "judge_calls": sum(request["kind"] == "gemini-judge" for row in rows
                               for request in row["proxy"]["requests"]),
            "judge_failures": sum(request["kind"] == "gemini-judge" and request["status"] != "ok"
                                  for row in rows for request in row["proxy"]["requests"]),
            "nudges": sum(row["metrics"]["counts"].get("HarnessNudge", 0) for row in rows),
            "approval_requests": sum(row["metrics"]["counts"].get("ApprovalRequested", 0) for row in rows),
            "subagents": sum(row["delegation_observed"] for row in rows),
            "runs_without_provider_execution": sum(not row["proxy"]["requests"] for row in rows),
            "median_seconds": statistics.median(times) if times else None,
            "p95_seconds": percentile(times, .95),
        }
    return summary


def render_report(report, out):
    title = "Hooter orchestrator evaluation"
    rows = []
    for row in report["results"]:
        link = (Path(row["events"]).parent / "result.json").as_uri()
        rows.append("<tr>" + "".join(f"<td>{html.escape(str(value))}</td>" for value in
                    (row["task"], row["variant"], row["trial"],
                     "PASS" if row["score"]["passed"] else "FAIL",
                     row["metrics"]["outcome"], row["grader"]["tests_run"]
                     if row["grader"]["tests_run"] is not None else "n/a (no artifact requested)",
                     len(row["proxy"]["requests"]), round(row["process"]["seconds"], 2))) +
                    f'<td><a href="{html.escape(link)}">Raw evidence</a></td></tr>')
    counts = sum(row["score"]["passed"] for row in report["results"])
    mode = "Live Antigravity Gemini" if report["live"] else "Offline scripted providers"
    checks = "".join(
        f"<tr><td>{html.escape(check['name'])}</td>"
        f"<td class=\"{'pass' if check['passed'] else 'fail'}\">"
        f"{'PASS' if check['passed'] else 'FAIL'}</td></tr>" for check in report["checks"])
    summary = []
    for variant, values in report["summary"].items():
        summary.append("<tr>" + "".join(f"<td>{html.escape(str(value))}</td>" for value in (
            variant, f"{values['scenario_passed']}/{values['scenarios']}",
            f"{values['deliverables_verified']}/{values['deliverable_tasks']}",
            values["artifacts_passed"], values["artifacts_held_by_host"],
            values["false_completions"], values["provider_calls"], values["judge_calls"],
            round(values["median_seconds"], 2) if values["median_seconds"] is not None else "n/a"
        )) + "</tr>")
    page = f"""<!doctype html><html lang="en"><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>{title}</title><style>
:root{{color-scheme:light dark;--pass:#147547;--fail:#a52323}}
@media(prefers-color-scheme:dark){{:root{{--pass:#64d49a;--fail:#ff9c9c}}}}
body{{font:16px/1.6 system-ui,sans-serif;margin:40px auto;
max-width:1200px;padding:0 24px}}h1{{line-height:1.2}}small{{overflow-wrap:anywhere}}
table{{border-collapse:collapse;width:100%;font-size:13px}}td,th{{text-align:left;padding:10px;
border-bottom:1px solid #8886}}.note{{padding:20px;border:1px solid #8886;border-radius:8px}}
pre{{white-space:pre-wrap;overflow-wrap:anywhere}}a{{color:#3589d6}}
.pass{{color:var(--pass);font-weight:700}}.fail{{color:var(--fail);font-weight:700}}
summary{{cursor:pointer}}.table{{overflow:auto}}
</style><h1>{title}</h1><p>{mode} · {counts}/{len(report['results'])} scenarios passed</p>
<div class="note"><strong>Evidence limits</strong><p>Offline passes prove the harness and native
tool/event paths, not model quality. Correctly reporting a missing dependency is a scenario pass,
not a shipped deliverable. The Gemini judge is experimental, not Typesafe JEV. Neither a
Completed turn nor a judge verdict is acceptance. Exact billed cost is not inferred from token counts.
This small pilot is not statistically conclusive.</p></div>
<h2>Checks</h2><table><thead><tr><th>Promise</th><th>Result</th></tr></thead>
<tbody>{checks}</tbody></table>
<details><summary>Raw check details</summary><pre>
{html.escape(json.dumps(report['checks'],indent=2))}</pre></details>
<h2>Comparison</h2><div class="table"><table><thead><tr>
<th>Variant</th><th>Scenarios</th><th>Verified deliverables</th>
<th>Artifacts passed</th><th>Host-held artifacts</th><th>False assessments</th>
<th>Calls</th><th>Judge calls</th><th>Median seconds</th></tr></thead>
<tbody>{''.join(summary)}</tbody></table></div>
<h2>Scenarios</h2><div style="overflow:auto"><table><thead><tr>
<th>Task</th><th>Variant</th><th>Trial</th><th>Result</th><th>Host outcome</th>
<th>Tests</th><th>Calls</th><th>Seconds</th><th>Evidence</th></tr></thead>
<tbody>{''.join(rows)}</tbody></table></div>
<h2>Provenance</h2><small>Source {report['source_sha256']}<br>
Fixtures {report['fixture_sha256']}<br>Bridge {report['bridge_sha256']}</small>
<details><summary>Batch provenance and cumulative budget</summary><pre>
{html.escape(json.dumps({key: report.get(key) for key in
 ('resumed_from', 'additional_prior_spending', 'budget')}, indent=2))}</pre></details>
<p><a href="report.json">Full machine-readable report</a></p></html>"""
    (out / "report.html").write_text(page)


def parser():
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("--live", action="store_true", help="explicit opt-in to subscription model calls")
    cli.add_argument("--gateway", default="http://127.0.0.1:20128/v1", type=local_url)
    cli.add_argument("--model", default="antigravity/gemini-3.8-flash-tiered")
    cli.add_argument("--gateway-db", type=Path, default=Path.home() / ".omniroute/storage.sqlite")
    cli.add_argument("--gateway-key-name", default="hermes-agent")
    cli.add_argument("--bridge", type=Path, default=REPO / "target/debug/examples/orchestrator_eval")
    cli.add_argument("--out", type=Path)
    cli.add_argument("--tasks", default=",".join(case.name for case in CASES))
    cli.add_argument("--variants", default=",".join(VARIANTS))
    cli.add_argument("--trials", type=int, default=2)
    cli.add_argument("--seed", type=int, default=1729)
    cli.add_argument("--max-calls", type=int, default=300)
    cli.add_argument("--max-tokens", type=int, default=1_000_000)
    cli.add_argument("--max-seconds", type=int, default=1800)
    cli.add_argument("--run-calls", type=int, default=24)
    cli.add_argument("--run-tokens", type=int, default=160_000)
    cli.add_argument("--run-seconds", type=int, default=180)
    cli.add_argument("--reply-tokens", type=int, default=2048)
    cli.add_argument("--resume", type=Path, help="retain executed scenarios from a frozen prior report")
    cli.add_argument("--spent-calls", type=int, default=0, help="additional earlier calls to debit")
    cli.add_argument("--spent-tokens", type=int, default=0, help="additional earlier tokens to debit")
    return cli


def main(argv=None):
    args = parser().parse_args(argv)
    task_names, variants = args.tasks.split(","), args.variants.split(",")
    if (not 1 <= args.trials <= 10 or len(set(task_names)) != len(task_names) or
            len(set(variants)) != len(variants) or
            not set(task_names) <= {case.name for case in CASES} or
            not set(variants) <= set(VARIANTS)):
        raise ValueError("invalid or duplicate tasks, variants or trials")
    for name in ("max_calls", "max_tokens", "max_seconds", "run_calls", "run_tokens",
                 "run_seconds", "reply_tokens"):
        if getattr(args, name) <= 0:
            raise ValueError("budgets must be positive")
    if args.run_seconds > 600 or args.reply_tokens > 8192:
        raise ValueError("run deadline/reply limit exceeds the harness safety cap")
    if args.spent_calls < 0 or args.spent_tokens < 0:
        raise ValueError("earlier spending cannot be negative")
    bridge = args.bridge.resolve(strict=True)
    runtime_receipt = None
    if any(case.runtime == "bun" and case.name in task_names for case in CASES):
        runtime_receipt = runtime_identity()
    out = fresh_output(args.out or REPO / "blueprint-out" /
                       f"orchestrator-{int(time.time())}-{uuid.uuid4().hex[:8]}")
    frozen = source_fingerprint()
    report = {"version": 1, "live": args.live, "model": args.model if args.live else "offline-scripted",
              "judge": "experimental Gemini adapter, not Typesafe JEV",
              "source_sha256": frozen, "fixture_sha256": fixture_digest(),
              "bridge_sha256": hashlib.sha256(bridge.read_bytes()).hexdigest(),
              "sandbox": "verified supervisor OS containment; no nested sandbox-exec",
              "python_executable": str(Path(sys.executable).resolve()),
              "approval": "Auto only within disposable, contained synthetic workspaces",
              "seed": args.seed, "budget_limits": {name: getattr(args, name) for name in
               ("max_calls", "max_tokens", "max_seconds", "run_calls", "run_tokens",
                "run_seconds", "reply_tokens")}, "results": [], "checks": [], "summary": {}}
    report["runtime_receipt"] = runtime_receipt
    previous = None
    spent_calls, spent_tokens = args.spent_calls, args.spent_tokens
    if args.resume:
        previous, report["results"] = resume_results(args.resume, args, report["bridge_sha256"])
        spent_calls += previous["budget"]["calls"]
        spent_tokens += previous["budget"]["charged_tokens"]
        report["resumed_from"] = {
            "report": str(args.resume.resolve()), "source_sha256": previous["source_sha256"],
            "retained_scenarios": len(report["results"]),
            "note": "Controller versions may differ; compiled bridge, fixtures and run caps match. "
                    "Executed failures are retained, not retried."}
    report["additional_prior_spending"] = {"calls": args.spent_calls, "tokens": args.spent_tokens}
    write_json(out / "manifest.json", report)
    budget = Budget(args.max_calls, args.max_tokens, args.max_seconds, spent_calls, spent_tokens)
    try:
        containment = containment_check(out)
        report["checks"].append({"name": "Host files and unapproved network are inaccessible",
                                 "passed": containment["passed"], "detail": containment})
        if not containment["passed"]:
            raise ValueError("containment preflight failed")
        credentials = omniroute_credentials(
            args.gateway_db, args.gateway_key_name, args.model) if args.live else None
        if credentials:
            report["gateway"] = {"url": args.gateway, "key_name": args.gateway_key_name,
                                 "connection_sha256": hashlib.sha256(credentials[1].encode()).hexdigest()}
            if previous and report["gateway"] != previous["gateway"]:
                raise ValueError("resume refuses a changed pinned subscription connection")
            write_json(out / "manifest.json", report)
        # Randomize cases and rotate variant order within each paired trial.
        rng = random.Random(args.seed)
        jobs = [(case, trial) for case in CASES if case.name in task_names
                for trial in range(args.trials)]
        rng.shuffle(jobs)
        expected = len(jobs) * len(variants)
        report["planned_scenarios"] = expected
        completed = {row["id"] for row in report["results"]}
        for case, trial in jobs:
            order = list(variants)
            rng.shuffle(order)
            for variant in order:
                if f"{case.name}-{trial}-{variant}" in completed:
                    continue
                result = run_case(case, variant, trial, out, bridge, budget, args, credentials)
                report["results"].append(result)
                print(f"{'PASS' if result['score']['passed'] else 'FAIL'} "
                      f"{case.name} #{trial} {variant} "
                      f"outcome={result['metrics']['outcome']} "
                      f"calls={len(result['proxy']['requests'])}", flush=True)
                write_json(out / "report.json", report)
                if args.live and not any(request["kind"] == "agent" and request["status"] == "ok"
                                         for request in result["proxy"]["requests"]):
                    raise ValueError("pinned subscription provider produced no agent response; "
                                     "stopped without switching providers")
                if budget.halted or budget.breach:
                    raise ValueError("provider safety gate halted the evaluation")
                if len(report["results"]) < expected and (
                        time.monotonic() >= budget.deadline or budget.calls >= budget.max_calls):
                    raise ValueError("global budget exhausted; remaining scenarios not run")
        report["checks"].extend([
            {"name": "Every planned scenario ran", "passed": len(report["results"]) == expected},
            {"name": "All independent scenarios pass",
             "passed": all(row["score"]["passed"] for row in report["results"])},
            {"name": "No false completion claims",
             "passed": not any(row["score"]["false_completion"] for row in report["results"])},
        ])
    except (ValueError, OSError, subprocess.SubprocessError) as error:
        # Controlled messages only, never an upstream exception or credential.
        report["checks"].append({"name": "Evaluation completed without a harness blocker",
                                 "passed": False, "detail": str(error)})
    finally:
        report["checks"].append({"name": "Relevant source stayed unchanged",
                                 "passed": source_fingerprint() == frozen})
        if runtime_receipt:
            report["checks"].append({"name": "Pinned Bun and TypeScript stayed unchanged",
                                    "passed": runtime_identity() == runtime_receipt})
        report["budget"] = budget.snapshot()
        report["checks"].append({"name": "Combined parent/child/judge call and token budgets respected",
                                 "passed": report["budget"]["calls"] <= args.max_calls and
                                 report["budget"]["charged_tokens"] +
                                 report["budget"]["pending_reserved_tokens"] <= args.max_tokens})
        report["checks"].append({"name": "Reported usage stayed within conservative reservations",
                                 "passed": not report["budget"]["token_bound_breached"] and
                                 not report["budget"]["safety_halted"] and
                                 report["budget"]["pending_reserved_tokens"] == 0})
        report["summary"] = summarize(report["results"])
        write_json(out / "report.json", report)
        render_report(report, out)
        print(f"report: {out / 'report.json'}", flush=True)
        print(f"view: {out / 'report.html'}", flush=True)
    return 0 if all(check["passed"] for check in report["checks"]) else 1


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (ValueError, OSError) as error:
        print(f"preflight blocked: {error}", file=sys.stderr)
        raise SystemExit(2)
