import type {
  RpcCommand,
  RpcExtensionUIResponse,
  RpcResponse,
  RpcSessionState,
} from '@earendil-works/pi-coding-agent';

// The worker protocol: the subset of Pi's RPC protocol an owner consumes from
// a headless child over stdio. The child is our own spawned Pi on a pinned,
// versioned protocol, so decoding checks each record's discriminant and the
// fields the owner reads, not whole events.

type PiCommand<T extends RpcCommand['type']> = Omit<
  Extract<RpcCommand, {type: T}>,
  'id'
>;
type PiData<C extends RpcResponse['command']> =
  Extract<RpcResponse, {command: C; success: true}> extends {data: infer D}
    ? D
    : never;

/** What an owner sends: Pi's prompt and get_state. */
export type WorkerCommand = PiCommand<'prompt'> | PiCommand<'get_state'>;
export type Disposition = PiData<'prompt'>['disposition'];
export type WorkerState = Pick<RpcSessionState, 'sessionFile' | 'isStreaming'>;
/** An answer to a blocking extension-UI request, without its envelope. */
export type UiAnswer = RpcExtensionUIResponse extends infer R
  ? R extends unknown
    ? Omit<R, 'type' | 'id'>
    : never
  : never;

/** A finished or streaming assistant update: only what owners and inspectors read. */
export interface AssistantUpdate {
  readonly type: string;
  readonly contentIndex?: number;
  readonly delta?: string;
  readonly content?: string;
}

export interface UiRequest {
  readonly type: 'extension_ui_request';
  readonly id: string;
  readonly method: string;
  readonly title?: string;
  readonly message?: string;
  readonly options?: readonly string[];
  readonly timeout?: number;
}

export type WorkerEvent =
  | {readonly type: 'agent_start'}
  | {readonly type: 'agent_settled'; readonly aborted: boolean}
  | {
      readonly type: 'tool_execution_start';
      readonly toolName: string;
      readonly args: unknown;
    }
  | {
      readonly type: 'tool_execution_end';
      readonly toolName: string;
      readonly isError: boolean;
    }
  | {
      readonly type: 'message_update';
      readonly usage?: {readonly totalTokens: number};
      readonly assistantMessageEvent: AssistantUpdate;
    }
  /** The message itself is decoded by each consumer; it is the child's own record. */
  | {readonly type: 'message_end'; readonly message: unknown}
  | {readonly type: 'compaction_start'}
  | {
      readonly type: 'compaction_end';
      readonly result?: {readonly estimatedTokensAfter?: number};
    }
  | {
      readonly type: 'auto_retry_start';
      readonly attempt: number;
      readonly maxAttempts: number;
      readonly errorMessage: string;
    }
  | {readonly type: 'auto_retry_end'}
  | UiRequest;

export interface ResponseRecord {
  readonly type: 'response';
  readonly id?: string;
  readonly success: boolean;
  readonly data?: unknown;
  readonly error?: string;
}

type Fields = Readonly<Record<string, unknown>>;

export function fields(value: unknown): Fields | undefined {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
    ? (value as Fields)
    : undefined;
}

const text = (value: unknown): string | undefined =>
  typeof value === 'string' ? value : undefined;
const count = (value: unknown): number | undefined =>
  typeof value === 'number' && Number.isSafeInteger(value) && value >= 0
    ? value
    : undefined;

function assistantUpdate(value: unknown): AssistantUpdate | undefined {
  const update = fields(value);
  const type = text(update?.type);
  if (!update || type === undefined) return undefined;
  const contentIndex = count(update.contentIndex);
  const delta = text(update.delta);
  const content = text(update.content);
  return {
    type,
    ...(contentIndex === undefined ? {} : {contentIndex}),
    ...(delta === undefined ? {} : {delta}),
    ...(content === undefined ? {} : {content}),
  };
}

function uiRequest(record: Fields): UiRequest | undefined {
  const id = text(record.id);
  const method = text(record.method);
  if (id === undefined || method === undefined) return undefined;
  const title = text(record.title);
  const message = text(record.message);
  const options =
    Array.isArray(record.options) &&
    record.options.every(option => typeof option === 'string')
      ? (record.options as readonly string[])
      : undefined;
  const timeout =
    typeof record.timeout === 'number' && Number.isFinite(record.timeout)
      ? Math.max(0, record.timeout)
      : undefined;
  return {
    type: 'extension_ui_request',
    id,
    method,
    ...(title === undefined ? {} : {title}),
    ...(message === undefined ? {} : {message}),
    ...(options === undefined ? {} : {options}),
    ...(timeout === undefined ? {} : {timeout}),
  };
}

function event(record: Fields): WorkerEvent | undefined {
  switch (record.type) {
    case 'agent_start':
      return {type: 'agent_start'};
    case 'agent_settled':
      return {type: 'agent_settled', aborted: record.aborted === true};
    case 'tool_execution_start': {
      const toolName = text(record.toolName);
      return toolName === undefined
        ? undefined
        : {type: 'tool_execution_start', toolName, args: record.args};
    }
    case 'tool_execution_end': {
      const toolName = text(record.toolName);
      return toolName === undefined
        ? undefined
        : {
            type: 'tool_execution_end',
            toolName,
            isError: record.isError === true,
          };
    }
    case 'message_update': {
      const update = assistantUpdate(record.assistantMessageEvent);
      if (!update) return undefined;
      const totalTokens = count(fields(record.usage)?.totalTokens);
      return {
        type: 'message_update',
        assistantMessageEvent: update,
        ...(totalTokens === undefined ? {} : {usage: {totalTokens}}),
      };
    }
    case 'message_end':
      return {type: 'message_end', message: record.message};
    case 'compaction_start':
      return {type: 'compaction_start'};
    case 'compaction_end': {
      const after = count(fields(record.result)?.estimatedTokensAfter);
      return {
        type: 'compaction_end',
        ...(after === undefined ? {} : {result: {estimatedTokensAfter: after}}),
      };
    }
    case 'auto_retry_start':
      return {
        type: 'auto_retry_start',
        attempt: count(record.attempt) ?? 0,
        maxAttempts: count(record.maxAttempts) ?? 0,
        errorMessage: text(record.errorMessage) ?? '',
      };
    case 'auto_retry_end':
      return {type: 'auto_retry_end'};
    case 'extension_ui_request':
      return uiRequest(record);
    default:
      return undefined;
  }
}

/** A response, a consumed worker event, or undefined for anything else the child emits. */
export function decodeRecord(
  value: unknown,
): ResponseRecord | WorkerEvent | undefined {
  const record = fields(value);
  if (!record) return undefined;
  if (record.type === 'response') {
    if (typeof record.success !== 'boolean') return undefined;
    const id = text(record.id);
    const error = text(record.error);
    return {
      type: 'response',
      success: record.success,
      data: record.data,
      ...(id === undefined ? {} : {id}),
      ...(error === undefined ? {} : {error}),
    };
  }
  return event(record);
}

export function decodeDisposition(data: unknown): Disposition {
  const disposition = fields(data)?.disposition;
  if (
    disposition === 'started' ||
    disposition === 'queued' ||
    disposition === 'handled'
  )
    return disposition;
  throw new Error(
    `prompt acknowledged with an unknown disposition: ${JSON.stringify(disposition)?.slice(0, 80)}`,
  );
}

export function decodeState(data: unknown): WorkerState {
  const state = fields(data);
  const sessionFile = state?.sessionFile;
  if (
    typeof state?.isStreaming !== 'boolean' ||
    (sessionFile !== undefined && typeof sessionFile !== 'string')
  )
    throw new Error('get_state returned no session state');
  return {
    isStreaming: state.isStreaming,
    ...(sessionFile === undefined ? {} : {sessionFile}),
  };
}

/** Calls `onValue` with each complete LF-terminated JSON line; U+2028/U+2029 stay legal inside strings (Pi's rpc.md framing). */
export function jsonLines(
  onValue: (value: unknown) => void,
): (chunk: string) => void {
  let buffer = '';
  return chunk => {
    buffer += chunk;
    let index = buffer.indexOf('\n');
    while (index !== -1) {
      const line = buffer.slice(0, index).replace(/\r$/, '');
      buffer = buffer.slice(index + 1);
      index = buffer.indexOf('\n');
      if (!line) continue;
      let value: unknown;
      try {
        value = JSON.parse(line);
      } catch {
        continue;
      } // Not a protocol record; Pi reserves stdout for records.
      onValue(value);
    }
  };
}

export function asError(value: unknown): Error {
  return value instanceof Error ? value : new Error(String(value));
}

interface Pending {
  readonly command: string;
  readonly resolve: (data: unknown) => void;
  readonly reject: (error: Error) => void;
  readonly timer: ReturnType<typeof setTimeout>;
}

/** Correlates commands with their responses, with an acknowledgement deadline per command. */
export class Requests {
  private readonly pending = new Map<string, Pending>();
  private readonly prefix: string;
  private readonly timeoutMs: number;
  private sequence = 0;

  constructor(prefix: string, timeoutMs: number) {
    this.prefix = prefix;
    this.timeoutMs = timeoutMs;
  }

  /** Writes `command` with a fresh id and resolves with its success data. `write` reports asynchronous write failures through `fail`. */
  send(
    command: WorkerCommand,
    write: (line: string, fail: (error: Error) => void) => void,
  ): Promise<unknown> {
    const id = `${this.prefix}${++this.sequence}`;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(
        () =>
          this.settle(
            id,
            new Error(
              `worker ${command.type} acknowledgement timed out after ${this.timeoutMs}ms`,
            ),
          ),
        this.timeoutMs,
      );
      this.pending.set(id, {command: command.type, resolve, reject, timer});
      try {
        write(`${JSON.stringify({...command, id})}\n`, error =>
          this.settle(id, error),
        );
      } catch (error) {
        this.settle(id, asError(error));
      }
    });
  }

  /** Settles the request a response answers; false when it answers none. */
  answer(response: ResponseRecord): boolean {
    if (response.id === undefined) return false;
    const request = this.pending.get(response.id);
    if (!request) return false;
    this.settle(
      response.id,
      response.success
        ? undefined
        : new Error(response.error ?? `${request.command} failed`),
      response.data,
    );
    return true;
  }

  failAll(error: Error): void {
    for (const id of [...this.pending.keys()]) this.settle(id, error);
  }

  private settle(id: string, error: Error | undefined, data?: unknown): void {
    const request = this.pending.get(id);
    if (!request) return;
    this.pending.delete(id);
    clearTimeout(request.timer);
    if (error) request.reject(error);
    else request.resolve(data);
  }
}
