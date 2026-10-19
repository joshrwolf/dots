// Executed by Node, not by Pi's extension loader. Retains its group identity
// through TERM grace and final self-KILL. Never signal this PGID from the owner.
import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process";
import { Capture } from "./capture.ts";
import { GRACE_MS, MAX_TIMEOUT_MS, emptyFacts, type OwnerMessage, type StopReason, type SupervisorMessage } from "./protocol.ts";

const facts = emptyFacts();
let root: ChildProcessWithoutNullStreams | undefined;
let capture: Capture | undefined;
let started = false;
let stopping = false;
let timer: ReturnType<typeof setTimeout> | undefined;
let heartbeat: ReturnType<typeof setInterval> | undefined;
let ended = 0;
let sending = false;

function send(message: SupervisorMessage): void {
  if (!process.connected || !process.send) { stop("owner-disconnect"); return; }
  if (sending) return; // One bounded in-flight snapshot even if the owner stalls.
  sending = true;
  try {
    process.send(message, (error) => { sending = false; if (error) stop("ipc-error"); });
  } catch { sending = false; stop("ipc-error"); }
}
function report(): void { send({ type: "facts", facts }); }
function stop(reason: StopReason): void {
  if (stopping) return;
  stopping = true;
  facts.reason = reason;
  clearTimeout(timer);
  clearInterval(heartbeat);
  // Install the hard deadline before any I/O. Neither IPC nor flush can extend it.
  setTimeout(() => { facts.killIntent = true; report(); }, GRACE_MS - 100);
  setTimeout(() => {
    // The live supervisor is the group leader. A SIGKILL exit plus this intent
    // is evidence, not proof against process-group/session escapes.
    try { process.kill(-process.pid, "SIGKILL"); } catch { process.exit(1); }
  }, GRACE_MS);
  setTimeout(() => {
    root?.stdout.destroy();
    root?.stderr.destroy();
    void (capture?.finish() ?? Promise.resolve()).then(report, () => report());
  }, GRACE_MS - 500);
  try { process.kill(-process.pid, "SIGTERM"); facts.termSent = true; } catch { /* reported as unobserved */ }
  report();
}

// Before ready, before spawn, and before any IPC send: fail closed on all paths.
process.on("disconnect", () => stop("owner-disconnect"));
process.on("error", () => stop("ipc-error"));
process.on("SIGTERM", () => stop("owner-shutdown"));
process.on("SIGINT", () => stop("owner-shutdown"));
process.on("SIGHUP", () => stop("owner-shutdown"));
process.on("uncaughtException", () => stop("launch-error"));
process.on("unhandledRejection", () => stop("launch-error"));
process.on("message", (raw: unknown) => {
  if (!raw || typeof raw !== "object" || !("type" in raw)) { stop("ipc-error"); return; }
  const message = raw as OwnerMessage;
  if (message.type === "stop") { stop(message.reason === "stop" ? "stop" : "owner-shutdown"); return; }
  if (message.type !== "start" || started || stopping) return;
  if (typeof message.command !== "string" || message.command.length > 8192 || typeof message.cwd !== "string" ||
      typeof message.directory !== "string" || !Number.isInteger(message.timeoutMs) || message.timeoutMs < 1 || message.timeoutMs > MAX_TIMEOUT_MS) {
    stop("ipc-error"); return;
  }
  if (!process.connected) { stop("owner-disconnect"); return; }
  started = true;
  clearTimeout(timer);
  capture = new Capture(message.directory, (paused) => {
    for (const pipe of [root?.stdout, root?.stderr]) { if (paused) pipe?.pause(); else pipe?.resume(); }
  });
  facts.capture = capture.facts;
  // stdin is a closed pipe; the shell and descendants share this supervisor's
  // group. Inherited environment is used for execution, never snapshotted.
  root = spawn("/bin/bash", ["--noprofile", "--norc", "-c", message.command], {
    cwd: message.cwd, detached: false, stdio: ["pipe", "pipe", "pipe"],
  });
  root.stdin.on("error", () => {});
  root.stdin.end();
  for (const pipe of [root.stdout, root.stderr]) {
    pipe.on("data", (data: Buffer) => capture?.data(data));
    pipe.on("end", () => { ended++; facts.capture.streamsEnded = ended === 2; });
    pipe.on("error", (error) => { facts.capture.error = error.message.slice(0, 512); });
  }
  root.on("error", (error) => { facts.root = { code: null, signal: null, error: error.message.slice(0, 512) }; stop("launch-error"); });
  root.on("exit", (code, signal) => { facts.root ??= { code, signal, error: null }; stop("root-exit"); report(); });
  timer = setTimeout(() => stop("timeout"), message.timeoutMs);
  heartbeat = setInterval(report, 1000); // Facts only, never output chunks.
  report();
});
timer = setTimeout(() => stop("owner-disconnect"), 10_000);
if (process.connected) send({ type: "ready" });
else stop("owner-disconnect");
