"""Real regular/fullscreen terminal acceptance, using only Python's POSIX stdlib."""
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


class PackagedTerminalTests(unittest.TestCase):
    def test_regular_and_fullscreen(self):
        subprocess.run(["node", "src/pi/install/artifact.mjs", "verify"], cwd=ROOT, check=True, timeout=30)
        with tempfile.TemporaryDirectory(prefix="dpt-", dir="/tmp") as directory:
            work = Path(directory)
            for mode in ("regular", "fullscreen"):
                with self.subTest(mode=mode):
                    self.exercise(work, mode)

    def exercise(self, work, mode):
        agent = work / mode
        agent.mkdir(mode=0o700)
        marker = agent / "events.log"
        master_fd, slave_fd = pty.openpty()
        with os.fdopen(master_fd, "r+b", buffering=0) as master, os.fdopen(slave_fd, "r+b", buffering=0) as slave:
            fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 120, 0, 0))
            env = {key: value for key, value in os.environ.items()
                   if not any(word in key for word in ("API_KEY", "TOKEN", "SECRET", "PASSWORD"))}
            env.update(TERM="xterm-256color", PI_CODING_AGENT_DIR=str(agent),
                       DOTS_TUI_MARKER=str(marker), PI_OFFLINE="1")
            capture = bytearray()
            process = subprocess.Popen([
                os.environ.get("PI_BIN", "pi"), "--no-session", "--offline", "--no-mcp", "--no-extensions",
                "--no-skills", "--no-prompt-templates", "--no-context-files", "--no-approve",
                "--extension", str(ROOT / "dist/pi-runtime"),
                "--extension", str(ROOT / "src/pi/tests/fixtures/terminal-observer.ts"), "--tui-mode", mode,
            ], cwd=work, env=env, stdin=slave, stdout=slave, stderr=slave, start_new_session=True)
            slave.close()

            def pump(seconds):
                deadline = time.monotonic() + seconds
                while time.monotonic() < deadline:
                    readable, _, _ = select.select([master], [], [], min(.1, max(0, deadline - time.monotonic())))
                    if not readable:
                        continue
                    try:
                        data = os.read(master_fd, 65536)
                    except OSError:
                        break
                    if not data:
                        break
                    capture.extend(data)
                    self.assertLessEqual(len(capture), 8 * 1024 * 1024, "terminal capture exceeded bound")
                    if b"\x1b[6n" in data:
                        os.write(master_fd, b"\x1b[1;1R")

            def events():
                return marker.read_text() if marker.exists() else ""

            def wait(predicate, label):
                deadline = time.monotonic() + 15
                while time.monotonic() < deadline:
                    pump(.1)
                    if predicate():
                        return
                    if process.poll() is not None:
                        if predicate():
                            return
                        self.fail(f"{mode}: exited before {label}; {capture[-4096:].decode(errors='replace')}")
                self.fail(f"{mode}: timeout waiting for {label}; {events()}; {capture[-4096:].decode(errors='replace')}")

            def command(text):
                os.write(master_fd, text.encode() + b"\r")

            try:
                wait(lambda: "start" in events(), "startup")
                pump(.5)
                command("/ui")
                wait(lambda: b"Presentation" in capture, "packaged UI menu")
                os.write(master_fd, b"\x1b")
                pump(.3)
                for rows, columns in ((28, 60), (48, 160)):
                    fcntl.ioctl(master, termios.TIOCSWINSZ, struct.pack("HHHH", rows, columns, 0, 0))
                    os.kill(process.pid, signal.SIGWINCH)
                    pump(.2)
                command("/accept-theme")
                wait(lambda: "theme true" in events(), "theme change")
                command("/reload")
                wait(lambda: events().count("start") >= 2, "package reload")
                # Exercise delayed refresh callbacks against the replaced context.
                pump(5.5)
                command("/accept-exit")
                wait(lambda: process.poll() is not None, "orderly shutdown")
                self.assertEqual(process.returncode, 0)
                self.assertNotIn("UNEXPECTED_PROVIDER_TURN", events())
                for error in (b"uncaughtException", b"UnhandledPromiseRejection", b"Failed to load extension", b"ERR_MODULE_NOT_FOUND"):
                    self.assertNotIn(error, capture)
            finally:
                if process.poll() is None:
                    os.killpg(process.pid, signal.SIGTERM)
                    try:
                        process.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        os.killpg(process.pid, signal.SIGKILL)
                        process.wait()


if __name__ == "__main__":
    unittest.main()
