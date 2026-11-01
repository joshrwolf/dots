import assert from 'node:assert/strict';
import {readFileSync, realpathSync} from 'node:fs';
import {execFileSync} from 'node:child_process';
import {dirname, join} from 'node:path';
import {findPackageJSON} from 'node:module';
import {pathToFileURL} from 'node:url';
import {parse as parseYaml} from 'yaml';

// SDK acceptance and terminal fixtures must use the same release as CLI acceptance.
// A candidate installation can be selected without changing the live Pi launcher.
const root =
  process.env.PI_TEST_HOST_ROOT ??
  join(
    execFileSync(
      process.env.MISE_BIN ?? 'mise',
      ['where', 'npm:@earendil-works/pi-coding-agent'],
      {
        encoding: 'utf8',
        timeout: 10000,
      },
    ).trim(),
    'node_modules/@earendil-works',
  );
export const hostPins: Record<string, string> = parseYaml(
  readFileSync(
    new URL('../../../../pnpm-workspace.yaml', import.meta.url),
    'utf8',
  ),
).catalog;

export function hostModule(name: string, entry = 'index.js'): string {
  const manifest = findPackageJSON(
    `@earendil-works/${name}`,
    pathToFileURL(realpathSync(join(root, 'pi-coding-agent/package.json'))),
  );
  assert.ok(manifest, `${name}: package must resolve from the installed host`);
  const directory = dirname(manifest);
  const {version} = JSON.parse(
    readFileSync(join(directory, 'package.json'), 'utf8'),
  );
  assert.equal(
    version,
    hostPins[`@earendil-works/${name}`],
    `${name}: test host must match the pinned Pi release`,
  );
  return pathToFileURL(join(directory, 'dist', entry)).href;
}
