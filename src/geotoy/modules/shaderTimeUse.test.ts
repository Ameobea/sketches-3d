// Run with:
//   yarn tsx --test src/geotoy/modules/shaderTimeUse.test.ts

import assert from 'node:assert/strict';
import { test } from 'node:test';

import { materialIsAnimated } from './shaderTimeUse';

const def = (shaders: Record<string, string>) => ({ type: 'customShader', name: 't', shaders }) as any;

const SIGNATURE_ONLY = `
vec4 getFragColor(vec3 baseColor, vec3 pos, vec3 normal, float curTimeSeconds, SceneCtx ctx) {
  return vec4(baseColor, 1.);
}`;
const body = (stmt: string) => SIGNATURE_ONLY.replace('return vec4(baseColor, 1.);', stmt);

test('a slot animates only when it reads the uniform outside its own signature', () => {
  assert.equal(materialIsAnimated(def({ colorShader: SIGNATURE_ONLY })), false);
  assert.equal(
    materialIsAnimated(def({ colorShader: body('return vec4(baseColor * sin(curTimeSeconds), 1.);') })),
    true
  );
  // Comments never count, in either syntax, including a `/*` nested in a `//` line.
  assert.equal(
    materialIsAnimated(
      def({
        colorShader: body('// t = curTimeSeconds;\n  /* curTimeSeconds */\n  return vec4(baseColor, 1.);'),
      })
    ),
    false
  );
  assert.equal(
    materialIsAnimated(
      def({
        colorShader: body(
          '// fix /* the ramp\n  float t = curTimeSeconds;\n  return vec4(baseColor * t, 1.);'
        ),
      })
    ),
    true
  );
  assert.equal(materialIsAnimated({ type: 'physical', name: 'x' } as any), false);
});

test('every declaring slot is accounted for', () => {
  assert.equal(
    materialIsAnimated(
      def({
        colorShader: SIGNATURE_ONLY,
        roughnessShader:
          'float getCustomRoughness(vec3 p, vec3 n, float r, float curTimeSeconds, SceneCtx ctx) { return r; }',
        pomHeightShader: 'float getPomHeight(vec3 pos, vec3 normal, float curTimeSeconds) { return 0.; }',
      })
    ),
    false
  );
  assert.equal(
    materialIsAnimated(
      def({
        emissiveShader: `vec3 getCustomEmissive(vec3 pos, vec3 e, float curTimeSeconds, SceneCtx ctx) {
          return e * (0.5 + 0.5 * sin(curTimeSeconds));
        }`,
      })
    ),
    true
  );
});

test('the shared chunk is conservative — any mention counts, since every slot can call it', () => {
  assert.equal(
    materialIsAnimated(
      def({ commonShader: 'float wobble(float curTimeSeconds) { return curTimeSeconds; }' })
    ),
    true
  );
  assert.equal(materialIsAnimated(def({ commonShader: 'float sq(float x) { return x * x; }' })), false);
});
