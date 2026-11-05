import assert from 'node:assert/strict';
import test from 'node:test';
import {agentEnv, bindAgent, endAgent} from '../src/host/agent.ts';

const runtimeOf = session => new RegExp(`^pi-${session}-[0-9a-f]{16}$`);

test('a reload re-import keeps the bound runtime without trusting the export', async () => {
  // Values inherited from a parent process are replaced, never adopted.
  const env = {
    PI_SESSION_ID: 'outer',
    DOTS_AGENT_OWNER: 'pi-s-0123456789abcdef',
  };
  const bound = bindAgent('s', 'startup', env);
  assert.match(bound.owner, runtimeOf('s'));
  assert.deepEqual(env, {PI_SESSION_ID: 's', DOTS_AGENT_OWNER: bound.owner});
  // Pi loads a fresh module instance on reload; the binding is process-wide.
  const reimported = await import('../src/host/agent.ts?reload');
  env.PI_SESSION_ID = 'outer';
  env.DOTS_AGENT_OWNER = 'pi-s-fedcba9876543210';
  assert.equal(reimported.bindAgent('s', 'reload', env), bound);
  assert.deepEqual(env, {PI_SESSION_ID: 's', DOTS_AGENT_OWNER: bound.owner});
  assert.throws(
    () => reimported.bindAgent('other', 'reload', env),
    /Reload changed the Pi session/,
  );
  assert.throws(() => bindAgent(' ', 'startup', env), /empty session/);
  endAgent(bound, env);
  assert.deepEqual(env, {});
  // A reload with nothing bound (the companion loaded mid-process) mints.
  const minted = bindAgent('s', 'reload', env);
  assert.notEqual(minted.owner, bound.owner);
  endAgent(minted, env);
});

test('only the bound runtime can end itself', () => {
  const env = {};
  const old = bindAgent('s', 'startup', env);
  const other = bindAgent('t', 'resume', env);
  endAgent(old, env);
  assert.deepEqual(env, {PI_SESSION_ID: 't', DOTS_AGENT_OWNER: other.owner});
  // Back to the first session: a stale callback of its earlier runtime
  // cannot erase the newer runtime of the same session.
  const next = bindAgent('s', 'resume', env);
  assert.notEqual(next.owner, old.owner);
  endAgent(old, env);
  endAgent(other, env);
  assert.deepEqual(env, {PI_SESSION_ID: 's', DOTS_AGENT_OWNER: next.owner});
  assert.equal(bindAgent('s', 'reload', env), next);
  endAgent(next, env);
  assert.deepEqual(env, {});
});

test("a companion subprocess acts as the caller's runtime with its native session", () => {
  const agent = {session: 's', owner: 'pi-s-0123456789abcdef'};
  assert.deepEqual(
    agentEnv(agent, {
      PATH: '/bin',
      PI_SESSION_ID: 'inherited',
      DOTS_AGENT_OWNER: 'pi-later-fedcba9876543210',
    }),
    {
      PATH: '/bin',
      PI_SESSION_ID: 's',
      DOTS_AGENT_OWNER: 'pi-s-0123456789abcdef',
    },
  );
});
