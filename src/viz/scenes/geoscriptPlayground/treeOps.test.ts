// Focused tests for treeOps. Each case mirrors an end-user flow the tree UI will
// exercise, not internal helpers. Run with:
//   yarn tsx --test src/viz/scenes/geoscriptPlayground/treeOps.test.ts

import assert from 'node:assert/strict';
import { test } from 'node:test';

// Inlined types — `geotoyAPIClient.ts` reads `import.meta.env` at module init and
// can't be imported under plain node.
interface Transform3 {
  pos: [number, number, number];
  rot: [number, number, number];
  scale: [number, number, number];
}
interface NodeDef {
  id: string;
  name: string;
  source: string;
  instances: Transform3[];
  children: string[];
  disabled?: boolean;
}
interface TreeDef {
  version: 1;
  rootId: string;
  globalsSource: string;
  nodes: Record<string, NodeDef>;
}

import {
  addInstance,
  computeMeshCounts,
  createNode,
  deleteNode,
  emptyTree,
  getNodeAncestorChain,
  isAncestorOf,
  removeInstance,
  renameNode,
  reparent,
  setInstanceTransform,
  setSource,
} from './treeOps';

const xform = (x: number, y: number, z: number): Transform3 => ({
  pos: [x, y, z],
  rot: [0, 0, 0],
  scale: [1, 1, 1],
});

test('emptyTree contains exactly the _root node', () => {
  const t = emptyTree() as TreeDef;
  assert.equal(Object.keys(t.nodes).length, 1);
  assert.equal(t.nodes[t.rootId].name, '_root');
});

test('createNode with no parentId attaches as a child of _root', () => {
  const t = emptyTree() as TreeDef;
  const id = createNode(t, { id: 'a', name: 'a' });
  assert.deepEqual(t.nodes[t.rootId].children, [id]);
});

test('create + child + delete subtree removes the subtree but keeps _root', () => {
  // A->B->C; delete A and only _root should remain.
  const t = emptyTree() as TreeDef;
  const a = createNode(t, { id: 'a', name: 'a' });
  const b = createNode(t, { id: 'b', name: 'b', parentId: a });
  createNode(t, { id: 'c', name: 'c', parentId: b });

  assert.equal(Object.keys(t.nodes).length, 4); // _root + a + b + c

  deleteNode(t, a);
  assert.equal(Object.keys(t.nodes).length, 1);
  assert.equal(t.nodes[t.rootId].name, '_root');
  assert.deepEqual(t.nodes[t.rootId].children, []);
});

test('deleting _root is rejected', () => {
  const t = emptyTree() as TreeDef;
  assert.throws(() => deleteNode(t, t.rootId), /root.*cannot be deleted/);
});

test('renaming _root is rejected', () => {
  const t = emptyTree() as TreeDef;
  assert.throws(() => renameNode(t, t.rootId, 'foo'), /root.*cannot be renamed/);
});

test('reparenting _root is rejected', () => {
  const t = emptyTree() as TreeDef;
  const a = createNode(t, { id: 'a', name: 'a' });
  assert.throws(() => reparent(t, t.rootId, a), /root.*cannot be reparented/);
});

test('creating a node named _root or _globals is rejected', () => {
  const t = emptyTree() as TreeDef;
  assert.throws(() => createNode(t, { name: '_root' }), /reserved/);
  assert.throws(() => createNode(t, { name: '_globals' }), /reserved/);
});

test('reparent with null parent attaches to _root', () => {
  const t = emptyTree() as TreeDef;
  const a = createNode(t, { id: 'a', name: 'a' });
  const b = createNode(t, { id: 'b', name: 'b', parentId: a });
  reparent(t, b, null);
  assert.deepEqual(t.nodes[t.rootId].children, [a, b]);
  assert.deepEqual(t.nodes[a].children, []);
});

test('reparent moves the subtree under a new parent and rejects cycles', () => {
  const t = emptyTree() as TreeDef;
  const p1 = createNode(t, { id: 'p1', name: 'p1' });
  const p2 = createNode(t, { id: 'p2', name: 'p2' });
  const c = createNode(t, { id: 'c', name: 'c', parentId: p1 });

  reparent(t, c, p2);
  assert.deepEqual(t.nodes[p1].children, []);
  assert.deepEqual(t.nodes[p2].children, [c]);

  assert.throws(() => reparent(t, p2, c), /cycle/);
  assert.deepEqual(t.nodes[p2].children, [c]);
  assert.ok(isAncestorOf(t, p2, c));
});

test('rename: valid + unique succeeds; collisions and invalid/reserved names throw', () => {
  const t = emptyTree() as TreeDef;
  const a = createNode(t, { id: 'a', name: 'foo' });
  const b = createNode(t, { id: 'b', name: 'bar' });

  renameNode(t, a, 'baz');
  assert.equal(t.nodes[a].name, 'baz');

  assert.throws(() => renameNode(t, a, 'bar'), /already in use/);
  assert.throws(() => renameNode(t, b, '1bad'), /not a valid/);
  assert.throws(() => renameNode(t, b, '_root'), /not a valid/);
  assert.throws(() => renameNode(t, b, '_globals'), /not a valid/);
});

test('uniqueName auto-disambiguates createNode collisions', () => {
  const t = emptyTree() as TreeDef;
  createNode(t, { id: 'a', name: 'box' });
  const b = createNode(t, { id: 'b', name: 'box' });
  assert.equal(t.nodes[b].name, 'box_2');

  setSource(t, b, 'render(box(1))');
  assert.equal(t.nodes[b].source, 'render(box(1))');
});

test('computeMeshCounts sums direct + descendant renders per node', () => {
  // _root -> a -> b (leaf); _root -> c (leaf)
  // Direct counts: a=2, b=3, c=5, _root=1
  // Expected recursive: b=3, a=2+3=5, c=5, _root=1+5+5=11
  const t = emptyTree() as TreeDef;
  const a = createNode(t, { id: 'a', name: 'a' });
  const b = createNode(t, { id: 'b', name: 'b', parentId: a });
  const c = createNode(t, { id: 'c', name: 'c' });

  const direct = new Map<string, number>([
    [t.rootId, 1],
    [a, 2],
    [b, 3],
    [c, 5],
  ]);
  const counts = computeMeshCounts(t, direct);
  assert.equal(counts.get(b), 3);
  assert.equal(counts.get(a), 5);
  assert.equal(counts.get(c), 5);
  assert.equal(counts.get(t.rootId), 11);
});

test('computeMeshCounts returns zero for nodes with no renders', () => {
  const t = emptyTree() as TreeDef;
  const a = createNode(t, { id: 'a', name: 'a' });
  const counts = computeMeshCounts(t, new Map());
  assert.equal(counts.get(t.rootId), 0);
  assert.equal(counts.get(a), 0);
});

test('createNode seeds exactly one identity instance', () => {
  const t = emptyTree() as TreeDef;
  const a = createNode(t, { id: 'a', name: 'a' });
  assert.equal(t.nodes[a].instances.length, 1);
  assert.deepEqual(t.nodes[a].instances[0], xform(0, 0, 0));
});

test('addInstance appends and returns its index; removeInstance refuses the last', () => {
  const t = emptyTree() as TreeDef;
  const a = createNode(t, { id: 'a', name: 'a' });

  assert.equal(addInstance(t, a, xform(1, 2, 3)), 1);
  assert.equal(t.nodes[a].instances.length, 2);
  assert.deepEqual(t.nodes[a].instances[1].pos, [1, 2, 3]);

  removeInstance(t, a, 0);
  assert.equal(t.nodes[a].instances.length, 1);
  removeInstance(t, a, 0); // last instance: no-op
  assert.equal(t.nodes[a].instances.length, 1);
});

test('instance ops refuse _root', () => {
  const t = emptyTree() as TreeDef;
  assert.equal(addInstance(t, t.rootId, xform(1, 0, 0)), -1);
  assert.equal(t.nodes[t.rootId].instances.length, 1);
});

test('setInstanceTransform writes one index; out-of-range is a no-op', () => {
  const t = emptyTree() as TreeDef;
  const a = createNode(t, { id: 'a', name: 'a' });
  addInstance(t, a, xform(0, 0, 0));

  setInstanceTransform(t, a, 1, xform(7, 8, 9));
  assert.deepEqual(t.nodes[a].instances[1].pos, [7, 8, 9]);

  setInstanceTransform(t, a, 5, xform(1, 1, 1));
  assert.equal(t.nodes[a].instances.length, 2);
});

test('getNodeAncestorChain returns [node, ..., _root]', () => {
  const t = emptyTree() as TreeDef;
  const gp = createNode(t, { id: 'gp', name: 'gp' });
  const p = createNode(t, { id: 'p', name: 'p', parentId: gp });
  const leaf = createNode(t, { id: 'leaf', name: 'leaf', parentId: p });

  const chain = getNodeAncestorChain(t, leaf);
  assert.ok(chain);
  assert.deepEqual(
    chain.map(n => n.id),
    [leaf, p, gp, t.rootId]
  );
});
