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
 */
const emitLayerBody = (layer: Layer | BackgroundLayer, tag: string, gate?: string): string => {
  const inner = gate ? `if (${gate}) {\n${layer.body}\n    }` : layer.body;
  return `
  // === ${layer.id}${tag} ===
  if (accumAlpha < SKY_SATURATION_ALPHA) {
    ${inner}
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
