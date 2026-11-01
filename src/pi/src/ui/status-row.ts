import {truncateToWidth, visibleWidth} from '@earendil-works/pi-tui';
import {statusText} from '@joshrwolf/pi-kit/ui';

export interface StatusItem {
  readonly key: string;
  readonly text: string;
}

/** Keep publishers opaque, including their colors; never infer state from prose.
 * Our activity summaries precede notices, then unknown publishers in key order. */
export function statusItems(
  items: readonly StatusItem[],
): readonly StatusItem[] {
  const priority = (key: string) =>
    key === 'dots.jobs' ? 2 : key === 'dots-ui-release' ? 1 : 0;
  return [
    ...new Map(
      items.map(item => [
        item.key,
        {key: item.key, text: statusText(item.text.slice(0, 512)).trim()},
      ]),
    ).values(),
  ]
    .filter(item => item.text.length > 0)
    .sort(
      (a, b) => priority(b.key) - priority(a.key) || a.key.localeCompare(b.key),
    );
}

/** One bounded line. Whole publisher items yield before an explicit omission count. */
export function renderStatusRow(
  items: readonly StatusItem[],
  width: number,
): string[] {
  if (width <= 0 || !items.length) return [];
  const shown: string[] = [];
  const separator = '  ';
  for (let i = 0; i < items.length; i++) {
    const item = items[i]!;
    const rest = items.length - i - 1;
    const suffix = rest ? `  +${rest} more` : '';
    const candidate = [...shown, item.text].join(separator);
    if (visibleWidth(candidate) + visibleWidth(suffix) <= width) {
      shown.push(item.text + '\x1b[0m');
      continue;
    }
    if (!shown.length) {
      const room = width - visibleWidth(suffix);
      return [
        room > 0
          ? truncateToWidth(item.text, room) + '\x1b[0m' + suffix
          : truncateToWidth(`+${items.length}`, width),
      ];
    }
    return [shown.join(separator) + `  +${items.length - i} more`];
  }
  return [shown.join(separator)];
}
