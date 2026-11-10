import assert from 'node:assert/strict';
import {spawn, spawnSync} from 'node:child_process';
import {
  mkdtempSync,
  mkdirSync,
  readFileSync,
  writeFileSync,
  existsSync,
  statSync,
  rmSync,
} from 'node:fs';
import {join, relative} from 'node:path';
import {tmpdir} from 'node:os';
import {fileURLToPath} from 'node:url';
import {once} from 'node:events';
import {test} from 'node:test';
import {hostModule, hostPins} from '../fixtures/host.ts';
const companion = fileURLToPath(new URL('../../', import.meta.url)).replace(
  /\/$/,
  '',
);

test('Pi loads the checkout package, runs a native child, reloads, and shuts down', async t => {
  const work = mkdtempSync(join(tmpdir(), 'dots-pi-host-'));
  t.after(() => rmSync(work, {recursive: true, force: true}));
  const manifest = JSON.parse(
    readFileSync(join(companion, 'package.json'), 'utf8'),
  );
  const binary = process.env.PI_BIN ?? 'pi';
  const version = spawnSync(binary, ['--version'], {
    encoding: 'utf8',
    timeout: 10000,
  });
  assert.equal(version.status, 0, version.stderr);
  assert.equal(
    version.stdout.trim(),
    hostPins['@earendil-works/pi-coding-agent'],
    'installed host must match the pinned Pi release',
  );
  const fixtureEnv = {
    ...process.env,
    HOME: work,
    XDG_DATA_HOME: join(work, 'data'),
    XDG_STATE_HOME: join(work, 'state'),
    XDG_CONFIG_HOME: join(work, 'config'),
  };
  const remote = join(work, 'remote.git');
  const remoteInit = spawnSync('git', ['init', '--bare', '--quiet', remote], {
    env: fixtureEnv,
    encoding: 'utf8',
  });
  assert.equal(remoteInit.status, 0, remoteInit.stderr);
  const substrateInit = spawnSync(
    'dots',
    ['--json', 'substrate', 'init', '--remote', `file://${remote}`],
    {cwd: work, env: fixtureEnv, encoding: 'utf8', timeout: 15_000},
  );
  assert.equal(substrateInit.status, 0, substrateInit.stderr);
  const knowledgeInit = spawnSync('dots', ['--json', 'memory', 'init'], {
    cwd: work,
    env: fixtureEnv,
    encoding: 'utf8',
    timeout: 15_000,
  });
  assert.equal(knowledgeInit.status, 0, knowledgeInit.stderr);
  const initial = JSON.parse(knowledgeInit.stdout);
  const changes = join(work, 'knowledge.json');
  writeFileSync(
    changes,
    JSON.stringify([
      {
        scope: {kind: 'global'},
        path: 'MEMORY.md',
        action: {kind: 'write', content: 'NATIVE_KNOWLEDGE_FACT'},
      },
    ]),
  );
  const knowledgePublish = spawnSync(
    'dots',
    [
      '--json',
      'memory',
      'publish',
      '--expected-snapshot',
      initial.snapshot,
      '--input-file',
      changes,
    ],
    {cwd: work, env: fixtureEnv, encoding: 'utf8', timeout: 15_000},
  );
  assert.equal(knowledgePublish.status, 0, knowledgePublish.stderr);
  const published = JSON.parse(knowledgePublish.stdout);
  const marker = join(work, 'markers.jsonl');
  const results = [];
  class Client {
    constructor(extension, name) {
      const agent = join(work, name);
      mkdirSync(agent, {recursive: true});
      this.pending = new Map();
      this.events = [];
      this.stderr = '';
      this.next = 0;
      this.buffer = '';
      const env = {
        ...fixtureEnv,
        PI_CODING_AGENT_DIR: agent,
        PI_OFFLINE: '1',
        PI_TRIAL_MARKER: marker,
        // The companion's dots calls, such as releasing claims at shutdown,
        // must not reach the user's catalog.
        XDG_DATA_HOME: join(work, 'data'),
      };
      for (const k of Object.keys(env))
        if (/API_KEY|TOKEN|SECRET|PASSWORD/.test(k)) delete env[k];
      this.child = spawn(
        binary,
        [
          '--mode',
          'rpc',
          '--no-session',
          ...(extension ? ['--no-extensions'] : []),
          '--no-mcp',
          '--no-context-files',
          '--no-skills',
          '--no-prompt-templates',
          '--no-themes',
          '--no-approve',
          '--offline',
          ...(extension ? ['-e', extension] : []),
        ],
        {cwd: work, env, stdio: ['pipe', 'pipe', 'pipe']},
      );
      this.exited = once(this.child, 'exit');
      this.child.stderr.on('data', b => {
        this.stderr = (this.stderr + b).slice(-65536);
      });
      this.child.stdout.on('data', b => {
        this.buffer += b;
        if (this.buffer.length > 4 * 1024 * 1024) {
          this.child.kill('SIGTERM');
          return;
        }
        for (let index; (index = this.buffer.indexOf('\n')) >= 0;) {
          const line = this.buffer.slice(0, index);
          this.buffer = this.buffer.slice(index + 1);
          if (!line.trim()) continue;
          let event;
          try {
            event = JSON.parse(line);
          } catch {
            continue;
          }
          this.events.push(event);
          if (event.type === 'response' && this.pending.has(event.id)) {
            const pending = this.pending.get(event.id);
            this.pending.delete(event.id);
            clearTimeout(pending.timer);
            event.success
              ? pending.resolve(event)
              : pending.reject(new Error(JSON.stringify(event)));
          }
        }
      });
      this.child.on('exit', () => {
        for (const p of this.pending.values()) {
          clearTimeout(p.timer);
          p.reject(new Error('Pi exited: ' + this.stderr));
        }
        this.pending.clear();
      });
      this.child.stdin.on('error', () => {});
    }
    call(type, data = {}) {
      const id = String(++this.next);
      return new Promise((resolve, reject) => {
        const timer = setTimeout(() => {
          this.pending.delete(id);
          reject(new Error('Pi command timeout: ' + type + '\n' + this.stderr));
        }, 25000);
        this.pending.set(id, {resolve, reject, timer});
        this.child.stdin.write(JSON.stringify({id, type, ...data}) + '\n');
      });
    }
    async close() {
      this.child.stdin.end();
      const timer = setTimeout(() => this.child.kill('SIGKILL'), 10000);
      try {
        const [code, signal] = await this.exited;
        assert.equal(
          code,
          0,
          'Pi did not shut down cleanly: ' + signal + ' ' + this.stderr,
        );
        this.verify();
      } finally {
        clearTimeout(timer);
      }
      writeFileSync(join(work, 'host-' + this.child.pid + '.log'), this.stderr);
    }
    verify() {
      assert.ok(
        !this.events.some(e => e.type === 'agent_start'),
        'smoke must not start provider requests',
      );
      const errors = this.events.filter(
        e =>
          e.type === 'extension_error' ||
          (e.type === 'extension_ui_request' &&
            e.method === 'notify' &&
            e.notifyType === 'error'),
      );
      assert.deepEqual(errors, [], 'host extension errors');
      assert.doesNotMatch(
        this.stderr,
        /Failed to load extension|ERR_MODULE_NOT_FOUND|Cannot find module|stale after session/i,
      );
    }
  }
  const expected = [
    'request-guard',
    'context',
    'memory',
    'workers',
    'wakes',
    'jobs',
    'goal',
  ];
  // Native local install registers the checkout path; Pi loads it without copying.
  const agent = join(work, 'installed');
  mkdirSync(agent, {recursive: true});
  for (let attempt = 0; attempt < 2; attempt++) {
    const result = spawnSync(binary, ['install', companion], {
      encoding: 'utf8',
      timeout: 30000,
      env: {...process.env, PI_CODING_AGENT_DIR: agent, PI_OFFLINE: '1'},
    });
    assert.equal(result.status, 0, result.stderr);
  }
  const packages = JSON.parse(
    readFileSync(join(agent, 'settings.json'), 'utf8'),
  ).packages;
  assert.equal(
    packages.length,
    1,
    'reinstalling must not duplicate the registration',
  );
  assert.equal(join(agent, packages[0]), companion);
  const installed = new Client(undefined, 'installed');
  try {
    const response = await installed.call('get_commands');
    for (const name of expected)
      assert.ok(
        response.data.commands.some(
          c =>
            c.name === name && c.sourceInfo?.path?.startsWith(companion + '/'),
        ),
        'installed resource missing: ' + name,
      );
    installed.verify();
    results.push({gate: 'native-local-install-loads-checkout', passed: true});
  } finally {
    await installed.close();
  }
  const normal = new Client(companion, 'manifest');
  try {
    const response = await normal.call('get_commands');
    for (const name of expected)
      assert.ok(
        response.data.commands.some(c => c.name === name),
        'manifest resource missing: ' + name,
      );
    await normal.call('prompt', {message: '/jobs'});
    await normal.call('prompt', {message: '/memory'});
    const memoryNotice = normal.events.find(
      event =>
        event.type === 'extension_ui_request' &&
        event.method === 'notify' &&
        event.message?.includes('NATIVE_KNOWLEDGE_FACT'),
    );
    assert.ok(
      memoryNotice,
      'native /memory reads the initialized disposable Git store',
    );
    assert.ok(
      memoryNotice.message.includes(published.snapshot),
      'native memory reports its pinned snapshot',
    );
    assert.doesNotMatch(memoryNotice.message, /Index: \//);
    normal.verify();
    results.push({gate: 'real-package-manifest-loading', passed: true});
  } finally {
    await normal.close();
  }
  // Hand-authored driver, kept outside the checkout, loads the identical package
  // factories with a narrow registration observer. Disable automatic model turns, not job execution.
  const driver = join(work, 'trial-driver.ts');
  writeFileSync(
    driver,
    `${manifest.pi.extensions.map((p, i) => `import extension${i} from ${JSON.stringify(join(companion, p))};`).join('\n')}
import {appendFileSync,readFileSync} from "node:fs";
import {join} from "node:path";
export default function(pi){
 const marker=process.env.PI_TRIAL_MARKER;
 const mark=(data)=>appendFileSync(marker,JSON.stringify(data)+"\\n");
 const tools=new Map();
 const proxy=new Proxy(pi,{get(target,key){
  if(key==="registerTool")return tool=>{tools.set(tool.name,tool);target.registerTool(tool);};
  if(key==="sendMessage")return (message,options)=>target.sendMessage(message,{...options,triggerTurn:false});
  return Reflect.get(target,key);
 }});
 ${manifest.pi.extensions.map((_, i) => `extension${i}(proxy);`).join('\n')}
 pi.on("session_start",()=>mark({event:"start"}));
 pi.on("session_shutdown",()=>mark({event:"shutdown"}));
 pi.registerCommand("lab-job",{description:"isolated package child-process acceptance",async handler(args,ctx){
  const started=await tools.get("job_start").execute("start",{command:"printf PACKAGE_CHILD_OK",cwd:ctx.cwd,timeoutMs:5000},undefined,undefined,ctx);
  const id=started.details.id;let job;
  const deadline=Date.now()+12000;
  do{
   const status=await tools.get("job_status").execute("status",{id},undefined,undefined,ctx);
   job=status.details.jobs[0];
   if(job.state==="finished")break;
   await new Promise(resolve=>setTimeout(resolve,50));
  }while(Date.now()<deadline);
  if(job.state!=="finished" || job.facts.root?.code!==0)throw new Error("package child did not finish: "+JSON.stringify(job));
  const output=readFileSync(join(job.directory,"output.log"),"utf8");
  if(output!=="PACKAGE_CHILD_OK")throw new Error("child capture mismatch");
  mark({event:"child-pass",id});
 }});
 pi.registerCommand("lab-reload",{description:"isolated runtime reload",async handler(args,ctx){await ctx.reload();}});
}
`,
  );
  const observed = new Client(driver, 'lifecycle');
  try {
    await observed.call('get_commands');
    await observed.call('prompt', {message: '/lab-job'});
    await observed.call('prompt', {message: '/lab-reload'});
    await observed.call('prompt', {message: '/lab-job'});
    observed.verify();
  } finally {
    await observed.close();
  }
  const marks = readFileSync(marker, 'utf8').trim().split('\n').map(JSON.parse);
  assert.equal(marks.filter(m => m.event === 'child-pass').length, 2);
  assert.ok(marks.filter(m => m.event === 'start').length >= 2);
  assert.ok(marks.filter(m => m.event === 'shutdown').length >= 2);
  results.push({
    gate: 'checkout-child-public-export-reload-shutdown',
    passed: true,
  });
  assert.ok(
    !existsSync(join(work, 'manifest/settings.json')),
    'manifest test must not register packages',
  );
  console.log(JSON.stringify({passed: true, results}, null, 2));
});

test('setup reconciles selected Pi resources without updating caches or unrelated settings', t => {
  const work = mkdtempSync(join(tmpdir(), 'dots-pi-setup-'));
  t.after(() => rmSync(work, {recursive: true, force: true}));
  const agent = join(work, 'agent');
  const cached = join(agent, 'npm/node_modules/neutral-cached-resource');
  mkdirSync(cached, {recursive: true});
  const packageFile = join(cached, 'package.json');
  const packageBytes = JSON.stringify({
    name: 'neutral-cached-resource',
    version: '1.0.0',
    pi: {extensions: []},
  });
  writeFileSync(packageFile, packageBytes);
  const before = statSync(packageFile);
  const settingsFile = join(agent, 'settings.json');
  const unrelated = join(work, 'unrelated-missing-resource');
  writeFileSync(
    settingsFile,
    JSON.stringify({
      theme: 'neutral',
      customSetting: {preserve: true},
      packages: [
        {source: 'npm:neutral-cached-resource@1.0.0', extensions: []},
        unrelated,
      ],
    }),
  );
  const installRoot = fileURLToPath(
    new URL('../../../../', hostModule('pi-coding-agent')),
  );
  const setup = join(companion, 'setup.mjs');
  const env = {
    ...process.env,
    HOME: work,
    PI_CODING_AGENT_DIR: agent,
    PI_OFFLINE: '1',
  };
  for (let attempt = 0; attempt < 2; attempt++) {
    const result = spawnSync(
      process.execPath,
      [setup, installRoot, 'npm:neutral-cached-resource'],
      {
        cwd: work,
        env,
        encoding: 'utf8',
        timeout: 30000,
      },
    );
    assert.equal(result.status, 0, result.stderr);
  }
  const settings = JSON.parse(readFileSync(settingsFile, 'utf8'));
  assert.equal(settings.theme, 'neutral');
  assert.deepEqual(settings.customSetting, {preserve: true});
  assert.deepEqual(settings.packages, [
    {source: 'npm:neutral-cached-resource', extensions: []},
    unrelated,
  ]);
  assert.equal(readFileSync(packageFile, 'utf8'), packageBytes);
  assert.equal(statSync(packageFile).ino, before.ino);
  assert.equal(statSync(packageFile).mtimeMs, before.mtimeMs);
  assert.ok(!existsSync(unrelated));

  const missing = join(work, 'required-resource');
  const failed = spawnSync(process.execPath, [setup, installRoot, missing], {
    cwd: work,
    env,
    encoding: 'utf8',
    timeout: 30000,
  });
  assert.notEqual(failed.status, 0);
  assert.ok(
    JSON.parse(readFileSync(settingsFile, 'utf8')).packages.includes(
      relative(agent, missing),
    ),
  );
  mkdirSync(missing);
  writeFileSync(
    join(missing, 'package.json'),
    JSON.stringify({name: 'required-resource', pi: {extensions: []}}),
  );
  const repaired = spawnSync(process.execPath, [setup, installRoot, missing], {
    cwd: work,
    env,
    encoding: 'utf8',
    timeout: 30000,
  });
  assert.equal(repaired.status, 0, repaired.stderr);
  assert.equal(
    JSON.parse(readFileSync(settingsFile, 'utf8')).packages.filter(
      source => source === relative(agent, missing),
    ).length,
    1,
  );

  const malformed = '{ invalid settings';
  writeFileSync(settingsFile, malformed);
  const invalid = spawnSync(process.execPath, [setup, installRoot, missing], {
    cwd: work,
    env,
    encoding: 'utf8',
    timeout: 30000,
  });
  assert.notEqual(invalid.status, 0);
  assert.equal(readFileSync(settingsFile, 'utf8'), malformed);
});
