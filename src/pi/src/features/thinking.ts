import type {MarkdownTransformer} from '@earendil-works/pi-coding-agent';
import {displayLimits} from '@joshrwolf/pi-kit/ui';

// A disclosure label, not a live status: completed thoughts must not look busy.
export const hiddenThinkingLabel = '▸ Thinking';

/** A reasoning summary's heading, such as "Checking status", or undefined when the block is not one. */
export function thinkingHeading(markdown: string): string | undefined {
  const text = markdown.trim();
  if (!text) return undefined;
  const line = text.split(/\r?\n/, 1)[0]!;
  return (
    /^#{1,6}\s+(.+?)(?:\s+#+)?$/.exec(line)?.[1] ??
    /^\*\*([^*]+)\*\*$/.exec(line)?.[1] ??
    /^__([^_]+)__$/.exec(line)?.[1]
  );
}

/** Display-only Markdown; Pi still owns reasoning visibility, input and records. */
export const thinkingMarkdown: MarkdownTransformer = (markdown, context) => {
  if (
    context.messageType !== 'assistant-thinking' ||
    markdown.length > displayLimits.sourceCodeUnits
  )
    return markdown;
  const text = markdown.trim();
  if (!text) return markdown;
  const lines = text.split(/\r?\n/, displayLimits.sourceLines + 1);
  if (lines.length > displayLimits.sourceLines) return markdown; // Keep oversized records on the native path.

  if (lines.length === 1 && text.length <= 240) {
    // Only flatten standalone summary headings, never lists, fences or quotations.
    const heading = thinkingHeading(text);
    if (heading) return `**Thinking** · ${heading}`;
    if (
      !/^(?:[-+*](?:\s|$)|>|`{3}|~{3}|#{1,6}(?:\s|$)|\d+[.)]\s|<|(?:[-*_]\s*){3,}$)/.test(
        text,
      )
    )
      return `**Thinking** · ${text}`;
  }
  return `**Thinking**\n\n${lines.map(line => `> ${line}`).join('\n')}`;
};
