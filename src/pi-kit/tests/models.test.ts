import assert from 'node:assert/strict';
import test from 'node:test';
import type {Api, Model} from '@earendil-works/pi-ai';
import {catalogOf, checkThinking, findModel} from '../src/models.ts';

// Hand-authored catalog entries; real model ids never appear in tests.
const model = (
  provider: string,
  id: string,
  extra: Partial<Model<Api>> = {},
): Model<Api> => ({
  provider,
  id,
  name: id,
  api: 'fixture',
  baseUrl: '',
  reasoning: true,
  input: ['text'],
  cost: {input: 1, output: 5, cacheRead: 0, cacheWrite: 0},
  contextWindow: 200_000,
  maxTokens: 32_000,
  ...extra,
});
const fast = model('alpha', 'fast');
const deep = model('beta', 'deep', {
  thinkingLevelMap: {off: null, minimal: null, xhigh: 'xhigh'},
});
const plain = model('alpha', 'plain', {reasoning: false});

test('a catalog keeps order, drops repeats, and records supported thinking levels', () => {
  const catalog = catalogOf([fast, deep, fast, plain]);
  assert.deepEqual(
    catalog.map(e => e.ref),
    ['alpha/fast', 'beta/deep', 'alpha/plain'],
  );
  assert.deepEqual(catalog[1]?.levels, ['low', 'medium', 'high', 'xhigh']);
  assert.deepEqual(catalog[2]?.levels, ['off']);
});

test('unknown models and unsupported levels fail with the valid choices', () => {
  const catalog = catalogOf([fast, deep]);
  assert.deepEqual(findModel('beta/deep', catalog), {
    ok: true,
    value: catalog[1],
  });
  assert.deepEqual(findModel('gamma/gone', catalog), {
    ok: false,
    error:
      'model gamma/gone is not available; choose one of: alpha/fast, beta/deep',
  });
  assert.deepEqual(findModel('gamma/gone', catalog, ['inherit']), {
    ok: false,
    error:
      'model gamma/gone is not available; choose inherit or one of: alpha/fast, beta/deep',
  });
  assert.deepEqual(findModel('gamma/gone', []), {
    ok: false,
    error: 'model gamma/gone is not available; choose one of: (none)',
  });
  assert.deepEqual(checkThinking(deep, 'xhigh'), {ok: true, value: 'xhigh'});
  assert.deepEqual(checkThinking(deep, 'off'), {
    ok: false,
    error:
      'thinking off is unsupported: beta/deep supports thinking low, medium, high, xhigh',
  });
  assert.deepEqual(checkThinking(deep, 'loud'), {
    ok: false,
    error:
      'thinking loud is unsupported: beta/deep supports thinking low, medium, high, xhigh',
  });
  assert.deepEqual(checkThinking(plain, 'minimal', 'inherit (minimal)'), {
    ok: false,
    error:
      'thinking inherit (minimal) is unsupported: alpha/plain supports thinking off',
  });
});
