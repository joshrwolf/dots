"""Neutral, temporary fixtures: never read or alter the user's Pi settings."""

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
SHA = "a" * 40


class PackagesTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="dots-pi-test-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.entries = {"example": {
            "url": "https://github.com/example/example.git", "ref": SHA, "checks": ["check"],
        }}
        self.agent = self.root / "agent"
        self.agent.mkdir()

    def package(self, relative, name):
        directory = self.root / relative
        directory.mkdir(parents=True)
        (directory / "package.json").write_text(json.dumps({
            "name": name, "pi": {"extensions": []}, "scripts": {"check": "node --test"},
        }))
        (directory / "package-lock.json").write_text(json.dumps({"name": name, "lockfileVersion": 3}))
        return directory

    def settings(self, packages):
        path = self.agent / "settings.json"
        path.write_text(json.dumps({"theme": "example", "packages": packages}))
        return path

    def test_registry_requires_full_sha(self):
        registry = self.root / "pi-sources.toml"
        for ref in ("main", "v1.0.0", "abc123", SHA):
            with self.subTest(ref=ref):
                registry.write_text(f'[sources.example]\nurl = "https://github.com/example/example.git"\nref = "{ref}"\nchecks = ["check"]\n')
                if ref == SHA:
                    self.assertEqual(manager.sources(self.root), self.entries)
                else:
                    with self.assertRaisesRegex(ValueError, "commit SHA"):
                        manager.sources(self.root)

    def test_missing_import_fails_before_install(self):
        self.package("pi-plugins/dots", "@example/dots")
        with self.assertRaisesRegex(ValueError, "pi-import SOURCE=example"):
            manager.packages(self.root, self.entries)

    def test_all_packages_and_checks_are_validated(self):
        self.package("pi-plugins/vendor/example", "example")
        self.package("pi-plugins/dots", "@example/dots")
        self.assertEqual(len(manager.packages(self.root, self.entries)), 2)
        self.entries["example"]["checks"] = ["missing"]
        with self.assertRaisesRegex(ValueError, "check script"):
            manager.packages(self.root, self.entries)

    def test_dirty_tree_blocks_subtree_before_fetch(self):
        with patch.object(manager.subprocess, "check_output", return_value="?? notes.txt\n"), patch.object(manager, "run") as run:
            with self.assertRaisesRegex(ValueError, "commit your changes first"):
                manager.subtree(self.root, self.entries, "import", "example")
            run.assert_not_called()

    def test_unknown_source_is_rejected_before_git(self):
        with patch.object(manager.subprocess, "check_output") as git:
            with self.assertRaisesRegex(ValueError, "choose SOURCE"):
                manager.subtree(self.root, self.entries, "import", "../escape")
            git.assert_not_called()

    def test_existing_import_is_not_replaced(self):
        self.package("pi-plugins/vendor/example", "example")
        with patch.object(manager.subprocess, "check_output", return_value=""), patch.object(manager, "run") as run:
            with self.assertRaisesRegex(ValueError, "already imported"):
                manager.subtree(self.root, self.entries, "import", "example")
            run.assert_not_called()

    def test_duplicate_npm_git_and_local_sources_fail_before_dependencies(self):
        directory = self.package("pi-plugins/vendor/example", "example")
        other = self.package("other/example", "example")
        for spec in ("npm:example@1.0.0", "git:github.com/example/example@v1", str(other)):
            with self.subTest(spec=spec), patch.object(manager, "run") as run:
                settings = self.settings([spec])
                before = settings.read_bytes()
                with self.assertRaisesRegex(ValueError, "Another copy"):
                    manager.install([(directory, ["check"])], self.agent)
                run.assert_not_called()
                self.assertEqual(before, settings.read_bytes())

    def test_install_uses_pi_cli_not_settings_replacement(self):
        directory = self.package("pi-plugins/dots", "@example/dots")
        settings = self.settings(["npm:unrelated@1.0.0"])
        before = settings.read_bytes()
        with patch.object(manager, "run") as run:
            manager.install([(directory, ["check"])], self.agent)
            self.assertEqual(run.call_args_list[0].args, (["npm", "ci", "--ignore-scripts"], directory))
            self.assertEqual(run.call_args_list[1].args, (["pi", "install", "--no-approve", str(directory.resolve())], directory))
        self.assertEqual(before, settings.read_bytes())

    def test_existing_filtered_local_registration_is_preserved(self):
        directory = self.package("pi-plugins/dots", "@example/dots")
        settings = self.settings([{"source": "../pi-plugins/dots", "prompts": []}])
        before = settings.read_bytes()
        with patch.object(manager, "run") as run:
            manager.install([(directory, ["check"])], self.agent)
            run.assert_called_once_with(["npm", "ci", "--ignore-scripts"], directory)
        self.assertEqual(before, settings.read_bytes())

    def test_failed_dependencies_do_not_register_packages(self):
        directory = self.package("pi-plugins/dots", "@example/dots")
        with patch.object(manager, "run", side_effect=subprocess.CalledProcessError(1, "npm")) as run:
            with self.assertRaises(subprocess.CalledProcessError):
                manager.install([(directory, ["check"])], self.agent)
            self.assertEqual(run.call_count, 1)
            self.assertFalse((self.agent / "settings.json").exists())


class SubtreeIntegrationTest(unittest.TestCase):
    def test_pinned_update_preserves_committed_local_patch(self):
        with tempfile.TemporaryDirectory(prefix="dots-pi-subtree-test-") as temp:
            root = Path(temp)
            upstream, dots = root / "upstream", root / "dots"

            def git(directory, *args):
                return subprocess.check_output(["git", *args], cwd=directory, text=True, stderr=subprocess.STDOUT).strip()

            for directory in (upstream, dots):
                directory.mkdir()
                git(directory, "init", "-q")
                git(directory, "config", "user.name", "Example")
                git(directory, "config", "user.email", "example@example.com")
                git(directory, "config", "commit.gpgsign", "false")
                git(directory, "config", "core.hooksPath", str(root / "no-hooks"))
                (directory / "README.md").write_text("Example\n")
                git(directory, "add", ".")
                git(directory, "commit", "-qm", "Initial")

            initial = git(upstream, "rev-parse", "HEAD")
            entries = {"example": {"url": str(upstream), "ref": initial, "checks": ["check"]}}
            # Only this neutral fixture uses a local upstream; public registry URLs require HTTPS.
            def quiet_run(args, cwd):
                subprocess.check_output(args, cwd=cwd, stderr=subprocess.STDOUT)

            with patch.object(manager, "run", side_effect=quiet_run):
                manager.subtree(dots, entries, "import", "example")
                patch_file = dots / "pi-plugins/vendor/example/local.txt"
                patch_file.write_text("Local customization\n")
                git(dots, "add", ".")
                git(dots, "commit", "-qm", "Local customization")
                (upstream / "README.md").write_text("Upstream improvement\n")
                git(upstream, "commit", "-qam", "Upstream improvement")
                entries["example"]["ref"] = git(upstream, "rev-parse", "HEAD")
                (upstream / "README.md").write_text("Newer, unselected revision\n")
                git(upstream, "commit", "-qam", "Do not import this revision")
                manager.subtree(dots, entries, "update", "example")

            self.assertEqual(patch_file.read_text(), "Local customization\n")
            self.assertEqual((patch_file.parent / "README.md").read_text(), "Upstream improvement\n")
            self.assertEqual(git(dots, "status", "--porcelain"), "")
            self.assertIn("git-subtree-split: " + entries["example"]["ref"], git(dots, "log", "--format=%B"))


if __name__ == "__main__":
    unittest.main()
