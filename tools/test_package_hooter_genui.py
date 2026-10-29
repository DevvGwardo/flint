"""Provider-free packaging safety checks; real codesign is a separate gate."""
import json
from pathlib import Path
import plistlib
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

import package_hooter_genui as package


class PackageTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.binary = self.root / "release/hooter"
        self.binary.parent.mkdir()
        self.binary.write_bytes(b"native test fixture")
        self.binary.chmod(0o700)
        self.helper = self.root / "helper.app"
        executable = self.helper / "Contents/MacOS/hooter_genui"
        executable.parent.mkdir(parents=True)
        executable.write_bytes(b"helper test fixture")
        executable.chmod(0o700)
        (self.helper / "Contents/Info.plist").write_bytes(plistlib.dumps({
            "CFBundleExecutable": "hooter_genui",
            "LSMinimumSystemVersion": "13.0",
        }))
        self.output = self.root / "artifacts/build-one"
        self.entitlements = package.EXPECTED_ENTITLEMENTS
        self.load_commands = "cmd LC_BUILD_VERSION\nplatform MACOS\nminos 14.0\n"

    def run_command(self, args, **kwargs):
        stdout = ""
        if args[0] == "/usr/bin/codesign" and "--display" in args:
            stdout = plistlib.dumps(self.entitlements)
        elif args[0] == "/usr/bin/otool":
            stdout = self.load_commands
        elif args[0] == "git":
            stdout = "fixture-commit\n"
        return subprocess.CompletedProcess(args, 0, stdout=stdout)

    def package(self):
        arguments = [
            "package", "--binary", str(self.binary), "--companion", str(self.helper),
            "--output", str(self.output),
        ]
        with patch.object(sys, "argv", arguments), patch.object(sys, "platform", "darwin"), \
             patch.object(package.subprocess, "run", side_effect=self.run_command), \
             patch.object(package, "source_receipts", return_value={"fixture.rs": "digest"}):
            return package.main()

    def test_new_bundle_preserves_inputs_and_records_held_activation(self):
        self.assertEqual(self.package(), 0)
        app = self.output / "Hooter.app"
        self.assertEqual((app / "Contents/MacOS/Hooter").read_bytes(), self.binary.read_bytes())
        self.assertEqual(self.binary.read_bytes(), b"native test fixture")
        self.assertTrue((app / "Contents/Helpers/hooter_genui.app/Contents/MacOS/hooter_genui").is_file())
        info = plistlib.loads((app / "Contents/Info.plist").read_bytes())
        self.assertEqual(info["LSMinimumSystemVersion"], "14.0")
        manifest = json.loads((self.output / "manifest.json").read_text())
        self.assertFalse(manifest["installed_or_activated"])
        self.assertFalse(manifest["distribution_signed"])
        self.assertFalse(manifest["notarized"])
        self.assertEqual(manifest["source_receipts"], {"fixture.rs": "digest"})
        self.assertEqual(manifest["packaged_links"], {})

    def test_internal_framework_link_targets_are_recorded(self):
        resources = self.helper / "Contents/Resources"
        resources.mkdir()
        (resources / "original.txt").write_text("fixture")
        (resources / "current").symlink_to("original.txt")
        self.package()
        manifest = json.loads((self.output / "manifest.json").read_text())
        self.assertEqual(manifest["packaged_links"], {
            "Contents/Helpers/hooter_genui.app/Contents/Resources/current": "original.txt",
        })

    def test_existing_output_is_never_overwritten(self):
        self.output.mkdir(parents=True)
        sentinel = self.output / "keep"
        sentinel.write_text("user work")
        with self.assertRaises(ValueError):
            self.package()
        self.assertEqual(sentinel.read_text(), "user work")

    def test_outside_companion_link_is_rejected_before_output_creation(self):
        (self.helper / "escape").symlink_to(self.binary)
        with self.assertRaises(ValueError):
            self.package()
        self.assertFalse(self.output.exists())

    def test_absolute_input_link_that_escapes_after_copy_is_rejected(self):
        (self.helper / "input-only-link").symlink_to(
            self.helper / "Contents/MacOS/hooter_genui",
        )
        with self.assertRaises(ValueError):
            self.package()
        self.assertFalse((self.output / "manifest.json").exists())
        self.assertTrue(self.binary.exists())

    def test_unsafe_release_entitlements_fail_closed(self):
        self.entitlements = {**self.entitlements, "com.apple.security.network.client": True}
        with self.assertRaises(ValueError):
            self.package()
        self.assertFalse(self.output.exists())

    def test_missing_native_deployment_metadata_is_not_guessed(self):
        self.load_commands = ""
        with self.assertRaises(ValueError):
            self.package()
        self.assertFalse(self.output.exists())

    def test_source_receipts_include_companion_native_build_inputs(self):
        names = [
            "genui/macos/Runner/AppDelegate.swift",
            "genui/macos/Runner/Configs/Release.xcconfig",
            "genui/macos/Runner/Info.plist",
            "genui/macos/Runner/Base.lproj/MainMenu.xib",
            "genui/macos/Runner/Assets.xcassets/AppIcon.appiconset/app_icon_16.png",
            "genui/macos/Runner/Assets.xcassets/AppIcon.appiconset/Contents.json",
            "genui/macos/Runner.xcodeproj/project.pbxproj",
            "genui/macos/Runner.xcodeproj/xcshareddata/xcschemes/Runner.xcscheme",
            "genui/macos/Runner.xcworkspace/contents.xcworkspacedata",
            "genui/macos/Flutter/Flutter-Release.xcconfig",
            "genui/macos/Flutter/GeneratedPluginRegistrant.swift",
        ]
        excluded = "genui/macos/Runner.xcodeproj/xcuserdata/user/Settings.plist"
        for name in [*names, excluded]:
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(b"fixture build input")
        with patch.object(package, "ROOT", self.root):
            receipts = package.source_receipts()
        self.assertEqual(set(receipts), set(names))
        self.assertTrue(all(len(value) == 64 for value in receipts.values()))

    def test_source_receipts_reject_outside_checkout_aliases(self):
        path = self.root / "crates/test/alias.rs"
        path.parent.mkdir(parents=True)
        with tempfile.TemporaryDirectory() as outside:
            target = Path(outside) / "outside.txt"
            target.write_bytes(b"fixture outside data")
            path.symlink_to(target)
            with patch.object(package, "ROOT", self.root), self.assertRaises(ValueError):
                package.source_receipts()


if __name__ == "__main__":
    unittest.main()
