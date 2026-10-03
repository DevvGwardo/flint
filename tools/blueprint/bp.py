"""Shared helpers for the flint blueprint harness.

Every script records one `check(name, ok, detail)` per user-facing promise and
writes `blueprint-out/data/report-<script>.json`; `finish()` exits non-zero
when any check failed. Raw evidence (screenshots, logs, frame dumps) goes
under `blueprint-out/`, which is gitignored.
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import statistics
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
OUT = Path(os.environ.get("FLINT_BP_OUTPUT_DIR", ROOT / "blueprint-out"))
DATA = OUT / "data"
UI = OUT / "ui"
LOGS = OUT / "logs"
BIN = OUT / "bin"
RELEASE_BIN = ROOT / "target" / "release" / "flint"


def args_parser(description: str) -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=description)
    parser.add_argument("--bin", type=Path, help="flint binary to test (default: build release)")
    parser.add_argument("--tag", default="after", help="label for this run, e.g. before/after")
    return parser


class Report:
    def __init__(self, script: str, tag: str):
        self.script = script
        self.tag = tag
        self.checks: list[dict] = []
        self.metrics: dict = {}
        self.started = time.time()
        for d in (DATA, UI, LOGS, BIN):
            d.mkdir(parents=True, exist_ok=True)

    def check(self, name: str, ok: bool, detail="") -> bool:
        ok = bool(ok)
        self.checks.append({"name": name, "ok": ok, "detail": str(detail)})
        mark = "PASS" if ok else "FAIL"
        print(f"  [{mark}] {name} — {detail}", flush=True)
        return ok

    def finish(self) -> None:
        failed = [c for c in self.checks if not c["ok"]]
        body = {
            "script": self.script,
            "tag": self.tag,
            "passed": len(self.checks) - len(failed),
            "failed": len(failed),
            "total": len(self.checks),
            "seconds": round(time.time() - self.started, 1),
            "metrics": self.metrics,
            "checks": self.checks,
        }
        path = DATA / f"report-{self.script}.json"
        path.write_text(json.dumps(body, indent=2))
        # Keep a per-tag copy so before/after can be compared.
        (DATA / f"report-{self.script}-{self.tag}.json").write_text(json.dumps(body, indent=2))
        print(f"{self.script}: {body['passed']}/{body['total']} passed -> {path}")
        sys.exit(1 if failed else 0)


def build_release() -> Path:
    print("building release binary…", flush=True)
    subprocess.run(
        ["cargo", "build", "--release", "-p", "flint-app"],
        cwd=ROOT,
        check=True,
    )
    return RELEASE_BIN


def resolve_bin(arg: Path | None) -> Path:
    return arg.resolve() if arg else build_release()


def winid_tool() -> Path:
    tool = BIN / "winid"
    src = ROOT / "tools" / "blueprint" / "winid.swift"
    if not tool.exists() or tool.stat().st_mtime < src.stat().st_mtime:
        BIN.mkdir(parents=True, exist_ok=True)
        subprocess.run(["swiftc", "-O", str(src), "-o", str(tool)], check=True)
    return tool


class App:
    """A launched flint process with window and resource probes."""

    def __init__(self, binary: Path, args: list[str], env: dict | None = None, log: Path | None = None):
        self.log_path = log or (LOGS / "flint.log")
        self.log = open(self.log_path, "w")
        full_env = dict(os.environ)
        # Never steal keyboard focus from the person at the machine, and never
        # touch their real settings or saved sessions.
        full_env.setdefault("FLINT_BP_NO_ACTIVATE", "1")
        full_env["FLINT_HOME"] = str(fresh_dir(OUT / "home" / f"{time.time_ns()}"))
        full_env.update(env or {})
        # Standard sweeps exercise an existing Ask installation. First-run
        # behavior is tested separately with a genuinely empty isolated home.
        config = Path(full_env["FLINT_HOME"]) / "config.toml"
        if not config.exists():
            config.parent.mkdir(parents=True, exist_ok=True)
            config.write_text('approval = "ask"\npermission_choice_pending = false\n')
        if "--workspace" not in args:
            args = [*args, "--workspace", str(fresh_dir(OUT / "workspace" / f"{time.time_ns()}"))]
        self.launched = time.time()
        self.proc = subprocess.Popen(
            [str(binary), *args], stdout=subprocess.DEVNULL, stderr=self.log, env=full_env
        )
        self.window: tuple[int, int, int, int, int] | None = None
        self.window_ms: float | None = None

    def wait_window(self, timeout: float = 20.0) -> bool:
        tool = winid_tool()
        deadline = time.time() + timeout
        while time.time() < deadline and self.proc.poll() is None:
            out = subprocess.run([str(tool), str(self.proc.pid)], capture_output=True, text=True).stdout.split()
            if len(out) == 5:
                self.window = tuple(int(v) for v in out)  # id x y w h
                self.window_ms = (time.time() - self.launched) * 1000
                return True
            time.sleep(0.02)
        return False

    def refresh_window(self) -> None:
        out = subprocess.run([str(winid_tool()), str(self.proc.pid)], capture_output=True, text=True).stdout.split()
        if len(out) == 5:
            self.window = tuple(int(v) for v in out)

    def screenshot(self, path: Path) -> bool:
        if not self.window:
            return False
        path.parent.mkdir(parents=True, exist_ok=True)
        subprocess.run(["screencapture", "-x", "-o", "-l", str(self.window[0]), str(path)], check=False)
        return path.exists()

    def rss_mb(self) -> float:
        out = subprocess.run(["ps", "-o", "rss=", "-p", str(self.proc.pid)], capture_output=True, text=True).stdout
        return int(out.strip() or 0) / 1024

    def cpu_seconds(self) -> float:
        out = subprocess.run(["ps", "-o", "time=", "-p", str(self.proc.pid)], capture_output=True, text=True).stdout
        return parse_cpu_time(out.strip())

    def cpu_percent(self, seconds: float) -> float:
        start = self.cpu_seconds()
        time.sleep(seconds)
        return (self.cpu_seconds() - start) / seconds * 100

    def stop(self) -> int:
        if self.proc.poll() is None:
            self.proc.terminate()
            try:
                self.proc.wait(5)
            except subprocess.TimeoutExpired:
                self.proc.kill()
        self.log.close()
        return self.proc.returncode

    def wait_exit(self, timeout: float) -> int | None:
        try:
            return self.proc.wait(timeout)
        except subprocess.TimeoutExpired:
            return None

    def stderr(self) -> str:
        if not self.log.closed:
            self.log.flush()
        return self.log_path.read_text(errors="replace")


def main_screen() -> tuple[int, int]:
    """Usable size of the display on which centered windows open, in points."""
    out = subprocess.run([str(winid_tool()), "--screen"], capture_output=True, text=True, check=True).stdout.split()
    return int(out[2]), int(out[3])


def parse_cpu_time(text: str) -> float:
    """`ps -o time=` is `[[dd-]hh:]mm:ss.cc`."""
    if not text:
        return 0.0
    days = 0
    if "-" in text:
        d, text = text.split("-", 1)
        days = int(d)
    parts = [float(p) for p in text.split(":")]
    seconds = 0.0
    for p in parts:
        seconds = seconds * 60 + p
    return days * 86400 + seconds


def image_stats(path: Path) -> tuple[float, tuple[int, int]]:
    from PIL import Image, ImageStat

    with Image.open(path) as img:
        gray = img.convert("L")
        return ImageStat.Stat(gray).stddev[0], img.size


def percentile(values: list[float], p: float) -> float:
    if not values:
        return float("nan")
    values = sorted(values)
    k = (len(values) - 1) * p / 100
    lo, hi = int(k), min(int(k) + 1, len(values) - 1)
    return values[lo] + (values[hi] - values[lo]) * (k - lo)


def median(values: list[float]) -> float:
    return statistics.median(values) if values else float("nan")


def fresh_dir(path: Path) -> Path:
    if path.exists():
        shutil.rmtree(path)
    path.mkdir(parents=True)
    return path
