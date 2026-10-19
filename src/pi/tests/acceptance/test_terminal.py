"""Real regular/fullscreen terminal acceptance, using only Python's POSIX stdlib."""
import fcntl
import json
import shutil
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

    def test_fixture_workbench(self):
        with tempfile.TemporaryDirectory(prefix="dpw-", dir="/tmp") as directory:
            for mode in ("regular", "fullscreen"):
                with self.subTest(mode=mode):
                    self.exercise(Path(directory), mode, workbench=True)

    def exercise(self, work, mode, workbench=False):
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
            transcript = bytearray()
            trial = None
            args = [
                os.environ.get("PI_BIN", "pi"), "--no-session", "--offline", "--no-mcp", "--no-extensions",
                "--no-skills", "--no-prompt-templates", "--no-context-files", "--no-approve",
                "--extension", str(ROOT / "dist/pi-runtime"),
                "--extension", str(ROOT / "src/pi/tests/fixtures/terminal-observer.ts"), "--tui-mode", mode,
            ]
            cwd = work
            if workbench:
                module = (ROOT / "src/pi/dev/run.mjs").as_uri()
                prepared = subprocess.run(["node", "--input-type=module", "-e",
                    f'import {{prepareWorkbench}} from {json.dumps(module)}; console.log(JSON.stringify(prepareWorkbench({{mode:{json.dumps(mode)},scenario:"bash"}})));'],
                    check=True, capture_output=True, text=True, timeout=15)
                trial = json.loads(prepared.stdout)
                args = [trial["binary"], *trial["args"], "--extension", str(ROOT / "src/pi/tests/fixtures/terminal-observer.ts")]
                env = {**trial["env"], "DOTS_TUI_MARKER": str(marker), "COLORFGBG": "0;15" if mode == "regular" else "15;0"}
                cwd = trial["cwd"]
            process = subprocess.Popen(args, cwd=cwd, env=env, stdin=slave, stdout=slave, stderr=slave, start_new_session=True)
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
                    transcript.extend(data)
                    self.assertLessEqual(len(transcript), 8 * 1024 * 1024, "terminal capture exceeded bound")
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
                if workbench:
                    self.exercise_workbench(mode, master, master_fd, process, capture, events, wait, command)
                    for error in (b"uncaughtException", b"UnhandledPromiseRejection", b"Failed to load extension", b"ERR_MODULE_NOT_FOUND"):
                        self.assertNotIn(error, transcript)
                    return
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
                if trial:
                    shutil.rmtree(trial["directory"])

    def exercise_workbench(self, mode, master, master_fd, process, capture, events, wait, command):
        def state(scenario, phase, expanded=None):
            records = [line for line in events().splitlines() if line.startswith("workbench {") and line.endswith("}")]
            if not records:
                return False
            latest = json.loads(records[-1].removeprefix("workbench "))
            return latest["scenario"] == scenario and latest["phase"] == phase and (expanded is None or latest["expanded"] == expanded)

        def send(data):
            capture.clear()
            os.write(master_fd, data)

        # Wait for the actual initial editor frame before accepting its prefilled command.
        wait(lambda: b"/ui-lab bash" in capture, "prefilled native editor")
        send(b"\r")
        wait(lambda: state("bash", 0) and b"UI workbench" in capture, "mounted workbench")
        initial_theme = "light" if mode == "regular" else "dark"
        wait(lambda: f"frame 120 {initial_theme}" in events(), "terminal-matched theme")
        for next_key in (b"N", b"\x1b[110u"):
            send(next_key)
            wait(lambda: state("read", 0) and b"Recorded source" in capture, "next scenario")
            send(b"P")
            wait(lambda: b"Tool output" in capture, "previous scenario")
        for phase in range(1, 7):
            send(b" ")
            wait(lambda: state("bash", phase), f"bash phase {phase}")
        send(b"e")
        wait(lambda: b"output line" in capture, "expanded native content")
        send(b"\x1b[F")
        wait(lambda: b"DIAGNOSTIC" in capture, "native scroll to tail diagnostic")
        send(b"\x1b[H")
        wait(lambda: b"output line 0000" in capture, "native scroll to start")
        if mode == "fullscreen":
            send(b"\x1b[<65;60;20M")
            wait(lambda: b"output line" in capture, "fullscreen wheel scrolling")
        for rows, columns in ((24, 44), (48, 160)):
            fcntl.ioctl(master, termios.TIOCSWINSZ, struct.pack("HHHH", rows, columns, 0, 0))
            os.kill(process.pid, signal.SIGWINCH)
            wait(lambda: f"frame {columns} " in events(), f"resize to {columns}")
        send(b"T")
        toggled_theme = "dark" if initial_theme == "light" else "light"
        wait(lambda: f"frame 160 {toggled_theme}" in events(), "native theme toggle")
        send(b"\x1b")
        wait(lambda: "workbench closed" in events(), "editor focus restored")
        wait(lambda: events().count(f"frame 160 {initial_theme}") >= 2, "original palette restored")
        for scenario, expected in (("frame", b"Application frame"), ("wake", b"Scheduled follow-up"), ("thinking", b"Thinking blocks"), ("goal", b"Goal dashboard")):
            closed = events().count("workbench closed")
            capture.clear()
            command(f"/ui-lab {scenario}")
            wait(lambda: state(scenario, 0) and expected in capture, f"{scenario} preview")
            if scenario == "frame":
                wait(lambda: b"accepted" in capture and b"exit 0" in capture, "registered job receipt and completion")
                send(b"E")
                wait(lambda: state("frame", 0, True), "expanded mixed transcript")
                send(b"\x1b[6~")
                wait(lambda: b"/fixture/jobs/example/output.log" in capture, "expanded recorded job evidence")
                send(b"E")
                wait(lambda: state("frame", 0, False), "collapsed mixed transcript")
                send(b"\x1b[H")
                send(b" ")
                wait(lambda: state("frame", 1), "running mixed transcript")
                send(b" ")
                wait(lambda: state("frame", 2) and b"supervisor SIGKILL" in capture, "cleanup distinct from root outcome")
                send(b"\x1b[F")
                wait(lambda: b"Review the result" in capture and b"example_lookup" in capture, "native editor and fallback tool")
            if scenario == "wake":
                send(b" ")
                wait(lambda: b"exit 1" in capture, "failed wake check")
            if scenario == "goal":
                wait(lambda: b"Core interface" in capture and b"1/3 done" in capture, "group preview with complete totals")
                send(b"E")
                wait(lambda: state("goal", 0, True) and b"Consumer migration" in capture, "nested slice inspection")
                send(b"\x1b[D")
                wait(lambda: b"Core interface" in capture and "▸".encode() in capture, "fold group in workbench")
                send(b"\x1b[C")
                wait(lambda: b"Consumer migration" in capture, "unfold group in workbench")
                send(b"E")
                wait(lambda: state("goal", 0, False), "hierarchy dashboard")
                send(b" ")
                wait(lambda: state("goal", 1), "history fixture")
                send(b"E")
                wait(lambda: b"Define shared types" in capture, "closed slice detail")
                send(b"E")
                wait(lambda: state("goal", 1, False), "history dashboard")
                send(b" ")
                wait(lambda: b"totals unavailable" in capture, "partial hierarchy without guessed totals")
                for phase, label in enumerate((b"Update exports", b"Resolve compatibility", b"4/4 slices done", b"3 followed", b"Goal source unavailable", b"Loading goal", b"Brief truncated", b"No slices recorded"), 3):
                    send(b" ")
                    wait(lambda: state("goal", phase) and label in capture, f"goal phase {phase}")
            if scenario == "thinking":
                wait(lambda: b"collapsed" in capture, "native thinking disclosure")
                send(b"E")
                wait(lambda: b"Reviewing dependency updates" in capture, "expanded thinking summary")
                send(b" ")
                wait(lambda: b"Decision: run the focused checks" in capture, "structured thinking detail")
                send(b" ")
                wait(lambda: state("thinking", 2, True) and b"version =" in capture, "unfinished streaming Markdown")
                send(b"E")
                wait(lambda: b"collapsed" in capture, "thinking collapsed again")
            send(b"\x1b")
            wait(lambda: events().count("workbench closed") > closed, f"{scenario} closed")
        command("/ui-lab job")
        wait(lambda: state("job", 0) and b"Job inspection" in capture, "actual job inspector")
        send(b"s")
        wait(lambda: b"Simulate stopping this job?" in capture, "native action confirmation")
        send(b"\x1b")
        wait(lambda: "confirmation false" in events(), "confirmation cancelled")
        # Cancel returns focus to the inspector; a second confirmation proves it.
        send(b"s")
        wait(lambda: b"Simulate stopping this job?" in capture, "confirmation reopened after cancel")
        send(b"\r")
        wait(lambda: b"stopping" in capture, "approved fixture action")
        send(b"\x1b")
        wait(lambda: events().count("workbench closed") >= 4, "job inspector closed")
        command("/reload")
        wait(lambda: events().count("start") >= 2 and b"/ui-lab bash" in capture, "isolated reload")
        # Prove focus and input interception after reload without contacting a model.
        send(b"\x15fixture prompt must not reach a provider\r")
        wait(lambda: "workbench input handled" in events(), "provider-free input")
        command("/ui-lab-exit")
        wait(lambda: process.poll() is not None, "orderly workbench shutdown")
        self.assertEqual(process.returncode, 0)
        self.assertNotIn("UNEXPECTED_PROVIDER_TURN", events())
        for error in (b"uncaughtException", b"UnhandledPromiseRejection", b"Failed to load extension", b"ERR_MODULE_NOT_FOUND"):
            self.assertNotIn(error, capture)


if __name__ == "__main__":
    unittest.main()
