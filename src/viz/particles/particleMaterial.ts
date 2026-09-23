import * as THREE from 'three';

import type { ShaderConstantJson } from 'src/viz/materials/schema';
import type { CustomUniformDef } from 'src/viz/shaders/customShader.types';
import type { ExposureGate, ParticleGate, ParticleSystemDef, ParticleVolume } from './schema';
import FRAG from './shaders/particle.frag?raw';
import VERT from './shaders/particle.vert?raw';

export interface ParticlePipelineCtx {
  fogShader?: string;
  hasEmissiveRT: boolean;
  /** Sampled by `softDepth` systems, which therefore can't render with it attached. */
  sceneDepth: THREE.Texture;
}

export const glslFloat = (v: number) => (Number.isInteger(v) ? `${v}.0` : `${v}`);

const uniformDecls = (uniforms: Record<string, CustomUniformDef>) =>
  Object.entries(uniforms)
    .map(
      ([name, def]) => `uniform ${def.type === 'sampler2DArray' ? 'highp sampler2DArray' : def.type} ${name};`
    )
    .join('\n');

const constantDefines = (constants: Record<string, ShaderConstantJson>) =>
  Object.entries(constants)
    .map(([name, c]) => {
      const v = c.value;
      const body =
        c.type === 'float'
          ? glslFloat(v as number)
          : c.type === 'int' || c.type === 'bool'
            ? `${v}`
            : `${c.type}(${(v as number[]).map(glslFloat).join(', ')})`;
      return `#define ${name} (${body})`;
    })
    .join('\n');

export const isExposureGate = (g: ParticleGate): g is ExposureGate =>
  g.gate === 'skyExposed' || g.gate === 'floorExposed';

const gateValue = (g: ParticleGate, i: number) => {
  const [a, b] = g.range.map(glslFloat);
  if (isExposureGate(g)) {
    const clearance = `pClearance(pFieldMap${i}, pFieldMatrix${i}, pFieldDepth${i}, ${glslFloat(g.soften ?? 1)}, wpos, id)`;
    return `smoothstep(${a}, ${b}, ${clearance})`;
  }
  const ss = `smoothstep(${a}, ${b}, dist)`;
  return g.gate === 'nearFade' ? ss : `(1.0 - ${ss})`;
};

const gateGlsl = (gates: ParticleGate[]) =>
  gates
    .map((g, i) => {
      const value = gateValue(g, i);
      switch (g.affects ?? 'alpha') {
        case 'alpha':
          return `alpha *= ${value};`;
        case 'size':
          return `size *= ${value};`;
        case 'density':
          return `if (pHash(id ^ ${0xa511e9b3 + i * 0x9e3779b9}u) > ${value}) size = 0.0;`;
      }
    })
    .join('\n  ');

const fieldDecls = (gates: ParticleGate[]) =>
  gates
    .map((g, i) =>
      isExposureGate(g)
        ? `uniform highp sampler2D pFieldMap${i};\nuniform mat4 pFieldMatrix${i};\nuniform float pFieldDepth${i};`
        : ''
    )
    .join('\n');

const distributeGlsl = (volume: ParticleVolume) => {
  const d = volume.kind === 'cameraSlab' ? volume.distributionY : undefined;
  if (!d) return '';
  const p = glslFloat(d.power);
  switch (d.toward) {
    case 'bottom':
      return `u.y = pow(u.y, ${p});`;
    case 'top':
      return `u.y = 1.0 - pow(u.y, ${p});`;
    case 'center':
      return `{ float v = u.y * 2.0 - 1.0; u.y = 0.5 + 0.5 * sign(v) * pow(abs(v), ${p}); }`;
  }
};

const edgeFadeVec = ({ size, edgeFade }: ParticleVolume) => {
  const f = edgeFade ?? Math.min(...size) * 0.1;
  const v = typeof f === 'number' ? new THREE.Vector3(f, f, f) : new THREE.Vector3().fromArray(f);
  return v.max(new THREE.Vector3().setScalar(1e-3));
};

const ORIENT_GLSL = {
  billboard: `
  float cr = cos(p.rot), sr = sin(p.rot);
  vec2 r = vec2(corner.x * cr - corner.y * sr, corner.x * sr + corner.y * cr) * size;
  offset = camRight * r.x + camUp * r.y;`,
  velocityStretch: `
  vec3 vel = (p.pos - motion(seed, age - 0.02, origin, params).pos) * 50.0;
  float speed = length(vel);
  vec3 axis = speed > 1e-5 ? vel / speed : camUp;
  vec3 side = cross(axis, normalize(cameraPosition - wpos));
  float sideLen = length(side);
  side = sideLen > 1e-4 ? side / sideLen : camRight;
  offset = side * (corner.x * size) + axis * (corner.y * max(size, speed * pStretch));`,
};

export class ParticleMaterial extends THREE.ShaderMaterial {
  constructor(
    def: ParticleSystemDef,
    customUniforms: Record<string, CustomUniformDef>,
    ctx: ParticlePipelineCtx
  ) {
    const output = def.output ?? 'scene';
    if (output === 'emissive' && !ctx.hasEmissiveRT) {
      throw new Error(`particle system "${def.id}": output "emissive" needs an emissive bypass pipeline`);
    }
    const hasFog = !!ctx.fogShader && output === 'scene';
    const additive = def.blend === 'additive' || output === 'emissive';

    const defines: Record<string, string> = {};
    if (ctx.hasEmissiveRT) defines.HAS_EMISSIVE_RT = '1';
    if (hasFog) defines.HAS_FOG = '1';
    if (output === 'emissive') defines.OUTPUT_EMISSIVE = '1';
    if (def.softDepth) defines.SOFT_DEPTH = glslFloat(def.softDepth);
    if (def.renderScale) {
      if (!def.softDepth || output !== 'scene') {
        throw new Error(`particle system "${def.id}": renderScale needs softDepth and scene output`);
      }
      defines.RENDER_SCALE_INV = glslFloat(1 / def.renderScale);
    }

    const decls = [constantDefines(def.shaders.constants ?? {}), uniformDecls(customUniforms)].join('\n');
    const vertexShader = VERT.replace('//__CUSTOM_UNIFORMS__', `${decls}\n${fieldDecls(def.gates ?? [])}`)
      .replace('//__FOG_SHADER__', hasFog ? ctx.fogShader! : '')
      .replace('//__DISTRIBUTE__', distributeGlsl(def.volume))
      .replace('//__MOTION__', def.shaders.motion)
      .replace('//__GATES__', gateGlsl(def.gates ?? []))
      .replace(
        '//__LIGHT__',
        def.light === 'sunGated' ? 'rgb *= pAmbient + pSunColor * pSunVisibility(wpos);' : ''
      )
      .replace('//__ORIENT__', ORIENT_GLSL[def.orient ?? 'billboard']);
    const fragmentShader = FRAG.replace('//__CUSTOM_UNIFORMS__', decls).replace(
      '//__SPRITE__',
      def.shaders.sprite
    );

    const uniforms: Record<string, THREE.IUniform> = {
      pTime: { value: 0 },
      pPlayerPos: { value: new THREE.Vector3() },
      pBoxSize: { value: new THREE.Vector3().fromArray(def.volume.size) },
      pBoxCenter: { value: new THREE.Vector3() },
      pBoxEdgeFade: { value: edgeFadeVec(def.volume) },
      pLifetime: { value: def.lifetime ?? 0 },
      pStretch: { value: def.stretch ?? 0.05 },
      pPxRange: { value: new THREE.Vector2(def.sizePx?.min ?? 1.5, def.sizePx?.max ?? 128) },
      pViewportHeight: { value: 1 },
      pAmbient: { value: new THREE.Color(0) },
      pSunColor: { value: new THREE.Color(0) },
      pSunShadowMap: { value: null },
      pSunShadowMatrix: { value: new THREE.Matrix4() },
      pSunShadowOn: { value: 0 },
      pSceneDepth: { value: ctx.sceneDepth },
      pNearFar: { value: new THREE.Vector2(0.1, 1000) },
    };
    for (const [name, u] of Object.entries(customUniforms)) uniforms[name] = { value: u.value };

    super({
      name: `ParticleMaterial:${def.id}`,
      glslVersion: THREE.GLSL3,
      defines,
      uniforms,
      vertexShader,
      fragmentShader,
      transparent: true,
      depthTest: !def.softDepth,
      depthWrite: false,
      side: THREE.DoubleSide,
      blending: THREE.CustomBlending,
      blendEquation: THREE.AddEquation,
      blendSrc: THREE.OneFactor,
      blendDst: additive ? THREE.OneFactor : THREE.OneMinusSrcAlphaFactor,
      blendSrcAlpha: THREE.OneFactor,
      blendDstAlpha: additive ? THREE.OneFactor : THREE.OneMinusSrcAlphaFactor,
    });
  }
}
