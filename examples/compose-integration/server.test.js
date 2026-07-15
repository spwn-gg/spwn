// Live test harness (runs under the `test` service via `node --test --watch`).
import { test } from 'node:test';
import assert from 'node:assert';
import { greeting } from './server.js';

test('greeting is set', () => {
  assert.match(greeting, /spwn/);
});
