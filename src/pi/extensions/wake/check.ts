import { spawn } from "node:child_process";
import { createHash } from "node:crypto";

export interface CheckResult {
  stdout: string;
  stderr: string;
  code: number;
  /** Hash of complete stdout/stderr, even when the retained preview is clipped. */
  digest?: string;
  truncated?: boolean;
}
export type CheckRunner = (command: string, cwd: string, signal?: AbortSignal) => Promise<CheckResult>;

const PREVIEW_BYTES = 4000;
const hash = () => createHash("sha256");
export function checkDigest(stdout: string, stderr: string): string {
  return `${hash().update(stdout).digest("hex")}:${hash().update(stderr).digest("hex")}:`;
}

/** Bounded capture; timeout/abort stops the shell's process group, not just bash. */
export function runCheck(command: string, cwd: string, signal?: AbortSignal, timeoutMs = 120_000): Promise<CheckResult> {
  if (signal?.aborted) return Promise.resolve({ stdout: "", stderr: "check cancelled", code: -1 });
  return new Promise((resolve) => {
    const grouped = process.platform !== "win32";
    const child = spawn("bash", ["-lc", command], { cwd, detached: grouped, stdio: ["ignore", "pipe", "pipe"] });
    const stdoutHash = hash();
    const stderrHash = hash();
    let stdout: Buffer = Buffer.alloc(0);
    let stderr: Buffer = Buffer.alloc(0);
    let truncated = false;
    let termination = "";
    let finished = false;
    let hardKill: ReturnType<typeof setTimeout> | undefined;
    let drain: ReturnType<typeof setTimeout> | undefined;
    const kill = (signal: NodeJS.Signals) => {
      try {
        if (grouped && child.pid) process.kill(-child.pid, signal);
        else child.kill(signal);
      } catch { /* already gone */ }
    };
    const stop = (reason: string) => {
      if (termination || finished) return;
      termination = reason;
      kill("SIGTERM");
      hardKill = setTimeout(() => {
        kill("SIGKILL");
        // An escaped descendant can retain inherited pipes outside this group.
        // Bound our observation/ownership; do not claim that escape was killed.
        drain = setTimeout(() => {
          child.stdout.destroy();
          child.stderr.destroy();
          finish(-1, "output drain incomplete; detached descendants may remain");
        }, 250);
      }, 1000);
    };
    const onAbort = () => stop("check cancelled");
    const deadline = setTimeout(() => stop(`check timed out after ${timeoutMs}ms`), timeoutMs);
    signal?.addEventListener("abort", onAbort, { once: true });
    if (signal?.aborted) onAbort();
    const capture = (current: Buffer, chunk: Buffer): Buffer => {
      const room = Math.max(0, PREVIEW_BYTES - current.length);
      if (chunk.length > room) truncated = true;
      return room ? Buffer.concat([current, chunk.subarray(0, room)]) : current;
    };
    child.stdout.on("data", (chunk: Buffer) => { stdoutHash.update(chunk); stdout = capture(stdout, chunk); });
    child.stderr.on("data", (chunk: Buffer) => { stderrHash.update(chunk); stderr = capture(stderr, chunk); });
    const finish = (code: number | null, error?: string) => {
      if (finished) return;
      finished = true;
      if (termination) kill("SIGKILL"); // the shell may exit before its descendants
      clearTimeout(deadline);
      if (hardKill) clearTimeout(hardKill);
      if (drain) clearTimeout(drain);
      signal?.removeEventListener("abort", onAbort);
      const diagnostic = [termination, error].filter(Boolean).join("; ");
      resolve({
        stdout: stdout.toString("utf8"),
        stderr: [stderr.toString("utf8"), diagnostic].filter(Boolean).join("\n"),
        code: diagnostic ? -1 : code ?? -1,
        digest: `${stdoutHash.digest("hex")}:${stderrHash.digest("hex")}:${diagnostic ?? ""}`,
        truncated,
      });
    };
    child.once("error", (error) => finish(-1, error.message));
    child.once("close", (code, signal) => finish(code, signal ? `check terminated by ${signal}` : undefined));
  });
}
