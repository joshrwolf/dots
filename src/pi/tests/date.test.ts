import assert from 'node:assert/strict';
import test from 'node:test';
import {isTimestamp} from '../src/date.ts';

test('wire timestamps require nonnegative integers in explicit representable units', () => {
  for (const unit of ['seconds', 'milliseconds'] as const) {
    const limit =
      unit === 'seconds' ? 8_640_000_000_000 : 8_640_000_000_000_000;
    for (const value of [0, 100, limit]) {
      assert.equal(isTimestamp(value, unit), true);
      assert.doesNotThrow(() =>
        new Date(unit === 'seconds' ? value * 1000 : value).toISOString(),
      );
    }
    for (const value of [
      -1,
      0.5,
      NaN,
      Infinity,
      limit + 1,
      Number.MAX_SAFE_INTEGER,
      '100',
      null,
    ]) {
      assert.equal(isTimestamp(value, unit), false);
    }
  }
});
