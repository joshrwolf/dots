"""Cache-wrapper contracts with no socket connections or signing operations."""
import io
import os
from pathlib import Path
import runpy
import sys
import unittest
from unittest.mock import patch


class CacheWrapperTests(unittest.TestCase):
    def exercise(self, failure=None, configured=None):
        environment = {"HOME": "/tmp/example-home", "EXAMPLE_UNRELATED": "preserved"}
        if configured is not None:
            environment["GITSIGN_CREDENTIAL_CACHE"] = configured
        expected = configured or "/tmp/example-home/Library/Caches/sigstore/gitsign/cache.sock"
        stream = io.StringIO("neutral signing input")
        with patch.dict(os.environ, environment, clear=True), \
             patch("socket.socket") as socket, patch("os.execv") as execute, \
             patch.object(sys, "argv", ["gitsign-cached", "--status-fd", "9"]), \
             patch.object(sys, "stdin", stream):
            probe = socket.return_value.__enter__.return_value
            probe.connect.side_effect = failure
            runpy.run_path(str(Path(__file__).with_name("cache-wrapper.py")), run_name="__main__")
            probe.settimeout.assert_called_once_with(0.25)
            probe.connect.assert_called_once_with(expected)
            probe.send.assert_not_called()
            probe.sendall.assert_not_called()
            if failure:
                self.assertNotIn("GITSIGN_CREDENTIAL_CACHE", os.environ)
            else:
                self.assertEqual(os.environ["GITSIGN_CREDENTIAL_CACHE"], expected)
            self.assertEqual(os.environ["EXAMPLE_UNRELATED"], "preserved")
            execute.assert_called_once_with("/opt/homebrew/bin/gitsign", ["gitsign", "--status-fd", "9"])
            self.assertEqual(stream.tell(), 0)

    def test_reachable_default_cache(self):
        self.exercise()

    def test_explicit_socket_is_preserved(self):
        self.exercise(configured="/tmp/example-cache.sock")

    def test_refusal_or_timeout_falls_back_without_consuming_input(self):
        for failure in (ConnectionRefusedError(), TimeoutError()):
            with self.subTest(failure=type(failure).__name__):
                self.exercise(failure=failure, configured="/tmp/example-cache.sock")


if __name__ == "__main__":
    unittest.main()
