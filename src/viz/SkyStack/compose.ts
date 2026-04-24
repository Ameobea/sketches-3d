import type * as THREE from 'three';

import noiseShader from 'src/viz/shaders/noise.frag?raw';

import skyUnifiedPrelude from './shaders/skyUnified.prelude.frag?raw';
import type { BackgroundLayer, DefineContribution, Layer, SharedModule } from './types';
import { asUniformRecord, type SkyStackSharedUniforms } from './uniforms';

export interface ComposedSkyShader {
  fragmentShader: string;
  uniforms: Record<string, THREE.IUniform>;
}

const mergeDefines = (contributions: DefineContribution[]): Record<string, number> => {
  const byKey = new Map<string, { value: number; merge: 'max' | 'sum' }>();
  for (const c of contributions) {
    const prev = byKey.get(c.key);
    if (!prev) {
      byKey.set(c.key, { value: c.value, merge: c.merge });
      continue;
    }
    if (prev.merge !== c.merge) {
      throw new Error(`SkyStack define "${c.key}" has conflicting merge ops (${prev.merge} vs ${c.merge})`);
    }
    prev.value = c.merge === 'max' ? Math.max(prev.value, c.value) : prev.value + c.value;
  }
  const out: Record<string, number> = {};
  for (const [k, v] of byKey) {
    out[k] = v.value;
  }
  return out;
};

const dedupModules = (modules: SharedModule[]): string[] => {
  const seen = new Map<string, string>();
  for (const m of modules) {
    const prev = seen.get(m.key);
    if (prev === undefined) {
      seen.set(m.key, m.glsl);
    } else if (prev !== m.glsl) {
      throw new Error(`SkyStack shared module "${m.key}" has conflicting GLSL bodies`);
    }
  }
  return Array.from(seen.values());
};

/**
 * Emit a layer body wrapped in the saturation early-out and optional gate.
 * Renders as:
 *   // === <id> ===
 *   if (accumAlpha < SKY_SATURATION_ALPHA) {
 *     [if (gate)] { body [}]
 *   }
 * Background layers use this same wrapper (no gate) — the outer saturation
 * check is cheap insurance and costs nothing in the typical case where the
 * background is the thing that finally saturates the stack.
 *
 * When `layer.oversample` is true, the body runs 4× with RGSS-jittered view
 * directions and the accumulator delta is averaged. The gate still applies
 * per-sample (a jittered direction may cross the gate boundary).
 */
const emitLayerBody = (layer: Layer | BackgroundLayer, tag: string, gate?: string): string => {
  const inner = gate ? `if (${gate}) {\n${layer.body}\n    }` : layer.body;

  if (!layer.oversample) {
    return `
  // === ${layer.id}${tag} ===
  if (accumAlpha < SKY_SATURATION_ALPHA) {
    ${inner}
  }`;
  }

  // 2×2 RGSS (rotated grid supersampling). Save accumulator state, run body
  // 4× with jittered dir/elev/azimuth/cosElev, average the delta, restore.
  return `
  // === ${layer.id}${tag} (oversampled 2×2 RGSS) ===
  if (accumAlpha < SKY_SATURATION_ALPHA) {
    vec3 _ssSky = accumSkyColor;
    vec3 _ssEm  = accumEmissive;
    float _ssA  = accumAlpha;
    float _ssEA = accumEmissiveAlpha;

    vec3 _dSky = vec3(0.0);
    vec3 _dEm  = vec3(0.0);
    float _dA  = 0.0;
    float _dEA = 0.0;

    vec2 _px = 1.0 / vec2(textureSize(uSceneDepth, 0));

    // RGSS offsets — rotated grid gives better diagonal coverage than a regular 2×2 box.
    const vec2 _ssOff[4] = vec2[4](
      vec2( 0.125,  0.375),
      vec2( 0.375, -0.125),
      vec2(-0.125, -0.375),
      vec2(-0.375,  0.125)
    );

    for (int _ss = 0; _ss < 4; _ss++) {
      accumSkyColor      = _ssSky;
      accumEmissive      = _ssEm;
      accumAlpha         = _ssA;
      accumEmissiveAlpha = _ssEA;

      vec2 _jUv  = vUv + _ssOff[_ss] * _px;
      vec4 _jNdc = vec4(_jUv * 2.0 - 1.0, 1.0, 1.0);
      vec4 _jV   = uProjectionMatrixInverse * _jNdc;
      _jV /= _jV.w;
      dir = normalize((uCameraWorldMatrix * vec4(_jV.xyz, 0.0)).xyz);
      skyCoords(dir, elev, azimuth, cosElev);
      horizonBlend = smoothstep(-uHorizonBlend, uHorizonBlend, elev);
      aboveHorizon = elev >= -uHorizonBlend;

      ${inner}

      _dSky += accumSkyColor      - _ssSky;
      _dEm  += accumEmissive      - _ssEm;
      _dA   += accumAlpha         - _ssA;
      _dEA  += accumEmissiveAlpha - _ssEA;
    }

    accumSkyColor      = _ssSky + _dSky * 0.25;
    accumEmissive      = _ssEm  + _dEm  * 0.25;
    accumAlpha         = _ssA   + _dA   * 0.25;
    accumEmissiveAlpha = _ssEA  + _dEA  * 0.25;

    // Restore pixel-center dir for subsequent layers.
    dir = skyViewDir();
    skyCoords(dir, elev, azimuth, cosElev);
    horizonBlend = smoothstep(-uHorizonBlend, uHorizonBlend, elev);
    aboveHorizon = elev >= -uHorizonBlend;
  }`;
};

export const composeSkyShader = (
  shared: SkyStackSharedUniforms,
  layers: Layer[],
  background: BackgroundLayer | null
): ComposedSkyShader => {
  const seenIds = new Set<string>();
  const assertUnique = (id: string) => {
    if (seenIds.has(id)) {
      throw new Error(`SkyStack: duplicate layer id "${id}"`);
    }
    seenIds.add(id);
  };
  for (const l of layers) {
    assertUnique(l.id);
  }
  if (background) {
    assertUnique(background.id);
  }

  // Highest zIndex first = nearest to camera = emitted first in front-to-back order.
  const sorted = [...layers].sort((a, b) => b.zIndex - a.zIndex);

  const allUniforms: Record<string, THREE.IUniform> = { ...asUniformRecord(shared) };
  const allModules: SharedModule[] = [];
  const allDefines: DefineContribution[] = [];
  const instanceGlslParts: string[] = [];

  const contributors: (Layer | BackgroundLayer)[] = [...sorted];
  if (background) {
    contributors.push(background);
  }
  for (const c of contributors) {
    for (const [name, uniform] of Object.entries(c.uniforms)) {
      if (allUniforms[name] !== undefined && allUniforms[name] !== uniform) {
        throw new Error(`SkyStack: uniform name collision on "${name}" (layer "${c.id}")`);
      }
      allUniforms[name] = uniform;
    }
    if (c.modules) {
      allModules.push(...c.modules);
    }
    if (c.defines) {
      allDefines.push(...c.defines);
    }
    if (c.instanceGlsl) {
      instanceGlslParts.push(`// === ${c.id} ===\n${c.instanceGlsl}`);
    }
  }

  const defineLines = Object.entries(mergeDefines(allDefines))
    .map(([k, v]) => `#define ${k} ${v}`)
    .join('\n');
  const modulesGlsl = dedupModules(allModules).join('\n// === module boundary ===\n');
  const instanceGlsl = instanceGlslParts.join('\n// === instance boundary ===\n');

  const bodyParts: string[] = [];
  for (const l of sorted) {
    bodyParts.push(emitLayerBody(l, '', l.gate));
  }
  if (background) {
    bodyParts.push(emitLayerBody(background, ' (background)'));
  }

  const mainBody = `
void main() {
  // Occlusion discard — leaves oColor/oEmissive at cleared values for fragments
  // where scene geometry sits in front of the sky.
  discardIfOccluded();

  vec3 dir = skyViewDir();
  float elev, azimuth, cosElev;
  skyCoords(dir, elev, azimuth, cosElev);

  // Compositor-scope bindings available to every layer body + gate.
  float horizonBlend = smoothstep(-uHorizonBlend, uHorizonBlend, elev);
  bool aboveHorizon = elev >= -uHorizonBlend;

  // === layers (front to back) ===
${bodyParts.join('\n')}

  oColor = vec4(accumSkyColor, 1.0);
  oEmissive = vec4(accumEmissive, accumEmissiveAlpha);
}
`;

  const fragmentShader = [
    defineLines,
    noiseShader,
    skyUnifiedPrelude,
    modulesGlsl,
    instanceGlsl,
    mainBody,
  ].join('\n');

  return { fragmentShader, uniforms: allUniforms };
};
