import {access} from 'node:fs/promises';
import {homedir} from 'node:os';
import {join} from 'node:path';
import {pathToFileURL} from 'node:url';

const [installRoot, ...sources] = process.argv.slice(2);
if (!installRoot || sources.length === 0) {
  throw new Error('Usage: node setup.mjs PI_INSTALL_ROOT SOURCE...');
}
const {DefaultPackageManager, SettingsManager, getAgentDir} = await import(
  pathToFileURL(
    join(
      installRoot,
      'node_modules/@earendil-works/pi-coding-agent/dist/index.js',
    ),
  ).href
);
const cwd = homedir();
const agentDir = getAgentDir();
const settings = SettingsManager.create(cwd, agentDir);
const manager = new DefaultPackageManager({
  cwd,
  agentDir,
  settingsManager: settings,
});

function checkSettingsErrors() {
  const errors = settings.drainErrors();
  if (errors.length > 0) {
    throw new AggregateError(
      errors.map(({error}) => error),
      'Pi settings failed',
    );
  }
}

checkSettingsErrors();
for (const source of sources) manager.addSourceToSettings(source);
await settings.flush();
checkSettingsErrors();
// Native resolution installs missing resources without updating installed ones.
await manager.resolveExtensionSources(sources);
for (const source of sources) {
  const installed = manager.getInstalledPath(source, 'user');
  if (!installed)
    throw new Error(`Pi cannot locate the selected resource: ${source}`);
  await access(installed);
}
