"""Provider-free tests for the read-only, externally anchored artifact gate."""
import hashlib
import json
from pathlib import Path
import plistlib
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

import package_hooter_genui as package
import verify_hooter_genui as gate


class VerificationTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.manifest_path = self.root / "manifest.json"
        self.bundle = self.root / "Hooter.app"
        for name in gate.REQUIRED_EXECUTABLES:
            path = self.bundle / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(b"fixture executable, never run")
            path.chmod(0o700)
        resources = self.bundle / "Contents/Resources"
        resources.mkdir()
        for name in ["a", "b"]:
            (resources / name).write_bytes(name.encode())
        (resources / "current").symlink_to("a")
        files, links = gate.artifact_receipts(self.bundle)
        self.sources = {"Cargo.toml": hashlib.sha256(b"fixture").hexdigest()}
        self.manifest = {
            "version": 1,
            "created_utc": "2026-10-14T08:13:00+00:00",
            "git_head": "a" * 40,
            "source_receipts": self.sources,
            "packaged_files": files,
            "packaged_links": links,
            "distribution_signed": False,
            "notarized": False,
            "installed_or_activated": False,
            "attestation": package.ATTESTATION,
            "genui_commit": package.GENUI_COMMIT,
        }
        self.write_manifest()
        self.calls = []
        self.entitlements = package.EXPECTED_ENTITLEMENTS

    def write_manifest(self, raw=None):
        data = raw if raw is not None else json.dumps(self.manifest).encode()
        self.manifest_path.write_bytes(data)
        self.anchor = hashlib.sha256(data).hexdigest()

    def run_command(self, args, **kwargs):
        self.calls.append(args)
        stdout = plistlib.dumps(self.entitlements) if "--display" in args else b""
        return subprocess.CompletedProcess(args, 0, stdout=stdout)

    def verify(self, anchor=None, path=None):
        with patch.object(sys, "platform", "darwin"), \
             patch.object(package, "source_receipts", return_value=self.sources), \
             patch.object(gate.subprocess, "run", side_effect=self.run_command):
            return gate.verify(path or self.manifest_path, anchor or self.anchor)

    def test_valid_gate_is_read_only_and_never_executes_candidate(self):
        before = {p: p.read_bytes() for p in self.bundle.rglob("*") if p.is_file()}
        self.assertEqual(self.verify(), (4, 1, 1))
        self.assertEqual(before, {p: p.read_bytes() for p in self.bundle.rglob("*") if p.is_file()})
        self.assertTrue(all(call[0] == "/usr/bin/codesign" for call in self.calls))
        self.assertTrue(all("--force" not in call and "--sign" not in call for call in self.calls))

    def test_changed_manifest_cannot_supply_its_own_new_anchor(self):
        old = self.anchor
        self.manifest["git_head"] = "b" * 40
        self.write_manifest()
        with self.assertRaises(ValueError):
            self.verify(anchor=old)
        self.assertFalse(self.calls)

    def test_duplicate_fields_fail_even_with_matching_anchor(self):
        raw = json.dumps(self.manifest).encode()
        self.write_manifest(b'{"version":1,' + raw[1:])
        with self.assertRaises(ValueError):
            self.verify()

    def test_unknown_or_missing_fields_are_not_legacy_adapters(self):
        for field in ["unknown", "packaged_links"]:
            value = dict(self.manifest)
            if field == "unknown":
                value[field] = "not supported"
            else:
                del value[field]
            self.write_manifest(json.dumps(value).encode())
            with self.assertRaises(ValueError):
                self.verify()

    def test_activation_distribution_and_bad_sdk_flags_are_rejected(self):
        for field, value in [
            ("installed_or_activated", True), ("distribution_signed", True),
            ("notarized", True), ("version", True), ("genui_commit", "wrong"),
            ("attestation", "trusted build"), ("created_utc", "2026-10-14"),
        ]:
            changed = {**self.manifest, field: value}
            self.write_manifest(json.dumps(changed).encode())
            with self.assertRaises(ValueError, msg=field):
                self.verify()

    def test_artifact_tamper_added_and_removed_files_fail(self):
        executable = self.bundle / "Contents/MacOS/Hooter"
        executable.write_bytes(b"tampered")
        with self.assertRaises(ValueError):
            self.verify()
        executable.write_bytes(b"fixture executable, never run")
        extra = self.bundle / "extra"
        extra.write_bytes(b"not recorded")
        with self.assertRaises(ValueError):
            self.verify()
        extra.unlink()
        executable.unlink()
        with self.assertRaises(ValueError):
            self.verify()

    def test_inside_tree_link_retarget_is_detected_without_file_changes(self):
        link = self.bundle / "Contents/Resources/current"
        link.unlink()
        link.symlink_to("b")
        with self.assertRaises(ValueError):
            self.verify()
        self.assertFalse(self.calls)

    def test_outside_link_and_fifo_fail_before_hashing(self):
        link = self.bundle / "Contents/Resources/current"
        link.unlink()
        link.symlink_to(self.manifest_path)
        with self.assertRaises(ValueError):
            self.verify()
        link.unlink()
        import os
        os.mkfifo(self.bundle / "fifo")
        with self.assertRaises(ValueError):
            self.verify()

    def test_missing_or_malformed_executable_receipts_are_rejected(self):
        for name in gate.REQUIRED_EXECUTABLES:
            value = {**self.manifest, "packaged_files": dict(self.manifest["packaged_files"])}
            del value["packaged_files"][name]
            self.write_manifest(json.dumps(value).encode())
            with self.assertRaises(ValueError):
                self.verify()

    def test_paths_cannot_escape_or_alias_the_declared_namespace(self):
        for name in ["../outside", "/outside", "a//b", "a/./b", ".", "a\\b", "a\0b"]:
            value = {**self.manifest, "packaged_files": {**self.manifest["packaged_files"], name: "a" * 64}}
            self.write_manifest(json.dumps(value).encode())
            with self.assertRaises(ValueError, msg=name):
                self.verify()
        self.assertFalse(self.calls)

    def test_recorded_source_drift_is_not_accepted(self):
        self.sources = {"Cargo.toml": "b" * 64}
        with self.assertRaises(ValueError):
            self.verify()

    def test_unsafe_entitlements_and_signature_failure_remain_held(self):
        self.entitlements = {**self.entitlements, "com.apple.security.network.client": True}
        with self.assertRaises(ValueError):
            self.verify()
        with patch.object(sys, "platform", "darwin"), \
             patch.object(package, "source_receipts", return_value=self.sources), \
             patch.object(gate.subprocess, "run", side_effect=subprocess.CalledProcessError(1, "codesign")):
            with self.assertRaises(subprocess.CalledProcessError):
                gate.verify(self.manifest_path, self.anchor)

    def test_drift_during_signature_inspection_is_rejected(self):
        command = self.run_command

        def drift(args, **kwargs):
            result = command(args, **kwargs)
            if "--display" in args:
                (self.bundle / "Contents/MacOS/Hooter").write_bytes(b"changed during inspection")
            return result

        with patch.object(sys, "platform", "darwin"), \
             patch.object(package, "source_receipts", return_value=self.sources), \
             patch.object(gate.subprocess, "run", side_effect=drift):
            with self.assertRaises(ValueError):
                gate.verify(self.manifest_path, self.anchor)

    def test_size_and_entry_budgets_apply_to_real_reads(self):
        with patch.object(gate, "MAX_MANIFEST_BYTES", 8):
            with self.assertRaises(ValueError):
                self.verify()
        with patch.object(gate, "MAX_ENTRIES", 2):
            with self.assertRaises(ValueError):
                gate.artifact_receipts(self.bundle)
        with patch.object(gate, "MAX_FILE_BYTES", 8):
            with self.assertRaises(ValueError):
                gate.artifact_receipts(self.bundle)
        with patch.object(gate, "MAX_TREE_BYTES", 8):
            with self.assertRaises(ValueError):
                gate.artifact_receipts(self.bundle)

    def test_manifest_links_and_hardlinks_are_not_followed(self):
        alias = self.root / "alias.json"
        alias.symlink_to(self.manifest_path)
        with self.assertRaises(OSError):
            gate.load_manifest(alias, self.anchor)
        import os
        os.link(self.manifest_path, self.root / "hardlink.json")
        with self.assertRaises(ValueError):
            self.verify()

    def test_artifact_hardlinks_are_refused(self):
        import os
        os.link(self.bundle / "Contents/MacOS/Hooter", self.root / "other")
        with self.assertRaises(ValueError):
            self.verify()


if __name__ == "__main__":
    unittest.main()
