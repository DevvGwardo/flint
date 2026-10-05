#!/usr/bin/env python3
"""Native ACP smoke test with an offline peer, isolated home and Cua Driver.

No provider, real credentials, original app restart or external Git operations.
The output directory must be new. Run scripts sequentially.
"""

from __future__ import annotations

import argparse
from collections import deque
import json
from pathlib import Path
import plistlib
import shutil
import subprocess
import sys
import threading
import time
import tomllib


ROOT = Path(__file__).resolve().parents[2]

def window_only(result: dict) -> dict:
    """Older Cua releases append global menus; never retain that unrelated data."""
    data = result.get("structuredContent", {})
    if "elements" not in data:
        return result
    keep = set()
    elements = []
    for element in data["elements"]:
        if element.get("role") == "AXWindow" or element.get("parent_index") in keep:
            keep.add(element["element_index"])
            elements.append(element)
    data["elements"] = elements
    data["element_count"] = data["returned_element_count"] = len(elements)
    data["tree_markdown"] = "\n".join(
        f"{'  ' * e.get('depth', 0)}[{e['element_index']}] {e['role']} {e.get('label', '')}"
        for e in elements
    )
    result["content"] = [{"type": "text", "text": data["tree_markdown"]}]
    return result


class Cua:
    """One persistent MCP transport, preserving snapshot-bound AX targets."""

    def __init__(self, out: Path):
        self.out = out
        self.serial = 0
        self.messages: deque = deque()
        self.condition = threading.Condition()
        self.log = (out / "cua.log").open("w")
        self.process = subprocess.Popen(
            ["cua-driver", "mcp"], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            stderr=self.log, text=True, bufsize=1,
        )
        threading.Thread(target=self.read, daemon=True).start()
        self.rpc("initialize", {
            "protocolVersion": "2024-11-05", "capabilities": {},
            "clientInfo": {"name": "flint-offline-acp", "version": "1"},
        })
        self.write({"jsonrpc": "2.0", "method": "notifications/initialized"})

    def read(self) -> None:
        for line in self.process.stdout:
            try:
                self.receive(json.loads(line))
            except json.JSONDecodeError:
                self.receive({"transport_error": line})
        self.receive({"transport_error": "Cua transport closed"})

    def receive(self, message: dict) -> None:
        with self.condition:
            self.messages.append(message)
            self.condition.notify()

    def write(self, message: dict) -> None:
        self.process.stdin.write(json.dumps(message) + "\n")
        self.process.stdin.flush()

    def rpc(self, method: str, params: dict) -> dict:
        self.serial += 1
        ident = self.serial
        self.write({"jsonrpc": "2.0", "id": ident, "method": method, "params": params})
        deadline = time.monotonic() + 90
        while time.monotonic() < deadline:
            with self.condition:
                if not self.condition.wait_for(
                    lambda: bool(self.messages), timeout=max(0.1, deadline - time.monotonic())
                ):
                    raise TimeoutError(method)
                result = self.messages.popleft()
            if "transport_error" in result:
                raise RuntimeError(result["transport_error"])
            if result.get("id") == ident:
                if "error" in result:
                    raise RuntimeError(result["error"])
                return result["result"]
        raise TimeoutError(method)

    def call(self, tool: str, **arguments: object) -> dict:
        result = window_only(self.rpc("tools/call", {"name": tool, "arguments": arguments}))
        (self.out / f"cua-{self.serial:03}-{tool}.json").write_text(
            json.dumps(result, indent=2) + "\n"
        )
        if result.get("isError"):
            raise RuntimeError(result)
        if "structuredContent" in result:
            return result["structuredContent"]
        for part in result.get("content", []):
            if part.get("type") == "text":
                try:
                    return json.loads(part["text"])
                except json.JSONDecodeError:
                    pass
        raise RuntimeError(f"No structured result from {tool}: {result}")

    def close(self) -> None:
        self.process.stdin.close()
        try:
            self.process.wait(5)
        except subprocess.TimeoutExpired:
            self.process.terminate()
            self.process.wait(5)
        self.log.close()


def prepare(out: Path, binary: Path, workspace: Path) -> Path:
    out.mkdir(parents=True, exist_ok=False)
    home = out / "home" / ".flint"
    home.mkdir(parents=True)
    (home / "config.toml").write_text(
        'default_agent = "droid"\napproval = "ask"\npermission_choice_pending = false\n'
    )
    bin_dir = out / "bin"
    bin_dir.mkdir()
    fixture = Path(__file__).with_name("acp_fixture.py").resolve()
    droid = bin_dir / "droid"
    droid.write_text(f"#!{sys.executable}\nexec(compile(open({str(fixture)!r}).read(), {str(fixture)!r}, 'exec'))\n")
    droid.chmod(0o755)
    for tool in ["node", "npm"]:
        executable = shutil.which(tool)
        if not executable:
            raise RuntimeError(f"{tool} is required")
        (bin_dir / tool).symlink_to(Path(executable).resolve())
    workspace.mkdir(parents=True)
    subprocess.run(["git", "init", "--quiet", str(workspace)], check=True)
    (workspace / "package.json").write_text(json.dumps({
        "name": "flint-offline-acp", "private": True,
        "scripts": {"test": "node --test fixture.test.cjs"},
    }))
    (workspace / "fixture.test.cjs").write_text(
        "const test = require('node:test'); const assert = require('node:assert/strict');\n"
        "test('foreground terminal', () => assert.equal(2 + 2, 4));\n"
    )
    alias = out / "symlinked workspace"
    alias.symlink_to(workspace, target_is_directory=True)
    (out / "launch.json").write_text(json.dumps({
        "binary": str(binary),
        "args": ["--workspace", str(alias), "--size", "1100x800",
                 "--prompt", "smoke symlink", "--open-terminal"],
    }))
    bundle = out / "Flint ACP Fixture.app"
    macos = bundle / "Contents" / "MacOS"
    macos.mkdir(parents=True)
    launcher = macos / "FlintACPFixture"
    launcher.write_text(f"#!{sys.executable}\nexec(compile(open({str(fixture)!r}).read(), {str(fixture)!r}, 'exec'))\n")
    launcher.chmod(0o755)
    (bundle / "Contents" / "Info.plist").write_bytes(plistlib.dumps({
        "CFBundleIdentifier": f"ai.flint.acp-fixture.{time.time_ns()}",
        "CFBundleName": "Flint ACP Fixture", "CFBundleExecutable": launcher.name,
        "CFBundlePackageType": "APPL",
    }))
    subprocess.run([
        "/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister",
        "-f", str(bundle),
    ], check=True)
    return bundle


def records(out: Path) -> list[dict]:
    path = out / "peer.jsonl"
    return [json.loads(line) for line in path.read_text().splitlines()] if path.exists() else []


def wait_for(predicate, timeout: float = 30):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        value = predicate()
        if value:
            return value
        time.sleep(0.05)
    raise TimeoutError("native postcondition not observed")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bin", type=Path, default=ROOT / "target/release/flint")
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    out = args.out.resolve()
    bundle = prepare(out, args.bin.resolve(), out / "workspace")
    cua = Cua(out)
    pid = window = None
    checks = []

    def check(name: str, ok: bool, detail: object = "") -> None:
        checks.append({"name": name, "ok": bool(ok), "detail": detail})
        (out / "report.json").write_text(json.dumps(checks, indent=2) + "\n")
        print(f"{'PASS' if ok else 'FAIL'} {name}: {detail}", flush=True)
        if not ok:
            raise AssertionError(name)

    def snapshot(tag: str) -> dict:
        return cua.call("get_window_state", pid=pid, window_id=window,
                        screenshot_out_file=str(out / f"{tag}.png"))

    def verify_label(label: str, tag: str) -> dict:
        result = cua.call("verify_state", pid=pid, window_id=window,
                          expect=[{"element": {"selector": {"label_contains": label},
                                                "exists": True}}],
                          stable_samples=3, timeout_ms=10000, include_screenshot=False)
        if result["status"] != "satisfied":
            raise AssertionError(f"{tag}: native state unconfirmed: {result}")
        return snapshot(tag)

    def click_label(label: str, tag: str, occurrence: int | None = None) -> dict:
        state = snapshot(f"{tag}-before")
        elements = [e for e in state["elements"] if e.get("label") == label]
        if not elements:
            state = snapshot(f"{tag}-before-retry")
            elements = [e for e in state["elements"] if e.get("label") == label]
        if not elements or (occurrence is None and len(elements) != 1):
            raise AssertionError(f"Expected one native control {label!r}: {elements}")
        element = elements[occurrence if occurrence is not None else 0]
        cua.call("click", pid=pid, window_id=window, element_token=element["element_token"])
        return snapshot(f"{tag}-after")

    def submit(text: str, tag: str) -> None:
        state = snapshot(f"{tag}-input-before")
        field = next(e for e in state["elements"] if e.get("role") == "AXTextArea")
        cua.call("type_text", pid=pid, window_id=window,
                 element_token=field["element_token"], text=text)
        verified = cua.call("verify_state", pid=pid, window_id=window,
                            expect=[{"element": {"selector": {"role": "AXTextArea"},
                                                   "value_equals": text}}],
                            stable_samples=3, timeout_ms=10000, include_screenshot=False)
        if verified["status"] != "satisfied":
            raise RuntimeError(
                f"{tag}: background keyboard delivery remains unconfirmed. "
                "Foreground control requires explicit user permission."
            )
        state = snapshot(f"{tag}-input-after")
        check(f"{tag}: native input receives text", any(
            e.get("role") == "AXTextArea" and e.get("value") == text
            for e in state["elements"]
        ))
        click_label("Send", f"{tag}-send")

    def select_identity() -> None:
        click_label("More agent options", "provider-menu")
        click_label("Provider", "provider-choices")
        click_label("provider-b", "provider-selected")
        wait_for(lambda: any(
            r["kind"] == "selected" and r.get("option") == "provider"
            and r.get("value") == "provider-b" for r in records(out)
        ))
        click_label("Model: model-a", "model-menu")
        state = click_label("model-b", "model-selected")
        check("native model chip confirms selected value", any(
            e.get("label") == "Model: model-b" for e in state["elements"]
        ))
        saved = wait_for(lambda: tomllib.loads(
            (out / "home/.flint/config.toml").read_text()
        ).get("agent_options", {}).get("droid", {}).get("model") == "model-b")
        check("confirmed native preferences saved", saved)

    try:
        bundle_id = plistlib.loads((bundle / "Contents/Info.plist").read_bytes())["CFBundleIdentifier"]
        launched = cua.call("launch_app", bundle_id=bundle_id,
                           creates_new_application_instance=True,
                           additional_arguments=["--fixture-root", str(out)])
        pid = launched["pid"]
        windows = launched.get("windows", [])
        if not windows:
            windows = wait_for(lambda: cua.call("list_windows", pid=pid).get("windows"))
        window = next(w["window_id"] for w in windows if w.get("is_on_screen"))
        snapshot("native-smoke")
        wait_for(lambda: any(r["kind"] == "turn-finished" for r in records(out)))
        terminals = {r["label"]: r for r in records(out) if r["kind"] == "terminal"}
        for label, code, text in [
            ("echo", 0, "ok"), ("npm", 0, "foreground terminal"),
            ("heredoc", 0, "foreground heredoc"), ("failed-heredoc", 7, "failed heredoc"),
        ]:
            row = terminals[label]
            check(f"foreground {label} via ACP", row.get("exit") == code and text in row["output"], row)
        check("bad binary has real peer reason", "cannot run" in terminals["missing"]["error"]["message"])
        check("symlink resolves to confined real workspace",
              all(r["cwd"] == str((out / "workspace").resolve()) for r in terminals.values()))
        verify_label("Model: model-a", "completed-smoke")
        check("no native panic", "panicked at" not in (out / "app.log").read_text())
        click_label("Terminal", "hide-terminal")
        state = snapshot("before-native-work-disclosure")
        work = next(e for e in state["elements"] if e.get("label", "").startswith("Show work"))
        state = click_label(work["label"], "native-work-expanded")
        check("native work disclosure expands", any(
            e.get("label", "").startswith("Hide work") for e in state["elements"]
        ))
        heredoc = next(e for e in state["elements"] if
                       e.get("label", "").startswith("Expand Ran: cat"))
        state = click_label(heredoc["label"], "native-heredoc-expanded", occurrence=0)
        check("native command expansion opens", any(
            e.get("label", "").startswith("Collapse Ran: cat") for e in state["elements"]
        ))
        select_identity()
        before = len([r for r in records(out) if r["kind"] == "new"])
        click_label("New agent", "new-session")
        wait_for(lambda: len([r for r in records(out) if r["kind"] == "new"]) > before)
        state = snapshot("new-session-restored")
        check("new native session retains model", any(
            e.get("label") == "Model: model-b" for e in state["elements"]
        ))
        submit("identity check", "identity-check")
        prompt = wait_for(lambda: next((r for r in reversed(records(out))
                           if r["kind"] == "prompt" and r["text"] == "identity check"), None))
        check("new session restores identity before first prompt",
              prompt["choices"] == {"model": "model-b", "provider": "provider-b", "mode": "ask"}, prompt)
        wait_for(lambda: any(r["kind"] == "turn-finished" and r["text"] == "identity check"
                             for r in records(out)))
        verify_label("Send", "identity-turn-settled")
        before = len([r for r in records(out) if r["kind"] == "new"])
        submit("/clear", "clear-session")
        wait_for(lambda: len([r for r in records(out) if r["kind"] == "new"]) > before)
        state = snapshot("clear-restored")
        check("clear retains the model", any(
            e.get("label") == "Model: model-b" for e in state["elements"]
        ))
        submit("approval", "approval")
        wait_for(lambda: any(r["kind"] == "prompt" and r["text"] == "approval"
                             for r in records(out)))
        approval = next(r for r in reversed(records(out))
                        if r["kind"] == "prompt" and r["text"] == "approval")
        check("clear restores both identity choices before prompts",
              approval["choices"] == {"model": "model-b", "provider": "provider-b", "mode": "ask"})
        state = snapshot("native-multiline-approval")
        bounds = state["window_bounds"]
        cua.call("set_window_frame", pid=pid, window_id=window,
                 x=bounds["x"], y=bounds["y"], width=900, height=560)
        state = snapshot("compact-multiline-approval")
        check("compact native approval frame verified",
              state["window_bounds"]["width"] == 900 and state["window_bounds"]["height"] == 560)
        click_label("Deny", "approval-denied")
        wait_for(lambda: any(r["kind"] == "turn-finished" and r["text"] == "approval"
                             for r in records(out)))
        answer = next(r for r in reversed(records(out)) if r["kind"] == "permission")
        check("native Deny reaches the ACP peer",
              answer["response"]["outcome"].get("optionId") == "no")
        verify_label("Send", "approval-turn-settled")
        state = snapshot("before-compact")
        bounds = state["window_bounds"]
        cua.call("set_window_frame", pid=pid, window_id=window, x=bounds["x"], y=bounds["y"],
                 width=900, height=560)
        state = snapshot("compact-native")
        check("compact native frame verified", state["window_bounds"]["width"] == 900
              and state["window_bounds"]["height"] == 560)
        submit("running", "compact-running")
        wait_for(lambda: any(r["kind"] == "prompt" and r["text"] == "running"
                             for r in records(out)))
        verify_label("Stop", "compact-multiline-running-task")
        wait_for(lambda: any(r["kind"] == "turn-finished" and r["text"] == "running"
                             for r in records(out)))
        other = out / "other workspace"
        other.mkdir()
        subprocess.run(["git", "init", "--quiet", str(other)], check=True)
        for name in ["package.json", "fixture.test.cjs"]:
            shutil.copy2(out / "workspace" / name, other / name)
        config = json.loads((out / "launch.json").read_text())
        config["args"] = ["--workspace", str(other), "--size", "900x560",
                          "--prompt", "smoke project-change"]
        (out / "launch.json").write_text(json.dumps(config))
        restart_at = time.time()
        snapshot("before-isolated-restart")
        cua.call("hotkey", pid=pid, window_id=window, keys=["cmd", "q"])
        wait_for(lambda: not cua.call("list_windows", pid=pid).get("windows"))
        launched = cua.call("launch_app", bundle_id=bundle_id,
                           creates_new_application_instance=True,
                           additional_arguments=["--fixture-root", str(out)])
        old_pid, pid = pid, launched["pid"]
        windows = launched.get("windows") or wait_for(lambda: cua.call("list_windows", pid=pid).get("windows"))
        window = next(w["window_id"] for w in windows if w.get("is_on_screen"))
        restored = wait_for(lambda: next((r for r in reversed(records(out))
                            if r["kind"] == "prompt" and r["at"] >= restart_at
                            and r["text"] == "smoke project-change"), None))
        check("restart and project change retain provider/model before first prompt",
              restored["choices"] == {"model": "model-b", "provider": "provider-b", "mode": "ask"}, restored)
        state = verify_label("Model: model-b", "restart-restored")
        check("isolated native restart retains selected model", pid != old_pid and any(
            e.get("label") == "Model: model-b" for e in state["elements"]
        ))
        wait_for(lambda: any(r["kind"] == "turn-finished" and r["at"] >= restart_at
                             and r["text"] == "smoke project-change" for r in records(out)))
        snapshot("restart-completed")
        check("final native run has no panic", "panicked at" not in (out / "app.log").read_text())
        print(f"Native fixture ready: pid={pid}, window={window}, evidence={out}", flush=True)
        (out / "native-target.json").write_text(json.dumps({"pid": pid, "window_id": window}))
        snapshot("before-fixture-close")
        cua.call("hotkey", pid=pid, window_id=window, keys=["cmd", "q"])
        wait_for(lambda: not cua.call("list_windows", pid=pid).get("windows"))
        check("isolated native fixture closes cleanly", True)
    except Exception as error:
        checks.append({"name": "native workflow", "ok": False, "detail": str(error)})
        (out / "report.json").write_text(json.dumps(checks, indent=2) + "\n")
        raise
    finally:
        # Captures and protocol evidence remain available after successful
        # cleanup. A failed run retains its isolated window for diagnosis.
        cua.close()


if __name__ == "__main__":
    main()
