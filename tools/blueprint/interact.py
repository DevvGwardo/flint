#!/usr/bin/env python3
"""Interaction checks: runs the headless GPUI UI tests in
`crates/flint-app/tests/blueprint_ui.rs` (real views, scripted engine events)
and records one check per test. Test names are the promises they protect.

    python3 tools/blueprint/interact.py [--tag before|after] [--src DIR] [--target-dir DIR]
"""

from __future__ import annotations

import os
import re
import subprocess
from pathlib import Path

import bp


def main() -> None:
    parser = bp.args_parser(__doc__)
    parser.add_argument("--src", type=Path, default=bp.ROOT, help="checkout to test (default: this repo)")
    parser.add_argument("--target-dir", type=Path, help="CARGO_TARGET_DIR for the test build")
    args = parser.parse_args()
    report = bp.Report("interact", args.tag)
    env = dict(os.environ)
    if args.target_dir:
        env["CARGO_TARGET_DIR"] = str(args.target_dir)
    cmd = ["cargo", "test", "-p", "flint-app", "--test", "blueprint_ui", "--", "--test-threads=1"]
    proc = subprocess.run(cmd, cwd=args.src, env=env, capture_output=True, text=True)
    log = bp.LOGS / f"interact-{args.tag}.log"
    log.write_text(proc.stdout + "\n--- stderr ---\n" + proc.stderr)
    results = re.findall(r"^test (\S+) \.\.\. (ok|FAILED|ignored)$", proc.stdout, re.M)
    if not results:
        report.check("UI test suite compiles and runs", False, proc.stderr.strip().splitlines()[-1:] or "no output")
    failures = dict(re.findall(r"---- (\S+) stdout ----\n(.*?)(?=\n----|\nfailures:|\Z)", proc.stdout, re.S))
    for name, status in results:
        detail = "passed" if status == "ok" else _first_assert(failures.get(name, "failed"))
        report.check(name.replace("_", " "), status == "ok", detail)
    report.metrics = {"tests": len(results), "log": str(log)}
    report.finish()


def _first_assert(text: str) -> str:
    lines = [l.strip() for l in text.splitlines() if l.strip()]
    keep = [l for l in lines if "panicked" in l or l.startswith(("left", "right", "assertion"))]
    return " | ".join(keep[:4]) or (lines[0] if lines else "failed")


if __name__ == "__main__":
    main()
