"""Real Pi goal inspection/focus in an isolated regular/fullscreen terminal."""
import fcntl
import os
from pathlib import Path
import pty
import select
import signal
import struct
import subprocess
import tempfile
import termios
import time
import unittest

ROOT = Path(__file__).resolve().parents[4]


class GoalTerminalTests(unittest.TestCase):
    def test_focus_inspect_scroll_resize_reload_unfocus(self):
        for mode in ("regular", "fullscreen"):
            with self.subTest(mode=mode), tempfile.TemporaryDirectory(prefix="dots-goal-tui-") as directory:
                self.exercise(Path(directory), mode)

    def exercise(self, work, mode):
        marker = work / "events.log"
        master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 120, 0, 0))
        env = {k: v for k, v in os.environ.items() if not any(w in k for w in ("API_KEY", "TOKEN", "SECRET", "PASSWORD"))}
        env.update(TERM="xterm-256color", PI_CODING_AGENT_DIR=str(work), PI_OFFLINE="1", DOTS_GOAL_MARKER=str(marker))
        process = subprocess.Popen([
            os.environ.get("PI_BIN", "pi"), "--no-session", "--offline", "--no-mcp", "--no-extensions",
            "--no-skills", "--no-prompt-templates", "--no-context-files", "--no-approve", "--no-themes",
            "--extension", str(ROOT / "src/pi/tests/fixtures/goal-terminal.ts"), "--tui-mode", mode,
        ], cwd=work, env=env, stdin=slave, stdout=slave, stderr=slave, start_new_session=True)
        os.close(slave)
        capture = bytearray()
        transcript = bytearray()

        def pump():
            readable, _, _ = select.select([master], [], [], .1)
            if not readable:
                return
            try:
                data = os.read(master, 65536)
            except OSError:
                return
            capture.extend(data)
            transcript.extend(data)
            self.assertLess(len(transcript), 4 * 1024 * 1024)
            if b"\x1b[6n" in data:
                os.write(master, b"\x1b[1;1R")

        def events():
            return marker.read_text() if marker.exists() else ""

        def wait(predicate, label):
            deadline = time.monotonic() + 15
            while time.monotonic() < deadline:
                pump()
                if predicate():
                    return
                if process.poll() is not None:
                    if predicate():
                        return
                    break
            self.fail(f"{mode}: {label}: {capture[-3000:].decode(errors='replace')}")

        def send(text):
            capture.clear()
            os.write(master, text)

        try:
            wait(lambda: "start" in events(), "startup")
            # Let the editor mount before sending input in either host mode.
            until = time.monotonic() + .5
            while time.monotonic() < until:
                pump()
            send(b"/goal focus t-123abc\r")
            wait(lambda: b"slices done" in capture and b"Unified fixture exports" in capture, "focused widget")
            send(b"/goal\r")
            wait(lambda: b"1 slices" in capture, "inspector")
            send(b"2")
            wait(lambda: b"brief" in capture, "brief tab")
            send(b"\x1b[F")
            wait(lambda: b"END_OF_GOAL_BRIEF" in capture, "scroll to end")
            for rows, columns in ((24, 45), (48, 160)):
                capture.clear()
                fcntl.ioctl(master, termios.TIOCSWINSZ, struct.pack("HHHH", rows, columns, 0, 0))
                os.kill(process.pid, signal.SIGWINCH)
                wait(lambda: b"Unified fixture exports" in capture, "resize")
            send(b"\r")
            until = time.monotonic() + .3
            while time.monotonic() < until:
                pump()
            send(b"/reload\r")
            wait(lambda: events().count("start") >= 2 and b"slices done" in capture, "reload focus")
            send(b"/goal unfocus\r")
            wait(lambda: b"Goal unfocused" in capture, "unfocus")
            send(b"/goal show\r")
            wait(lambda: b"No focused goal" in capture, "unfocused inspection")
            send(b"/goal-fixture-exit\r")
            wait(lambda: process.poll() is not None, "exit")
            self.assertEqual(process.returncode, 0)
            self.assertNotIn("UNEXPECTED_PROVIDER_TURN", events())
            for error in (b"UnhandledPromiseRejection", b"Failed to load extension", b"stale after session"):
                self.assertNotIn(error, transcript)
        finally:
            if process.poll() is None:
                os.killpg(process.pid, signal.SIGTERM)
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    os.killpg(process.pid, signal.SIGKILL)
                    process.wait()
            os.close(master)
