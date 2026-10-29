#!/usr/bin/env python3
"""Local, provider-free release-bundle IPC/render smoke; not a Rust-host test."""

import argparse
import hmac
import json
import os
from pathlib import Path
import secrets
import signal
import socket
import stat
import struct
import subprocess
import sys

LIMIT = 256 * 1024
ROOT = Path(__file__).resolve().parent.parent


def frame(value):
    payload = json.dumps(value, separators=(",", ":")).encode("utf-8")
    if not 0 < len(payload) <= LIMIT:
        raise RuntimeError("Invalid local fixture size")
    return struct.pack(">I", len(payload)) + payload


def read_exact(connection, length):
    result = bytearray()
    while len(result) < length:
        chunk = connection.recv(length - len(result))
        if not chunk:
            raise RuntimeError("Companion closed before expected smoke frame")
        result.extend(chunk)
    return bytes(result)


def read_frame(connection):
    length = struct.unpack(">I", read_exact(connection, 4))[0]
    if not 0 < length <= LIMIT:
        raise RuntimeError("Invalid companion smoke frame length")
    return json.loads(read_exact(connection, length).decode("utf-8", "strict"))


def run(app):
    executable = app / "Contents" / "MacOS" / "hooter_genui"
    if not executable.is_file():
        raise RuntimeError("Build the release companion first")
    runtime = Path.home() / ".flint" / "genui-runtime"
    if not runtime.parent.is_dir():
        raise RuntimeError("The native ~/.flint directory does not exist")
    created = False
    if not runtime.exists():
        runtime.mkdir(mode=0o700)
        created = True
    info = runtime.lstat()
    if (not stat.S_ISDIR(info.st_mode) or info.st_uid != os.getuid()
            or stat.S_IMODE(info.st_mode) != 0o700):
        raise RuntimeError("Runtime directory must be owned and mode 0700")
    launch = runtime / ("g-" + secrets.token_hex(3))
    launch.mkdir(mode=0o700)
    path = launch / "s"
    generation = "smoke-" + secrets.token_hex(12)
    capability = secrets.token_urlsafe(32)
    base = {"version": 1, "generation": generation}
    process = None
    try:
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as server:
            server.bind(str(path))
            os.chmod(path, 0o600)
            server.listen(1)
            server.settimeout(12)
            environment = {
                "HOME": str(Path.home()),
                "PATH": "/usr/bin:/bin",
                "LANG": "en_US.UTF-8",
                "HOOTER_GENUI_SOCKET": str(path),
                "HOOTER_GENUI_GENERATION": generation,
                "HOOTER_GENUI_CAPABILITY": capability,
            }
            process = subprocess.Popen(
                [str(executable)], env=environment, start_new_session=True,
                stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
            )
            connection, _ = server.accept()
            with connection:
                connection.settimeout(8)
                hello = read_frame(connection)
                if (set(hello) != {"version", "kind", "generation", "token"}
                        or hello.get("version") != 1
                        or hello.get("kind") != "hello"
                        or hello.get("generation") != generation
                        or not isinstance(hello.get("token"), str)
                        or not hmac.compare_digest(hello["token"], capability)):
                    raise RuntimeError("Companion handshake failed")
                connection.sendall(frame({**base, "kind": "ready"}))
                for sequence in (1, 2):
                    snapshot = {
                        **base, "kind": "snapshot", "sequence": sequence,
                        "task": {
                            "id": 1, "contract_revision": 1, "work_revision": 2,
                            "outcome": "needs_review",
                        },
                        "surface": {
                            "surface_id": "task",
                            "catalog_id": "dev.hooter.safe.v1",
                            "components": [
                                {"id": "root", "component": "Column",
                                 "children": ["title", "card", "button"]},
                                {"id": "title", "component": "PlainText",
                                 "text": "A private task view", "style": "heading"},
                                {"id": "card", "component": "Card",
                                 "child": "body"},
                                {"id": "body", "component": "PlainText",
                                 "text": {"path": "/context"}},
                                {"id": "button", "component": "Button",
                                 "child": "label",
                                 "action": {"name": "open_task"}},
                                {"id": "label", "component": "PlainText",
                                 "text": "Open native task"},
                            ],
                            "data": {"context": "Local SDK render fixture "
                                     + str(sequence) + ". No provider requests."},
                        },
                    }
                    connection.sendall(frame(snapshot))
                    if read_frame(connection) != {
                        **base, "kind": "ack", "sequence": sequence,
                    }:
                        raise RuntimeError("Genuine SDK render was not acknowledged")
                snapshot["sequence"] = 3
                snapshot["surface"]["components"][4]["action"] = {
                    "functionCall": {"call": "openUrl"}
                }
                connection.sendall(frame(snapshot))
                if read_frame(connection) != {
                    **base, "kind": "error", "code": "invalid_snapshot",
                }:
                    raise RuntimeError("Unsafe component was not rejected")
                if connection.recv(1) != b"":
                    raise RuntimeError("Protocol failure was not terminal")
        print("PASS: sandboxed bundle launch, private Unix authentication,")
        print("two genuine SDK mounted-frame acks, unsafe ingress rejection, EOF.")
        print("Not exercised: Rust native host, GUI action clicks, distribution signing.")
    finally:
        if process is not None and process.poll() is None:
            os.killpg(process.pid, signal.SIGTERM)
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL)
                process.wait(timeout=2)
        path.unlink(missing_ok=True)
        launch.rmdir()
        if created:
            try:
                runtime.rmdir()
            except OSError:
                pass  # Never remove another session's runtime files.


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--app", type=Path, default=(
        ROOT / "build/macos/Build/Products/Release/hooter_genui.app"))
    arguments = parser.parse_args()
    try:
        run(arguments.app.resolve())
        return 0
    except Exception:
        # Never echo frames, capabilities, bootstrap variables or socket paths.
        print("FAIL: local companion smoke did not complete; "
              "check sandbox/runtime access and release build.", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
