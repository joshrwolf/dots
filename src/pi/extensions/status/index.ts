import type {ExtensionAPI} from '@earendil-works/pi-coding-agent';
import {
  ACTIVITY_EVENT,
  type ActivityItem,
  type ActivitySnapshot,
} from '../../src/host/activity.ts';
import {
  childRecords,
  formatClear,
  formatRecord,
  rootRecord,
  type Rest,
  type StatusRecord,
} from './model.ts';
import {summaryLine} from '../../src/host/summary.ts';

/**
 * Owns this terminal's program status (OSC 7501), counting the agent and the
 * jobs and workers it started: the session stays `working` until all of them
 * have settled. Pi's own reporter, which counts only the agent loop, is off.
 * Jobs and workers publish their live items as child records.
 */

export type Write = (data: string) => void;
export type Schedule = (run: () => void, ms: number) => () => void;

/**
 * Coalesce settled transitions so queued work or a retry can replace them
 * before the terminal presents a state that is about to change.
 */
export const SETTLE_MS = 1000;

const schedule: Schedule = (run, ms) => {
  const timer = setTimeout(run, ms);
  timer.unref();
  return () => clearTimeout(timer);
};

const failure = (text: string | undefined) => summaryLine(text) ?? 'Run failed';

/** The text of a user message's content. */
const contentText = (content: unknown): string | undefined => {
  if (typeof content === 'string') return content;
  if (!Array.isArray(content)) return undefined;
  return content
    .map(part =>
      part && typeof part === 'object' && 'text' in part
        ? String((part as {text: unknown}).text)
        : '',
    )
    .join('\n');
};

/** The latest prompt on a branch, so a resumed session reports what it was about. */
const latestPrompt = (
  branch: readonly {
    type: string;
    message?: {role?: string; content?: unknown};
  }[],
): string | undefined => {
  for (let i = branch.length - 1; i >= 0; i--) {
    const entry = branch[i]!;
    if (entry.type !== 'message' || entry.message?.role !== 'user') continue;
    const line = summaryLine(contentText(entry.message.content));
    if (line !== undefined) return line;
  }
  return undefined;
};

export default function status(
  pi: ExtensionAPI,
  env: NodeJS.ProcessEnv = process.env,
  write: Write = data => {
    process.stdout.write(data);
  },
  isTty: () => boolean = () => process.stdout.isTTY === true,
  later: Schedule = schedule,
) {
  // Pi reads this when its terminal starts, after extensions load.
  env.PI_PROGRAM_STATUS = '0';

  let enabled = false;
  let running = false;
  let compacting = false;
  let rest: Rest = {state: 'idle'};
  const prompts: {kind: string; title?: string}[] = [];
  const activities = new Map<string, readonly ActivityItem[]>();
  let prompt: string | undefined;
  let lastRoot: StatusRecord = {state: 'idle'};
  const lastChildren = new Map<string, string>();
  let pending: {root: StatusRecord; cancel: () => void} | undefined;

  const writeRoot = (root: StatusRecord) => {
    pending?.cancel();
    pending = undefined;
    if (JSON.stringify(root) === JSON.stringify(lastRoot)) return;
    lastRoot = root;
    write(formatRecord(root));
  };
  const flush = () => {
    if (pending) writeRoot(pending.root);
  };

  const report = () => {
    if (!enabled) return;
    const sessionName = pi.getSessionName();
    const inputs = {
      running,
      compacting,
      prompts,
      rest,
      activities,
      ...(sessionName === undefined ? {} : {sessionName}),
      ...(prompt === undefined ? {} : {prompt}),
    };
    const root = rootRecord(inputs);
    const settling =
      (root.state === 'done' ||
        root.state === 'error' ||
        root.state === 'idle') &&
      (lastRoot.state === 'working' || lastRoot.state === 'blocked');
    if (!settling) writeRoot(root);
    else if (JSON.stringify(root) !== JSON.stringify(pending?.root)) {
      pending?.cancel();
      const cancel = later(() => {
        pending = undefined;
        writeRoot(root);
      }, SETTLE_MS);
      pending = {root, cancel};
    }
    const seen = new Set<string>();
    for (const child of childRecords(inputs)) {
      const id = child.id!;
      seen.add(id);
      const key = JSON.stringify(child);
      if (lastChildren.get(id) === key) continue;
      lastChildren.set(id, key);
      write(formatRecord(child));
    }
    for (const id of lastChildren.keys()) {
      if (seen.has(id)) continue;
      lastChildren.delete(id);
      write(formatClear(id));
    }
  };

  pi.on('session_start', (_event, ctx) => {
    enabled = ctx.mode === 'tui' && isTty() && !env.DOTS_WORKER_ID;
    prompt = latestPrompt(ctx.sessionManager.getBranch());
    // Replace whatever an earlier instance left, even when it matches this one's idle start.
    lastRoot = {state: 'idle', message: '\u0000'};
    report();
  });
  pi.on('session_shutdown', event => {
    running = false;
    compacting = false;
    prompts.length = 0;
    pending?.cancel();
    pending = undefined;
    // Quitting removes every record. A reload or session switch reports
    // nothing; the next instance reports the current state when it starts.
    if (enabled && event.reason === 'quit') write(formatClear());
    enabled = false;
  });
  pi.on('before_agent_start', event => {
    if (event.messageType !== 'user') return;
    prompt = summaryLine(event.prompt) ?? prompt;
  });
  pi.on('agent_start', () => {
    running = true;
    rest = {state: 'done'};
    report();
  });
  pi.on('message_end', event => {
    // The latest response decides the outcome, so a retried error is replaced by its successful retry.
    if (event.message.role !== 'assistant') return;
    rest =
      event.message.stopReason === 'error'
        ? {state: 'error', message: failure(event.message.errorMessage)}
        : {
            state: 'done',
            message: summaryLine(contentText(event.message.content)),
          };
  });
  pi.on('session_before_compact', () => {
    compacting = true;
    report();
  });
  pi.on('session_compact', event => {
    compacting = false;
    if (!running && event.reason === 'manual') rest = {state: 'idle'};
    report();
  });
  // Also fires when a compaction is cancelled, so `compacting` cannot stick.
  pi.on('session_compact_failed', event => {
    compacting = false;
    if (event.aborted) rest = {state: 'idle'};
    else if (event.errorMessage)
      rest = {state: 'error', message: failure(event.errorMessage)};
    report();
  });
  pi.on('agent_settled', event => {
    running = false;
    if (event.aborted || (rest.state === 'done' && activities.size > 0))
      rest = {state: 'idle'};
    report();
  });
  pi.on('session_info_changed', () => report());
  pi.on('ui_prompt_start', event => {
    // Idle pickers, inspectors and loaders are not an agent waiting for input.
    if (event.kind === 'custom' || !running) return;
    prompts.push({
      kind: event.kind,
      ...(event.title === undefined ? {} : {title: event.title}),
    });
    report();
  });
  pi.on('ui_prompt_end', event => {
    if (event.kind === 'custom') return;
    const index = prompts.map(prompt => prompt.kind).lastIndexOf(event.kind);
    if (index >= 0) prompts.splice(index, 1);
    report();
  });

  pi.events.on(ACTIVITY_EVENT, data => {
    const snapshot = data as ActivitySnapshot;
    if (snapshot.items.length > 0) {
      activities.set(snapshot.source, snapshot.items);
      if (!running && rest.state === 'done') rest = {state: 'idle'};
    } else activities.delete(snapshot.source);
    report();
  });
}
