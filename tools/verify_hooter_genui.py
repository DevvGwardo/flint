#!/usr/bin/env python3
"""Read-only local artifact gate. Never runs, installs or activates the candidate."""
from __future__ import annotations

import argparse
from contextlib import contextmanager
from datetime import datetime, timezone
import hashlib
import hmac
import json
import os
from pathlib import Path, PurePosixPath
import plistlib
import re
import stat
import subprocess
import sys

import package_hooter_genui as package

MAX_MANIFEST_BYTES = 1024 * 1024
MAX_ENTRIES = 4096
MAX_FILE_BYTES = 512 * 1024 * 1024
MAX_TREE_BYTES = 2 * 1024 * 1024 * 1024
FIELDS = {
    "version", "created_utc", "git_head", "source_receipts", "packaged_files",
    "packaged_links", "distribution_signed", "notarized", "installed_or_activated",
    "attestation", "genui_commit",
}
REQUIRED_EXECUTABLES = {
    "Contents/MacOS/Hooter",
    "Contents/Helpers/hooter_genui.app/Contents/MacOS/hooter_genui",
}


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("Duplicate receipt field.")
        result[key] = value
    return result


def relative_name(name):
    if (not isinstance(name, str) or not name or len(name) > 1024
            or "\\" in name or any(ord(char) < 32 for char in name)):
        raise ValueError("Invalid receipt path.")
    path = PurePosixPath(name)
    if path.is_absolute() or ".." in path.parts or str(path) != name or name == ".":
        raise ValueError("Receipt path is not a normalized relative path.")
    return name


def digest_map(value):
    if not isinstance(value, dict) or not 0 < len(value) <= MAX_ENTRIES:
        raise ValueError("Invalid checksum map.")
    for name, checksum in value.items():
        relative_name(name)
        if not isinstance(checksum, str) or not re.fullmatch(r"[0-9a-f]{64}", checksum):
            raise ValueError("Invalid checksum.")


@contextmanager
def regular_read(path, limit):
    """Open without following the final link; FIFOs must not block open()."""
    flags = os.O_RDONLY | os.O_NONBLOCK | os.O_NOFOLLOW
    descriptor = os.open(path, flags)
    with os.fdopen(descriptor, "rb") as stream:
        metadata = os.fstat(stream.fileno())
        if not stat.S_ISREG(metadata.st_mode) or metadata.st_nlink != 1 or metadata.st_size > limit:
            raise ValueError("Receipt input is not a bounded, unlinked regular file.")
        yield stream


def load_manifest(path, expected_digest):
    if not re.fullmatch(r"[0-9a-f]{64}", expected_digest):
        raise ValueError("Supply a trusted, independently retained manifest SHA-256.")
    with regular_read(path, MAX_MANIFEST_BYTES) as stream:
        raw = stream.read(MAX_MANIFEST_BYTES + 1)
    if len(raw) > MAX_MANIFEST_BYTES or not hmac.compare_digest(
            hashlib.sha256(raw).hexdigest(), expected_digest):
        raise ValueError("Manifest does not match its independently supplied digest.")
    value = json.loads(raw.decode("utf-8"), object_pairs_hook=unique_object)
    if not isinstance(value, dict) or set(value) != FIELDS or type(value["version"]) is not int or value["version"] != 1:
        raise ValueError("Unsupported receipt schema.")
    if any(value[name] is not False for name in (
            "distribution_signed", "notarized", "installed_or_activated")):
        raise ValueError("Candidate receipt must retain all activation/distribution holds.")
    if value["genui_commit"] != package.GENUI_COMMIT or value["attestation"] != package.ATTESTATION:
        raise ValueError("Unexpected SDK or attestation policy.")
    if not isinstance(value["git_head"], str) or not re.fullmatch(r"[0-9a-f]{40}", value["git_head"]):
        raise ValueError("Invalid source revision.")
    if not isinstance(value["created_utc"], str):
        raise ValueError("Invalid receipt timestamp.")
    stamp = datetime.fromisoformat(value["created_utc"])
    if stamp.tzinfo is None or stamp.utcoffset() != timezone.utc.utcoffset(stamp):
        raise ValueError("Receipt timestamp must be UTC.")
    digest_map(value["source_receipts"])
    digest_map(value["packaged_files"])
    if not REQUIRED_EXECUTABLES <= value["packaged_files"].keys():
        raise ValueError("Required executables are missing from the receipt.")
    links = value["packaged_links"]
    if not isinstance(links, dict) or len(links) > MAX_ENTRIES:
        raise ValueError("Invalid symbolic-link map.")
    for name, target in links.items():
        relative_name(name)
        if not isinstance(target, str) or not target or len(target) > 1024 or "\0" in target:
            raise ValueError("Invalid symbolic-link target.")
    if value["packaged_files"].keys() & links.keys():
        raise ValueError("A receipt path cannot be both a file and a link.")
    return value


def artifact_receipts(bundle):
    if not stat.S_ISDIR(bundle.lstat().st_mode):
        raise ValueError("Candidate bundle must be a real directory.")
    bundle = bundle.resolve(strict=True)
    files, links = {}, {}
    total = 0
    for index, path in enumerate(bundle.rglob("*"), start=1):
        if index > MAX_ENTRIES:
            raise ValueError("Artifact tree exceeds its entry budget.")
        name = str(path.relative_to(bundle))
        metadata = path.lstat()
        if stat.S_ISLNK(metadata.st_mode):
            if not path.resolve(strict=True).is_relative_to(bundle):
                raise ValueError("Artifact link escapes the bundle.")
            links[name] = str(path.readlink())
        elif stat.S_ISREG(metadata.st_mode):
            total += metadata.st_size
            if total > MAX_TREE_BYTES:
                raise ValueError("Artifact tree exceeds its byte budget.")
            with regular_read(path, MAX_FILE_BYTES) as stream:
                files[name] = package.stream_digest(stream, MAX_FILE_BYTES)
        elif not stat.S_ISDIR(metadata.st_mode):
            raise ValueError("Artifact tree contains a special filesystem entry.")
    return files, links


def verify(manifest_path, expected_digest):
    if sys.platform != "darwin" or not manifest_path.is_absolute() or manifest_path.name != "manifest.json":
        raise ValueError("Use an absolute local macOS manifest.json path.")
    manifest = load_manifest(manifest_path, expected_digest)
    bundle = manifest_path.parent.resolve(strict=True) / "Hooter.app"
    files, links = artifact_receipts(bundle)
    if files != manifest["packaged_files"] or links != manifest["packaged_links"]:
        raise ValueError("Artifact file or symbolic-link receipts do not match.")
    # Source names come from trusted code, not from paths chosen by the candidate.
    if package.source_receipts() != manifest["source_receipts"]:
        raise ValueError("Recorded source inputs no longer match the checkout.")
    for name in REQUIRED_EXECUTABLES:
        if not os.access(bundle / name, os.X_OK):
            raise ValueError("Candidate executable is not executable.")
    helper = bundle / "Contents/Helpers/hooter_genui.app"
    subprocess.run(
        ["/usr/bin/codesign", "--verify", "--deep", "--strict", str(bundle)],
        check=True, capture_output=True, timeout=30,
    )
    result = subprocess.run(
        ["/usr/bin/codesign", "--display", "--entitlements", "-", "--xml", str(helper)],
        check=True, capture_output=True, timeout=30,
    )
    if plistlib.loads(result.stdout) != package.EXPECTED_ENTITLEMENTS:
        raise ValueError("Helper entitlement policy does not match.")
    # Detect ordinary drift during signature inspection. This is not a seal or
    # an activation-time race defense; an external immutable supervisor is held.
    if artifact_receipts(bundle) != (files, links):
        raise ValueError("Artifact changed during signature inspection.")
    return len(files), len(links), len(manifest["source_receipts"])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", required=True, type=Path)
    parser.add_argument("--expected-manifest-sha256", required=True)
    args = parser.parse_args()
    files, links, sources = verify(args.manifest, args.expected_manifest_sha256)
    print(f"Verified local integrity: {files} files, {links} links, {sources} source receipts.")
    print("Activation remains held. This is not build attestation, isolation or distribution approval.")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError, TypeError, RecursionError, subprocess.SubprocessError):
        # Never echo candidate-controlled paths, document contents or tool stderr.
        print("Artifact verification failed; candidate remains held.", file=sys.stderr)
        raise SystemExit(1)
