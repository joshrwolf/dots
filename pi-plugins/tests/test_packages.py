"""Neutral temporary fixtures; never read or alter the user's Pi settings."""

import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location(
    "pi_packages", Path(__file__).resolve().parents[2] / "scripts/pi-packages.py",
)
manager = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(manager)


class PackagesTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="dots-pi-test-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.agent = self.root / "agent"
        self.agent.mkdir()
        self.upstream = {"example": "npm:example@1.2.3"}
        self.directory = self.package("pi-plugins/dots", "@example/dots")
        self.local = [(self.directory, ["check"])]

    def package(self, relative, name):
        directory = self.root / relative
        directory.mkdir(parents=True)
        (directory / "package.json").write_text(json.dumps({
            "name": name, "pi": {"extensions": []}, "scripts": {"check": "node --test"},
        }))
        (directory / "package-lock.json").write_text(json.dumps({"name": name, "lockfileVersion": 3}))
        return directory

    def settings(self, entries):
        path = self.agent / "settings.json"
        path.write_text(json.dumps({"theme": "example", "packages": entries}))
        return path

    def test_exact_pins_required(self):
        for spec in ("npm:example", "npm:example@latest", "npm:example@^1.2.3", "git:example", 12):
            (self.root / "pi-packages.json").write_text(json.dumps({"npm": [spec]}))
            with self.subTest(spec=spec), self.assertRaisesRegex(ValueError, "exact npm version"):
                manager.sources(self.root)
        (self.root / "pi-packages.json").write_text(json.dumps({"npm": ["npm:@example/tools@1.2.3"]}))
        self.assertEqual(manager.sources(self.root), {"@example/tools": "npm:@example/tools@1.2.3"})

    def test_duplicate_pins_rejected(self):
        (self.root / "pi-packages.json").write_text(json.dumps({"npm": ["npm:example@1.2.3", "npm:example@2.0.0"]}))
        with self.assertRaisesRegex(ValueError, "Duplicate"):
            manager.sources(self.root)

    def test_companion_is_validated(self):
        self.assertEqual(manager.packages(self.root), self.local)
        (self.directory / "package-lock.json").write_text('{"name":"wrong"}')
        with self.assertRaisesRegex(ValueError, "lockfile"):
            manager.packages(self.root)

    def test_conflicts_fail_before_dependencies(self):
        old = self.package("vendor/example", "example")
        other = self.package("other/dots", "@example/dots")
        for spec in (str(old), "npm:example@1.0.0", "git:github.com/example/example@v1", str(other), "npm:@example/dots@1.0.0"):
            with self.subTest(spec=spec), patch.object(manager, "run") as run:
                settings = self.settings([spec])
                before = settings.read_bytes()
                with self.assertRaisesRegex(ValueError, "Another copy"):
                    manager.install(self.local, self.upstream, self.agent)
                run.assert_not_called()
                self.assertEqual(before, settings.read_bytes())

    def test_removed_vendor_directory_still_detected(self):
        self.settings(["../vendor/example"])
        with self.assertRaisesRegex(ValueError, "Another copy"):
            manager.check_registrations(self.local, self.upstream, self.agent)

    def test_duplicate_registration_rejected(self):
        self.settings(["npm:example@1.2.3", "npm:example@1.2.3"])
        with self.assertRaisesRegex(ValueError, "Another copy"):
            manager.check_registrations(self.local, self.upstream, self.agent)

    def test_install_uses_cli_and_preserves_unrelated_settings(self):
        settings = self.settings(["npm:unrelated@1.0.0"])
        before = settings.read_bytes()
        with patch.object(manager, "run") as run:
            manager.install(self.local, self.upstream, self.agent)
            self.assertEqual([call.args[0] for call in run.call_args_list], [
                ["npm", "ci", "--ignore-scripts"],
                ["pi", "install", "--no-approve", "npm:example@1.2.3"],
                ["pi", "install", "--no-approve", str(self.directory.resolve())],
            ])
        self.assertEqual(before, settings.read_bytes())

    def test_existing_filtered_registrations_preserved(self):
        settings = self.settings([
            {"source": "../pi-plugins/dots", "prompts": []},
            {"source": "npm:example@1.2.3", "skills": []},
        ])
        before = settings.read_bytes()
        with patch.object(manager, "run") as run:
            manager.install(self.local, self.upstream, self.agent)
            self.assertEqual([call.args[0] for call in run.call_args_list], [
                ["npm", "ci", "--ignore-scripts"],
                ["pi", "update", "--no-approve", "npm:example@1.2.3"],
            ])
        self.assertEqual(before, settings.read_bytes())

    def test_dependency_failure_does_not_register(self):
        with patch.object(manager, "run", side_effect=subprocess.CalledProcessError(1, "npm")) as run:
            with self.assertRaises(subprocess.CalledProcessError):
                manager.install(self.local, self.upstream, self.agent)
            self.assertEqual(run.call_count, 1)
            self.assertFalse((self.agent / "settings.json").exists())

    def test_scoped_identity(self):
        for spec in ("npm:@example/tools", "npm:@example/tools@1.2.3"):
            self.assertEqual(manager.source_identity(spec, self.agent)[0], "@example/tools")


if __name__ == "__main__":
    unittest.main()
