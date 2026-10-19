"""Release policy tests; Cargo, Stow, and host registration have integration coverage."""
import errno
import importlib.util
import shutil
from pathlib import Path
import tempfile
import tomllib
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("deploy", Path(__file__).with_name("main.py"))
deploy = importlib.util.module_from_spec(spec)
spec.loader.exec_module(deploy)


class ReleaseTests(unittest.TestCase):
    def setUp(self):
        scratch = tempfile.TemporaryDirectory(prefix="dots-herdr-release-")
        self.addCleanup(scratch.cleanup)
        self.home = Path(scratch.name) / "home"
        self.home.mkdir()
        self.repository = Path(scratch.name) / "repo"
        self.manifest = self.repository / "src/herdr/example/herdr-plugin.toml"
        self.manifest.parent.mkdir(parents=True)
        self.manifest.write_text('id = "herdr-example"\n\n[[build]]\ncommand = ["mise", "run", ":prepare"]\n\n[[actions]]\nid = "run"\ncommand = ["./bin/herdr-example"]\n')
        self.binary = self.repository / "dist/bin/herdr-example"
        self.binary.parent.mkdir(parents=True)
        self.binary.write_bytes(b"#!/bin/sh\nexit 0\n")
        self.binary.chmod(0o755)
        self.releases = self.home / ".local/share/dots/herdr/releases"
        root = patch.object(deploy, "ROOT", self.repository)
        root.start()
        self.addCleanup(root.stop)

    def test_repeatable_release_preserves_all_runtime_fields(self):
        first = deploy.prepare(self.home)
        self.assertEqual(deploy.prepare(self.home, ["example"]), first)
        expected = tomllib.loads(self.manifest.read_text())
        del expected["build"]
        self.assertEqual(tomllib.loads((first[0] / "herdr-plugin.toml").read_text()), expected)
        self.assertEqual((first[0] / "bin/herdr-example").read_bytes(), self.binary.read_bytes())

    def test_corrupt_or_nonexecutable_release_is_rejected(self):
        plugin = deploy.prepare(self.home)[0]
        executable = plugin / "bin/herdr-example"
        executable.chmod(0o644)
        with self.assertRaisesRegex(RuntimeError, "release changed"):
            deploy.prepare(self.home)
        executable.chmod(0o755)
        executable.write_text("corrupt")
        with self.assertRaisesRegex(RuntimeError, "release changed"):
            deploy.prepare(self.home)

    def test_release_cannot_alias_the_build_tree(self):
        plugin = deploy.prepare(self.home)[0]
        executable = plugin / "bin/herdr-example"
        executable.unlink()
        executable.symlink_to(self.binary)
        with self.assertRaisesRegex(RuntimeError, "release changed"):
            deploy.prepare(self.home)

    def test_failed_preparation_preserves_existing_release(self):
        first = deploy.prepare(self.home)[0]
        expected = (first / "bin/herdr-example").read_bytes()
        self.binary.write_text("new build")
        with patch.object(Path, "write_bytes", side_effect=OSError("injected")):
            with self.assertRaisesRegex(OSError, "injected"):
                deploy.prepare(self.home)
        self.assertEqual((first / "bin/herdr-example").read_bytes(), expected)
        self.assertEqual(list(self.releases.iterdir()), [first.parent])

    def test_published_bytes_are_the_snapshot_that_was_hashed(self):
        expected, write_bytes = self.binary.read_bytes(), Path.write_bytes

        def write_snapshot(path, payload):
            self.binary.write_text("concurrent rebuild")
            return write_bytes(path, payload)

        with patch.object(Path, "write_bytes", write_snapshot):
            plugin = deploy.prepare(self.home)[0]
        self.assertEqual((plugin / "bin/herdr-example").read_bytes(), expected)
        self.assertNotEqual(self.binary.read_bytes(), expected)

    def test_release_identity_frames_plugin_boundaries(self):
        other = self.repository / "src/herdr/other/herdr-plugin.toml"
        other.parent.mkdir(parents=True)
        other.write_text(self.manifest.read_text().replace("example", "other"))
        binary = self.repository / "dist/bin/herdr-other"
        binary.write_bytes(b"other executable")
        binary.chmod(0o755)
        first = deploy.prepare(self.home, ["example", "other"])[0].parent
        # This payload collided with the old unframed two-entry hash input.
        self.binary.write_bytes(self.binary.read_bytes() + b"other\0" +
                                deploy.runtime_manifest(other).encode() + b"\0" + binary.read_bytes())
        second = deploy.prepare(self.home, ["example"])[0].parent
        self.assertNotEqual(first, second)

    def test_concurrent_publication_reuses_only_a_verified_winner(self):
        def winner(source, destination):
            shutil.copytree(source, destination)
            raise OSError(errno.ENOTEMPTY, "concurrent publication")

        with patch.object(Path, "rename", winner):
            plugin = deploy.prepare(self.home)[0]
        self.assertEqual((plugin / "bin/herdr-example").read_bytes(), self.binary.read_bytes())
        self.assertEqual(list(self.releases.iterdir()), [plugin.parent])

    def test_concurrent_corrupt_publication_is_rejected(self):
        def winner(source, destination):
            shutil.copytree(source, destination)
            (destination / "example/bin/herdr-example").write_text("corrupt winner")
            raise OSError(errno.ENOTEMPTY, "concurrent publication")

        with patch.object(Path, "rename", winner):
            with self.assertRaisesRegex(RuntimeError, "release changed"):
                deploy.prepare(self.home)

    def test_invalid_selection_or_missing_build_cannot_publish(self):
        for selection in (["missing"], ["example", "example"]):
            with self.assertRaises(ValueError):
                deploy.prepare(self.home, selection)
        self.binary.unlink()
        with self.assertRaisesRegex(RuntimeError, "build first"):
            deploy.prepare(self.home)
        self.assertFalse(self.releases.exists())

    def test_runtime_data_cannot_resolve_into_checkout(self):
        parent = self.releases.parent
        parent.mkdir(parents=True)
        self.releases.symlink_to(self.repository, target_is_directory=True)
        with self.assertRaisesRegex(RuntimeError, "checkout"):
            deploy.prepare(self.home)


class SourceManifestTests(unittest.TestCase):
    def test_source_manifests_can_be_published_without_build_callbacks(self):
        manifests = list((deploy.ROOT / "src/herdr").glob("*/herdr-plugin.toml"))
        self.assertTrue(manifests)
        for manifest in manifests:
            with self.subTest(plugin=manifest.parent.name):
                expected = tomllib.loads(manifest.read_text())
                del expected["build"]
                self.assertEqual(tomllib.loads(deploy.runtime_manifest(manifest)), expected)


if __name__ == "__main__":
    unittest.main()
