import assert from 'node:assert/strict';
import test from 'node:test';
import register, {rexContext} from '../extensions/rex/index.ts';

const orientation = "You're running in a Rex terminal.\n";

test('asks Rex for its orientation only inside a Rex terminal', async () => {
  const calls = [];
  const run = async (file, args) => {
    calls.push([file, ...args]);
    return orientation;
  };
  assert.equal(await rexContext({}, run), undefined);
  assert.equal(calls.length, 0);
  assert.equal(
    await rexContext({REX_SESSION: 'session:a', REX_BIN_DIR: '/opt/rex'}, run),
    orientation.trim(),
  );
  assert.deepEqual(calls, [
    ['/opt/rex/rex', '__internal', 'session-start', '--agent', 'pi'],
  ]);
  assert.equal(
    await rexContext({REX_SESSION: 'session:a'}, run),
    orientation.trim(),
  );
  // Workers share the terminal's session but do not own it.
  const asked = calls.length;
  assert.equal(
    await rexContext({REX_SESSION: 'session:a', DOTS_WORKER_ID: 'w-1'}, run),
    undefined,
  );
  assert.equal(calls.length, asked);
  assert.equal(calls[1][0], 'rex');
});

test('a failing or silent Rex adds nothing', async () => {
  const env = {REX_SESSION: 'session:a'};
  assert.equal(
    await rexContext(env, async () => {
      throw new Error('no server');
    }),
    undefined,
  );
  assert.equal(await rexContext(env, async () => '  \n'), undefined);
});

test('each run carries the orientation in an independent section', async () => {
  const handlers = new Map();
  register(
    {on: (name, handler) => handlers.set(name, handler)},
    {REX_SESSION: 'session:a'},
    async () => orientation,
  );
  await handlers.get('session_start')(
    {type: 'session_start', reason: 'startup'},
    {},
  );
  const contextFiles = [{path: '/repo/AGENTS.md', content: 'rules'}];
  const event = {systemPromptOptions: {contextFiles, sections: {}}};
  handlers.get('before_agent_start')(event, {cwd: '/repo'});
  handlers.get('before_agent_start')(event, {cwd: '/repo'});
  assert.deepEqual(contextFiles, [{path: '/repo/AGENTS.md', content: 'rules'}]);
  assert.deepEqual(event.systemPromptOptions.sections, {
    dots_rex: orientation.trim(),
  });
  handlers.get('session_shutdown')({}, {});
  const fresh = {
    systemPromptOptions: {contextFiles: [], sections: {dots_rex: 'stale'}},
  };
  handlers.get('before_agent_start')(fresh, {cwd: '/repo'});
  assert.deepEqual(fresh.systemPromptOptions.contextFiles, []);
  assert.deepEqual(fresh.systemPromptOptions.sections, {});
});
