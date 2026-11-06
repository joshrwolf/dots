import {closeSync, fstatSync, openSync, readSync} from 'node:fs';
import {
  buildContextEntries,
  type SessionEntry,
  type SessionHeader,
} from '@earendil-works/pi-coding-agent';

/** What a follower currently knows about one saved worker session. */
export interface FollowedSession {
  readonly header: SessionHeader | undefined;
  /** The active branch with compaction applied, as Pi renders it. */
  readonly entries: readonly SessionEntry[];
  /** The latest session_info name, if any. */
  readonly name: string | undefined;
  /** Why the view may be incomplete; undefined when every saved line was read. */
  readonly notice: string | undefined;
}

const READ_CHUNK = 1024 * 1024;
const LF = 0x0a;
const clean = (text: string) =>
  text.replace(/[\x00-\x08\x0b-\x1f\x7f-\x9f]/g, ' ').trim();

function record(value: unknown): Record<string, unknown> | undefined {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : undefined;
}

/**
 * Follows a session file another Pi is appending to, read-only. Pi's own
 * loader repairs a partial last line and writes a header into an empty file,
 * so this never opens the file through `SessionManager`. Only complete
 * LF-terminated lines are parsed; a partial trailing line is read again on
 * the next poll. A file that shrinks, is replaced, or no longer matches what
 * was read is reloaded from the start.
 */
export class SessionFollower {
  readonly file: string | undefined;
  private inode: number | undefined;
  private consumed = 0;
  private headerBytes: Buffer | undefined;
  private header: SessionHeader | undefined;
  private all: SessionEntry[] = [];
  private readonly byId = new Map<string, SessionEntry>();
  private name: string | undefined;
  private skipped = 0;
  /** The first line is not a session header, as Pi's own loader requires. */
  private foreign = false;
  private notice: string | undefined;
  private active: readonly SessionEntry[] = [];

  constructor(file: string | undefined) {
    this.file = file;
    if (!file) this.notice = 'session transcript not saved yet';
  }

  get session(): FollowedSession {
    const notices = [
      this.notice,
      this.foreign ? 'not a Pi session file' : undefined,
      this.skipped
        ? `${this.skipped} unreadable record${this.skipped === 1 ? '' : 's'} skipped`
        : undefined,
    ].filter(Boolean);
    return {
      header: this.header,
      entries: this.active,
      name: this.name,
      notice: notices.length ? notices.join('; ') : undefined,
    };
  }

  /** Reads what was appended since the last poll; true when the session changed. */
  poll(): boolean {
    if (!this.file) return false;
    let fd: number | undefined;
    try {
      fd = openSync(this.file, 'r');
      const stat = fstatSync(fd);
      let reset = false;
      if (this.inode !== undefined && stat.ino !== this.inode) reset = true;
      else if (stat.size < this.consumed) reset = true;
      else if (this.consumed > 0 && !this.continuous(fd)) reset = true;
      const changed = reset || this.notice !== undefined;
      if (reset) this.clear();
      this.inode = stat.ino;
      this.notice = undefined;
      return this.read(fd, stat.size) || changed;
    } catch (error) {
      const code = (error as NodeJS.ErrnoException).code;
      const notice =
        code === 'ENOENT'
          ? this.consumed > 0
            ? 'saved session transcript is missing'
            : 'session transcript not saved yet'
          : `transcript unavailable: ${clean((error as Error).message).slice(0, 256)}`;
      const changed = notice !== this.notice || this.consumed > 0;
      if (code === 'ENOENT') this.clear();
      this.notice = notice;
      return changed;
    } finally {
      if (fd !== undefined) closeSync(fd);
    }
  }

  private clear(): void {
    this.inode = undefined;
    this.consumed = 0;
    this.headerBytes = undefined;
    this.header = undefined;
    this.all = [];
    this.byId.clear();
    this.name = undefined;
    this.skipped = 0;
    this.foreign = false;
    this.active = [];
  }

  /** The bytes already read are still the file's: same header, a line boundary where reading stopped. */
  private continuous(fd: number): boolean {
    const boundary = Buffer.alloc(1);
    if (readSync(fd, boundary, 0, 1, this.consumed - 1) !== 1) return false;
    if (boundary[0] !== LF) return false;
    if (!this.headerBytes) return true;
    const header = Buffer.alloc(this.headerBytes.length);
    const read = readSync(fd, header, 0, header.length, 0);
    return read === header.length && header.equals(this.headerBytes);
  }

  private read(fd: number, size: number): boolean {
    let added = false;
    let pending = Buffer.alloc(0);
    let position = this.consumed;
    while (position < size) {
      const chunk = Buffer.alloc(Math.min(READ_CHUNK, size - position));
      const count = readSync(fd, chunk, 0, chunk.length, position);
      if (count === 0) break;
      position += count;
      let bytes = Buffer.concat([pending, chunk.subarray(0, count)]);
      let start = 0;
      for (
        let newline = bytes.indexOf(LF, start);
        newline >= 0;
        newline = bytes.indexOf(LF, start)
      ) {
        const line = bytes.subarray(start, newline);
        if (this.accept(line)) added = true;
        this.consumed += newline + 1 - start;
        start = newline + 1;
      }
      bytes = bytes.subarray(start);
      pending = Buffer.from(bytes);
    }
    if (added)
      this.active = buildContextEntries(
        this.all,
        this.all.at(-1)?.id ?? null,
        this.byId,
      );
    return added;
  }

  /** One complete line; true when it changed the session. */
  private accept(line: Buffer): boolean {
    const text = line.toString('utf8').replace(/\r$/, '');
    if (!text.trim()) return false;
    let value: Record<string, unknown> | undefined;
    try {
      value = record(JSON.parse(text));
    } catch {
      value = undefined;
    }
    if (this.foreign) return false;
    if (!this.header) {
      if (value?.type === 'session' && typeof value.id === 'string') {
        this.header = value as unknown as SessionHeader;
        this.headerBytes = Buffer.from(line);
      } else this.foreign = true;
      return true;
    }
    if (
      !value ||
      typeof value.type !== 'string' ||
      typeof value.id !== 'string' ||
      value.type === 'session'
    ) {
      this.skipped++;
      return true;
    }
    const entry = value as unknown as SessionEntry;
    this.all.push(entry);
    this.byId.set(entry.id, entry);
    if (entry.type === 'session_info') this.name = entry.name || undefined;
    return true;
  }
}
