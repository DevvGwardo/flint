#!/usr/bin/env python3
"""Package an already validated local build, without replacing installed apps."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import plistlib
import re
import shutil
import subprocess
import sys
from datetime import datetime, timezone

ROOT = Path(__file__).resolve().parent.parent
GENUI_COMMIT = "3a9721bf57d5e45877ba2093379c92b522c77392"
ATTESTATION = "Local checksums, not a signed build attestation."

EXPECTED_ENTITLEMENTS = {
    "com.apple.security.app-sandbox": True,
    "com.apple.security.temporary-exception.files.home-relative-path.read-write":
        ["/.flint/genui-runtime/"],
    "com.apple.security.temporary-exception.sbpl":
        ['(allow network-outbound (home-subpath "/.flint/genui-runtime/"))'],
}


def stream_digest(stream, max_bytes: int | None = None) -> str:
    value = hashlib.sha256()
    size = 0
    for chunk in iter(lambda: stream.read(1024 * 1024), b""):
        size += len(chunk)
        if max_bytes is not None and size > max_bytes:
            raise ValueError("Checksum input exceeds its byte budget.")
        value.update(chunk)
    return value.hexdigest()


def digest(path: Path) -> str:
    with path.open("rb") as stream:
        return stream_digest(stream)


def check_tree(root: Path) -> None:
    """Retain normal framework links, never package an outside-tree target."""
    for path in root.rglob("*"):
        if path.is_symlink():
            if not path.resolve(strict=True).is_relative_to(root):
                raise ValueError("Companion contains an outside-tree symlink.")
        elif not path.is_file() and not path.is_dir():
            raise ValueError("Companion contains a non-regular filesystem entry.")


def source_receipts() -> dict[str, str]:
    paths = [ROOT / "Cargo.toml", ROOT / "Cargo.lock"]
    paths.extend(ROOT / "tools" / name for name in (
        "package_hooter_genui.py", "verify_hooter_genui.py",
        "test_package_hooter_genui.py", "test_verify_hooter_genui.py",
    ))
    paths.extend((ROOT / "crates").rglob("*.rs"))
    paths.extend((ROOT / "crates").rglob("Cargo.toml"))
    for name in ("pubspec.yaml", "pubspec.lock"):
        paths.append(ROOT / "genui" / name)
    for directory in ("lib", "test"):
        paths.extend((ROOT / "genui" / directory).rglob("*.dart"))
    macos = ROOT / "genui/macos"
    extensions = {
        ".swift", ".xcconfig", ".entitlements", ".plist", ".pbxproj",
        ".xcscheme", ".xcworkspacedata", ".xib", ".json", ".png",
    }
    for directory in ("Runner", "RunnerTests", "Runner.xcodeproj", "Runner.xcworkspace"):
        paths.extend(
            path for path in (macos / directory).rglob("*")
            if path.suffix in extensions and "xcuserdata" not in path.parts
        )
    for extension in ("*.swift", "*.xcconfig"):
        paths.extend((macos / "Flutter").glob(extension))
    receipts = {}
    root = ROOT.resolve(strict=True)
    for path in sorted(set(paths)):
        if not path.is_file():
            continue
        if not path.resolve(strict=True).is_relative_to(root):
            raise ValueError("Source receipt contains an outside-checkout target.")
        receipts[str(path.relative_to(ROOT))] = digest(path)
    return receipts


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path,
                        help="New absolute directory; existing output is never overwritten.")
    parser.add_argument("--companion", type=Path, default=ROOT / "genui/build/macos/Build/Products/Release/hooter_genui.app")
    args = parser.parse_args()
    if sys.platform != "darwin":
        raise ValueError("This local bundle packager requires macOS codesign.")
    if not all(path.is_absolute() for path in (args.binary, args.output, args.companion)):
        raise ValueError("All artifact paths must be absolute.")
    binary = args.binary.resolve(strict=True)
    companion = args.companion.resolve(strict=True)
    output = args.output.resolve(strict=False)
    if not binary.is_file() or not os.access(binary, os.X_OK):
        raise ValueError("Native binary must be an existing executable.")
    helper_binary = companion / "Contents/MacOS/hooter_genui"
    if not helper_binary.is_file() or not os.access(helper_binary, os.X_OK):
        raise ValueError("Companion must be a complete built release app.")
    if output.exists() or output.is_symlink():
        raise ValueError("Output already exists; choose a new versioned directory.")
    if output.is_relative_to(companion) or output.is_relative_to(binary.parent):
        raise ValueError("Output must be separate from the input artifact directories.")
    check_tree(companion)
    subprocess.run(["/usr/bin/codesign", "--verify", "--deep", "--strict", str(companion)], check=True)
    entitlements = subprocess.run(
        ["/usr/bin/codesign", "--display", "--entitlements", "-", "--xml", str(companion)],
        check=True, capture_output=True,
    )
    if plistlib.loads(entitlements.stdout) != EXPECTED_ENTITLEMENTS:
        raise ValueError("Companion entitlements differ from the narrow release policy.")
    with (companion / "Contents/Info.plist").open("rb") as stream:
        helper_info = plistlib.load(stream)
        if helper_info.get("CFBundleExecutable") != "hooter_genui":
            raise ValueError("Unexpected companion executable.")
    load_commands = subprocess.run(
        ["/usr/bin/otool", "-l", str(binary)], check=True,
        capture_output=True, text=True,
    ).stdout
    minimums = re.findall(r"\bminos\s+([0-9.]+)", load_commands)
    if not minimums:
        raise ValueError("Native binary has no verified macOS build-version command.")
    minimums.append(helper_info.get("LSMinimumSystemVersion", ""))
    if any(not re.fullmatch(r"[0-9]+(?:\.[0-9]+){1,2}", value) for value in minimums):
        raise ValueError("Cannot verify the artifacts' minimum macOS version.")
    minimum_macos = max(minimums, key=lambda value: tuple(map(int, value.split("."))))
    receipts = source_receipts()
    output.parent.mkdir(parents=True, exist_ok=True)
    output.mkdir(mode=0o700)  # Exclusive creation: never modify a prior artifact.
    bundle = output / "Hooter.app"
    contents = bundle / "Contents"
    (contents / "MacOS").mkdir(parents=True)
    (contents / "Helpers").mkdir()
    shutil.copy2(binary, contents / "MacOS/Hooter")
    shutil.copytree(companion, contents / "Helpers/hooter_genui.app", symlinks=True)
    # Absolute links that were inside the input tree can escape after copying.
    check_tree(bundle)
    stamp = datetime.now(timezone.utc)
    with (contents / "Info.plist").open("wb") as stream:
        plistlib.dump({
            "CFBundleExecutable": "Hooter",
            "CFBundleName": "Hooter",
            "CFBundleDisplayName": "Hooter (local)",
            "CFBundleIdentifier": "dev.hooter.local",
            "CFBundlePackageType": "APPL",
            "CFBundleShortVersionString": "0.1.0",
            "CFBundleVersion": stamp.strftime("%Y.%m.%d"),
            "LSMinimumSystemVersion": minimum_macos,
            "NSPrincipalClass": "NSApplication",
            "NSHighResolutionCapable": True,
            "NSSupportsAutomaticGraphicsSwitching": True,
        }, stream)
    # The helper retains its verified narrow App Sandbox signature. Sign only
    # this newly created outer app; no identity, keychain or installed app changes.
    subprocess.run(["/usr/bin/codesign", "--force", "--sign", "-", "--timestamp=none", str(bundle)], check=True)
    subprocess.run(["/usr/bin/codesign", "--verify", "--deep", "--strict", str(bundle)], check=True)
    files = {
        str(path.relative_to(bundle)): digest(path)
        for path in sorted(bundle.rglob("*"))
        if path.is_file() and not path.is_symlink()
    }
    links = {
        str(path.relative_to(bundle)): str(path.readlink())
        for path in sorted(bundle.rglob("*"))
        if path.is_symlink()
    }
    head = subprocess.run(
        ["git", "-C", str(ROOT), "rev-parse", "HEAD"], check=True,
        capture_output=True, text=True,
    ).stdout.strip()
    manifest = {
        "version": 1,
        "created_utc": stamp.isoformat(),
        "git_head": head,
        "source_receipts": receipts,
        "packaged_files": files,
        "packaged_links": links,
        "distribution_signed": False,
        "notarized": False,
        "installed_or_activated": False,
        "attestation": ATTESTATION,
        "genui_commit": GENUI_COMMIT,
    }
    with (output / "manifest.json").open("x", encoding="utf-8") as stream:
        json.dump(manifest, stream, indent=2, sort_keys=True)
        stream.write("\n")
    print(f"Local bundle, not installed or distribution-notarized: {bundle}")
    print(f"Checksums and source receipts: {output / 'manifest.json'}")
    print(f"Manifest SHA-256 (retain outside the candidate): {digest(output / 'manifest.json')}")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        print(f"Packaging failed: {error}", file=sys.stderr)
        raise SystemExit(1)
