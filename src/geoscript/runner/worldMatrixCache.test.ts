// Cartesian expansion of node instances into per-copy world matrices. Run with:
//   yarn tsx --test src/geoscript/runner/worldMatrixCache.test.ts

import assert from 'node:assert/strict';
import { test } from 'node:test';

import type { NodeDef, Transform3, TreeDef } from '../geotoyAPIClient';
import { buildParentMap } from 'src/viz/scenes/geoscriptPlayground/treeOps';
import { buildWorldMatrixCache } from './worldMatrixCache';

const at = (x: number, y: number, z: number): Transform3 => ({
  pos: [x, y, z],
  rot: [0, 0, 0],
  scale: [1, 1, 1],
});

const node = (id: string, instances: Transform3[], children: string[] = []): NodeDef => ({
  id,
  name: id,
  source: '',
  instances,
  children,
});

const findPath = (cache: ReturnType<typeof buildWorldMatrixCache>, id: string, path: number[]) =>
  cache.get(id)!.find(e => e.path.length === path.length && e.path.every((v, i) => v === path[i]))!;

test('nested instances expand to the cartesian product of ancestor counts', () => {
  // _root(1) -> a(2) -> b(3); copies: root 1, a 2, b 6.
  const tree: TreeDef = {
    version: 1,
    rootId: 'r',
    globalsSource: '',
    nodes: {
      r: node('r', [at(0, 0, 0)], ['a']),
      a: node('a', [at(10, 0, 0), at(20, 0, 0)], ['b']),
      b: node('b', [at(0, 1, 0), at(0, 2, 0), at(0, 3, 0)]),
    },
  };
  const cache = buildWorldMatrixCache(tree, buildParentMap(tree));

  assert.equal(cache.get('r')!.length, 1);
  assert.equal(cache.get('a')!.length, 2);
  assert.equal(cache.get('b')!.length, 6);

  // Pure translations compose additively: copy [0,1,2] = root[0] · a[1] · b[2] = (20,3,0).
  const copy = findPath(cache, 'b', [0, 1, 2]);
  const p = copy.world.elements; // column-major; translation is elements 12,13,14
  assert.equal(p[12], 20);
  assert.equal(p[13], 3);
  assert.equal(p[14], 0);

  // Every b copy's path is [0, ai, bj].
  const paths = cache.get('b')!.map(e => e.path.join(','));
  assert.deepEqual(paths.sort(), ['0,0,0', '0,0,1', '0,0,2', '0,1,0', '0,1,1', '0,1,2']);
});

test('single-instance tree matches the legacy one-copy-per-node shape', () => {
  const tree: TreeDef = {
    version: 1,
    rootId: 'r',
    globalsSource: '',
    nodes: {
      r: node('r', [at(0, 0, 0)], ['a']),
      a: node('a', [at(5, 0, 0)]),
    },
  };
  const cache = buildWorldMatrixCache(tree, buildParentMap(tree));
  assert.equal(cache.get('a')!.length, 1);
  assert.deepEqual(cache.get('a')![0].path, [0, 0]);
  assert.equal(cache.get('a')![0].world.elements[12], 5);
});
