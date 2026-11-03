/**
 * One plain-text line that summarizes Markdown, such as an agent's answer, for
 * status surfaces that show a single line. Deterministic: it picks and cleans
 * a line the text already contains.
 */

/** Longer summaries end at a sentence or word boundary within this many characters. */
const MAX_CHARS = 200;

const FENCE = /^(`{3,}|~{3,})/;
const HEADING = /^#{1,6}\s+/;
const ITEM = /^(?:[-*+]|\d+[.)])\s+(?:\[[ xX]\]\s+)?/;
const RULE = /^(?:[-*_]\s*){3,}$/;
/** Lines with no words, such as a lone list number, say nothing. */
const WORDLESS = /^[\p{P}\p{S}\d\s]*$/u;

/** Inline Markdown reduced to its text: links, images, code spans, emphasis. */
function plain(line: string): string {
  return line
    .replace(/!?\[([^\]]*)\]\([^)]*\)/g, '$1')
    .replace(/`+([^`]*)`+/g, '$1')
    .replace(/(\*\*|__|~~)(.+?)\1/g, '$2')
    .replace(/(^|\W)[*_](\S(?:.*?\S)?)[*_](?=\W|$)/g, '$1$2')
    .replace(/\s+/g, ' ')
    .trim();
}

function bound(line: string): string {
  if (line.length <= MAX_CHARS) return line;
  const head = line.slice(0, MAX_CHARS);
  const sentence = head.match(/^.*[.!?](?=\s)/)?.[0];
  if (sentence && sentence.length >= MAX_CHARS / 2) return sentence;
  const word = head.lastIndexOf(' ');
  return `${head.slice(0, word > 0 ? word : MAX_CHARS - 1)}\u2026`;
}

/**
 * The first prose line, skipping code, tables and rules. A lead-in (a line
 * ending in `:`) joins the line after it: "Changes: - Fixed the parser" reads
 * "Changes: Fixed the parser". Prose outranks list items; without prose, the
 * first heading gives the first item its context. Undefined when the text has
 * no words outside code.
 */
export function summaryLine(text: string | undefined): string | undefined {
  let fence: string | undefined;
  let heading: string | undefined;
  let lead: string | undefined;
  let item: string | undefined;
  for (const raw of (text ?? '').split('\n')) {
    const line = raw.trim();
    const marker = line.match(FENCE)?.[1];
    if (fence !== undefined) {
      if (marker?.startsWith(fence)) fence = undefined;
      continue;
    }
    if (marker) {
      fence = marker;
      continue;
    }
    if (line.startsWith('|') || RULE.test(line)) continue;
    const unquoted = line.replace(/^(?:>\s?)+/, '');
    const isHeading = HEADING.test(unquoted);
    const isItem = !isHeading && ITEM.test(unquoted);
    const words = plain(unquoted.replace(HEADING, '').replace(ITEM, ''));
    if (WORDLESS.test(words)) continue;
    if (isHeading) heading ??= words;
    else if (lead !== undefined) return bound(`${lead} ${words}`);
    else if (words.endsWith(':')) lead = words;
    else if (!isItem) return bound(words);
    else item ??= words;
  }
  if (lead !== undefined) return bound(lead.slice(0, -1));
  const parts = [heading, item].filter(part => part !== undefined);
  return parts.length > 0 ? bound(parts.join(': ')) : undefined;
}
