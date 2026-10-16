import { randomBytes } from "node:crypto";
import type { ExtensionAPI, ExtensionContext } from "@earendil-works/pi-coding-agent";
import { Type } from "typebox";

// Wake: "wake this session at a time, with a note to self", optionally gated
// on a cheap shell check so the model only runs when there is something to act
// on. Owned by the creating session, persisted in its session file, re-armed on
// reload. A loop is a recurring wake plus the session doing the work each fire.
// Bounded by construction: every recurring wake has a fire limit, intervals have
// a floor, and a session has a cap on active wakes.

type On = "always" | "change" | "failure" | "success";

export interface Wake {
  id: string;
  text: string;
  /** Next planned fire, epoch ms. */
  at: number;
  /** Interval in ms for recurring wakes. */
  every?: number;
  limit: number;
  fired: number;
  check?: string;
  on: On;
  lastOutput?: string;
  lastCode?: number;
  /** Set while a fire's message is queued and not yet consumed by a turn. */
  pending?: boolean;
  cancelled?: boolean;
  createdAt: number;
}

export interface Clock {
  now(): number;
  setTimeout(fn: () => void, ms: number): unknown;
  clearTimeout(handle: unknown): void;
}

export interface CheckResult {
  stdout: string;
  stderr: string;
  code: number;
}

export type CheckRunner = (command: string, cwd: string) => Promise<CheckResult>;

export const ENTRY = "dots.wake";
export const MIN_INTERVAL_MS = 60_000;
export const DEFAULT_LIMIT = 50;
export const MAX_LIMIT = 1000;
export const MAX_ACTIVE = 20;
export const MAX_FUTURE_MS = 30 * 24 * 3_600_000;
const OUTPUT_CHARS = 4_000;

export function parseDuration(value: string): number {
  const match = /^\s*(\d+(?:\.\d+)?)\s*(s|m|h|d)\s*$/i.exec(value);
  if (!match) throw new Error(`duration must look like 20m, 2h, or 1d (got ${JSON.stringify(value)})`);
  const amount = Number(match[1]);
  const unit = { s: 1_000, m: 60_000, h: 3_600_000, d: 86_400_000 }[match[2]!.toLowerCase() as "s" | "m" | "h" | "d"];
  return Math.round(amount * unit);
}

export function describe(ms: number): string {
  const seconds = Math.round(ms / 1000);
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.round(seconds / 60);
  if (minutes < 90) return `${minutes}m`;
  const hours = Math.round(minutes / 60);
  return hours < 48 ? `${hours}h` : `${Math.round(hours / 24)}d`;
}

/** Decide whether a check result should wake the session. */
export function shouldFire(wake: Wake, result: CheckResult): boolean {
  switch (wake.on) {
    case "always":
      return true;
    case "failure":
      return result.code !== 0;
    case "success":
      return result.code === 0;
    case "change":
      return wake.lastOutput === undefined || wake.lastOutput !== result.stdout || wake.lastCode !== result.code;
    default:
      return true;
  }
}

export interface WakeHost {
  clock?: Clock;
  run?: CheckRunner;
}

export default function (pi: ExtensionAPI, host: WakeHost = {}) {
  const clock: Clock = host.clock ?? {
    now: () => Date.now(),
    setTimeout: (fn, ms) => setTimeout(fn, ms),
    clearTimeout: (handle) => clearTimeout(handle as NodeJS.Timeout),
  };
  const run: CheckRunner = host.run ?? (async (command, cwd) => {
    const result = await pi.exec("bash", ["-lc", command], { cwd, timeout: 120_000 });
    return { stdout: result.stdout, stderr: result.stderr, code: result.code };
  });

  const wakes = new Map<string, Wake>();
  const timers = new Map<string, unknown>();
  let cwd = process.cwd();
  let armed = false;

  const persist = (wake: Wake) => pi.appendEntry(ENTRY, { ...wake, pending: undefined });

  const active = () => [...wakes.values()].filter((w) => !w.cancelled);

  const disarm = (id: string) => {
    const handle = timers.get(id);
    if (handle !== undefined) clock.clearTimeout(handle);
    timers.delete(id);
  };

  const arm = (wake: Wake) => {
    disarm(wake.id);
    if (wake.cancelled) return;
    const delay = Math.max(0, wake.at - clock.now());
    timers.set(wake.id, clock.setTimeout(() => void fire(wake, "timer"), delay));
  };

  const finish = (wake: Wake) => {
    wake.cancelled = true;
    disarm(wake.id);
    persist(wake);
  };

  /** Advance a recurring wake from its planned time; collapse missed fires to one. */
  const advance = (wake: Wake) => {
    if (!wake.every) {
      finish(wake);
      return;
    }
    if (wake.fired >= wake.limit) {
      finish(wake);
      return;
    }
    const now = clock.now();
    let next = wake.at + wake.every;
    if (next <= now) next = now + wake.every; // we were away; one catch-up fire already happened
    wake.at = next;
    persist(wake);
    arm(wake);
  };

  const fire = async (wake: Wake, reason: "timer" | "manual" | "restore") => {
    if (wake.cancelled) return;
    if (wake.pending && reason === "timer") {
      // The previous fire has not been consumed by a turn yet: skip, do not pile up.
      advance(wake);
      return;
    }
    const planned = wake.at;
    const now = clock.now();
    let body = wake.text;
    let gated = false;
    if (wake.check) {
      let result: CheckResult;
      try {
        result = await run(wake.check, cwd);
      } catch (error) {
        result = { stdout: "", stderr: (error as Error).message, code: -1 };
      }
      gated = !shouldFire(wake, result);
      wake.lastOutput = result.stdout;
      wake.lastCode = result.code;
      const output = [result.stdout.trim(), result.stderr.trim() ? `stderr: ${result.stderr.trim()}` : ""].filter(Boolean).join("\n");
      body += `\n\ncheck: ${wake.check}\nexit: ${result.code}\n${output.slice(0, OUTPUT_CHARS)}${output.length > OUTPUT_CHARS ? "\n[output truncated]" : ""}`;
    }
    if (gated && reason !== "manual") {
      if (reason === "timer") advance(wake);
      return;
    }
    wake.fired += 1;
    const late = now - planned;
    const header = `Wake ${wake.id}${wake.every ? ` (fire ${wake.fired} of ${wake.limit}, every ${describe(wake.every)})` : ""}${late > 60_000 && reason !== "manual" ? ` · late by ${describe(late)}` : ""}${reason === "manual" ? " · manual" : ""}`;
    wake.pending = true;
    // A tick is time-based, not urgent: start a turn when idle, otherwise queue
    // after the current turn rather than interrupting it.
    pi.sendMessage(
      { customType: "dots.wake", content: `${header}\n\n${body}\n\nCancel with wake_cancel { id: "${wake.id}" }.`, display: true, details: { id: wake.id, fired: wake.fired, limit: wake.limit, late } },
      { triggerTurn: true, deliverAs: "followUp" },
    );
    if (reason === "timer" || reason === "restore") advance(wake);
    else persist(wake);
  };

  const summary = (wake: Wake) => ({
    id: wake.id,
    text: wake.text,
    next: wake.cancelled ? null : new Date(wake.at).toISOString(),
    in: wake.cancelled ? null : describe(Math.max(0, wake.at - clock.now())),
    every: wake.every ? describe(wake.every) : null,
    fired: wake.fired,
    limit: wake.limit,
    check: wake.check ?? null,
    on: wake.check ? wake.on : null,
    lastCode: wake.lastCode ?? null,
    active: !wake.cancelled,
  });

  const find = (id: string): Wake => {
    const wanted = id.trim();
    if (!wanted) throw new Error("a wake id is required");
    const wake = wakes.get(wanted) ?? [...wakes.values()].find((w) => w.id.startsWith(wanted));
    if (!wake) throw new Error(`no wake ${wanted}; active: ${active().map((w) => w.id).join(", ") || "none"}`);
    return wake;
  };

  const restore = (ctx: ExtensionContext) => {
    cwd = ctx.cwd;
    for (const timer of timers.keys()) disarm(timer);
    wakes.clear();
    // Later entries win: each change appends the whole record.
    for (const entry of ctx.sessionManager.getBranch()) {
      if (entry.type !== "custom" || entry.customType !== ENTRY) continue;
      const data = entry.data as Wake;
      wakes.set(data.id, { ...data, pending: false });
    }
    const now = clock.now();
    for (const wake of active()) {
      if (wake.at <= now) {
        // Missed while we were away: fire once now (late), then continue from now.
        void fire(wake, "restore");
      } else {
        arm(wake);
      }
    }
    armed = true;
  };

  pi.on("session_start", (_event, ctx) => restore(ctx));
  pi.on("session_shutdown", () => {
    for (const id of [...timers.keys()]) disarm(id);
    armed = false;
  });
  // A fire is consumed once a turn starts; the next fire may queue again.
  pi.on("before_agent_start", () => {
    for (const wake of wakes.values()) wake.pending = false;
  });

  pi.registerTool({
    name: "wake",
    label: "Wake",
    description:
      "Wake this session later with a note to self, once (in/at) or on an interval (every). The fire arrives as a message that starts a turn when idle or follows the current one. With check, a shell command runs at each tick and the session wakes only per `on` (always, change in output, failure, success), so polling costs no tokens until something changes. Recurring wakes stop after limit fires. Use this for follow-ups, supervision of long work, and CI polling; not for sub-minute loops.",
    promptSnippet: "Schedule a future or recurring wake-up for this session, optionally gated on a shell check.",
    parameters: Type.Object({
      text: Type.String({ description: "What to do when woken; self-contained" }),
      in: Type.Optional(Type.String({ description: "Delay like 20m, 2h (one-shot)" })),
      at: Type.Optional(Type.String({ description: "ISO timestamp (one-shot)" })),
      every: Type.Optional(Type.String({ description: "Interval like 10m, 1h (recurring; first fire after one interval)" })),
      limit: Type.Optional(Type.Number({ description: `Maximum fires for a recurring wake (default ${DEFAULT_LIMIT}, max ${MAX_LIMIT})` })),
      check: Type.Optional(Type.String({ description: "Shell command run at each tick in this session's cwd; its output is included in the fire" })),
      on: Type.Optional(Type.String({ description: "With check: always | change | failure | success (default change)" })),
    }),
    async execute(_id, params, _signal, _update, ctx) {
      cwd = ctx.cwd;
      const kinds = [params.in, params.at, params.every].filter(Boolean).length;
      if (kinds !== 1) throw new Error("give exactly one of in, at, or every");
      if (active().length >= MAX_ACTIVE) throw new Error(`this session already has ${MAX_ACTIVE} active wakes; cancel some first`);
      const now = clock.now();
      let at: number;
      let every: number | undefined;
      if (params.every) {
        every = parseDuration(params.every);
        if (every < MIN_INTERVAL_MS) throw new Error(`every must be at least ${describe(MIN_INTERVAL_MS)}`);
        at = now + every;
      } else if (params.in) {
        const delay = parseDuration(params.in);
        if (delay < 1_000) throw new Error("in must be at least 1s");
        at = now + delay;
      } else {
        at = Date.parse(params.at!);
        if (!Number.isFinite(at)) throw new Error(`at is not a valid timestamp: ${params.at}`);
        if (at <= now) throw new Error("at is in the past");
      }
      if (at - now > MAX_FUTURE_MS) throw new Error("wakes more than 30 days out are not kept; note it in memory instead");
      const on = (params.on ?? "change") as On;
      if (!["always", "change", "failure", "success"].includes(on)) throw new Error("on must be always, change, failure, or success");
      if (params.on && !params.check) throw new Error("on requires check");
      const limit = every ? Math.min(MAX_LIMIT, Math.max(1, Math.floor(params.limit ?? DEFAULT_LIMIT))) : 1;
      const wake: Wake = {
        id: `k-${randomBytes(3).toString("hex")}`,
        text: params.text,
        at,
        every,
        limit,
        fired: 0,
        check: params.check?.trim() || undefined,
        on,
        createdAt: now,
      };
      wakes.set(wake.id, wake);
      persist(wake);
      if (armed) arm(wake);
      const when = every ? `every ${describe(every)} (first in ${describe(at - now)}, up to ${limit} fires)` : `in ${describe(at - now)}`;
      return { content: [{ type: "text", text: `Wake ${wake.id} set: ${when}${wake.check ? `, checking \`${wake.check}\` and firing on ${on}` : ""}. End your turn; the fire will start a new one.` }], details: summary(wake) };
    },
  });

  pi.registerTool({
    name: "wake_list",
    label: "List wakes",
    description: "List this session's active wakes with next fire, interval, fire count, and check.",
    parameters: Type.Object({}),
    async execute() {
      const rows = active();
      const text = rows.length === 0
        ? "No active wakes."
        : rows.map((w) => `${w.id}  in ${describe(Math.max(0, w.at - clock.now()))}${w.every ? ` · every ${describe(w.every)} · ${w.fired}/${w.limit}` : " · once"}${w.check ? ` · check(${w.on})` : ""}  ${w.text.slice(0, 80)}`).join("\n");
      return { content: [{ type: "text", text }], details: rows.map(summary) };
    },
  });

  pi.registerTool({
    name: "wake_cancel",
    label: "Cancel wake",
    description: "Cancel a wake by id (or unique prefix).",
    parameters: Type.Object({ id: Type.String() }),
    async execute(_id, params) {
      const wake = find(params.id);
      finish(wake);
      return { content: [{ type: "text", text: `${wake.id} cancelled after ${wake.fired} fire(s)` }], details: summary(wake) };
    },
  });

  pi.registerTool({
    name: "wake_run",
    label: "Fire wake now",
    description: "Fire a wake immediately without waiting (runs its check too, ignoring the `on` gate). The schedule is unchanged.",
    parameters: Type.Object({ id: Type.String() }),
    async execute(_id, params) {
      const wake = find(params.id);
      await fire(wake, "manual");
      return { content: [{ type: "text", text: `${wake.id} fired (${wake.fired} total)` }], details: summary(wake) };
    },
  });

  pi.registerCommand("wakes", {
    description: "List this session's active wakes",
    handler: async (_args, ctx) => {
      if (!ctx.hasUI) return;
      const rows = active();
      ctx.ui.notify(rows.length === 0 ? "No active wakes." : rows.map((w) => `${w.id}  in ${describe(Math.max(0, w.at - clock.now()))}${w.every ? ` · every ${describe(w.every)} · ${w.fired}/${w.limit}` : ""}  ${w.text.slice(0, 80)}`).join("\n"), "info");
    },
  });
}
