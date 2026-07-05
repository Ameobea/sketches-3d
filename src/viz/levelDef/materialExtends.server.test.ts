// Run with: yarn tsx --test src/viz/levelDef/materialExtends.server.test.ts
import assert from 'node:assert/strict';
import { test } from 'node:test';

import { resolveMaterialExtends, type ExternalParentResolver } from './materialExtends.server';
import type { MaterialDefRaw, TextureDef } from './types';

const cs = (extra: Record<string, unknown>): MaterialDefRaw =>
  ({ type: 'customShader', ...extra }) as MaterialDefRaw;
const props = (m: MaterialDefRaw): Record<string, number> =>
  (m as unknown as { props: Record<string, number> }).props;

test('resolveMaterialExtends merges local, library, and geotoy parents uniformly', async () => {
  const materials: Record<string, MaterialDefRaw> = {
    base: cs({ props: { color: 0x111111, roughness: 0.8, metalness: 0.1 }, shaders: { colorShader: 'A' } }),
    child_local: cs({ extends: { type: 'local', name: 'base' }, props: { metalness: 0.9 } }),
    child_lib: cs({ extends: { type: 'library', path: 'procedural/x' }, props: { roughness: 0.2 } }),
    child_geo: cs({ extends: { type: 'geotoy', materialId: 42 }, props: { color: 0x222222 } }),
  };
  const textures: Record<string, TextureDef> = {};
  const resolveExternal: ExternalParentResolver = async (ref, tex) => {
    if (ref.type === 'library') {
      tex['__lib__/x'] = { url: 'u' } as TextureDef;
      return cs({ props: { color: 0x999999, roughness: 0.7, metalness: 0.3 } });
    }
    tex['__geotoy__/42'] = { url: 'g' } as TextureDef;
    return cs({ props: { color: 0x777777, roughness: 0.5, metalness: 0.6 } });
  };

  const out = await resolveMaterialExtends(materials, resolveExternal, textures);

  // local: inherits color+roughness from base, overrides only metalness; shader inherited; extends stripped
  assert.deepEqual(props(out.child_local), { color: 0x111111, roughness: 0.8, metalness: 0.9 });
  assert.equal((out.child_local as unknown as { shaders: { colorShader: string } }).shaders.colorShader, 'A');
  assert.equal('extends' in out.child_local, false);

  // library + geotoy: parent props merged, child override wins, pulled-in textures accumulated
  assert.deepEqual(props(out.child_lib), { color: 0x999999, roughness: 0.2, metalness: 0.3 });
  assert.equal(textures['__lib__/x'].url, 'u');
  assert.deepEqual(props(out.child_geo), { color: 0x222222, roughness: 0.5, metalness: 0.6 });
  assert.equal(textures['__geotoy__/42'].url, 'g');
});

test('resolveMaterialExtends detects cycles and unknown parents', async () => {
  const noop: ExternalParentResolver = async () => cs({});
  await assert.rejects(
    resolveMaterialExtends(
      { a: cs({ extends: { type: 'local', name: 'b' } }), b: cs({ extends: { type: 'local', name: 'a' } }) },
      noop,
      {}
    ),
    /cyclic/
  );
  await assert.rejects(
    resolveMaterialExtends({ a: cs({ extends: { type: 'local', name: 'missing' } }) }, noop, {}),
    /unknown material/
  );
});

test('resolveMaterialExtends rejects a non-customShader parent', async () => {
  const basicParent: ExternalParentResolver = async () =>
    ({ type: 'customBasicShader', props: {} }) as MaterialDefRaw;
  await assert.rejects(
    resolveMaterialExtends({ a: cs({ extends: { type: 'geotoy', materialId: 1 } }) }, basicParent, {}),
    /only customShader/
  );
});
