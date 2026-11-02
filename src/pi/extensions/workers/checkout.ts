import path from 'node:path';
import type {ExtensionAPI} from '@earendil-works/pi-coding-agent';
import {sourceText} from '../../src/index.ts';

export interface CheckoutCue {
  readonly kind: 'branch' | 'directory';
  readonly label: string;
  readonly detached?: boolean;
}
type Probe =
  | {
      readonly kind: 'git';
      readonly root: string;
      readonly label: string;
      readonly detached?: boolean;
    }
  | {readonly kind: 'nonGit'};
type Entry = {value: Probe | undefined; expires: number; pending: boolean};

const TIMEOUT_MS = 1200;
// Each probe prints one path, ref or hash; more output is not a Git answer.
const MAX_OUTPUT_BYTES = 64 * 1024;
const CACHE_MS = 10_000;
const MAX_DIRS = 64;
const MAX_PROBES = 2;
const safe = (value: string): string | undefined =>
  value && sourceText(value) === value ? value : undefined;
const branchCue = (git: Extract<Probe, {kind: 'git'}>): CheckoutCue => ({
  kind: 'branch',
  label: git.label.length > 120 ? `${git.label.slice(0, 119)}…` : git.label,
  ...(git.detached ? {detached: true} : {}),
});

/** Git facts come from the host's bounded process runner, never from a renderer.
 * An ambiguous failure has no cue; only Git's explicit non-repository error is a folder.
 * A killed run (deadline, abort or output cap) is ambiguous. */
async function probe(
  cwd: string,
  exec: ExtensionAPI['exec'],
  signal: AbortSignal,
): Promise<Probe | undefined> {
  const run = (args: string[]) =>
    exec('git', args, {
      cwd,
      timeout: TIMEOUT_MS,
      maxBuffer: MAX_OUTPUT_BYTES,
      signal,
    });
  try {
    const root = await run(['rev-parse', '--show-toplevel']);
    if (root.killed || signal.aborted) return undefined;
    if (root.code !== 0)
      return root.code === 128 && /not a git repository/i.test(root.stderr)
        ? {kind: 'nonGit'}
        : undefined;
    const top = safe(root.stdout.trim());
    if (!top || !path.isAbsolute(top)) return undefined;
    const branch = await run(['symbolic-ref', '--quiet', '--short', 'HEAD']);
    if (branch.killed || signal.aborted) return undefined;
    if (branch.code === 0) {
      const name = safe(branch.stdout.trim());
      return name
        ? {kind: 'git', root: path.normalize(top), label: name}
        : undefined;
    }
    if (branch.code !== 1) return undefined;
    const head = await run(['rev-parse', '--short=8', 'HEAD']);
    if (head.killed || head.code !== 0 || signal.aborted) return undefined;
    const sha = head.stdout.trim();
    return /^[a-f0-9]{4,40}$/i.test(sha)
      ? {kind: 'git', root: path.normalize(top), label: sha, detached: true}
      : undefined;
  } catch {
    return undefined;
  }
}

/** Cache Git identity briefly per directory and probe at most two directories
 * concurrently. Observer ticks or an open inspector retry skipped directories. */
export function createCheckoutResolver(
  exec: ExtensionAPI['exec'],
  onChange: () => void,
  onError: (error: unknown) => void = error => {
    console.error('checkout observation failed:', error);
  },
) {
  const entries = new Map<string, Entry>();
  const controller = new AbortController();
  let active = 0;
  let disposed = false;
  const read = (cwd: string): Probe | undefined => {
    if (!path.isAbsolute(cwd) || disposed) return undefined;
    const key = path.normalize(cwd);
    const old = entries.get(key);
    if (old?.pending || (old && old.expires > Date.now())) return old.value;
    if (active >= MAX_PROBES) return old?.value;
    if (!old && entries.size >= MAX_DIRS) {
      const evict = [...entries].find(([, value]) => !value.pending)?.[0];
      if (!evict) return undefined;
      entries.delete(evict);
    }
    const entry: Entry = {
      value: old?.value,
      expires: old?.expires ?? 0,
      pending: true,
    };
    entries.set(key, entry);
    active++;
    // probe() reports Git failures as unknown; a throwing consumer callback is ours to report.
    probe(key, exec, controller.signal)
      .then(value => {
        if (disposed || entries.get(key) !== entry) return;
        entry.value = value;
        entry.expires = Date.now() + CACHE_MS;
        entry.pending = false;
        onChange();
      })
      .catch((error: unknown) => {
        if (!disposed) onError(error);
      })
      .finally(() => {
        active--;
      });
    return entry.value;
  };
  return {
    branch(cwd: string): CheckoutCue | undefined {
      const value = read(cwd);
      return value?.kind === 'git' ? branchCue(value) : undefined;
    },
    get(cwd: string, ownerCwd: string): CheckoutCue | undefined {
      if (path.normalize(cwd) === path.normalize(ownerCwd)) return undefined;
      const target = read(cwd);
      const owner = read(ownerCwd);
      if (!target || !owner) return undefined;
      if (
        target.kind === 'git' &&
        owner.kind === 'git' &&
        target.root === owner.root
      )
        return undefined;
      return target.kind === 'git'
        ? branchCue(target)
        : {kind: 'directory', label: path.basename(cwd) || cwd};
    },
    dispose(): void {
      disposed = true;
      controller.abort();
      entries.clear();
    },
  };
}
