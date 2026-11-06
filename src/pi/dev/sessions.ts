import {appendFileSync, writeFileSync} from 'node:fs';
import {CURRENT_SESSION_VERSION} from '@earendil-works/pi-coding-agent';

/**
 * A neutral Pi session file for fixtures, written the way Pi's session
 * manager writes one: a header line, then one entry per line, each the child
 * of the one before.
 */
export class SessionFixture {
  readonly file: string;
  private last: string | null = null;
  private count = 0;
  private clock = Date.parse('2026-01-01T00:00:00.000Z');

  constructor(file: string, cwd: string) {
    this.file = file;
    writeFileSync(
      file,
      JSON.stringify({
        type: 'session',
        version: CURRENT_SESSION_VERSION,
        id: 'fixture-session',
        timestamp: this.time(),
        cwd,
      }) + '\n',
      {flag: 'wx'},
    );
  }

  private time(): string {
    this.clock += 1000;
    return new Date(this.clock).toISOString();
  }

  /** Appends one message entry; returns the message as recorded. */
  message(message: Record<string, unknown>): Record<string, unknown> {
    const timestamp = this.time();
    const recorded = {timestamp: Date.parse(timestamp), ...message};
    this.entry({type: 'message', message: recorded}, timestamp);
    return recorded;
  }

  user(text: string): void {
    this.message({role: 'user', content: text});
  }

  assistant(
    content: readonly Record<string, unknown>[],
    stopReason = 'stop',
  ): Record<string, unknown> {
    return this.message(assistantMessage(content, stopReason));
  }

  toolResult(toolCallId: string, toolName: string, text: string): void {
    this.message({
      role: 'toolResult',
      toolCallId,
      toolName,
      content: [{type: 'text', text}],
      isError: false,
    });
  }

  private entry(value: Record<string, unknown>, timestamp: string): void {
    const id = (++this.count).toString(16).padStart(8, '0');
    appendFileSync(
      this.file,
      JSON.stringify({...value, id, parentId: this.last, timestamp}) + '\n',
    );
    this.last = id;
  }
}

/** An assistant message as a provider records it, with fixture metadata. */
export function assistantMessage(
  content: readonly Record<string, unknown>[],
  stopReason = 'stop',
): Record<string, unknown> {
  return {
    role: 'assistant',
    content,
    api: 'fixture',
    provider: 'fixture',
    model: 'fixture',
    usage: {
      input: 0,
      output: 0,
      cacheRead: 0,
      cacheWrite: 0,
      totalTokens: 0,
      cost: {input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0},
    },
    stopReason,
  };
}
