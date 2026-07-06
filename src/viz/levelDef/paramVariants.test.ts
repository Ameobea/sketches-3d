// Run with: yarn tsx --test src/viz/levelDef/paramVariants.test.ts
import assert from 'node:assert/strict';
import { test } from 'node:test';

import { expandParamVariants, variantAssetId } from './paramVariants';
import type { AssetDef, ObjectDef } from './types';

const tower: AssetDef = {
  type: 'geoscript',
  code: 'n = input_int("segments", default=10)\nexport mesh = box(1)',
  inputs: { segments: { type: 'int', value: 10 } },
};
const assets: Record<string, AssetDef> = {
  tower,
  plain: { type: 'geoscript', code: 'export mesh = box(1)' },
};

const obj = (id: string, asset: string, inputs?: ObjectDef['inputs']): ObjectDef => ({ id, asset, inputs });

test('objects without inputs (or matching asset defaults) resolve to the authored asset id', () => {
  const pv = expandParamVariants(assets, [
    obj('a', 'tower'),
    obj('b', 'tower', { segments: { type: 'int', value: 10 } }),
    obj('c', 'plain'),
  ]);
  assert.equal(pv.variantIds.size, 0);
  assert.equal(pv.effectiveAssetId(obj('a', 'tower')), 'tower');
  assert.equal(pv.effectiveAssetId(obj('b', 'tower', { segments: { type: 'int', value: 10 } })), 'tower');
});

test('distinct merged inputs synthesize deduped variants merged over asset-level defaults', () => {
  const base: AssetDef = { ...tower, inputs: { ...tower.inputs!, r: { type: 'float', value: 1 } } };
  const pv = expandParamVariants({ tower: base }, [
    obj('a', 'tower', { segments: { type: 'int', value: 14 } }),
    obj('b', 'tower', { segments: { type: 'int', value: 14 } }),
    obj('c', 'tower', { segments: { type: 'int', value: 3 } }),
  ]);
  assert.equal(pv.variantIds.size, 2);
  assert.deepEqual(pv.variantsByBase.get('tower')?.length, 2);

  const vidA = pv.effectiveAssetId(obj('a', 'tower', { segments: { type: 'int', value: 14 } }));
  const vidB = pv.effectiveAssetId(obj('b', 'tower', { segments: { type: 'int', value: 14 } }));
  const vidC = pv.effectiveAssetId(obj('c', 'tower', { segments: { type: 'int', value: 3 } }));
  assert.equal(vidA, vidB);
  assert.notEqual(vidA, vidC);

  // Variant def carries merged inputs: object override + untouched asset-level default.
  const vdef = pv.assets[vidA];
  assert.equal(vdef.type, 'geoscript');
  assert.deepEqual((vdef as typeof base).inputs, {
    segments: { type: 'int', value: 14 },
    r: { type: 'float', value: 1 },
  });
});

test('variant ids are stable under input key ordering', () => {
  const i1 = { a: { type: 'int', value: 1 }, b: { type: 'int', value: 2 } } as const;
  const i2 = { b: { type: 'int', value: 2 }, a: { type: 'int', value: 1 } } as const;
  assert.equal(variantAssetId('x', { ...i1 }), variantAssetId('x', { ...i2 }));
});

test('effectiveAssetId works for defs not present at expansion time (paste/spawn)', () => {
  const pv = expandParamVariants(assets, [obj('a', 'tower', { segments: { type: 'int', value: 14 } })]);
  const pasted = obj('freshly_pasted', 'tower', { segments: { type: 'int', value: 14 } });
  assert.equal(pv.effectiveAssetId(pasted), pv.variantsByBase.get('tower')![0]);
});

test('synthesize rebuilds a variant def from an updated base', () => {
  const pv = expandParamVariants(assets, [obj('a', 'tower', { segments: { type: 'int', value: 14 } })]);
  const vid = pv.variantsByBase.get('tower')![0];
  const updatedBase: AssetDef = { ...tower, code: 'export mesh = box(2)' };
  const re = pv.synthesize(updatedBase, vid);
  assert.equal((re as { code: string }).code, 'export mesh = box(2)');
  assert.deepEqual((re as typeof tower).inputs, { segments: { type: 'int', value: 14 } });
});
