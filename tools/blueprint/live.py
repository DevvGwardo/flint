#!/usr/bin/env python3
"""Live runs: real turns on deepseek-v4.1-flash through the local Surplus shim.

For each fixture in `tools/blueprint/fixtures/`: copy `repo/` to a fresh temp
dir (never the flint repo), `git init` it, launch
`flint --workspace <dir> --prompt "<task>" --exit-after-turn` with
`FLINT_BP_STATE`, screenshot the final state, then run the fixture's hidden
`check.sh` independently and compare the files-changed card with
`git diff --numstat`.

    python3 tools/blueprint/live.py [--bin PATH] [--tag before|after] [--only NAME]
"""

from __future__ import annotations

import json
import shutil
import subprocess
import tempfile
import time
from pathlib import Path

import bp

FIXTURES = bp.ROOT / "tools" / "blueprint" / "fixtures"
TURN_TIMEOUT_S = 420


def git(cwd: Path, *args: str) -> str:
    return subprocess.run(["git", *args], cwd=cwd, capture_output=True, text=True, check=True).stdout


def numstat(workspace: Path) -> dict[str, tuple[int, int]]:
    git(workspace, "add", "-A")
    stats = {}
    for line in git(workspace, "diff", "--cached", "--numstat").splitlines():
        added, removed, path = line.split("\t", 2)
        if path.startswith("__pycache__") or "/__pycache__/" in path or path.endswith(".pyc"):
            continue
        stats[path] = (int(added), int(removed))
    return stats


def relative(path: str, workspace: Path) -> str:
    for root in (str(workspace.resolve()), str(workspace)):
        if path.startswith(root + "/"):
            return path[len(root) + 1 :]
    return path.lstrip("./")


def run_fixture(binary: Path, fixture: Path, report: bp.Report, tag: str) -> dict:
    name = fixture.name
    task = (fixture / "task.txt").read_text().strip()
    workspace = Path(tempfile.mkdtemp(prefix=f"flint-bp-{name}-"))
    shutil.copytree(fixture / "repo", workspace, dirs_exist_ok=True)
    git(workspace, "init", "-q")
    git(workspace, "-c", "user.email=bp@flint", "-c", "user.name=bp", "add", "-A")
    git(workspace, "-c", "user.email=bp@flint", "-c", "user.name=bp", "commit", "-qm", "fixture")
    state_path = bp.DATA / f"live-{tag}-{name}.json"
    state_path.unlink(missing_ok=True)
    print(f"fixture {name} in {workspace}", flush=True)
    app = bp.App(
        binary,
        ["--workspace", str(workspace), "--prompt", task, "--exit-after-turn"],
        env={"FLINT_BP_STATE": str(state_path)},
        log=bp.LOGS / f"live-{tag}-{name}.log",
    )
    app.wait_window()
    started = time.time()
    while not state_path.exists() and time.time() - started < TURN_TIMEOUT_S and app.proc.poll() is None:
        time.sleep(0.5)
    elapsed = time.time() - started
    shot = bp.UI / f"{tag}-live-{name}.png"
    if state_path.exists():
        time.sleep(0.4)
        app.refresh_window()
        app.screenshot(shot)
    if app.wait_exit(15) is None:
        app.stop()
    state = json.loads(state_path.read_text()) if state_path.exists() else {}
    session = next((s for s in state.get("sessions", []) if s.get("active")), {})
    turn = (session.get("turns") or [{}])[-1]

    report.check(
        f"{name}: the turn completes",
        turn.get("end_reason") == "Completed",
        f"{turn.get('end_reason')} after {elapsed:.0f}s",
    )
    check = subprocess.run(
        ["sh", str(fixture / "check.sh"), str(workspace), str(fixture)], capture_output=True, text=True
    )
    tail = (check.stdout + check.stderr).strip().splitlines()[-1:] or [""]
    report.check(f"{name}: the hidden check passes", check.returncode == 0, tail[0])

    actual = numstat(workspace)
    card_files = {relative(p, workspace) for p in turn.get("files", [])}
    report.check(
        f"{name}: files-changed card lists the files git changed",
        card_files == set(actual),
        f"card {sorted(card_files)} vs git {sorted(actual)}",
    )
    git_added = sum(a for a, _ in actual.values())
    git_removed = sum(r for _, r in actual.values())
    report.check(
        f"{name}: files-changed card counts match git diff",
        (turn.get("added"), turn.get("removed")) == (git_added, git_removed),
        f"card +{turn.get('added')} -{turn.get('removed')} vs git +{git_added} -{git_removed}",
    )
    guard = session.get("guard_events")
    report.check(
        f"{name}: guard events are recorded",
        isinstance(guard, list),
        f"{len(guard or [])} events: {[g.get('reason') for g in guard or []]}",
    )
    usage = session.get("usage", {})
    report.check(
        f"{name}: token usage is recorded",
        usage.get("input", 0) > 0 and usage.get("output", 0) > 0,
        f"in {usage.get('input')} (cached {usage.get('cached')}), out {usage.get('output')}, "
        f"reasoning {usage.get('reasoning')}",
    )
    report.check(
        f"{name}: time to first token is recorded",
        session.get("first_token_ms") is not None,
        f"first token {session.get('first_token_ms')} ms, first text {session.get('first_text_ms')} ms",
    )
    stddev = bp.image_stats(shot)[0] if shot.exists() else 0
    report.check(f"{name}: final screenshot is not blank", stddev > 6, f"{shot.name} stddev {stddev:.1f}")
    shutil.rmtree(workspace, ignore_errors=True)
    return {
        "seconds": round(elapsed, 1),
        "usage": usage,
        "first_token_ms": session.get("first_token_ms"),
        "first_text_ms": session.get("first_text_ms"),
        "item_kinds": session.get("item_kinds"),
        "guard_events": guard,
        "card": {"files": sorted(card_files), "added": turn.get("added"), "removed": turn.get("removed")},
        "git": {k: list(v) for k, v in actual.items()},
    }


def main() -> None:
    parser = bp.args_parser(__doc__)
    parser.add_argument("--only", help="run a single fixture")
    args = parser.parse_args()
    binary = bp.resolve_bin(args.bin)
    report = bp.Report("live", args.tag)
    fixtures = sorted(p for p in FIXTURES.iterdir() if (p / "task.txt").exists())
    if args.only:
        fixtures = [p for p in fixtures if p.name == args.only]
    report.metrics = {fx.name: run_fixture(binary, fx, report, args.tag) for fx in fixtures}
    report.finish()


if __name__ == "__main__":
    main()
