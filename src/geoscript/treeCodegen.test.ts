// Focused, end-to-end-ish tests for compileTree. Each case mirrors a flow the
// Geotoy tree feature relies on; we assert on the emitted source strings as a
// whole rather than dissecting internal helpers. Run with:
//   yarn tsx --test src/geoscript/treeCodegen.test.ts

import assert from 'node:assert/strict';
import { test } from 'node:test';

// `geotoyAPIClient` reads `import.meta.env` at module top level (Vite), which is
// undefined under plain node. Re-declare the small handful of types we need here
// so the test file is node-runnable without pulling in the API client.
import { compileTree } from './treeCodegen';

interface Transform3 {
  pos: [number, number, number];
  rot: [number, number, number];
  scale: [number, number, number];
}
interface NodeDef {
  id: string;
  name: string;
  source: string;
  instances: (Transform3 & { id: string })[];
  children: string[];
  disabled?: boolean;
}
interface TreeDef {
  version: 1;
  rootId: string;
  globalsSource: string;
  nodes: Record<string, NodeDef>;
}

const identity = (): Transform3 => ({ pos: [0, 0, 0], rot: [0, 0, 0], scale: [1, 1, 1] });

const node = (overrides: Partial<NodeDef> & Pick<NodeDef, 'id' | 'name'>): NodeDef => ({
  source: '',
  instances: [{ ...identity(), id: `${overrides.id}-0` }],
  children: [],
  ...overrides,
});

const tree = (rootId: string, nodes: NodeDef[], globalsSource = ''): TreeDef => ({
  version: 1,
  rootId,
  globalsSource,
  nodes: Object.fromEntries(nodes.map(n => [n.id, n])),
});

test('legacy _root-only tree: source verbatim is the rootSource', () => {
  // Mirrors a composition migrated from the flat-source era: one node named `_root`,
  // source is `box(1) | render`. The rootSource is the user's source verbatim and
  // the modules map is empty.
  const t = tree('r', [node({ id: 'r', name: '_root', source: 'box(1) | render' })]);
  const { modules, rootSource } = compileTree(t);

  assert.equal(rootSource, 'box(1) | render');
  assert.deepEqual(modules, {});
});

test('_root with children: rootSource side-effect-imports each enabled child', () => {
  const t = tree('r', [
    node({ id: 'r', name: '_root', source: '', children: ['a', 'b'] }),
    node({ id: 'a', name: 'a', source: 'render(box(1))' }),
    node({ id: 'b', name: 'b', source: 'render(box(2))' }),
  ]);
  const { modules, rootSource } = compileTree(t);

  assert.match(rootSource, /import \{ \} from "a"/);
  assert.match(rootSource, /import \{ \} from "b"/);
  assert.equal(modules.a, 'render(box(1))');
  assert.equal(modules.b, 'render(box(2))');
});

test('disabled child is not emitted and not imported by its parent', () => {
  const t = tree('r', [
    node({ id: 'r', name: '_root', children: ['a', 'b'] }),
    node({ id: 'a', name: 'a', source: 'render(box(1))' }),
    node({ id: 'b', name: 'b', source: 'render(box(2))', disabled: true }),
  ]);
  const { modules, rootSource } = compileTree(t);

  assert.ok(modules.a);
  assert.equal(modules.b, undefined);
  assert.match(rootSource, /import \{ \} from "a"/);
  assert.doesNotMatch(rootSource, /from "b"/);
});

test('deeply nested tree: each parent side-effect-imports each enabled child', () => {
  const t = tree('r', [
    node({ id: 'r', name: '_root', children: ['gp'] }),
    node({ id: 'gp', name: 'gp', children: ['p'] }),
    node({ id: 'p', name: 'p', children: ['leaf'] }),
    node({ id: 'leaf', name: 'leaf', source: 'render(box(1))' }),
  ]);
  const { modules, rootSource } = compileTree(t);

  assert.match(rootSource, /import \{ \} from "gp"/);
  assert.match(modules.gp, /import \{ \} from "p"/);
  assert.match(modules.p, /import \{ \} from "leaf"/);
  assert.equal(modules.leaf, 'render(box(1))');
});

test('node body is preserved verbatim after the side-effect imports', () => {
  // No transformation of the user's source — they own it entirely.
  const t = tree('r', [
    node({
      id: 'r',
      name: '_root',
      source: 'import { thing } from "child"\nrender(thing | scale(2))',
      children: ['c'],
    }),
    node({ id: 'c', name: 'child', source: 'export thing = box(1)' }),
  ]);
  const { rootSource } = compileTree(t);

  // The side-effect import comes first, then the user's verbatim source.
  const lines = rootSource.split('\n');
  assert.equal(lines[0], 'import { } from "child"');
  assert.equal(lines[1], 'import { thing } from "child"');
  assert.equal(lines[2], 'render(thing | scale(2))');
});
