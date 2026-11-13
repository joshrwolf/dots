import type {Agent} from '../../src/host/agent.ts';
import type {ActivityItem} from '../../src/host/activity.ts';

/** One OSC 7501 record. Named root records identify the live Pi runtime. */
export interface StatusRecord {
  readonly state: 'idle' | 'working' | 'done' | 'blocked' | 'error';
  readonly id?: string;
  readonly kind?: 'permission' | 'question';
  readonly title?: string;
  readonly message?: string;
}

export type Rest =
  | {readonly state: 'idle'}
  | {readonly state: 'done'; readonly message?: string | undefined}
  | {readonly state: 'error'; readonly message: string};

/** Everything the session's status depends on. */
export interface StatusInputs {
  readonly running: boolean;
  readonly compacting: boolean;
  /** An open dialog the agent waits on. */
  readonly dialog?: {readonly kind: string; readonly title?: string};
  /** How the last run ended, reported once nothing is active. */
  readonly rest: Rest;
  readonly activities: ReadonlyMap<string, readonly ActivityItem[]>;
  readonly sessionName?: string;
  /** A one-line summary of the user's latest prompt, what the session is about without a name. */
  readonly prompt?: string;
}

function backgroundSummary(inputs: StatusInputs): string | undefined {
  const counts = new Map<string, number>();
  for (const items of inputs.activities.values()) {
    for (const item of items) {
      const kind = item.id.split('/')[0]!;
      counts.set(kind, (counts.get(kind) ?? 0) + 1);
    }
  }
  if (counts.size === 0) return undefined;
  return [...counts]
    .map(([kind, n]) => `${n} ${kind}${n === 1 ? '' : 's'}`)
    .join(' \u00b7 ');
}

/**
 * The root record: blocked while the user must answer a dialog, working while
 * the agent runs, working while background work is active, then how the last
 * run ended.
 * Background work keeps the session `working`, so it never reports an unread
 * `done` while something it started still runs.
 */
export function rootRecord(inputs: StatusInputs): StatusRecord {
  const {dialog} = inputs;
  if (dialog)
    return {
      state: 'blocked',
      kind: dialog.kind === 'confirm' ? 'permission' : 'question',
      ...(dialog.title === undefined ? {} : {message: dialog.title}),
    };
  if (inputs.compacting)
    return {state: 'working', message: 'Compacting context'};
  const subject = inputs.sessionName ?? inputs.prompt;
  const sessionMessage = subject === undefined ? {} : {message: subject};
  if (inputs.running) return {state: 'working', ...sessionMessage};
  const background = backgroundSummary(inputs);
  if (background) return {state: 'working', message: background};
  if (inputs.rest.state === 'done')
    return {
      state: 'done',
      message: inputs.rest.message ?? 'Ready for your next instruction',
    };
  return inputs.rest;
}

/** One `working` child record per live background item. */
export function childRecords(inputs: StatusInputs): StatusRecord[] {
  const records: StatusRecord[] = [];
  for (const items of inputs.activities.values()) {
    for (const item of items)
      records.push({
        state: 'working',
        id: item.id,
        title: item.title,
        ...(item.message === undefined ? {} : {message: item.message}),
      });
  }
  return records;
}

const CONTROLS = /[\u0000-\u001f\u007f-\u009f]+/g;

/** Single-line text cut on a code point boundary to `max` UTF-8 bytes, base64 encoded. */
function text(value: string | undefined, max: number): string | undefined {
  const clean = (value ?? '').replace(CONTROLS, ' ').trim();
  if (!clean) return undefined;
  let bytes = 0;
  let end = 0;
  for (const char of clean) {
    const size = Buffer.byteLength(char, 'utf8');
    if (bytes + size > max) break;
    bytes += size;
    end += char.length;
  }
  return Buffer.from(clean.slice(0, end), 'utf8').toString('base64');
}

/** A live runtime's native status identity; segments fit OSC 7501's limits. */
export function agentStatusId(agent: Agent): string {
  return `pi/${agent.session.replaceAll('-', '')}/${agent.owner.slice(-16)}`;
}

/** The OSC 7501 report for a record, within the protocol's limits. */
export function formatRecord(record: StatusRecord): string {
  const pairs = [`state=${record.state}`];
  if (record.id) pairs.push(`id=${record.id}`);
  pairs.push('app=pi');
  if (record.state === 'blocked' && record.kind)
    pairs.push(`kind=${record.kind}`);
  const title = text(record.title, 192);
  if (title) pairs.push(`title=${title}`);
  const message = text(record.message, 2048);
  if (message) pairs.push(`msg=${message}`);
  return `\x1b]7501;${pairs.join(':')}\x1b\\`;
}

/** Removes the record at `id` and everything beneath it; no id removes every record. */
export function formatClear(id?: string): string {
  return `\x1b]7501;state=clear${id ? `:id=${id}` : ''}\x1b\\`;
}
