// Run with: yarn tsx --test src/viz/levelDef/inputInjection.test.ts
import assert from 'node:assert/strict';
import { test } from 'node:test';

import { reifyInput, injectInputs } from './inputInjection';

test('reifyInput converts each input type to its wire form', () => {
  assert.deepEqual(reifyInput({ type: 'float', value: 1.5 }), { kind: 'float', value: [1.5] });
  assert.deepEqual(reifyInput({ type: 'int', value: 4 }), { kind: 'int', value: [4] });
  assert.deepEqual(reifyInput({ type: 'bool', value: true }), { kind: 'bool', value: [1] });
  assert.deepEqual(reifyInput({ type: 'select', value: 'b' }), { kind: 'select', str_value: 'b' });
});

test('reifyInput normalizes color from triple, hex, and int to a 0..1 rgb triple', () => {
  assert.deepEqual(reifyInput({ type: 'color', value: [0.2, 0.4, 0.6] }), {
    kind: 'color',
    value: [0.2, 0.4, 0.6],
  });
  assert.deepEqual(reifyInput({ type: 'color', value: '#ff8000' }), {
    kind: 'color',
    value: [1, 128 / 255, 0],
  });
  assert.deepEqual(reifyInput({ type: 'color', value: 0xff8000 }), {
    kind: 'color',
    value: [1, 128 / 255, 0],
  });
});

test('injectInputs spreads bare-named inputs across every given module, merging onto existing', () => {
  const map = injectInputs(
    { code: { existing: { kind: 'float', value: [9] } } },
    { amp: { type: 'float', value: 0.5 } },
    ['code', '_root']
  );
  assert.deepEqual(map.code, {
    existing: { kind: 'float', value: [9] },
    amp: { kind: 'float', value: [0.5] },
  });
  assert.deepEqual(map._root, { amp: { kind: 'float', value: [0.5] } });
});

test('injectInputs is a no-op when there are no inputs', () => {
  const base = { code: {} };
  assert.equal(injectInputs(base, undefined, ['code']), base);
  assert.equal(injectInputs(base, {}, ['code']), base);
});
