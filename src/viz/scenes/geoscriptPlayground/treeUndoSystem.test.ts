// Drive live `treeOps` mutations and verify a round-trip through the undo
// stack matches the original / final tree.
//
//   yarn tsx --test src/viz/scenes/geoscriptPlayground/treeUndoSystem.test.ts

import assert from 'node:assert/strict';
import { test } from 'node:test';

import { buildEmptyTree, buildIdentityTransform, type TreeDef } from 'src/geoscript/geotoyAPIClient';
import { createNode as opsCreateNode, deleteNode as opsDeleteNode, reparent as opsReparent } from './treeOps';
import {
  applyGeotoyUndoEntry,
  buildGeotoyUndoSystem,
  captureSubtreeNodes,
  type GeotoyUndoSystem,
} from './treeUndoSystem';

const apply = (sys: GeotoyUndoSystem, tree: TreeDef, dir: 'undo' | 'redo') =>
  dir === 'undo'
    ? sys.undo((e, d) => applyGeotoyUndoEntry(tree, e, d))
    : sys.redo((e, d) => applyGeotoyUndoEntry(tree, e, d));

// Compare two trees structurally — `tree.nodes` is a Record so JSON.stringify
// is sensitive to key-insertion order, which doesn't survive delete + restore.
const treesEqual = (a: TreeDef, b: TreeDef): boolean =>
  a.rootId === b.rootId &&
  a.globalsSource === b.globalsSource &&
  Object.keys(a.nodes).length === Object.keys(b.nodes).length &&
  Object.keys(a.nodes).every(k => JSON.stringify(a.nodes[k]) === JSON.stringify(b.nodes[k]));

test('transform: undo restores pre-edit transform; redo reapplies', () => {
  const tree = buildEmptyTree();
  const sys = buildGeotoyUndoSystem();
  const id = opsCreateNode(tree, { name: 'a' });

  const t0 = structuredClone(tree.nodes[id].instances[0]);
  const t1 = {
    pos: [1, 2, 3] as [number, number, number],
    rot: [0, 0, 0] as [number, number, number],
    scale: [1, 1, 1] as [number, number, number],
  };
  tree.nodes[id].instances[0] = structuredClone(t1);
  sys.push({ type: 'transform', id, index: 0, before: t0, after: structuredClone(t1) });
  const afterPush = structuredClone(tree);

  apply(sys, tree, 'undo');
  assert.deepEqual(tree.nodes[id].instances[0], t0);

  apply(sys, tree, 'redo');
  assert.ok(treesEqual(tree, afterPush));
});

test('deleteSubtree: undo restores the whole subtree at the correct index', () => {
  const tree = buildEmptyTree();
  const sys = buildGeotoyUndoSystem();
  const a = opsCreateNode(tree, { name: 'a' });
  const b = opsCreateNode(tree, { name: 'b' });
  const c = opsCreateNode(tree, { name: 'c' });
  const aChild = opsCreateNode(tree, { name: 'aChild', parentId: a });

  const original = structuredClone(tree);

  const parentId = tree.rootId;
  const index = tree.nodes[parentId].children.indexOf(a);
  const nodes = captureSubtreeNodes(tree, a);
  // Mutate before pushing (matches treeState.deleteNode order).
  opsDeleteNode(tree, a);
  sys.push({ type: 'deleteSubtree', rootId: a, nodes, parentId, index });

  assert.equal(tree.nodes[a], undefined);
  assert.equal(tree.nodes[aChild], undefined);
  assert.deepEqual(tree.nodes[tree.rootId].children, [b, c]);

  apply(sys, tree, 'undo');
  assert.ok(treesEqual(tree, original));

  apply(sys, tree, 'redo');
  assert.equal(tree.nodes[a], undefined);
  assert.deepEqual(tree.nodes[tree.rootId].children, [b, c]);
});

test('createNode: undo deletes the new node; redo recreates with the same id', () => {
  const tree = buildEmptyTree();
  const sys = buildGeotoyUndoSystem();
  const id = opsCreateNode(tree, { name: 'a', transform: buildIdentityTransform() });
  const parentId = tree.rootId;
  const index = tree.nodes[parentId].children.indexOf(id);
  const nodeDef = structuredClone(tree.nodes[id]);
  sys.push({ type: 'createNode', nodeDef, parentId, index });

  apply(sys, tree, 'undo');
  assert.equal(id in tree.nodes, false);

  apply(sys, tree, 'redo');
  assert.equal(id in tree.nodes, true);
  assert.equal(tree.nodes[id]?.name, 'a');
});

test('reparent: undo restores the original parent and child-index', () => {
  const tree = buildEmptyTree();
  const sys = buildGeotoyUndoSystem();
  const group = opsCreateNode(tree, { name: 'group' });
  const target = opsCreateNode(tree, { name: 'target' });
  const original = structuredClone(tree);

  const oldParentId = tree.rootId;
  const oldIndex = tree.nodes[oldParentId].children.indexOf(target);
  opsReparent(tree, target, group);
  const newIndex = tree.nodes[group].children.indexOf(target);
  sys.push({ type: 'reparent', id: target, oldParentId, oldIndex, newParentId: group, newIndex });

  apply(sys, tree, 'undo');
  assert.ok(treesEqual(tree, original));

  apply(sys, tree, 'redo');
  assert.deepEqual(tree.nodes[group].children, [target]);
});

test('stale entries no-op rather than throwing', () => {
  const tree = buildEmptyTree();
  const sys = buildGeotoyUndoSystem();
  sys.push({
    type: 'transform',
    id: 'ghost',
    index: 0,
    before: buildIdentityTransform(),
    after: { pos: [1, 0, 0], rot: [0, 0, 0], scale: [1, 1, 1] },
  });
  assert.doesNotThrow(() => apply(sys, tree, 'undo'));
  assert.doesNotThrow(() => apply(sys, tree, 'redo'));
});
