"""Shared helpers for the flint blueprint harness.

Every script records one `check(name, ok, detail)` per user-facing promise and
writes `blueprint-out/data/report-<script>.json`; `finish()` exits non-zero
when any check failed. Raw evidence (screenshots, logs, frame dumps) goes
under `blueprint-out/`, which is gitignored.
"""

from __future__ import annotations

import argparse
import json
import math
import os
import re
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


def metric_evidence(value, path: str = "$") -> tuple[object, list[dict]]:
    """Copy metrics without inventing samples; retain missing paths/reasons."""
    if value is None or isinstance(value, float) and not math.isfinite(value):
        reason = "measurement unavailable" if value is None else f"non-finite measurement: {value}"
        return None, [{"path": path, "reason": reason}]
    if isinstance(value, (dict, list, tuple)):
        items = value.items() if isinstance(value, dict) else enumerate(value)
        clean = {} if isinstance(value, dict) else []
        missing = []
        for key, item in items:
            child = f"{path}.{key}" if isinstance(value, dict) else f"{path}[{key}]"
            normalized, unavailable = metric_evidence(item, child)
            if isinstance(value, dict):
                clean[key] = normalized
            else:
                clean.append(normalized)
            missing.extend(unavailable)
        return clean, missing
    return value, []


def strict_json(values: dict, *, metrics_key: str | None = None) -> str:
    """Serialize raw metrics or a report as standard JSON, with missing evidence."""
    if metrics_key is None:
        clean, missing = metric_evidence(values)
    else:
        clean = dict(values)
        clean[metrics_key], missing = metric_evidence(values[metrics_key], f"$.{metrics_key}")
    if missing:
        clean["missing_metrics"] = missing
    return json.dumps(clean, indent=2, allow_nan=False)


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

    def skip(self, name: str, detail="") -> None:
        self.checks.append({"name": name, "ok": None, "skipped": True, "detail": str(detail)})
        print(f"  [SKIP] {name} — {detail}", flush=True)

    def finish(self) -> None:
        # Historical before values are context, not required current evidence.
        required = self.metrics.get("current", self.metrics)
        prefix = "$.metrics.current" if "current" in self.metrics else "$.metrics"
        _, missing = metric_evidence(required, prefix)
        if missing:
            self.check(
                "required measurements are available", False,
                "; ".join(f"{item['path']}: {item['reason']}" for item in missing),
            )
        skipped = [c for c in self.checks if c.get("skipped")]
        failed = [c for c in self.checks if not c["ok"] and not c.get("skipped")]
        body = {
            "script": self.script,
            "tag": self.tag,
            "passed": len(self.checks) - len(failed) - len(skipped),
            "failed": len(failed),
            "skipped": len(skipped),
            "total": len(self.checks),
            "seconds": round(time.time() - self.started, 1),
            "metrics": self.metrics,
            "checks": self.checks,
        }
        path = DATA / f"report-{self.script}.json"
        text = strict_json(body, metrics_key="metrics")
        path.write_text(text)
        # Keep a per-tag copy so before/after can be compared.
        (DATA / f"report-{self.script}-{self.tag}.json").write_text(text)
        print(f"{self.script}: {body['passed']}/{body['total']} passed, {body['skipped']} skipped -> {path}")
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
        probe = subprocess.run(["ps", "-o", "rss=", "-p", str(self.proc.pid)], capture_output=True, text=True)
        try:
            rss = int(probe.stdout.strip()) if probe.returncode == 0 else 0
        except ValueError:
            rss = 0
        return rss / 1024 if rss > 0 else float("nan")

    def cpu_seconds(self) -> float:
        probe = subprocess.run(["ps", "-o", "time=", "-p", str(self.proc.pid)], capture_output=True, text=True)
        return parse_cpu_time(probe.stdout.strip()) if probe.returncode == 0 else float("nan")

    def cpu_percent(self, seconds: float) -> float:
        start = self.cpu_seconds()
        time.sleep(seconds)
        end = self.cpu_seconds()
        if not math.isfinite(start) or not math.isfinite(end) or end < start:
            return float("nan")
        return (end - start) / seconds * 100

    def stop(self) -> int:
        try:
            if self.proc.poll() is None:
                self.proc.terminate()
                try:
                    self.proc.wait(5)
                except subprocess.TimeoutExpired:
                    self.proc.kill()
                    self.proc.wait()
        finally:
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
    match = re.fullmatch(r"(?:(\d+)-)?(?:(\d+):)?(\d+):([0-5]\d(?:\.\d+)?)", text)
    if not match:
        return float("nan")
    days, hours, minutes, seconds = match.groups()
    if (days is not None and hours is None) or (hours is not None and int(minutes) >= 60):
        return float("nan")
    return int(days or 0) * 86400 + int(hours or 0) * 3600 + int(minutes) * 60 + float(seconds)


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
    """Create a fresh directory; never delete an earlier run or user files."""
    path.mkdir(parents=True)
    return path
