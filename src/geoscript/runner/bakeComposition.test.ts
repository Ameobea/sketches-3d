// Headless composition bake + material-name resolution. Run with:
//   yarn tsx --test src/geoscript/runner/bakeComposition.test.ts

import assert from 'node:assert/strict';
import { test } from 'node:test';
import * as THREE from 'three';

import type { NodeDef, Transform3, TreeDef } from '../geotoyAPIClient';
import type { GeneratedObject } from './types';
import { bakeCompositionMeshes, resolveCompositionMaterial } from './bakeComposition';

const at = (x: number, y: number, z: number): Transform3 => ({
  pos: [x, y, z],
  rot: [0, 0, 0],
  scale: [1, 1, 1],
});

const node = (id: string, instances: Transform3[], children: string[] = []): NodeDef => ({
  id,
  name: id,
  source: '',
  instances: instances.map((t, i) => ({ ...t, id: `${id}-${i}` })),
  children,
});

const meshObj = (sourceModule: string, tx: number, materialName: string): GeneratedObject =>
  ({
    type: 'mesh',
    geometry: new THREE.BufferGeometry(),
    transform: new THREE.Matrix4().makeTranslation(tx, 0, 0),
    sourceModule,
    materialName,
    meshId: 0,
  }) as unknown as GeneratedObject;

const pos = (m: THREE.Matrix4) => new THREE.Vector3().setFromMatrixPosition(m).toArray();

test('bake composes ancestor instance world × mesh transform, once per instance copy', () => {
  // _root(1 @ origin) -> a(2 instances). One mesh rendered by `a`, plus a light + path to drop.
  const tree: TreeDef = {
    version: 1,
    rootId: '_root',
    globalsSource: '',
    nodes: {
      _root: node('_root', [at(0, 0, 0)], ['a']),
      a: node('a', [at(10, 0, 0), at(20, 0, 0)]),
    },
  };
  const objects: GeneratedObject[] = [
    meshObj('a', 5, 'red'),
    { type: 'light' } as unknown as GeneratedObject,
    { type: 'path' } as unknown as GeneratedObject,
  ];

  const baked = bakeCompositionMeshes(tree, objects);

  assert.equal(baked.length, 2); // one per `a` instance; light + path dropped
  assert.deepEqual(pos(baked[0].matrix), [15, 0, 0]); // 10 + 5
  assert.deepEqual(pos(baked[1].matrix), [25, 0, 0]); // 20 + 5
  assert.equal(baked[0].materialName, 'red');
});

test('material resolves map → auto-import → object material → undefined, with unmapped flag', () => {
  const names = new Set(['red', 'wood', 'default', '__comp:x:red']);
  // explicit map override wins
  assert.deepEqual(resolveCompositionMaterial(names, { red: 'wood' }, 'x', 'fallback', 'red'), {
    name: 'wood',
    unmapped: false,
  });
  // no override → auto-imported composition material (`__comp:<assetId>:<name>`)
  assert.deepEqual(resolveCompositionMaterial(names, {}, 'x', 'fallback', 'red'), {
    name: '__comp:x:red',
    unmapped: false,
  });
  // override points at a missing material → falls through to auto-import
  assert.deepEqual(resolveCompositionMaterial(names, { red: 'missing' }, 'x', undefined, 'red'), {
    name: '__comp:x:red',
    unmapped: false,
  });
  // no override, no auto entry → object material
  assert.deepEqual(resolveCompositionMaterial(names, {}, 'y', 'wood', 'unknown'), {
    name: 'wood',
    unmapped: true,
  });
  // nothing resolves → placeholder
  assert.deepEqual(resolveCompositionMaterial(names, {}, 'y', undefined, 'unknown'), {
    name: undefined,
    unmapped: true,
  });
});
