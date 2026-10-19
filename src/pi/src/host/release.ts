import { realpathSync } from "node:fs";
import { basename, join } from "node:path";
import { fileURLToPath } from "node:url";
import { getAgentDir } from "@earendil-works/pi-coding-agent";

function identity(path: string): string | undefined {
  try {
    const name = basename(realpathSync(path));
    return /^[a-f0-9]{64}$/.test(name) ? name : "checkout";
  } catch { return undefined; }
}

export interface ReleaseStatus {
  readonly loaded: string;
  readonly installed: string | undefined;
  readonly reloadNeeded: boolean;
}

/** Capture the loaded module's identity once. Inspect the installed pointer only
 * on lifecycle events or explicit requests, never inside a render callback. */
export function releaseTracker(loadedRoot: string, installedPath: string): () => ReleaseStatus {
  const loaded = identity(loadedRoot) ?? "unknown";
  return () => {
    const installed = identity(installedPath);
    return { loaded, installed, reloadNeeded: /^[a-f0-9]{64}$/.test(loaded) && installed !== undefined && installed !== "checkout" && loaded !== installed };
  };
}

export const releaseStatus = releaseTracker(fileURLToPath(new URL("../../", import.meta.url)), join(getAgentDir(), "dots/packages/pi-dots/current"));

export function releaseDescription(status: ReleaseStatus): string {
  const short = (value: string | undefined) => value === undefined ? "unavailable" : value.slice(0, 12);
  return `Companion · loaded ${short(status.loaded)} · installed ${short(status.installed)}` +
    (status.reloadNeeded ? " · /reload when background work is idle" : status.loaded === "checkout" ? " · development source" : status.installed === status.loaded ? " · up to date" : "");
}
