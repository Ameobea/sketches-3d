// Tree-core + v2-container validation guards. Run with:
//   yarn tsx --test src/geoscript/geotoyAPIClient.test.ts

import assert from 'node:assert/strict';
import { test } from 'node:test';

import {
  buildEmptyDoc,
  buildEmptyTree,
  defaultTreeEntry,
  getRootNodeSource,
  isCompositionDocV2,
  isTreeDefV1,
  withTree,
  wrapTree,
} from './geotoyAPIClient';

test('isTreeDefV1 accepts a freshly-built v1 tree core', () => {
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

test('isCompositionDocV2 accepts wrapped cores and rejects bare v1 / empty containers', () => {
  const doc = buildEmptyDoc();
  assert.equal(isCompositionDocV2(doc), true);
  assert.equal(isCompositionDocV2(buildEmptyTree()), false);
  assert.equal(isCompositionDocV2({ version: 2, trees: [] }), false);
  assert.equal(isCompositionDocV2({ version: 1, trees: doc.trees }), false);
  assert.equal(isCompositionDocV2(null), false);
});

test('defaultTreeEntry picks first mesh tree; withTree swaps cores by id', () => {
  const core = buildEmptyTree();
  const doc = wrapTree(core);
  assert.equal(defaultTreeEntry(doc).id, 'main');
  assert.equal(getRootNodeSource(doc), '');

  const edited = {
    ...core,
    nodes: { ...core.nodes, [core.rootId]: { ...core.nodes[core.rootId], source: 'box(1) | render' } },
  };
  const next = withTree(doc, 'main', edited);
  assert.equal(getRootNodeSource(next), 'box(1) | render');
  assert.equal(getRootNodeSource(doc), ''); // original untouched
});
