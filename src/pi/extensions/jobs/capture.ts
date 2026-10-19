import { createWriteStream } from "node:fs";
import type { Writable } from "node:stream";
import { writeFile } from "node:fs/promises";
import { join } from "node:path";
import { CHUNK_BYTES, PENDING_BYTES, PREFIX_BYTES, TAIL_BYTES, emptyCapture, type CaptureFacts } from "./protocol.ts";

/** Combined stdout/stderr arrival order, not a reconstruction of cross-pipe write order. */
export class Capture {
  readonly facts: CaptureFacts = emptyCapture();
  private readonly output: Writable;
  private readonly directory: string;
  private tail = Buffer.alloc(0);
  private prefixSeen = 0;
  private closed = false;
  private failed = false;
  private readonly pressure: (paused: boolean) => void;

  constructor(directory: string, pressure: (paused: boolean) => void, output: Writable = createWriteStream(join(directory, "output.log"), { flags: "wx", mode: 0o600 })) {
    this.directory = directory;
    this.pressure = pressure;
    this.output = output;
    this.output.on("error", (error) => {
      this.failed = true;
      this.facts.error = error.message.slice(0, 512);
      this.pressure(false); // A broken disk must not leave payload pipes paused forever.
    });
  }

  data(data: Buffer): void {
    if (this.closed) return;
    this.facts.bytesSeen += data.length;
    this.facts.capped = this.facts.bytesSeen > PREFIX_BYTES;
    this.tail = data.length >= TAIL_BYTES ? Buffer.from(data.subarray(-TAIL_BYTES)) : Buffer.concat([this.tail.subarray(Math.max(0, this.tail.length + data.length - TAIL_BYTES)), data]);
    this.facts.tailBytes = this.tail.length;
    const count = Math.min(data.length, PREFIX_BYTES - this.prefixSeen);
    this.prefixSeen += count;
    for (let offset = 0; offset < count; offset += CHUNK_BYTES) {
      const piece = data.subarray(offset, Math.min(count, offset + CHUNK_BYTES));
      // Normally pause at 256 KiB. Allow one explicitly bounded 64 KiB chunk
      // overshoot; abnormal oversized callbacks discard rather than allocate more.
      if (this.failed || this.facts.pendingBytes + piece.length > PENDING_BYTES + CHUNK_BYTES) {
        this.facts.droppedBytes += piece.length;
        continue;
      }
      const copy = Buffer.from(piece);
      this.facts.pendingBytes += copy.length;
      this.facts.peakPendingBytes = Math.max(this.facts.peakPendingBytes, this.facts.pendingBytes);
      this.output.write(copy, (error) => {
        this.facts.pendingBytes -= copy.length;
        if (error) this.facts.droppedBytes += copy.length;
        else this.facts.prefixBytes += copy.length;
        if (this.facts.pendingBytes < PENDING_BYTES / 2) this.pressure(false);
      });
    }
    if (!this.failed && this.prefixSeen < PREFIX_BYTES && this.facts.pendingBytes >= PENDING_BYTES) this.pressure(true);
    else if (this.prefixSeen >= PREFIX_BYTES) this.pressure(false); // Drain/discard forever after the cap.
  }

  async finish(deadlineMs = 250): Promise<void> {
    if (this.closed) return;
    this.closed = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const prefix = new Promise<void>((resolve) => {
      if (this.failed || this.output.destroyed) { resolve(); return; }
      this.output.once("error", () => resolve());
      this.output.end(() => { this.facts.prefixFlushed = !this.failed; resolve(); });
    });
    const tail = writeFile(join(this.directory, "tail.log"), this.tail, { flag: "wx", mode: 0o600 })
      .then(() => { this.facts.tailFlushed = true; }, (error: unknown) => { this.facts.error = String(error).slice(0, 512); });
    await Promise.race([
      Promise.all([prefix, tail]),
      new Promise<void>((resolve) => { timer = setTimeout(() => { this.facts.flushExpired = true; resolve(); }, deadlineMs); }),
    ]);
    clearTimeout(timer);
    if (this.facts.flushExpired) this.output.destroy();
  }
}
