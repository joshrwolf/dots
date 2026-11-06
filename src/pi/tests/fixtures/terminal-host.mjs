// Drives Pi's real InteractiveMode through its public `terminal` option in one
// TUI mode, then exits through Pi's own shutdown. Usage: <flow> <regular|fullscreen>.
// The terminal is virtual: Pi's renderer, input parser, overlays, reload, and
// shutdown are real; raw-mode tty handling is Pi's own ProcessTerminal concern.
import assert from 'node:assert/strict';
import {existsSync, readFileSync, statSync} from 'node:fs';
import {join} from 'node:path';
import {fileURLToPath} from 'node:url';

const companion = fileURLToPath(new URL('../../', import.meta.url)).replace(
  /\/$/,
  '',
);
const fixtures = join(companion, 'tests/fixtures');
const [flow, mode] = process.argv.slice(2);
if (flow === 'attention') process.env.DOTS_FIXTURE_ATTENTION = '1';
if (
  !['package', 'workbench', 'goal', 'attention', 'viewer'].includes(flow) ||
  !['regular', 'fullscreen'].includes(mode)
)
  throw new Error(
    'usage: terminal-host.mjs <package|workbench|goal|attention|viewer> <regular|fullscreen>',
  );
const {SessionFixture} = await import(join(companion, 'dev/sessions.ts'));

import {hostModule} from './host.ts';
const host = await import(hostModule('pi-coding-agent'));
const tui = await import(hostModule('pi-tui'));

/** Records output and feeds input through Pi's own sequence splitter. */
class VirtualTerminal {
  columns = 120;
  rows = 40;
  kittyProtocolActive = false;
  capture = '';
  transcript = '';
  start(onInput, onResize) {
    this.buffer = new tui.StdinBuffer();
    this.buffer.on('data', onInput);
    this.buffer.on('paste', content => onInput(`\x1b[200~${content}\x1b[201~`));
    this.onResize = onResize;
  }
  stop() {
    this.buffer?.destroy();
  }
  async drainInput() {}
  write(data) {
    this.capture += data;
    this.transcript += data;
    if (this.transcript.length > 8 * 1024 * 1024)
      throw new Error('terminal output exceeded bound');
  }
  moveBy(lines) {
    if (lines) this.write(`\x1b[${Math.abs(lines)}${lines > 0 ? 'B' : 'A'}`);
  }
  hideCursor() {
    this.write('\x1b[?25l');
  }
  showCursor() {
    this.write('\x1b[?25h');
  }
  clearLine() {
    this.write('\x1b[K');
  }
  clearFromCursor() {
    this.write('\x1b[J');
  }
  clearScreen() {
    this.write('\x1b[2J\x1b[H');
  }
  setTitle() {}
  setProgress() {}
  setProgramStatus() {}
  input(data) {
    this.buffer.process(data);
  }
  resize(rows, columns) {
    this.rows = rows;
    this.columns = columns;
    this.onResize();
  }
}

// Translate the workbench launcher's CLI contract strictly, so launcher drift fails here.
function workbenchOptions(args) {
  const options = {extensions: [], themes: []};
  const booleans = new Set([
    '--no-session',
    '--offline',
    '--no-mcp',
    '--no-extensions',
    '--no-skills',
    '--no-prompt-templates',
    '--no-context-files',
    '--no-approve',
    '--no-themes',
    '--no-tools',
  ]);
  for (let i = 0; i < args.length; i++) {
    const arg = args[i];
    if (booleans.has(arg)) options[arg] = true;
    else if (arg === '--extension') options.extensions.push(args[++i]);
    else if (arg === '--theme') options.themes.push(args[++i]);
    else if (arg === '--use-theme') options.useTheme = args[++i];
    else if (arg === '--tui-mode') options.tuiMode = args[++i];
    else
      throw new Error(
        `workbench launcher argument not understood by the terminal driver: ${arg}`,
      );
  }
  assert.ok(
    options['--no-session'] &&
      options['--no-extensions'] &&
      options['--no-tools'] &&
      options['--no-themes'],
    'workbench must stay isolated',
  );
  return options;
}

let config;
if (flow === 'workbench') {
  const {prepareWorkbench} = await import(join(companion, 'dev/run.mjs'));
  const trial = prepareWorkbench({
    environment: process.env,
    mode,
    scenario: 'bash',
  });
  process.on('exit', trial.cleanup);
  // Adopt the launcher's isolated environment wholesale, keeping only the marker.
  for (const key of Object.keys(process.env))
    if (!(key in trial.env) && key !== 'DOTS_TUI_MARKER')
      delete process.env[key];
  Object.assign(process.env, trial.env);
  process.chdir(trial.cwd);
  const options = workbenchOptions(trial.args);
  assert.equal(options.tuiMode, mode);
  config = {
    cwd: trial.cwd,
    extensions: [...options.extensions, join(fixtures, 'terminal-observer.ts')],
    themes: options.themes,
    noThemes: true,
    noTools: 'all',
    useTheme: options.useTheme,
  };
} else if (flow === 'package') {
  config = {
    cwd: process.cwd(),
    extensions: [companion, join(fixtures, 'terminal-observer.ts')],
    themes: [],
    noThemes: false,
  };
} else if (flow === 'viewer') {
  // What the Rex `workers_view` action runs: the viewer extension alone,
  // no tools and no saved session, over the worker's session file.
  config = {
    cwd: process.cwd(),
    // The observer loads first, so it sees any prompt before the viewer's
    // own input guard handles it.
    extensions: [
      join(fixtures, 'terminal-observer.ts'),
      join(companion, 'extensions/worker-view/index.ts'),
    ],
    themes: [],
    noThemes: true,
    noTools: 'all',
    flags: new Map([['worker-session', join(process.cwd(), 'viewed.jsonl')]]),
  };
} else {
  config = {
    cwd: process.cwd(),
    extensions: [join(fixtures, 'goal-terminal.ts')],
    themes: [],
    noThemes: true,
  };
}

const agentDir = host.getAgentDir();
const createRuntime = async ({cwd, sessionManager, sessionStartEvent}) => {
  const services = await host.createAgentSessionServices({
    cwd,
    agentDir,
    extensionFlagValues: config.flags,
    resourceLoaderOptions: {
      additionalExtensionPaths: config.extensions,
      additionalThemePaths: config.themes,
      noExtensions: true,
      noSkills: true,
      noPromptTemplates: true,
      noThemes: config.noThemes,
      noContextFiles: true,
    },
  });
  const errors = services.resourceLoader
    .getExtensions()
    .errors.map(({path, error}) => ({
      type: 'error',
      message: `Failed to load extension "${path}": ${error}`,
    }));
  const created = await host.createAgentSessionFromServices({
    services,
    sessionManager,
    sessionStartEvent,
    noTools: config.noTools,
  });
  return {
    ...created,
    services,
    diagnostics: [...services.diagnostics, ...errors],
  };
};
const sessionManager = host.SessionManager.inMemory(config.cwd);
if (flow === 'package') {
  sessionManager.appendCustomEntry('dots.worker', {
    version: 1,
    owner: sessionManager.getSessionId(),
    id: 'w-runtime',
    label: 'Runtime fixture',
    state: 'completed',
    cwd: '/fixture/runtime',
    sessionDir: join(agentDir, 'dots/workers/w-runtime'),
    sessionFile: join(agentDir, 'fixture-worker.jsonl'),
    startedAt: Date.now() - 1000,
    updatedAt: Date.now(),
    calls: 1,
    tokens: 7,
    settlements: 1,
    result: 'Fixture completed',
  });
  sessionManager.appendCustomEntry('dots.wake', {
    version: 2,
    owner: sessionManager.getSessionId(),
    id: 'k-0123456789abcdef',
    text: 'Inspect fixture verification',
    fired: 0,
    limit: 5,
    active: true,
    createdAt: Date.now(),
    trigger: {
      kind: 'time',
      at: Date.now() + 1200000,
      every: 1200000,
      check: {command: 'fixture-check', on: 'change'},
    },
  });
}
const runtime = await host.createAgentSessionRuntime(createRuntime, {
  cwd: config.cwd,
  agentDir,
  sessionManager,
});
/** A worker's saved transcript, and the only bytes it may hold. */
let viewed;
const viewedUnchanged = () =>
  readFileSync(viewed.session.file).equals(viewed.bytes) &&
  statSync(viewed.session.file).mtimeMs === viewed.mtime;
if (flow === 'package' || flow === 'viewer') {
  const file =
    flow === 'package'
      ? join(agentDir, 'fixture-worker.jsonl')
      : config.flags.get('worker-session');
  const session = new SessionFixture(file, '/fixture/runtime');
  session.user('Run the fixture check');
  session.assistant(
    [
      {type: 'text', text: 'Runtime recorded response'},
      {
        type: 'toolCall',
        id: 'call-1',
        name: 'bash',
        arguments: {command: 'echo RUNTIME_TOOL_COMMAND'},
      },
    ],
    'toolUse',
  );
  session.toolResult('call-1', 'bash', 'RUNTIME_TOOL_OUTPUT');
  viewed = {session, bytes: readFileSync(file), mtime: statSync(file).mtimeMs};
}
if (flow === 'package') {
  const {openRegistry} = await import('../../extensions/workers/registry.ts');
  const registry = openRegistry(join(agentDir, 'dots/workers.db'), warning =>
    assert.fail(warning),
  );
  assert.ok(registry);
  registry.upsert({
    id: 'w-runtime-child',
    parent: 'w-runtime',
    root: 'w-runtime',
    owner_session: null,
    label: 'Nested fixture',
    state: 'completed',
    cwd: '/fixture/child',
    task_project: null,
    task_id: null,
    model: 'example/fixture-model',
    thinking: 'high',
    activity: null,
    failed: null,
    question: null,
    calls: 1,
    tokens: 4,
    started_at: Date.now() - 500,
    last_event_at: Date.now(),
    updated_at: Date.now(),
    session_file: null,
    pid: process.pid,
  });
  registry.close();
}
assert.deepEqual(
  runtime.diagnostics.filter(d => d.type === 'error'),
  [],
  'startup diagnostics',
);
if (flow === 'goal')
  runtime.services.settingsManager.setTheme(
    mode === 'regular' ? 'light' : 'dark',
  );
host.initTheme(runtime.services.settingsManager.getTheme(), true);

const terminal = new VirtualTerminal();
const marker = process.env.DOTS_TUI_MARKER ?? process.env.DOTS_GOAL_MARKER;
const events = () => (existsSync(marker) ? readFileSync(marker, 'utf8') : '');
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
const plain = () => tui.stripTerminalSequences(terminal.capture);
const shows = text => terminal.capture.includes(text) || plain().includes(text);
const forbidden = [
  'uncaughtException',
  'UnhandledPromiseRejection',
  'Failed to load extension',
  'ERR_MODULE_NOT_FOUND',
  'stale after session',
];
async function wait(predicate, label) {
  const deadline = Date.now() + 15_000;
  while (Date.now() < deadline) {
    if (predicate()) return;
    await sleep(20);
  }
  throw new Error(
    `${flow}/${mode}: timeout waiting for ${label}\nevents: ${events()}\nscreen: ${plain().slice(-3000)}`,
  );
}
const send = data => {
  terminal.capture = '';
  terminal.input(data);
};
// Commands keep the capture; `fresh` starts a new one. Pi finishes an extension
// command shortly after its handler reports closing, so commands follow a brief settle.
const command = async text => {
  await sleep(100);
  terminal.input(text + '\r');
};
const fresh = async text => {
  await sleep(100);
  send(text + '\r');
};
// Pi's shutdown exits the process; judge the whole transcript on the way out.
process.on('exit', code => {
  if (code !== 0) return;
  const leaked = forbidden.filter(text =>
    tui.stripTerminalSequences(terminal.transcript).includes(text),
  );
  const unexpected = events().includes('UNEXPECTED_PROVIDER_TURN');
  const written = viewed && !viewedUnchanged();
  if (leaked.length || unexpected || written) {
    console.error(
      `${flow}/${mode}: ${leaked.join(', ')}${unexpected ? ' provider turn' : ''}${written ? ' worker session written' : ''}`,
    );
    process.exitCode = 1;
  }
});

const interactive = new host.InteractiveMode(runtime, {
  terminal,
  tuiMode: mode,
  initialThemeSetting: config.useTheme,
});
interactive.run().catch(error => {
  console.error(error);
  process.exit(1);
});

async function packageFlow() {
  await wait(() => events().includes('start'), 'startup');
  await sleep(500);
  await wait(
    () => shows('◷ Wakes') && shows('Inspect fixture verification'),
    'persistent wake section',
  );
  await fresh('/wakes');
  await wait(
    () => shows('Check (change): fixture-check') && shows('Scheduled:'),
    'native wake inspection',
  );
  for (const [rows, columns] of [
    [24, 44],
    [48, 160],
  ]) {
    terminal.resize(rows, columns);
    await sleep(150);
  }
  send('\x1b');
  await sleep(300);
  await fresh('/workers');
  await wait(
    () => shows('Runtime fixture') && shows('Nested fixture'),
    'real runtime ownership picker',
  );
  send('\x1b[B\r');
  await wait(
    () => shows('/fixture/child') && shows('Read-only descendant'),
    'real runtime descendant',
  );
  send('\x1b');
  await sleep(150);
  await fresh('/workers w-runtime');
  await wait(
    () =>
      shows('Runtime recorded response') &&
      shows('Run the fixture check') &&
      shows('RUNTIME_TOOL_COMMAND'),
    'real runtime recorded worker in Pi message components',
  );
  assert.ok(!shows('open in Rex'), 'Rex view offered only inside Rex');
  send('\x1b');
  await sleep(150);
  await fresh('/workers w-runtime-child');
  await wait(
    () => shows('/fixture/child') && shows('Read-only descendant'),
    'direct descendant inspection by id',
  );
  send('\x1b');
  await sleep(150);
  for (const [rows, columns] of [
    [28, 60],
    [48, 160],
  ]) {
    terminal.resize(rows, columns);
    await sleep(200);
  }
  for (const kind of ['job', 'worker']) {
    const marker = `${kind} inspection draft true`;
    const before = countLine(marker);
    await fresh(`/accept-${kind}-inspect`);
    if (kind === 'job') {
      await wait(() => shows('Inspect'), 'job action picker');
      send('\r');
    }
    if (kind === 'worker') {
      await wait(
        () => shows('Review') && shows('Child'),
        'worker ownership picker',
      );
      send('\x1b[B\r');
      await wait(
        () => shows('Read-only descendant') && shows('/fixture/child'),
        'nested read-only inspection',
      );
      assert.ok(
        !shows('M message') && !shows('X stop'),
        'descendant has no controls',
      );
      send('l');
      await wait(
        () => shows('Review') && shows('Child'),
        'back to ownership list',
      );
      send('\x1b[A\r');
      await wait(() => shows('Worker · Review'), 'owned live transcript');
      send('\x1b[H');
      await wait(() => shows('/fixture/review'), 'full recorded directory');
      send('\x1b[F');
      await wait(
        () =>
          events().includes('worker-streamed') &&
          shows('STREAMED_ACCEPTANCE_TEXT'),
        'live partial update',
      );
      send('\x1b[A');
      await wait(() => shows('Paused'), 'scroll pauses follow');
      send('f');
      await wait(() => shows('Following'), 'F resumes follow');
      await wait(
        () => events().includes('worker-completed') && shows('completed'),
        'completion retains inspector',
      );
      send('mreject\r');
      await wait(
        () =>
          events().includes('worker-message-rejected') &&
          shows('fixture delivery failed'),
        'delivery failure retains inspector',
      );
      send('\x15hello fixture\r');
      await wait(
        () => events().includes('worker-message:hello fixture'),
        'owned message delivered after failure',
      );
      send('x');
      await wait(() => shows('Press X again'), 'stop needs confirmation');
      send('a');
      assert.ok(!events().includes('worker-stopped'), 'other key cancels stop');
      send('xx');
      await wait(() => events().includes('worker-stopped'), 'confirmed stop');
    } else
      await wait(() => shows('Cleanup: unknown'), `${kind} inspection overlay`);
    send('\x1b');
    await wait(
      () => countLine(marker) > before,
      `${kind} overlay restores draft`,
    );
  }
  await command('/accept-theme');
  await wait(() => events().includes('theme true'), 'theme change');
  await command('/reload');
  await wait(() => events().split('start').length > 2, 'package reload');
  await fresh('/wakes');
  await wait(
    () => shows('Check (change): fixture-check'),
    'wake inspection restored after reload',
  );
  send('\x1b');
  await sleep(5_500); // Delayed refresh callbacks must not use the replaced context.
  await command('/accept-exit');
}

function workbenchState(scenario, phase, expanded) {
  const records = events()
    .split('\n')
    .filter(line => line.startsWith('workbench {'));
  if (!records.length) return false;
  const latest = JSON.parse(records.at(-1).slice('workbench '.length));
  return (
    latest.scenario === scenario &&
    latest.phase === phase &&
    (expanded === undefined || latest.expanded === expanded)
  );
}
const count = text => events().split(text).length - 1;
const lines = () => events().split('\n');
const countLine = line => lines().filter(entry => entry === line).length;
/** Rows where the host painted `text` after an absolute cursor move to column 1. */
function paintedRows(text) {
  const parts = terminal.capture.split(/\x1b\[(\d+);1H/);
  const rows = [];
  for (let i = 1; i < parts.length; i += 2)
    if (parts[i + 1].includes(text)) rows.push(Number(parts[i]));
  return rows;
}

async function workbenchFlow() {
  const state = workbenchState;
  await wait(() => events().includes('start'), 'startup');
  await wait(() => shows('/ui-lab bash'), 'prefilled native editor');
  send('\r');
  await wait(
    () => state('bash', 0) && shows('UI workbench'),
    'mounted workbench',
  );
  const initial = mode === 'regular' ? 'light' : 'dark';
  await wait(
    () => events().includes(`frame 120 ${initial}`),
    'terminal-matched theme',
  );
  for (const next of ['N', '\x1b[110u']) {
    send(next);
    await wait(
      () => state('read', 0) && shows('Recorded source'),
      'next scenario',
    );
    await wait(
      () => terminal.capture.includes('\x1b[2J'),
      'scenario transition clears stale overlay rows',
    );
    send('P');
    await wait(() => shows('Tool output'), 'previous scenario');
  }
  for (let phase = 1; phase <= 7; phase++) {
    send(' ');
    await wait(() => state('bash', phase), `bash phase ${phase}`);
  }
  send('e');
  await wait(() => shows('output line'), 'expanded native content');
  send('\x1b[F');
  await wait(() => shows('DIAGNOSTIC'), 'native scroll to tail diagnostic');
  send('\x1b[H');
  await wait(() => shows('output line 0000'), 'native scroll to start');
  if (mode === 'fullscreen') {
    send('\x1b[<65;60;20M');
    await wait(() => shows('output line'), 'fullscreen wheel scrolling');
  }
  for (const [rows, columns] of [
    [24, 44],
    [48, 160],
  ]) {
    terminal.resize(rows, columns);
    await wait(
      () => events().includes(`frame ${columns} `),
      `resize to ${columns}`,
    );
  }
  send('T');
  const toggled = initial === 'light' ? 'dark' : 'light';
  await wait(
    () => events().includes(`frame 160 ${toggled}`),
    'native theme toggle',
  );
  send('\x1b');
  await wait(
    () => events().includes('workbench closed'),
    'editor focus restored',
  );
  await wait(
    () => count(`frame 160 ${initial}`) >= 2,
    'original palette restored',
  );
  for (const [scenario, expected] of [
    ['frame', 'Application frame'],
    ['workers', 'Worker activity'],
    ['wake', 'Scheduled follow-up'],
    ['thinking', 'Thinking blocks'],
    ['goal', 'Goal popup'],
    ['edit', 'Recorded diff'],
    ['codemode', 'Nested tool activity'],
    ['inspection-shell', 'Workers'],
  ]) {
    const closed = count('workbench closed');
    await fresh(`/ui-lab ${scenario}`);
    await wait(
      () => state(scenario, 0) && shows(expected),
      `${scenario} preview`,
    );
    if (scenario === 'frame') {
      await wait(
        () => shows('accepted') && shows('exit 0'),
        'registered job receipt and completion',
      );
      send('E');
      await wait(() => state('frame', 0, true), 'expanded mixed transcript');
      send('\x1b[6~');
      await wait(
        () => shows('/fixture/jobs/example/output.log'),
        'expanded recorded job evidence',
      );
      send('E');
      await wait(() => state('frame', 0, false), 'collapsed mixed transcript');
      send('\x1b[H');
      send(' ');
      await wait(() => state('frame', 1), 'running mixed transcript');
      send(' ');
      await wait(
        () => state('frame', 2) && shows('supervisor SIGKILL'),
        'cleanup distinct from root outcome',
      );
      send('B');
      await wait(
        () => shows('Review the result'),
        'native editor below transcript',
      );
      await wait(
        () => shows('━━━━━━━━━━ 95%') && shows('✦ high'),
        'icon footer below editor and goals',
      );
      send(' ');
      await wait(() => state('frame', 3), 'unknown context fixture');
      send('B');
      await wait(
        () => shows('────────── ?'),
        'unknown context in footer study',
      );
      for (let phase = 4; phase <= 8; phase++) {
        send(' ');
        await wait(() => state('frame', phase), `activity phase ${phase}`);
      }
      send('B');
      await wait(
        () =>
          shows('◇ Workers') &&
          shows('▷ Jobs') &&
          shows('Which revision') &&
          shows('unknown'),
        'combined activity and attention',
      );
      assert.ok(
        shows('+6 more') && shows('+3 more'),
        'section overflow is explicit',
      );
      send(' ');
      await wait(
        () => state('frame', 9) && shows('4/4'),
        'settled execution retains followed goal',
      );
    }
    if (scenario === 'workers') {
      await wait(
        () => shows('Check public API') && shows('read src/report.ts'),
        'separate intent and activity',
      );
      terminal.capture = '';
      await wait(
        () => new Set(plain().match(/[⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏]/g)).size >= 2,
        'animated dots advance through distinct frames',
      );
      for (let phase = 1; phase <= 7; phase++) {
        send(' ');
        await wait(() => state('workers', phase), `worker phase ${phase}`);
        if (phase === 5)
          await wait(() => shows('+6 more'), 'whole-row overflow');
        if (phase === 6)
          await wait(
            () => shows('Documentation checked'),
            'compact completed row',
          );
      }
    }
    if (scenario === 'wake') {
      send(' ');
      await wait(() => shows('exit 1'), 'failed wake check');
      send(' ');
      await wait(
        () => state('wake', 2) && shows('◷ Wakes') && shows('in 2m'),
        'scheduled wake section',
      );
      send(' ');
      await wait(
        () => state('wake', 3) && shows('checking') && shows('queued'),
        'checking and queued wake states',
      );
      send(' ');
      await wait(
        () => state('wake', 4) && shows('+3 more'),
        'wake section overflow',
      );
      send(' ');
      await wait(
        () => state('wake', 5) && shows('Check (change): fixture-check'),
        'full wake evidence',
      );
    }
    if (scenario === 'goal') {
      await wait(
        () => shows('2/12 done') && shows('now: Update exports'),
        'summary line below the editor',
      );
      send('E');
      await wait(
        () =>
          state('goal', 0, true) &&
          shows('Build CLI adapter') &&
          shows('Build SDK adapter') &&
          shows('├───┬─╮'),
        'popup over the transcript lists the graph',
      );
      send('o');
      await wait(() => shows('o graph'), 'outline in workbench');
      send('\x1b[A');
      send('\x1b[A');
      await wait(
        () => shows('→ ▾ Core interface'),
        'select parent in workbench',
      );
      send('E');
      await wait(() => state('goal', 0, false), 'panel hidden');
      const labels = [
        '1 worker',
        '1 asking',
        'Unify the export interface',
        '2/6 done',
        'partial',
        'now: Update exports',
        '1 held',
        '4/4 done',
        '+2 more',
        'unavailable · /goal refresh',
        'loading',
        'brief warning',
        'no slices',
      ];
      for (const [index, label] of labels.entries()) {
        send(' ');
        await wait(
          () => state('goal', index + 1) && shows(label),
          `goal phase ${index + 1}`,
        );
      }
    }
    if (scenario === 'edit') {
      send('E');
      await wait(
        () => shows('Hello') && shows('Welcome'),
        'native recorded diff',
      );
      send(' ');
      await wait(
        () => state('edit', 1) && shows('shippingComponent'),
        'full-width recorded comparison',
      );
      send(' ');
      await wait(
        () => state('edit', 2) && shows('diff unavailable'),
        'explicit recorded diff fallback',
      );
    }
    if (scenario === 'codemode') {
      send(' ');
      send(' ');
      await wait(
        () => state('codemode', 2) && shows('shipping-component'),
        'recorded nested path',
      );
      send('E');
      await wait(
        () => shows('upstream-truncated preview'),
        'native preview limit identified',
      );
    }
    if (scenario === 'inspection-shell') {
      send('\x1b[B');
      await wait(() => shows('review-2'), 'native picker selection');
      send('\x00');
      await wait(
        () => state('inspection-shell', 1) && shows('No retained workers'),
        'picker fixture phase shortcut',
      );
      send('\x00');
      await wait(
        () => state('inspection-shell', 2) && shows('long heading'),
        'picker long heading phase',
      );
    }
    if (scenario === 'thinking') {
      await wait(() => shows('▸ Thinking'), 'native thinking disclosure');
      send('E');
      await wait(
        () => shows('Reviewing dependency updates'),
        'expanded thinking summary',
      );
      send(' ');
      await wait(
        () => shows('Decision: run the focused checks'),
        'structured thinking detail',
      );
      send(' ');
      await wait(
        () => state('thinking', 2, true) && shows('version ='),
        'unfinished streaming Markdown',
      );
      send('E');
      await wait(() => shows('▸ Thinking'), 'thinking collapsed again');
    }
    send('\x1b');
    await wait(() => count('workbench closed') > closed, `${scenario} closed`);
  }
  const closedBeforeJob = count('workbench closed');
  await command('/ui-lab job');
  await wait(() => state('job', 0) && shows('Job ·'), 'actual job inspector');
  send('s');
  await wait(
    () => shows('Simulate stopping this job?'),
    'native action confirmation',
  );
  send('\x1b');
  await wait(
    () => events().includes('confirmation false'),
    'confirmation cancelled',
  );
  // Cancel returns focus to the inspector; a second confirmation proves it.
  send('s');
  await wait(
    () => shows('Simulate stopping this job?'),
    'confirmation reopened after cancel',
  );
  send('\r');
  await wait(() => shows('stopping'), 'approved fixture action');
  send('\x1b');
  await wait(
    () => count('workbench closed') > closedBeforeJob,
    'job inspector closed',
  );
  await command('/reload');
  await wait(
    () => count('start') >= 2 && shows('/ui-lab bash'),
    'isolated reload',
  );
  // Prove focus and input interception after reload without contacting a model.
  send('\x15fixture prompt must not reach a provider\r');
  await wait(
    () => events().includes('workbench input handled'),
    'provider-free input',
  );
  await command('/ui-lab-exit');
}

async function goalFlow() {
  // alt+g closes the popup.
  const closePanel = async label => {
    const before = countLine('inspector-closed');
    send('\x1bg');
    await wait(() => countLine('inspector-closed') > before, label);
  };
  const renders = () =>
    lines().filter(line => line.startsWith('inspector-render:'));
  await wait(() => events().includes('start'), 'startup');
  await sleep(500); // Let the editor mount before sending input in either host mode.
  await fresh('/goal focus t-123abc');
  await wait(
    () =>
      shows('Unified fixture exports') &&
      shows('0/1 done') &&
      shows('◎ ·') &&
      shows('alt+g'),
    'one-line goal summary',
  );
  if (mode === 'fullscreen') {
    // Use the host's painted summary position, not a fixed screen row.
    const goalRow = paintedRows('Unified fixture exports').at(-1);
    assert.ok(goalRow, 'goal summary has a fullscreen cursor position');
    send('DRAFT_SURVIVES_INSPECTION');
    await wait(() => shows('DRAFT_SURVIVES_INSPECTION'), 'editor draft');
    const draftRow = paintedRows('DRAFT_SURVIVES_INSPECTION').at(-1);
    assert.ok(draftRow, 'editor draft has a fullscreen cursor position');
    assert.ok(
      draftRow < goalRow,
      `summary stays below the editor (draft ${draftRow}, goal ${goalRow})`,
    );
    send(`\x1b[<0;4;${goalRow}M\x1b[<0;4;${goalRow}m`);
    await wait(
      () => shows('1 overview') && shows('Reference fixture slice'),
      'click opens the popup',
    );
    const size = renders().at(-1)?.split(':').slice(1).map(Number);
    assert.ok(size, 'popup render recorded');
    assert.ok(
      size[0] >= terminal.columns * 0.9,
      `popup uses most of the width (${size[0]})`,
    );
    assert.ok(size[1] <= terminal.rows, `popup fits the screen (${size[1]})`);
    assert.ok(size[1] < 20, `one task has no tall blank tail (${size[1]})`);
    const before = countLine('inspector-closed');
    send('\x1b');
    await wait(
      () => countLine('inspector-closed') > before,
      'Esc closes the popup',
    );
    assert.ok(
      lines().includes('editor:DRAFT_SURVIVES_INSPECTION'),
      'editor draft survives inspection',
    );
    send('\x15');
  }
  assert.ok(!shows('Reference fixture slice'), 'no task rows below the editor');
  send('KEEPS_EDITOR_DRAFT');
  await wait(() => shows('KEEPS_EDITOR_DRAFT'), 'editor draft');
  send('\x1bg');
  await wait(
    () => shows('Reference fixture slice') && shows('1 overview'),
    'shortcut opens the popup',
  );
  send('2');
  await wait(() => shows('Fixture brief'), 'popup takes input');
  await closePanel('shortcut closes the popup');
  assert.ok(
    lines().includes('editor:KEEPS_EDITOR_DRAFT'),
    'editor draft survives the popup',
  );
  send('\x15');
  await fresh('/goal');
  await wait(() => shows('1 overview'), 'focused panel');
  send('2');
  await wait(() => shows('Fixture brief'), 'brief body mounted');
  send('\x1b[F');
  await wait(() => shows('END_OF_GOAL_BRIEF'), 'scroll to end');
  for (const [rows, columns] of [
    [24, 45],
    [48, 160],
  ]) {
    terminal.capture = '';
    terminal.resize(rows, columns);
    await wait(() => shows('Unified fixture exports'), 'resize');
  }
  await closePanel('panel closed');
  await fresh('/goal follow t-abcdef');
  await wait(() => shows('Nested fixture goal'), 'nested summary');
  await fresh('/goal show t-abcdef');
  await wait(
    () =>
      shows('Claimed') &&
      shows('Waiting') &&
      shows('Adapt consumers') &&
      shows('Define shared types') &&
      shows('all tasks') &&
      !shows('▾'),
    'nested panel lists the dependency graph',
  );
  send('h');
  await wait(() => shows('2 closed hidden'), 'hide closed tasks on request');
  // A short screen still gives the list its rows before the goal result.
  terminal.capture = '';
  terminal.resize(14, 120);
  await wait(
    () =>
      shows('Update exports') &&
      shows('Document the interface') &&
      shows('Detail') &&
      !/ 0 of /.test(plain()),
    'short popup keeps the list',
  );
  terminal.capture = '';
  const beforeRestore = renders().length;
  terminal.resize(40, 120);
  await wait(() => {
    const size = renders().at(-1)?.split(':').slice(1).map(Number);
    return (
      renders().length > beforeRestore &&
      size?.[0] === 108 &&
      size[1] <= 34 &&
      shows('h show closed')
    );
  }, 'restored content-sized popup');
  send('o');
  await wait(
    () => shows('o graph') && shows('▾') && shows('Core interface'),
    'group outline',
  );
  send('\x1b[A');
  send(' ');
  await wait(() => shows('▸') && shows('Core interface'), 'fold parent');
  assert.ok(!shows('Adapt consumers'), 'folded parent hides its leaves');
  send(' ');
  await wait(
    () => shows('Consumer migration') && shows('Adapt consumers'),
    'unfold parent',
  );
  for (let i = 0; i < 5 && !/→ ▾ Consumer migration/.test(plain()); i++) {
    send('\x1b[B');
    await sleep(100);
  }
  await wait(
    () => /→ ▾ Consumer migration/.test(plain()),
    'select nested group',
  );
  send(' ');
  await wait(
    () => shows('▸') && shows('Consumer migration'),
    'fold nested group',
  );
  assert.ok(!shows('Adapt consumers'), 'folded nested group hides its leaves');
  send(' ');
  await wait(() => shows('Adapt consumers'), 'unfold nested group');
  send('h');
  await wait(
    () =>
      shows('all tasks') &&
      shows('Define shared types') &&
      shows('Document shared types'),
    'closed leaves on request',
  );
  await closePanel('nested panel closed');
  await fresh('/goal unfollow t-abcdef');
  await wait(() => shows('Unfollowed t-abcdef'), 'remove hierarchy fixture');
  await fresh('/goal follow t-999999');
  await wait(
    () => shows('Second fixture goal') && shows('+1 more'),
    'two followed goals',
  );
  await fresh('/goal');
  await wait(
    () => shows('Second fixture goal') && shows('Tab goal'),
    'panel for the selected goal',
  );
  send('\t');
  await wait(
    () => shows('Unified fixture exports'),
    'Tab selects the next goal',
  );
  await closePanel('selected panel closed');
  await fresh('/reload');
  await wait(
    () =>
      count('start') >= 2 &&
      shows('Unified fixture exports') &&
      shows('+1 more'),
    'reload followed set',
  );
  await fresh('/goal unfollow t-999999');
  await wait(
    () => shows('Unfollowed t-999999') && shows('Unified fixture exports'),
    'remove one follow',
  );
  await fresh('/goal unfocus');
  await wait(() => shows('All follows cleared'), 'unfocus');
  await fresh('/goal show');
  await wait(() => shows('No focused goal'), 'unfocused inspection');
  await fresh('/goal evidence t-456def 9');
  await wait(
    () => shows('Recorded task evidence') && shows('fixture-report'),
    'task-only evidence picker',
  );
  send('\x1b[B');
  send('\x1b[B');
  send('\r');
  await wait(() => shows('EVIDENCE_PAGE_ONE'), 'exact historical evidence');
  terminal.capture = '';
  send('n');
  await wait(() => shows('EVIDENCE_PAGE_TWO'), 'next evidence page');
  terminal.capture = '';
  send('b');
  await wait(() => shows('EVIDENCE_PAGE_ONE'), 'previous evidence page');
  terminal.capture = '';
  send('\x1b');
  await wait(
    () => shows('Recorded task evidence') && shows('fixture-report'),
    'evidence selections restored',
  );
  send('\x1b');
  await wait(
    () => events().includes('command-end:evidence t-456def 9'),
    'evidence inspection closed',
  );
  assert.equal(countLine('evidence-page:1'), 2);
  assert.equal(countLine('evidence-page:201'), 1);
  assert.equal(countLine('UNEXPECTED_PROVIDER_TURN'), 0);
  await fresh('/goal follow t-fedcba');
  await wait(
    () => shows('Completed fixture goal') && shows('1/1 done'),
    'finished summary',
  );
  await fresh('/goal show t-fedcba');
  await wait(
    () => shows('Completed fixture slice') && shows('all tasks'),
    'finished panel includes closed leaves',
  );
  await closePanel('finished panel closed');
  await fresh('/goal unfollow t-fedcba');
  await wait(() => shows('Unfollowed t-fedcba'), 'remove finished fixture');
  await fresh('/goal follow t-123abc');
  for (const name of ['light', 'dark']) {
    await fresh(`/goal-fixture-theme ${name}`);
    await wait(
      () => lines().includes(`fixture-theme:${name}`),
      `${name} stock theme`,
    );
    const before = countLine(`inspector-surface:${name}`);
    await fresh('/goal show t-123abc');
    await wait(
      () => countLine(`inspector-surface:${name}`) > before,
      `${name} popup colors verified`,
    );
    await closePanel(`${name} popup closed`);
  }
  await fresh('/goal-fixture-exit');
}

async function attentionFlow() {
  const frames = () => lines().filter(line => line.startsWith('frame:'));
  const framed = pattern => frames().some(line => pattern.test(line));
  await wait(() => events().includes('start'), 'startup');
  await sleep(500);
  await fresh('/goal focus t-123abc');
  await wait(
    () => shows('Needs your attention: Choose the rollout scope'),
    'one quiet attention line',
  );
  send('\x1bg');
  await wait(
    () => framed(/^frame:attention:question:options:/),
    'popup opens on the question and its options',
  );
  send('\r'); // Into the options.
  send('\x1b[B');
  send('\x1b[B');
  send('\r'); // Type a different answer.
  for (const key of ['a', 'b', 'c', '\x1b[D', 'X']) send(key);
  await wait(
    () => framed(/^frame:attention:question:-:abXc:/),
    'Left moves the cursor while typing and the question stays',
  );
  send('\x1b'); // Leave the field.
  send('\x1b[C');
  await wait(() => framed(/^frame:work:/), 'Right switches to Work');
  send('\x1b[D');
  await wait(
    () => frames().at(-1)?.startsWith('frame:attention:question:options'),
    'Left returns to Attention',
  );
  const closed = countLine('inspector-closed');
  send('\x1bg');
  await wait(() => countLine('inspector-closed') > closed, 'Alt+G closes');
  assert.ok(!lines().some(line => /Answer saved/.test(line)));
  const opened = renders().length;
  send('\x1bg');
  await wait(() => renders().length > opened, 'the popup reopens');
  send('\r');
  send('\x1b[B');
  send('\x1b[B');
  send('\r'); // The unsent draft returns to the answer field.
  await wait(
    () => frames().at(-1) === 'frame:attention:question:-:abXc:-',
    'the draft survives closing',
  );
  for (const [rows, columns] of [
    [16, 40],
    [40, 120],
  ]) {
    const before = renders().length;
    terminal.resize(rows, columns);
    await wait(() => {
      const size = renders().slice(before).at(-1)?.split(':').slice(1);
      return !!size && Number(size[0]) <= columns && Number(size[1]) <= rows;
    }, `popup fits ${columns}x${rows}`);
    assert.ok(
      frames().at(-1)?.startsWith('frame:attention:question:-:abXc'),
      `question and draft stay at ${columns}x${rows}`,
    );
  }
  send('\r');
  await wait(
    () => framed(/:abXc$/),
    'the answer is saved before the view says so',
  );
  await closePanel('popup closes');
  await fresh('/goal-fixture-exit');
  function renders() {
    return lines().filter(line => line.startsWith('inspector-render:'));
  }
  async function closePanel(label) {
    const before = countLine('inspector-closed');
    send('\x1bg');
    await wait(() => countLine('inspector-closed') > before, label);
  }
}

async function viewerFlow() {
  await wait(() => events().includes('start'), 'startup');
  await wait(
    () =>
      shows('Worker · viewed.jsonl') &&
      shows('Run the fixture check') &&
      shows('Runtime recorded response') &&
      shows('RUNTIME_TOOL_COMMAND') &&
      shows('streaming not shown'),
    'saved transcript in Pi message components',
  );
  // Typing and Enter go to the view, never to a prompt.
  send('submit this\r');
  await sleep(500);
  assert.ok(
    !events().includes('prompt submitted') &&
      !events().includes('UNEXPECTED_PROVIDER_TURN'),
    'no prompt was submitted',
  );
  assert.ok(viewedUnchanged(), 'viewer left the session file as written');
  viewed.session.assistant([{type: 'text', text: 'VIEWER_APPENDED_REPLY'}]);
  viewed.bytes = readFileSync(viewed.session.file);
  viewed.mtime = statSync(viewed.session.file).mtimeMs;
  await wait(() => shows('VIEWER_APPENDED_REPLY'), 'follows appended records');
  for (const [rows, columns] of [
    [24, 44],
    [48, 160],
  ]) {
    terminal.resize(rows, columns);
    await sleep(200);
  }
  await wait(() => shows('VIEWER_APPENDED_REPLY'), 'resized view');
  send(mode === 'regular' ? 'q' : '\x1b');
}

try {
  await {
    viewer: viewerFlow,
    package: packageFlow,
    workbench: workbenchFlow,
    goal: goalFlow,
    attention: attentionFlow,
  }[flow]();
  await sleep(15_000);
  throw new Error(`${flow}/${mode}: Pi did not shut down`);
} catch (error) {
  console.error(error);
  process.exit(1);
}
