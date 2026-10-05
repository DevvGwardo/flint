#!/usr/bin/env python3
"""Offline ACP peer and isolated native launcher for acp.py. No inference."""

from __future__ import annotations

import json
import os
from pathlib import Path
import sys
import time


def launch() -> None:
    root = Path(sys.argv[sys.argv.index("--fixture-root") + 1]).resolve()
    config = json.loads((root / "launch.json").read_text())
    env = dict(os.environ)
    for key in list(env):
        if key.endswith(("_KEY", "_TOKEN", "_PASSWORD")) or key in {
            "SSH_AUTH_SOCK", "GIT_ASKPASS", "ANTHROPIC_AUTH_TOKEN",
            "FLINT_MODEL", "FLINT_BASE_URL", "FLINT_API_KEY_FILE",
            "CMUX_SOCKET_CAPABILITY",
        }:
            env.pop(key, None)
    env.update(
        HOME=str(root / "home"), FLINT_HOME=str(root / "home" / ".flint"),
        FLINT_ACP_FIXTURE_ROOT=str(root), FLINT_BP_NO_ACTIVATE="1",
        FLINT_LIVE="0", FLINT_EVAL="0", FLINT_BENCH="0",
        PATH=str(root / "bin") + ":/usr/bin:/bin:/usr/local/bin:/opt/homebrew/bin",
        FLINT_BP_STATE=str(root / "state.json"), FLINT_BP_DUMP_AFTER_MS="1200",
    )
    log = os.open(root / "app.log", os.O_CREAT | os.O_WRONLY | os.O_APPEND, 0o600)
    os.dup2(log, 2)
    os.close(log)
    os.execve(config["binary"], [config["binary"], *config["args"]], env)


class Peer:
    def __init__(self) -> None:
        self.root = Path(os.environ["FLINT_ACP_FIXTURE_ROOT"])
        self.sid = f"offline-{os.getpid()}"
        self.workspace = ""
        self.values = {"model": "model-a", "provider": "provider-a", "mode": "ask"}
        self.serial = 0

    def trace(self, kind: str, **fields: object) -> None:
        with (self.root / "peer.jsonl").open("a") as stream:
            stream.write(json.dumps({
                "kind": kind, "pid": os.getpid(), "session": self.sid,
                "at": time.time(), **fields,
            }) + "\n")

    def send(self, body: dict) -> None:
        print(json.dumps({"jsonrpc": "2.0", **body}), flush=True)

    def respond(self, message: dict, result: dict) -> None:
        self.send({"id": message["id"], "result": result})

    def options(self) -> list[dict]:
        result = []
        for key, choices in [
            ("model", ["model-a", "model-b"]),
            ("provider", ["provider-a", "provider-b"]),
            ("mode", ["ask", "bypass"]),
        ]:
            option = {
                "id": key, "name": key.title(), "type": "select",
                "currentValue": self.values[key],
                "options": [{"value": value, "name": value} for value in choices],
            }
            if key != "provider":
                option["category"] = key
            result.append(option)
        return result

    def update(self, update: dict) -> None:
        self.send({"method": "session/update", "params": {
            "sessionId": self.sid, "update": update,
        }})

    def request(self, method: str, **params: object) -> dict:
        self.serial += 1
        ident = f"fixture-{self.serial}"
        self.send({"id": ident, "method": method, "params": {
            "sessionId": self.sid, **params,
        }})
        for line in sys.stdin:
            message = json.loads(line)
            if message.get("id") == ident and "method" not in message:
                return message
            if message.get("method") == "session/cancel":
                raise RuntimeError("fixture prompt cancelled")
        raise EOFError("client closed")

    def command(self, label: str, command: str, args: list[str] | None = None) -> None:
        args = args or []
        call = f"tool-{label}"
        self.update({
            "sessionUpdate": "tool_call", "toolCallId": call,
            "title": command, "kind": "execute", "status": "in_progress",
            "rawInput": {"command": command, "args": args},
        })
        created = self.request(
            "terminal/create", command=command, args=args,
            cwd=os.path.realpath(self.workspace),
        )
        if "error" in created:
            self.trace("terminal", label=label, ok=False, error=created["error"],
                       command_bytes=len(command.encode()), multiline="\n" in command,
                       arg_count=len(args), cwd=os.path.realpath(self.workspace))
            self.update({
                "sessionUpdate": "tool_call_update", "toolCallId": call,
                "status": "failed", "rawOutput": "Unknown error",
            })
            return
        terminal = created["result"]["terminalId"]
        waited = self.request("terminal/wait_for_exit", terminalId=terminal)
        output = self.request("terminal/output", terminalId=terminal)
        self.request("terminal/release", terminalId=terminal)
        result = output.get("result", {})
        self.trace("terminal", label=label, ok="error" not in waited,
                   exit=waited.get("result", {}).get("exitCode"),
                   output=result.get("output", ""), cwd=os.path.realpath(self.workspace),
                   command_bytes=len(command.encode()), multiline="\n" in command,
                   arg_count=len(args))
        success = waited.get("result", {}).get("exitCode") == 0
        self.update({
            "sessionUpdate": "tool_call_update", "toolCallId": call,
            "status": "completed" if success else "failed",
            "rawOutput": result.get("output", ""),
        })

    def prompt(self, message: dict) -> None:
        text = " ".join(block.get("text", "") for block in message["params"]["prompt"])
        self.trace("prompt", text=text, choices=dict(self.values), workspace=self.workspace)
        if text.startswith("smoke"):
            self.command("echo", "/bin/echo", ["ok"])
            self.command("npm", "npm", ["test"])
            self.command("heredoc", "cat <<'FLINT_EOF'\nforeground heredoc\nFLINT_EOF")
            self.command("failed-heredoc", "cat <<'FLINT_EOF'\nfailed heredoc\nFLINT_EOF\nexit 7")
            self.command("missing", "nonexistent-bin")
        elif text == "running":
            self.command("slow", "cat <<'FLINT_EOF'\nrunning heredoc\nFLINT_EOF\nsleep 10")
        elif text == "approval":
            script = "cat <<'FLINT_EOF'\npermission heredoc\nFLINT_EOF"
            self.update({
                "sessionUpdate": "tool_call", "toolCallId": "approval-command",
                "title": script, "kind": "execute", "status": "pending",
                "rawInput": {"command": script},
            })
            answer = self.request("session/request_permission", toolCall={
                "toolCallId": "approval-command", "title": script, "kind": "execute",
                "status": "pending", "rawInput": {"command": script},
            }, options=[
                {"optionId": "yes", "name": "Allow", "kind": "allow_once"},
                {"optionId": "no", "name": "Reject", "kind": "reject_once"},
            ])
            self.trace("permission", response=answer.get("result", {}))
            self.update({"sessionUpdate": "tool_call_update",
                         "toolCallId": "approval-command", "status": "failed",
                         "rawOutput": "Fixture permission handled; no command executed."})
        self.update({"sessionUpdate": "agent_message_chunk",
                     "content": {"type": "text", "text": "Offline fixture completed."}})
        self.respond(message, {"stopReason": "end_turn"})
        self.trace("turn-finished", text=text)

    def run(self) -> None:
        self.trace("started")
        for line in sys.stdin:
            message = json.loads(line)
            method = message.get("method")
            if method == "initialize":
                self.respond(message, {"protocolVersion": 1, "authMethods": [],
                                       "agentCapabilities": {"loadSession": False}})
            elif method == "session/new":
                self.workspace = message["params"]["cwd"]
                self.trace("new", workspace=self.workspace)
                self.respond(message, {"sessionId": self.sid, "configOptions": self.options()})
            elif method == "session/set_config_option":
                key = message["params"]["configId"]
                value = message["params"]["value"]
                self.values[key] = value
                self.trace("selected", option=key, value=value)
                self.respond(message, {"configOptions": self.options()})
            elif method == "session/prompt":
                self.prompt(message)
            elif "id" in message and method:
                self.send({"id": message["id"], "error": {
                    "code": -32601, "message": f"Unsupported fixture method: {method}",
                }})


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "exec":
        Peer().run()
    else:
        launch()
