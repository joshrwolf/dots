import { fork, type ChildProcess } from "node:child_process";
import { mkdirSync, rmSync, statSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { randomUUID } from "node:crypto";
import { DEFAULT_TIMEOUT_MS, MAX_JOBS, MAX_RETAINED, MAX_TIMEOUT_MS, emptyFacts, live, snapshot, type Job, type OwnerMessage, type SupervisorMessage } from "./protocol.ts";

interface Handle { child: ChildProcess; exited: Promise<void>; recorded: boolean; lastReport: number }
export interface RuntimeOptions {
  root: string;
  persist: (job: Job) => void;
  terminal: (job: Job) => void;
  error: (message: string) => void;
}

/** One session runtime. Historical records never acquire handles or deletion authority. */
export class JobsRuntime {
  private readonly jobs = new Map<string, Job>();
  private readonly handles = new Map<string, Handle>();
  private readonly owned = new Set<string>();
  private readonly finalized = new Set<string>();
  private readonly directory: string;
  private readonly options: RuntimeOptions;
  private closed = false;
  private watchdog: ReturnType<typeof setInterval> | undefined;

  constructor(options: RuntimeOptions, restored: readonly Job[] = []) {
    this.options = options;
    this.directory = join(options.root, randomUUID());
    for (const job of restored.slice(-MAX_RETAINED)) this.jobs.set(job.id, job);
  }
  /** Tree navigation changes visible history, not the runtime's storage budget.
   * Off-branch artifacts remain reserved; we conservatively do not evict them.
   */
  replaceHistory(restored: readonly Job[]): void {
    if (this.handles.size) throw new Error("Cannot replace job history with live jobs");
    this.jobs.clear();
    for (const job of restored.slice(-MAX_RETAINED)) this.jobs.set(job.id, job);
  }
  list(): Job[] { return [...this.jobs.values()].map(snapshot); }
  get(id: string): Job {
    const job = this.jobs.get(id);
    if (!job) throw new Error(`Unknown job: ${id}`);
    return snapshot(job);
  }
  hasLive(): boolean { return this.handles.size > 0; }
  private save(job: Job): void {
    try { this.options.persist(snapshot(job)); }
    catch (error) { this.options.error(`Job state could not be persisted: ${String(error).slice(0, 256)}`); }
  }
  private changed(job: Job, terminal = false): void {
    if (this.closed) return;
    this.save(job);
    const handle = this.handles.get(job.id);
    if (terminal && !handle?.recorded) {
      if (handle) handle.recorded = true;
      try { this.options.terminal(snapshot(job)); }
      catch (error) { this.options.error(`Job notification failed (not retried): ${String(error).slice(0, 256)}`); }
    }
  }

  start(command: string, cwd: string, timeoutMs = DEFAULT_TIMEOUT_MS, signal?: AbortSignal): Job {
    if (this.closed) throw new Error("Jobs runtime is closed");
    if (process.platform !== "darwin") throw new Error("Jobs currently support verified macOS only");
    if (!command.trim() || command.length > 8192) throw new Error("Command must contain 1–8192 characters");
    if (!Number.isInteger(timeoutMs) || timeoutMs < 1 || timeoutMs > MAX_TIMEOUT_MS) throw new Error("timeoutMs must be 1–86400000");
    cwd = resolve(cwd);
    if (cwd.length > 4096 || !statSync(cwd).isDirectory()) throw new Error("cwd must be an existing directory");
    if (signal?.aborted) throw new Error("Job admission aborted");
    if (this.handles.size >= MAX_JOBS) throw new Error("At most four jobs may be active");
    // Never scan old runtime directories, or use age as evidence of safety.
    if (this.owned.size >= MAX_RETAINED) {
      const evict = [...this.jobs.values()].find((j) => this.owned.has(j.id) && this.finalized.has(j.id) && j.state === "finished" && !this.handles.has(j.id) && !j.observationLoss);
      if (!evict) throw new Error("Job artifact budget exhausted (16 retained/reserved directories); uncertain jobs cannot be evicted");
      // Deletion authority comes from generated runtime paths, never restored data.
      rmSync(join(this.directory, evict.id), { recursive: true, force: true });
      evict.artifactsEvicted = true;
      this.save(evict);
      this.owned.delete(evict.id);
      this.finalized.delete(evict.id);
      this.jobs.delete(evict.id);
    }
    const id = `j-${randomUUID()}`;
    const directory = join(this.directory, id);
    mkdirSync(directory, { recursive: true, mode: 0o700 });
    const job: Job = {
      version: 1, id, command, cwd, directory, timeoutMs, startedAt: Date.now(), finishedAt: null,
      state: "accepted", stopIntent: null, facts: emptyFacts(), supervisorExit: null, observationLoss: null, artifactsEvicted: false,
    };
    this.jobs.set(id, job);
    this.owned.add(id);
    // Admission is synchronous: reserve and persist before dispatch. An aborted
    // caller after this point does not own this job's lifetime.
    try { this.options.persist(snapshot(job)); }
    catch (error) { this.jobs.delete(id); this.owned.delete(id); rmSync(directory, { recursive: true, force: true }); throw error; }
    const receipt = snapshot(job);
    try {
      const child = fork(fileURLToPath(new URL("./supervisor.ts", import.meta.url)), [], {
        detached: true, stdio: ["ignore", "ignore", "ignore", "ipc"], execArgv: ["--experimental-strip-types"],
      });
      let exited!: () => void;
      const handle: Handle = { child, exited: new Promise<void>((resolve) => { exited = resolve; }), recorded: false, lastReport: Date.now() };
      this.handles.set(id, handle);
      child.on("error", (error) => {
        job.observationLoss = `Supervisor IPC/spawn error: ${error.message.slice(0, 512)}`;
        this.send(job, handle, { type: "stop", reason: "owner-shutdown" });
      });
      child.on("message", (raw: unknown) => {
        if (!raw || typeof raw !== "object" || !("type" in raw)) return;
        const message = raw as SupervisorMessage;
        handle.lastReport = Date.now();
        if (message.type === "ready") {
          if (this.closed) { this.send(job, handle, { type: "stop", reason: "owner-shutdown" }); return; }
          if (job.stopIntent) { this.send(job, handle, { type: "stop", reason: job.stopIntent }); return; }
          this.send(job, handle, { type: "start", command, cwd, directory, timeoutMs });
          job.state = "running";
          this.changed(job);
        } else if (message.type === "facts") {
          job.facts = message.facts;
          if (job.facts.reason && live(job)) job.state = "stopping";
          // Periodic snapshots are ephemeral. Persist transitions, not one entry/second.
        }
      });
      child.on("exit", (code, exitSignal) => {
        job.supervisorExit = { code, signal: exitSignal };
        job.finishedAt = Date.now();
        const expected = job.facts.killIntent && exitSignal === "SIGKILL";
        job.state = expected ? "finished" : "unknown";
        if (!expected) job.observationLoss ??= "Supervisor exited without observed group-KILL intent; descendants may survive";
        else if (!job.observationLoss) this.finalized.add(id);
        this.changed(job, true);
        this.handles.delete(id);
        if (!this.handles.size) { clearInterval(this.watchdog); this.watchdog = undefined; }
        exited();
      });
      if (!this.watchdog) this.watchdog = setInterval(() => {
        for (const [jobId, h] of this.handles) {
          const j = this.jobs.get(jobId);
          if (j && !h.recorded && Date.now() - h.lastReport > 15_000) {
            j.state = "unknown";
            j.observationLoss = "Supervisor stopped reporting; cleanup cannot be established (no numeric PID fallback)";
            this.changed(j, true);
          }
        }
      }, 5000);
    } catch (error) {
      job.state = "unknown";
      job.observationLoss = `Supervisor dispatch failed: ${String(error).slice(0, 512)}`;
      this.changed(job, true);
    }
    return receipt;
  }

  private send(job: Job, handle: Handle, message: OwnerMessage): void {
    const failed = () => {
      job.observationLoss ??= "Owner IPC failed; supervisor disconnect cleanup requested, not confirmed";
      // Closing IPC is safe even after exit; numeric group signalling is not.
      if (handle.child.connected) { try { handle.child.disconnect(); } catch { /* already disconnected */ } }
    };
    try {
      if (!handle.child.connected) { failed(); return; }
      handle.child.send(message, (error) => { if (error) failed(); });
    } catch { failed(); }
  }
  stop(id: string): Job {
    if (this.closed) throw new Error("Jobs runtime is closed");
    const job = this.jobs.get(id);
    if (!job) throw new Error(`Unknown job: ${id}`);
    const handle = this.handles.get(id);
    if (handle && !job.stopIntent && !job.facts.reason) {
      job.stopIntent = "stop";
      job.state = "stopping";
      this.save(job);
      this.send(job, handle, { type: "stop", reason: "stop" });
    }
    return snapshot(job);
  }
  async shutdown(): Promise<void> {
    if (this.closed) return;
    this.closed = true; // Suppress terminal records and guard every late callback.
    clearInterval(this.watchdog);
    const handles = [...this.handles.entries()];
    for (const [id, handle] of handles) {
      const job = this.jobs.get(id);
      if (!job) continue;
      job.stopIntent ??= "owner-shutdown";
      job.state = "stopping";
      this.save(job);
      this.send(job, handle, { type: "stop", reason: "owner-shutdown" });
    }
    let timer: ReturnType<typeof setTimeout> | undefined;
    await Promise.race([Promise.all(handles.map(([, h]) => h.exited)), new Promise<void>((resolve) => { timer = setTimeout(resolve, 4000); })]);
    clearTimeout(timer);
    for (const [id, h] of handles) {
      const job = this.jobs.get(id);
      if (!job) continue;
      if (this.handles.has(id)) {
        job.state = "unknown";
        job.observationLoss = "Shutdown observation deadline expired; cleanup unconfirmed";
        if (h.child.connected) { try { h.child.disconnect(); } catch { /* already disconnected */ } }
        h.child.unref();
      }
      this.save(job);
    }
  }
}
