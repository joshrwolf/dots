import {labelText} from '@joshrwolf/pi-kit/ui';
import {SessionFollower} from './follow.ts';
import {projectTranscript} from './projection.ts';

/** Bytes of a session a tail reads; older records stay in the session file. */
const TAIL_BYTES = 1024 * 1024;
const MAX_LINES = 100;

/**
 * The latest assistant text, tool calls and tool results on a worker's active
 * branch, as one-line text for model-facing status. It reads through the same
 * follower and projection as the transcript view, from the end of the file.
 */
export function transcriptTail(
  sessionFile: string | undefined,
  limit = 20,
  width = 160,
): string[] {
  const count = Number.isFinite(limit)
    ? Math.min(MAX_LINES, Math.max(0, Math.floor(limit)))
    : 20;
  if (!sessionFile || count === 0) return [];
  const columns = Number.isFinite(width)
    ? Math.min(1000, Math.max(1, Math.floor(width)))
    : 160;
  const follower = new SessionFollower(sessionFile, TAIL_BYTES);
  follower.poll();
  const {entries, truncated, notice} = follower.session;
  const lines: string[] = [];
  const add = (prefix: string, text: string) => {
    const line = labelText(text);
    if (line) lines.push(`${prefix}${line}`.slice(0, columns));
  };
  for (const item of projectTranscript(entries)) {
    if (item.kind === 'assistant') {
      for (const block of item.message.content)
        if (block.type === 'text') add('', block.text);
    } else if (item.kind === 'tool') {
      const args = item.call.arguments
        ? JSON.stringify(item.call.arguments)
        : '';
      add('', `-> ${item.call.name} ${args}`);
      add(
        '   <- ',
        (item.result?.content ?? [])
          .map(block => (block.type === 'text' ? block.text : ''))
          .join(' '),
      );
    }
    if (lines.length > MAX_LINES) lines.splice(0, lines.length - MAX_LINES);
  }
  const notes = [
    truncated ? 'older records omitted; inspect the session file' : '',
    notice ?? '',
  ].filter(Boolean);
  if (notes.length && lines.length < count)
    lines.unshift(`(${notes.join('; ')})`);
  return lines.slice(-count);
}
