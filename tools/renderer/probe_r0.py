#!/usr/bin/env python3
"""R0 offline probe; every compiler/native subprocess holds the Cargo flock."""
import argparse
import fcntl
import hashlib
import json
import os
from pathlib import Path
import resource
import select
import signal
import socket
import struct
import subprocess
import threading
import time
import zlib

REPO = Path("/Volumes/T7 Shield/mac-offload/Projects/flint")
ROOT = Path("/Volumes/T7 Shield/mac-offload/flint-t3-implementation-glct94hg")
LOCK = ROOT / "evidence/build.lock"
SDK = Path("/Applications/Xcode.app/Contents/Developer/Platforms/MacOSX.platform/Developer/SDKs/MacOSX26.2.sdk")
ALLOWED_VERSION = "26.6.2"


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n")


def clean_env(scratch):
    return {"HOME": str(scratch), "TMPDIR": str(scratch) + "/",
            "PATH": "/usr/bin:/bin:/usr/sbin:/sbin", "LANG": "en_US.UTF-8", "LC_ALL": "en_US.UTF-8"}


def owned_cleanup(process):
    """Only our still-live session leader. Never discover/kill named WK services."""
    if process.poll() is not None:
        return
    if os.getpgid(process.pid) != process.pid:
        raise RuntimeError("Owned process lost its expected private process group")
    os.killpg(process.pid, signal.SIGTERM)
    try:
        process.wait(timeout=0.25)
    except subprocess.TimeoutExpired:
        if process.poll() is None:
            os.killpg(process.pid, signal.SIGKILL)
        process.wait(timeout=2)


def read_process_snapshot(env):
    # Read only PID/PPID/PGID/state/comm; never args/env, user URLs or source.
    output = subprocess.run(["/bin/ps", "-axo", "pid=,ppid=,pgid=,stat=,comm="],
                            capture_output=True, env=env, timeout=3)
    rows = []
    for line in output.stdout.decode(errors="replace").splitlines():
        parts = line.strip().split(None, 4)
        if len(parts) == 5 and ("WebKit" in parts[4] or "FlintRendererProbe" in parts[4]):
            rows.append({"pid": int(parts[0]), "ppid": int(parts[1]), "pgid": int(parts[2]),
                         "state": parts[3], "comm": parts[4]})
    return rows


class Canary:
    def __init__(self):
        self.socket = socket.socket()
        self.socket.bind(("127.0.0.1", 0))
        self.socket.listen(8)
        self.socket.settimeout(0.1)
        self.port = self.socket.getsockname()[1]
        self.hits = []
        self.running = True
        self.thread = threading.Thread(target=self.accept, daemon=True)
        self.thread.start()

    def accept(self):
        while self.running:
            try:
                connection, _ = self.socket.accept()
            except socket.timeout:
                continue
            except OSError:
                return
            with connection:
                connection.settimeout(0.3)
                try:
                    data = connection.recv(1024)
                except OSError:
                    data = b""
                self.hits.append({"time": time.monotonic(), "bytes": len(data),
                                  "syntheticRequestHex": data.hex()})
                try:
                    connection.sendall(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK")
                except OSError:
                    pass

    def close(self):
        self.running = False
        self.socket.close()
        self.thread.join(timeout=1)


def decode_frames(raw):
    frames = []
    offset = 0
    while offset + 4 <= len(raw):
        size, = struct.unpack_from(">I", raw, offset)
        if size > 8 * 1024 * 1024 or offset + 4 + size > len(raw):
            return frames, False
        frames.append(raw[offset + 4:offset + 4 + size])
        offset += size + 4
    return frames, offset == len(raw)


def save_raster(folder, frames):
    records = []
    raster = None
    for frame in frames:
        if records and records[-1].get("event") == "raster" and raster is None:
            metadata = records[-1]
            if metadata.get("width") == 128 and metadata.get("height") == 96 and len(frame) == 128 * 96 * 4:
                raster = frame
                (folder / "raster.bgra").write_bytes(frame)
                continue
        try:
            value = json.loads(frame)
            if isinstance(value, dict):
                records.append(value)
        except (ValueError, UnicodeDecodeError):
            records.append({"event": "invalid_frame", "bytes": len(frame)})
    if raster is None:
        return records, None
    width, height = 128, 96
    rows = bytearray()
    for y in range(height):
        rows.append(0)
        for x in range(width):
            b, g, r, a = raster[(y * width + x) * 4:(y * width + x) * 4 + 4]
            rows.extend((r, g, b, a))
    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))
    png = (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0))
           + chunk(b"IDAT", zlib.compress(bytes(rows))) + chunk(b"IEND", b""))
    (folder / "raster.png").write_bytes(png)
    samples = {}
    for name, (x, y) in {"topLeft": (16, 16), "topRight": (96, 16),
                         "bottomLeft": (16, 72), "bottomRight": (96, 72)}.items():
        b, g, r, a = raster[(y * width + x) * 4:(y * width + x) * 4 + 4]
        samples[name] = [r, g, b, a]
    return records, {"samplesRGBA": samples, "bgraSha256": digest(folder / "raster.bgra"),
                     "pngSha256": digest(folder / "raster.png"), "bytes": len(raster)}


def run_command(folder, command, env, deadline):
    started = time.monotonic()
    with (folder / "stdout.raw").open("xb") as out, (folder / "stderr.raw").open("xb") as err:
        process = subprocess.Popen(command, cwd=folder, env=env, stdin=subprocess.DEVNULL,
                                   stdout=out, stderr=err, start_new_session=True, close_fds=True)
        timed_out = False
        try:
            code = process.wait(timeout=max(0.1, min(60, deadline - time.monotonic())))
        except subprocess.TimeoutExpired:
            timed_out = True
            owned_cleanup(process)
            code = process.returncode
    result = {"command": command, "exit": code, "timedOut": timed_out,
              "seconds": time.monotonic() - started, "pid": process.pid,
              "environmentNames": sorted(env)}
    write_json(folder / "result.json", result)
    return result


def run_case(folder, command, request_bytes, env, timeout, behavior):
    folder.mkdir(mode=0o700)
    (folder / "request.raw").write_bytes(request_bytes)
    before = read_process_snapshot(env)
    started = time.monotonic()
    transitions = []
    output_overflow = threading.Event()
    with (folder / "stdout.raw").open("xb") as out, (folder / "stderr.raw").open("xb") as err:
        process = subprocess.Popen(command, cwd=folder, env=env, stdin=subprocess.PIPE,
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                   start_new_session=True, close_fds=True)
        def drain(pipe, destination, limit):
            retained = 0
            while True:
                block = pipe.read(4096)
                if not block:
                    break
                count = min(len(block), max(0, limit - retained))
                destination.write(block[:count])
                retained += count
                if count < len(block):
                    output_overflow.set()
            pipe.close()
        drains = [
            threading.Thread(target=drain, args=(process.stdout, out, 9 * 1024 * 1024), daemon=True),
            threading.Thread(target=drain, args=(process.stderr, err, 64 * 1024), daemon=True),
        ]
        for thread in drains:
            thread.start()
        try:
            process.stdin.write(request_bytes)
            process.stdin.flush()
            if behavior == "eof":
                time.sleep(0.7)
                process.stdin.close()
                transitions.append({"event": "stdin_closed", "seconds": time.monotonic() - started})
            elif behavior == "cancel":
                time.sleep(0.7)
                process.stdin.write(b"C")
                process.stdin.flush()
                transitions.append({"event": "cancel_sent", "seconds": time.monotonic() - started})
            end = started + timeout
            timed_out = False
            while process.poll() is None:
                if time.monotonic() >= end or output_overflow.is_set():
                    timed_out = time.monotonic() >= end
                    owned_cleanup(process)
                    break
                time.sleep(0.025)
            code = process.returncode
        except BrokenPipeError:
            code = process.wait(timeout=2)
            timed_out = False
        finally:
            if not process.stdin.closed:
                try:
                    process.stdin.close()
                except BrokenPipeError:
                    transitions.append({"event": "stdin_broken_pipe", "seconds": time.monotonic() - started})
            owned_cleanup(process)
            for thread in drains:
                thread.join(timeout=1)
            if any(thread.is_alive() for thread in drains):
                raise RuntimeError("Owned output pipe failed to close; stop rather than interleave jobs")
    immediately = read_process_snapshot(env)
    time.sleep(0.3)
    after = read_process_snapshot(env)
    raw = (folder / "stdout.raw").read_bytes()
    frames, complete = decode_frames(raw)
    records, raster = save_raster(folder, frames)
    result = {"command": command, "exit": code, "timedOut": timed_out,
              "seconds": time.monotonic() - started, "pid": process.pid, "pgid": process.pid,
              "framesComplete": complete, "frameCount": len(frames), "records": records,
              "outputOverflow": output_overflow.is_set(),
              "raster": raster, "transitions": transitions, "processesBefore": before,
              "processesImmediatelyAfter": immediately, "processesAfter300ms": after,
              "auxiliaryOwnership": "Unknown: comm/PPID observation is not a public WK ownership handle",
              "environmentNames": sorted(env)}
    write_json(folder / "result.json", result)
    return result


def main():
    # Inherited by compiler/launcher before any helper/runtime startup can abort.
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    parser = argparse.ArgumentParser()
    parser.add_argument("--execution-seconds", type=int, default=900)
    args = parser.parse_args()
    if not 1 <= args.execution_seconds <= 1200:
        parser.error("execution budget must be 1..1200 seconds")
    evidence = ROOT / "renderer-evidence" / ("r0-" + str(time.time_ns()))
    evidence.mkdir(parents=True, mode=0o700)
    scratch = evidence / "scratch"
    scratch.mkdir(mode=0o700)
    sentinel = evidence / "outside-scratch-sentinel.txt"
    sentinel.write_text("SYNTHETIC_FLINT_R0_SENTINEL_NOT_A_SECRET\n")
    os.chmod(sentinel, 0o600)
    inputs = evidence / "inputs"
    inputs.mkdir(mode=0o700)
    for relative in ["renderer/macos/ProbeMain.swift", "renderer/macos/probe-base.sb",
                     "renderer/tests/raster.html", "renderer/tests/native_probe.c", "tools/renderer/probe_r0.py"]:
        (inputs / Path(relative).name).write_bytes((REPO / relative).read_bytes())
    write_json(evidence / "input-hashes.json", {path.name: digest(path) for path in inputs.iterdir()})
    env = clean_env(scratch)
    print("Evidence:", evidence, flush=True)
    print("Waiting for shared Cargo/Swift/native lock:", LOCK, flush=True)
    with LOCK.open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        started = time.monotonic()
        deadline = started + args.execution_seconds
        lock_stat = os.fstat(lock.fileno())
        write_json(evidence / "lock.json", {"path": str(LOCK), "inode": lock_stat.st_ino,
                                          "device": lock_stat.st_dev, "acquiredTime": time.time(),
                                          "executionBudgetSeconds": args.execution_seconds})
        print("Acquired shared lock; starting execution budget.", flush=True)
        versions = {}
        for name, command in {
            "os": ["/usr/bin/sw_vers"], "swift": ["/usr/bin/swiftc", "--version"],
            "sdk": ["/usr/bin/xcrun", "--sdk", "macosx", "--show-sdk-version"],
            "webkit": ["/usr/bin/plutil", "-extract", "CFBundleVersion", "raw",
                       "/System/Library/Frameworks/WebKit.framework/Resources/Info.plist"]
        }.items():
            completed = subprocess.run(command, capture_output=True, env=env, timeout=5)
            versions[name] = {"command": command, "exit": completed.returncode,
                              "stdout": completed.stdout.decode(errors="replace"),
                              "stderr": completed.stderr.decode(errors="replace")}
        write_json(evidence / "versions.json", versions)
        if "ProductVersion:\t\t" + ALLOWED_VERSION not in versions["os"]["stdout"]:
            write_json(evidence / "blocked.json", {"code": "os_version_gate"})
            return 2
        compile_folder = evidence / "compile-01"
        compile_folder.mkdir(mode=0o700)
        helper = evidence / "FlintRendererProbe"
        command = ["/usr/bin/swiftc", "-parse-as-library", "-sdk", str(SDK),
                   "-module-cache-path", str(scratch / "module-cache"),
                   "-framework", "AppKit", "-framework", "WebKit", "-framework", "CoreGraphics",
                   str(inputs / "ProbeMain.swift"), "-o", str(helper)]
        compiled = run_command(compile_folder, command, env, deadline)
        print("Compile exit:", compiled["exit"], flush=True)
        if compiled["exit"] != 0:
            return 2
        native_helper = evidence / "NativeIsolationProbe"
        native_compile = evidence / "compile-native"
        native_compile.mkdir(mode=0o700)
        native_result = run_command(native_compile,
            ["/usr/bin/clang", "-Wall", "-Wextra", "-Werror", "-isysroot", str(SDK),
             str(inputs / "native_probe.c"), "-o", str(native_helper)], env, deadline)
        if native_result["exit"] != 0:
            return 2
        base = (REPO / "renderer/macos/probe-base.sb").read_text()
        # Exact service additions are retained as separate attempted profiles.
        profile_variants = [
            ("01-base", ""),
            ("02-window-preferences", """
(allow mach-lookup
    (global-name "com.apple.windowserver.active")
    (global-name "com.apple.cfprefsd.agent")
    (global-name "com.apple.cfprefsd.daemon"))
"""),
            ("03-webkit-exact", """
(allow mach-lookup
    (global-name "com.apple.windowserver.active")
    (global-name "com.apple.cfprefsd.agent")
    (global-name "com.apple.cfprefsd.daemon")
    (global-name "com.apple.WebKit.WebContent")
    (global-name "com.apple.WebKit.Networking")
    (global-name "com.apple.WebKit.GPU"))
"""),
            ("04-system-cryptex", """
(allow file-read*
    (subpath "/System/Volumes/Preboot/Cryptexes/OS/System/Library")
    (subpath "/System/Volumes/Preboot/Cryptexes/OS/usr/lib"))
(allow mach-lookup
    (global-name "com.apple.windowserver.active")
    (global-name "com.apple.cfprefsd.agent")
    (global-name "com.apple.cfprefsd.daemon")
    (global-name "com.apple.WebKit.WebContent")
    (global-name "com.apple.WebKit.Networking")
    (global-name "com.apple.WebKit.GPU"))
"""),
            ("05-executable-maps", """
(allow file-read*
    (subpath "/System/Volumes/Preboot/Cryptexes/OS/System/Library")
    (subpath "/System/Volumes/Preboot/Cryptexes/OS/usr/lib"))
(allow file-map-executable
    (literal (param "HELPER")) (subpath "/System/Library") (subpath "/usr/lib")
    (subpath "/System/Volumes/Preboot/Cryptexes/OS/System/Library")
    (subpath "/System/Volumes/Preboot/Cryptexes/OS/usr/lib"))
(allow mach-lookup
    (global-name "com.apple.windowserver.active")
    (global-name "com.apple.cfprefsd.agent")
    (global-name "com.apple.cfprefsd.daemon")
    (global-name "com.apple.WebKit.WebContent")
    (global-name "com.apple.WebKit.Networking")
    (global-name "com.apple.WebKit.GPU"))
""")
        ]
        fixture = (REPO / "renderer/tests/raster.html").read_text()
        request = {"version": 1, "revision": 1, "source": fixture,
                   "fixture": "raster", "sentinel": str(sentinel)}
        canary = Canary()
        outcomes = []
        try:
            request["port"] = canary.port
            active_profile = None
            for label, additions in profile_variants:
                if time.monotonic() + 15 > deadline:
                    break
                profile = evidence / (label + ".sb")
                profile.write_text(base + additions)
                folder = evidence / label
                # Fresh private scratch for every process/document.
                job_scratch = scratch / label
                job_scratch.mkdir(mode=0o700)
                job_env = clean_env(job_scratch)
                body = json.dumps(request).encode()
                command = ["/usr/bin/sandbox-exec", "-f", str(profile),
                           "-D", "HELPER=" + str(helper), "-D", "SCRATCH=" + str(job_scratch), str(helper)]
                result = run_case(folder, command, struct.pack(">I", len(body)) + body, job_env, 12, "normal")
                outcomes.append({"label": label, "exit": result["exit"], "raster": result["raster"] is not None})
                print("Profile:", label, "exit:", result["exit"], "raster:", bool(result["raster"]), flush=True)
                # Same deny rules, minimal libSystem executable: distinguish framework startup
                # failure from the direct native syscall boundary. No unsandboxed control.
                native_folder = evidence / (label + "-native")
                native_folder.mkdir(mode=0o700)
                native_command = ["/usr/bin/sandbox-exec", "-f", str(profile),
                                  "-D", "HELPER=" + str(native_helper), "-D", "SCRATCH=" + str(job_scratch),
                                  str(native_helper), str(sentinel), str(canary.port)]
                native_probe = run_command(native_folder, native_command, job_env, deadline)
                outcomes.append({"label": label + "-native", "exit": native_probe["exit"]})
                print("Native probe:", label, "exit:", native_probe["exit"], flush=True)
                if result["raster"] is not None:
                    active_profile = profile
                    break
            # Framing and cancellation probes stay sandboxed, even if WebKit is blocked.
            profile = active_profile or evidence / "01-base.sb"
            for label, payload, behavior, timeout in [
                ("input-oversize", struct.pack(">I", 512 * 1024 + 1), "normal", 5),
                ("input-truncated", struct.pack(">I", 50) + b"{}", "eof", 5),
                ("hold-eof", None, "eof", 12),
                ("hold-cancel", None, "cancel", 12),
                ("hold-parent-timeout", None, "normal", 0.8),
            ]:
                if time.monotonic() + 15 > deadline:
                    break
                job_scratch = scratch / label
                job_scratch.mkdir(mode=0o700)
                hold_request = dict(request, revision=len(outcomes) + 2, fixture="hold")
                body = json.dumps(hold_request).encode()
                packet = payload if payload is not None else struct.pack(">I", len(body)) + body
                command = ["/usr/bin/sandbox-exec", "-f", str(profile),
                           "-D", "HELPER=" + str(helper), "-D", "SCRATCH=" + str(job_scratch), str(helper)]
                result = run_case(evidence / label, command, packet, clean_env(job_scratch), timeout, behavior)
                outcomes.append({"label": label, "exit": result["exit"], "timedOut": result["timedOut"]})
                print("Fixture:", label, "exit:", result["exit"], flush=True)
        finally:
            canary.close()
            write_json(evidence / "canary.json", {"port": canary.port, "hits": canary.hits,
                                                 "hitCount": len(canary.hits), "bound": "127.0.0.1 only"})
        files = [REPO / "renderer/macos/ProbeMain.swift", REPO / "renderer/macos/probe-base.sb",
                 REPO / "renderer/tests/raster.html", REPO / "tools/renderer/probe_r0.py",
                 REPO / "renderer/tests/native_probe.c", helper, native_helper, sentinel]
        files.extend(evidence.glob("*.sb"))
        write_json(evidence / "hashes.json", {str(path): digest(path) for path in files})
        write_json(evidence / "summary.json", {"outcomes": outcomes,
                                             "executionSeconds": time.monotonic() - started,
                                             "canaryHits": len(canary.hits),
                                             "productionCapability": False, "interactiveCapability": False,
                                             "hardAggregateMemoryEstablished": False})
        print("Finished:", evidence, flush=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
