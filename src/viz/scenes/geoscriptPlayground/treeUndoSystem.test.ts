// Focused tests for the snapshot-based tree undo system. Each case mirrors a
// flow the UI exercises. Run with:
//   yarn tsx --test src/viz/scenes/geoscriptPlayground/treeUndoSystem.test.ts

import assert from 'node:assert/strict';
import { test } from 'node:test';

import { TreeUndoSystem, type TreeSnapshot } from './treeUndoSystem';

// Minimal hand-rolled tree shape — matches `TreeDef` structurally enough for
// JSON-equality comparison inside the undo system.
const tree = (rootId: string, extra: Record<string, unknown> = {}) =>
  ({ rootId, globalsSource: '', nodes: { [rootId]: { id: rootId, name: '_root' } }, ...extra }) as any;

const snap = (
  rootId: string,
  selectedId: string | null = null,
  soloId: string | null = null
): TreeSnapshot => ({
  tree: tree(rootId),
  selectedId,
  soloId,
});

test('undo/redo round-trips a single edit', () => {
  const u = new TreeUndoSystem();
  const before = snap('a');
  const after = snap('b');
  u.push(null, before, after, 1000);

  assert.equal(u.canUndo(), true);
  assert.equal(u.canRedo(), false);

  assert.deepEqual(u.undo(), before);
  assert.equal(u.canUndo(), false);
  assert.equal(u.canRedo(), true);

  assert.deepEqual(u.redo(), after);
  assert.equal(u.canUndo(), true);
  assert.equal(u.canRedo(), false);
});

test('no-op edits (equal before/after) are not pushed', () => {
  const u = new TreeUndoSystem();
  const s = snap('a');
  u.push(null, s, s, 1000);
  assert.equal(u.canUndo(), false);
});

test('same coalesce key within window mutates the previous entry', () => {
  const u = new TreeUndoSystem();
  // Window measures gap from the *previous* push (not the burst start) — a long
  // drag extends one entry across many seconds as long as ticks stay close.
  u.push('transform:n1', snap('a'), snap('b'), 1000);
  u.push('transform:n1', snap('b'), snap('c'), 1700); // 700ms gap, ok
  u.push('transform:n1', snap('c'), snap('d'), 2400); // 700ms gap, ok (1400ms from start)

  // All three collapsed into a single entry: before=a, after=d.
  assert.deepEqual(u.undo()?.tree.rootId, 'a');
  assert.equal(u.canUndo(), false);
});

test('wouldCoalesce + null `before` is the lazy-snapshot fast path', () => {
  const u = new TreeUndoSystem();
  // First push has no prior entry to coalesce with — wouldCoalesce reports false.
  assert.equal(u.wouldCoalesce('transform:n1', 1000), false);
  u.push('transform:n1', snap('a'), snap('b'), 1000);

  // Now there's a recent same-key entry — coalescing applies, caller can skip
  // capturing `before`.
  assert.equal(u.wouldCoalesce('transform:n1', 1100), true);
  u.push('transform:n1', null, snap('c'), 1100);
  assert.deepEqual(u.undo()?.tree.rootId, 'a');
  assert.equal(u.canUndo(), false);

  // If a null-before push arrives but the coalesce target aged out, it's a
  // no-op (defensive: caller predicted coalescing but it no longer applies).
  u.push('transform:n1', snap('a'), snap('b'), 1000);
  u.push('transform:n1', null, snap('z'), 9999);
  assert.deepEqual(u.undo()?.tree.rootId, 'a'); // 'z' was dropped
});

test('same coalesce key outside the window pushes a new entry', () => {
  const u = new TreeUndoSystem();
  u.push('transform:n1', snap('a'), snap('b'), 1000);
  u.push('transform:n1', snap('b'), snap('c'), 5000); // 4s gap > 800ms window

  // Two separate entries: undoing once goes back to b, again to a.
  assert.deepEqual(u.undo()?.tree.rootId, 'b');
  assert.deepEqual(u.undo()?.tree.rootId, 'a');
});

test('different coalesce keys never collapse, even back-to-back', () => {
  const u = new TreeUndoSystem();
  u.push('transform:n1', snap('a'), snap('b'), 1000);
  u.push('transform:n2', snap('b'), snap('c'), 1050);

  assert.deepEqual(u.undo()?.tree.rootId, 'b');
  assert.deepEqual(u.undo()?.tree.rootId, 'a');
});

test('null coalesce key never collapses', () => {
  const u = new TreeUndoSystem();
  u.push(null, snap('a'), snap('b'), 1000);
  u.push(null, snap('b'), snap('c'), 1050);

  assert.deepEqual(u.undo()?.tree.rootId, 'b');
  assert.deepEqual(u.undo()?.tree.rootId, 'a');
});

test('a new push invalidates the redo stack', () => {
  const u = new TreeUndoSystem();
  u.push(null, snap('a'), snap('b'), 1000);
  u.push(null, snap('b'), snap('c'), 2000);
  u.undo(); // c -> b on redo
  assert.equal(u.canRedo(), true);

  u.push(null, snap('b'), snap('d'), 3000);
  assert.equal(u.canRedo(), false);
});

test('a coalesced push invalidates the redo stack', () => {
  const u = new TreeUndoSystem();
  u.push('transform:n1', snap('a'), snap('b'), 1000);
  u.push(null, snap('b'), snap('c'), 2000);
  u.undo();
  assert.equal(u.canRedo(), true);

  u.push('transform:n1', snap('b'), snap('d'), 1700);
  assert.equal(u.canRedo(), false);
  assert.deepEqual(u.undo()?.tree.rootId, 'a');
});

test('selection and solo are captured and round-trip', () => {
  const u = new TreeUndoSystem();
  const before: TreeSnapshot = { tree: tree('a'), selectedId: 'n1', soloId: 'n2' };
  const after: TreeSnapshot = { tree: tree('a', { extra: 1 }), selectedId: 'n3', soloId: null };
  u.push(null, before, after, 1000);

  const restored = u.undo()!;
  assert.equal(restored.selectedId, 'n1');
  assert.equal(restored.soloId, 'n2');
});

test('clear empties both stacks', () => {
  const u = new TreeUndoSystem();
  u.push(null, snap('a'), snap('b'), 1000);
  u.push(null, snap('b'), snap('c'), 2000);
  u.undo();
  u.clear();
  assert.equal(u.canUndo(), false);
  assert.equal(u.canRedo(), false);
});
