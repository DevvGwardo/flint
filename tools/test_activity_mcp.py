import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

import activity_mcp as activity


class ActivityTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.projects = self.root / "projects"
        self.sessions = self.root / "sessions"
        self.projects.mkdir()
        self.sessions.mkdir()

    def session(self, name, **changes):
        directory = self.sessions / name
        directory.mkdir()
        value = dict(workspace=str(self.projects / "one"), created_at=10,
                     updated_at=400000, title="PRIVATE TITLE")
        value.update(changes)
        (directory / "meta.json").write_text(json.dumps(value))
        (directory / "history.json").write_text("PRIVATE TRANSCRIPT")
        return directory

    def query(self, arguments=None):
        return activity.Activity([str(self.projects)], str(self.sessions)).query(
            arguments or {}, now=410000)

    def test_explicit_roots_sources_and_metadata_only(self):
        (self.projects / "one" / ".git").mkdir(parents=True)
        self.session("recent")
        self.session("old", updated_at=1)
        self.session("future", updated_at=999999)
        result = self.query()
        self.assertEqual(len(result["projects"]), 1)
        self.assertEqual(len(result["sessions"]), 1)
        self.assertTrue(result["sessions"][0]["source"].endswith("/recent/meta.json"))
        self.assertNotIn("PRIVATE", json.dumps(result))
        self.assertNotIn("created_at", result["projects"][0])
        self.assertFalse(result["incomplete"])

    def test_empty_scope_does_not_search_home_or_scratch(self):
        self.session("recent")
        result = activity.Activity().query({})
        self.assertEqual(result["projects"], [])
        self.assertEqual(result["sessions"], [])
        self.assertIn("Partial", result["note"])

    def test_missing_root_is_not_evidence_of_inactivity(self):
        result = activity.Activity([str(self.root / "missing")]).query({})
        self.assertTrue(result["incomplete"])
        self.assertEqual(result["unavailable"][0]["error"], "FileNotFoundError")

    def test_rejects_model_selected_roots_and_invalid_bounds(self):
        for arguments in ({"path": "/etc"}, {"days": True}, {"days": 31},
                          {"limit": 0}, {"limit": 51}, {"days": "3"}, []):
            with self.subTest(arguments=arguments), self.assertRaises(ValueError):
                activity.Activity().query(arguments)

    def test_symlinked_directories_and_metadata_are_not_followed(self):
        outside = self.root / "outside"
        outside.mkdir()
        (outside / ".git").mkdir()
        (self.projects / "alias").symlink_to(outside, target_is_directory=True)
        target = self.session("target")
        linked = self.sessions / "linked"
        linked.mkdir()
        (linked / "meta.json").symlink_to(target / "meta.json")
        result = self.query()
        self.assertEqual(result["projects"], [])
        self.assertEqual(len(result["sessions"]), 1)
        self.assertTrue(result["incomplete"])

    def test_limits_and_corrupt_or_special_metadata(self):
        for name in ("one", "two", "three"):
            self.session(name)
        corrupt = self.session("corrupt")
        (corrupt / "meta.json").write_text("[" * 20000)
        fifo = self.sessions / "fifo"
        fifo.mkdir()
        os.mkfifo(fifo / "meta.json")
        result = self.query({"limit": 1})
        self.assertEqual(len(result["sessions"]), 1)
        self.assertTrue(result["incomplete"])
        self.assertLessEqual(len(json.dumps(result).encode()), activity.MAX_OUTPUT)

    def test_scan_budget_reports_incomplete(self):
        from unittest.mock import patch
        self.session("one")
        self.session("two")
        with patch.object(activity, "MAX_ENTRIES", 1):
            self.assertTrue(self.query()["incomplete"])

    def test_swapped_ancestor_cannot_escape_the_configured_root(self):
        parent = self.root / "container"
        root = parent / "projects"
        root.mkdir(parents=True)
        configured = activity.Activity([str(root)])
        moved = self.root / "moved"
        parent.rename(moved)
        outside = self.root / "outside"
        (outside / "projects" / "private" / ".git").mkdir(parents=True)
        parent.symlink_to(outside, target_is_directory=True)
        result = configured.query({})
        self.assertEqual(result["projects"], [])
        self.assertTrue(result["incomplete"])
        self.assertTrue(result["unavailable"])

    def test_deep_or_invalid_metadata_does_not_crash_the_server(self):
        directory = self.session("deep")
        (directory / "meta.json").write_text("[" * 1200 + "]" * 1200)
        self.assertTrue(self.query()["incomplete"])
        (directory / "meta.json").write_text('{"workspace": [], "updated_at": 10}')
        self.assertTrue(self.query()["incomplete"])

    def test_output_budget_does_not_silently_drop_records(self):
        from unittest.mock import patch
        for name in ("one", "two", "three"):
            self.session(name)
        with patch.object(activity, "MAX_OUTPUT", 750):
            result = self.query()
        self.assertLessEqual(len(json.dumps(result).encode()), 750)
        self.assertTrue(result["incomplete"])

    def test_stdio_mcp_roundtrip_and_invalid_input(self):
        requests = [
            {"id": 1, "method": "initialize"},
            {"method": "notifications/initialized"},
            {"id": 2, "method": "tools/list"},
            {"id": 3, "method": "tools/call",
             "params": {"name": "project_activity", "arguments": {}}},
            {"id": 4, "method": "tools/call",
             "params": {"name": "project_activity", "arguments": {"path": "/etc"}}},
            {"id": 5, "method": "ping"},
        ]
        process = subprocess.run(
            [sys.executable, str(Path(activity.__file__).resolve())],
            input="\n".join(json.dumps(item) for item in requests) + "\n",
            text=True, capture_output=True, timeout=5, check=True)
        replies = [json.loads(line) for line in process.stdout.splitlines()]
        self.assertEqual([reply["id"] for reply in replies], [1, 2, 3, 4, 5])
        self.assertTrue(replies[1]["result"]["tools"][0]["annotations"]["readOnlyHint"])
        self.assertFalse(replies[2]["result"]["isError"])
        self.assertIn("error", replies[3])
        self.assertEqual(replies[4]["result"], {})


if __name__ == "__main__":
    unittest.main()
