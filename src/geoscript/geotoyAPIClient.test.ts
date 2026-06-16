// v1 tree validation — the guard that drops pre-migration drafts. Run with:
//   yarn tsx --test src/geoscript/geotoyAPIClient.test.ts

import assert from 'node:assert/strict';
import { test } from 'node:test';

import { buildEmptyTree, isTreeDefV1 } from './geotoyAPIClient';

test('isTreeDefV1 accepts a freshly-built v1 tree', () => {
  assert.equal(isTreeDefV1(buildEmptyTree()), true);
});

test('isTreeDefV1 rejects pre-migration (v0) and malformed trees', () => {
  const v0 = { rootId: 'r', globalsSource: '', nodes: { r: { id: 'r', name: '_root' } } };
  assert.equal(isTreeDefV1(v0), false); // no version field
  assert.equal(isTreeDefV1({ ...buildEmptyTree(), version: 2 }), false);
  assert.equal(isTreeDefV1({ version: 1, rootId: 'r' }), false); // no nodes
  assert.equal(isTreeDefV1(null), false);
  assert.equal(isTreeDefV1('nope'), false);
});
