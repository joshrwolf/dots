import {watch} from 'node:fs';
import path from 'node:path';

/**
 * Call `changed` whenever a writer touches the substrate's changes marker:
 * the sync agent after it imports other machines' state, and local writers
 * after they defer updates. The watch covers the marker's directory, since
 * writers create or replace the file. A failing watch closes and calls
 * `lost`. Returns undefined when the directory cannot be watched; callers
 * then rely on their ordinary reads.
 */
export function watchChanges(
  marker: string,
  changed: () => void,
  lost: () => void = () => {},
): {readonly close: () => void} | undefined {
  try {
    const name = path.basename(marker);
    const watcher = watch(
      path.dirname(marker),
      {persistent: false},
      (_, file) => {
        if (file === null || file === name) changed();
      },
    );
    let open = true;
    const close = () => {
      if (!open) return;
      open = false;
      watcher.close();
    };
    watcher.on('error', () => {
      close();
      lost();
    });
    return {close};
  } catch {
    return undefined;
  }
}
