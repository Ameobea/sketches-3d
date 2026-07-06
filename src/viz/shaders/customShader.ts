import * as THREE from 'three';
import { UniformsLib } from 'three';

import commonShaderCode from './common.frag?raw';
import proceduralMaterialAACode from './proceduralMaterialAA.glsl?raw';
import proceduralMaterialGridCode from './proceduralMaterialGrid.glsl?raw';
import softOcclusionPreamble from './softOcclusionPreamble.frag?raw';
import softOcclusionDiscard from './softOcclusionDiscard.frag?raw';
import CustomLightsFragmentBegin from './customLightsFragmentBegin.frag?raw';
import GeneratedUVsFragment from './generatedUVs.vert?raw';
import depthExactVertexBody from './depthExactVertex.glsl?raw';
import noiseShaders from './noise.frag?raw';
import tileBreakingNeyretFragment from './tileBreakingNeyret.frag?raw';
import { buildTriplanarDefsFragment, type TriplanarMappingParams } from './triplanarMapping';
import { buildReverseColorRampGenerator, ReverseColorRampCommonFunctions } from './reverseColorRamp';
import {
  buildPomDefs,
  buildPomMainBlock,
  buildPomUniformDecls,
  buildPomNormalApply,
  buildPomSelfShadowApply,
  buildPomHeightSources,
  buildPomDebug,
  POM_BOUNDED_SILHOUETTE_FLAG,
  type PomTexturing,
} from './pom';
import { MaterialClass } from './customShader.types';
import type {
  AmbientDistanceAmpParams,
  CustomShaderProps,
  CustomShaderShaders,
  CustomShaderOptions,
} from './customShader.types';
import { buildHeightAlphaEarlyOut, buildHeightAlphaFragment } from './heightAlpha';
import { getTextureMeanColor } from './meanTextureColor';
import VERTEX_LIGHTING_FRAGMENT from './vertexLighting.frag?raw';
import PLAYER_SHADOW_FRAGMENT from './playerShadow.frag?raw';

export { MaterialClass } from './customShader.types';
export type {
  AmbientDistanceAmpParams,
  CustomShaderProps,
  CustomShaderShaders,
  CustomShaderOptions,
  CustomUniformDef,
} from './customShader.types';

// import noise2Shaders from './noise2.frag?raw';
const noise2Shaders = 'DISABLED TO SAVE SPACE';

const DEFAULT_MAP_DISABLE_DISTANCE = 2000;

/**
 * Builds a GLSL expression for sampling a texture using the configured tile-breaking mode.
 * Does not include a swizzle — append `.xyz` etc. as needed at the call site.
 * `mean` is a GLSL expression for the sampler's precomputed mean color; it defaults to the
 * `<sampler>MeanColor` uniform naming convention.
 */
const buildTileBreakSampleExpr = (
  sampler: string,
  uv: string,
  tileBreaking: CustomShaderOptions['tileBreaking'],
  mean: string = `${sampler}MeanColor`
): string =>
  tileBreaking
    ? /* glsl */ `textureNoTileNeyret(${sampler}, ${uv}, ${mean})`
    : /* glsl */ `texture2D(${sampler}, ${uv})`;

const MAX_ANISO_TAPS = 6;

// Anisotropic in-plane oversampling for the procedural color + roughness shaders.
// The footprint is built analytically (not from `dFdx`/`dFdy`, which are
// unreliable on the POM `discard` path): an in-plane ellipse with minor axis ≈
// `unitsPerPx` and major axis stretched by `1/NdotV` along the view direction.
// Taps spread along the major axis (where grazing aliasing lives) × a 2-wide
// minor pair, so head-on fragments cost 4 taps and only grazing ones pay more.
const buildAnisotropicOversample = (opts: {
  basePos: string;
  accType: 'vec4' | 'float';
  sampleExpr: (offsetPos: string) => string;
  resultStmt: (acc: string) => string;
}): string => {
  const { basePos, accType, sampleExpr, resultStmt } = opts;
  const zero = accType === 'vec4' ? 'vec4(0.)' : '0.';
  return /* glsl */ `
  {
    vec3 _aaN = normalize(vWorldNormal);
    vec3 _aaV = normalize(vWorldPos - cameraPosition);
    float _aaNdotV = max(abs(dot(_aaN, _aaV)), 1e-3);
    float _aaAniso = clamp(1. / _aaNdotV, 1., float(${MAX_ANISO_TAPS}));
    if (_aaAniso < 1.2) {
      // Near head-on the footprint is sub-pixel; a single tap is indistinguishable.
      ${accType} _aaAcc = ${sampleExpr(basePos)};
      ${resultStmt('_aaAcc')}
    } else {
    vec3 _aaProj = _aaV - dot(_aaV, _aaN) * _aaN;
    float _aaProjLen = length(_aaProj);
    vec3 _aaUp = abs(_aaN.y) < 0.99 ? vec3(0., 1., 0.) : vec3(1., 0., 0.);
    vec3 _aaU = _aaProjLen > 1e-4 ? _aaProj / _aaProjLen : normalize(cross(_aaN, _aaUp));
    vec3 _aaW = cross(_aaN, _aaU);
    float _aaMajor = unitsPerPx * _aaAniso;
    float _aaMinor = unitsPerPx;
    int _aaCount = clamp(int(ceil(_aaAniso)), 2, ${MAX_ANISO_TAPS});
    float _aaCountF = float(_aaCount);
    ${accType} _aaAcc = ${zero};
    for (int _i = 0; _i < ${MAX_ANISO_TAPS}; _i++) {
      if (_i >= _aaCount) { break; }
      float _aaT = float(_i) / (_aaCountF - 1.) - 0.5;
      for (int _j = 0; _j < 2; _j++) {
        float _aaS = (float(_j) - 0.5) * 0.5;
        vec3 _aaP = ${basePos} + _aaU * (_aaT * _aaMajor) + _aaW * (_aaS * _aaMinor);
        _aaAcc += ${sampleExpr('_aaP')};
      }
    }
    _aaAcc /= (_aaCountF * 2.);
    ${resultStmt('_aaAcc')}
    }
  }`;
};

const buildRoughnessShaderFragment = (antialiasRoughnessShader?: boolean) => {
  if (antialiasRoughnessShader) {
    return buildAnisotropicOversample({
      basePos: 'vWorldPos',
      accType: 'float',
      sampleExpr: P => `getCustomRoughness(${P}, vObjectNormal, roughnessFactor, curTimeSeconds, ctx)`,
      resultStmt: acc => `roughnessFactor = ${acc};`,
    });
  }

  return /* glsl */ `roughnessFactor = getCustomRoughness(vWorldPos, vObjectNormal, roughnessFactor, curTimeSeconds, ctx);`;
};

const buildUnpackDiffuseNormalGBAFragment = (params: true | { lut: Uint8Array }): string => {
  if (params === true) {
    return /* glsl */ `
    mapN = sampledDiffuseColor_.gba;
    sampledDiffuseColor_ = vec4(sampledDiffuseColor_.rrr, 1.);
  `;
  } else {
    return /* glsl */ `
    mapN = sampledDiffuseColor_.gba;
    float index = sampledDiffuseColor_.r;
    vec4 lutEntry = texelFetch(diffuseLUT, ivec2(index * 255., 0), 0);
    sampledDiffuseColor_ = lutEntry;
      `;
  }
};

const hashSeedToVec2GLSL = /* glsl */ `
vec2 hashSeedToVec2(float seed) {
  return vec2(
    fract(sin(seed * 127.1 + 311.7) * 43758.5453),
    fract(sin(seed * 269.5 + 183.3) * 43758.5453)
  );
}`;

const hashSeedToVec3GLSL = /* glsl */ `
vec3 hashSeedToVec3(float seed) {
  return vec3(
    fract(sin(seed * 127.1 + 311.7) * 43758.5453),
    fract(sin(seed * 269.5 + 183.3) * 43758.5453),
    fract(sin(seed * 419.2 + 271.9) * 43758.5453)
  );
}`;

const buildUVVertexFragment = (
  randomizeUVOffset: boolean | undefined,
  uvAlreadyTransformed: boolean
): string => {
  if (!randomizeUVOffset) {
    return '';
  }

  // When `vUv` already went through `uvTransform` (generated UVs), only the random
  // offset is added; otherwise scale + offset are applied together here.
  if (uvAlreadyTransformed) {
    return /* glsl */ `
      #ifdef USE_UV
        vUv += hashSeedToVec2(uvOffsetSeed);
      #endif
      `;
  }

  return /* glsl */ `
      #ifdef USE_UV
        vec2 uvOffset = hashSeedToVec2(uvOffsetSeed);
        float uvScaleX = uvTransform[0][0];
        float uvScaleY = uvTransform[1][1];
        mat3 newUVTransform = mat3(
          uvScaleX, 0., 0.,
          0., uvScaleY, 0.,
          uvOffset.x, uvOffset.y, 1.
        );
        vUv = ( newUVTransform * vec3( vUv, 1 ) ).xy;
      #endif
      `;
};

const buildRunColorShaderFragment = (
  colorShader: string | undefined,
  antialiasColorShader: boolean | undefined,
  pomActive: boolean
): string => {
  if (!colorShader) {
    return '';
  }

  // Under POM the procedural shader must see the displaced hit + floor normal.
  const posSym = pomActive ? '_pomHit' : 'vWorldPos';
  const normalSym = pomActive ? '_pomNormalW' : 'vObjectNormal';

  if (antialiasColorShader) {
    return buildAnisotropicOversample({
      basePos: posSym,
      accType: 'vec4',
      sampleExpr: P => `getFragColor(diffuseColor.xyz, ${P}, ${normalSym}, curTimeSeconds, ctx)`,
      resultStmt: acc => `diffuseColor = ${acc};\n    ctx.diffuseColor = diffuseColor;`,
    });
  } else {
    return /* glsl */ `
  diffuseColor = getFragColor(diffuseColor.xyz, ${posSym}, ${normalSym}, curTimeSeconds, ctx);
  ctx.diffuseColor = diffuseColor;`;
  }
};

// Scales direct/indirect light by the slot's `(directMul, indirectMul)`. Emitted
// after `<lights_fragment_end>` so it reaches specular too, not just albedo.
const buildRunLightAttenuationFragment = (
  lightAttenuationShader: string | undefined,
  pomActive: boolean
): string => {
  if (!lightAttenuationShader) {
    return '';
  }
  const posSym = pomActive ? '_pomHit' : 'vWorldPos';
  const normalSym = pomActive ? '_pomNormalW' : 'vObjectNormal';
  return /* glsl */ `
  {
    vec2 _lightAtten = getLightAttenuation(${posSym}, ${normalSym}, curTimeSeconds, ctx);
    reflectedLight.directDiffuse *= _lightAtten.x;
    reflectedLight.directSpecular *= _lightAtten.x;
    reflectedLight.indirectDiffuse *= _lightAtten.y;
    reflectedLight.indirectSpecular *= _lightAtten.y;
  }`;
};

const buildRunIridescenceShaderFragment = (iridescenceShader: string | undefined): string => {
  if (!iridescenceShader) {
    return '';
  }

  return /* glsl */ `
material.iridescence = getCustomIridescence(vWorldPos, vObjectNormal, material.iridescence, curTimeSeconds, ctx);`;
};

const buildTextureDisableFragment = (
  mapDisableDistance: number | null | undefined,
  mapDisableTransitionThreshold: number
): string => {
  if (typeof mapDisableDistance !== 'number') {
    return '';
  }

  const startEdge = (mapDisableDistance - mapDisableTransitionThreshold).toFixed(3);
  const endEdge = mapDisableDistance.toFixed(3);

  return /* glsl */ `
    float textureActivation = 1. - smoothstep(${startEdge}, ${endEdge}, texDisableDistance);
  `;
};

const buildLightsFragmentBegin = (
  disabledDirectionalLightIndices: number[] | undefined,
  disabledSpotLightIndices: number[] | undefined,
  ambientLightScale: number,
  ambientDistanceAmp: AmbientDistanceAmpParams | undefined
): string => {
  let frag = CustomLightsFragmentBegin.replace(
    '__DIR_LIGHTS_DISABLE__',
    (() => {
      if (!disabledDirectionalLightIndices) {
        return '0';
      }

      return disabledDirectionalLightIndices.map(i => `UNROLLED_LOOP_INDEX == ${i.toFixed(0)}`).join(' || ');
    })()
  )
    .replace(
      '__SPOT_LIGHTS_DISABLE__',
      (() => {
        if (!disabledSpotLightIndices) {
          return '0';
        }

        return disabledSpotLightIndices.map(i => `UNROLLED_LOOP_INDEX == ${i.toFixed(0)}`).join(' || ');
      })()
    )
    .replaceAll('__AMBIENT_LIGHT_SCALE__', ambientLightScale.toFixed(4))
    .replace('__USE_AMBIENT_LIGHT_DISTANCE_AMP__', ambientDistanceAmp ? '1' : '0');

  if (ambientDistanceAmp) {
    frag = frag
      .replace(
        '__AMBIENT_LIGHT_DISTANCE_AMP_FALLOFF_START_DISTANCE__',
        ambientDistanceAmp.falloffStartDistance.toFixed(4)
      )
      .replace(
        '__AMBIENT_LIGHT_DISTANCE_AMP_FALLOFF_END_DISTANCE__',
        ambientDistanceAmp.falloffEndDistance.toFixed(4)
      )
      .replace('__AMBIENT_LIGHT_DISTANCE_AMP_EXPONENT__', (ambientDistanceAmp.exponent ?? 1).toFixed(4))
      .replace('__AMBIENT_LIGHT_DISTANCE_AMP_FACTOR__', ambientDistanceAmp.ampFactor.toFixed(4));
  }

  return frag;
};

export interface PlayerShadowParams {
  radius: number;
  /** Shadow darkness. Default: 0.85 */
  intensity: number;
}

interface CustomShaderGlobalConfig {
  ambientDistanceAmp?: AmbientDistanceAmpParams;
}

let globalConfig: CustomShaderGlobalConfig = {};
let occlusionBackfaceRenderingEnabled = false;

// Which face custom-shader materials cast into shadow maps. `null` keeps three's default
// (back faces for a FrontSide material), which produces a second-depth "peter-panning" gap at
// wall/floor contacts. `THREE.DoubleSide` casts the near face and closes that gap. When set, it
// is pinned here so the third-person occlusion toggle can't revert it mid-scene. Per-scene
// opt-in via setShadowCastSide; reset on scene teardown.
let shadowCastSideOverride: THREE.Side | null = null;

const setMaterialOcclusionBackfaceRendering = (mat: CustomShaderMaterial, enable: boolean) => {
  const targetSide = enable ? THREE.DoubleSide : THREE.FrontSide;
  const targetShadowSide = shadowCastSideOverride ?? (enable ? THREE.BackSide : null);

  if (mat.side !== targetSide || mat.shadowSide !== targetShadowSide) {
    mat.side = targetSide;
    mat.shadowSide = targetShadowSide;
    mat.needsUpdate = true;
  }
};

export const configureCustomShaderGlobals = (config: Partial<CustomShaderGlobalConfig>) => {
  if ('ambientDistanceAmp' in config) {
    globalConfig.ambientDistanceAmp = config.ambientDistanceAmp ?? undefined;
  }
};

export const setOcclusionBackfaceRendering = (scene: THREE.Scene, enable: boolean) => {
  occlusionBackfaceRenderingEnabled = enable;
  scene.traverse(obj => {
    const materials = (obj as THREE.Mesh).material;
    for (const mat of Array.isArray(materials) ? materials : [materials]) {
      if (mat instanceof CustomShaderMaterial && !mat.userData.occlusionExclude) {
        setMaterialOcclusionBackfaceRendering(mat, enable);
      }
    }
  });
};

/**
 * Sets the face all custom-shader materials cast into shadow maps and pins it through the
 * occlusion toggle. `THREE.DoubleSide` closes the second-depth ("peter-panning") gap at the
 * base of walls by casting the near face; `null` restores legacy back-face casting. Newly
 * built materials pick this up automatically (see the `setMaterialOcclusionBackfaceRendering`
 * call in `buildCustomShader`), so setting it early also covers geometry whose materials
 * resolve asynchronously. Pair with a texel-scaled `normalBias` on the casting light
 * (`deriveDirectionalShadowNormalBias`) to suppress the self-shadow acne front-face casting
 * reintroduces.
 */
export const setShadowCastSide = (scene: THREE.Scene, side: THREE.Side | null) => {
  shadowCastSideOverride = side;
  scene.traverse(obj => {
    const materials = (obj as THREE.Mesh).material;
    for (const mat of Array.isArray(materials) ? materials : [materials]) {
      if (mat instanceof CustomShaderMaterial && !mat.userData.occlusionExclude) {
        setMaterialOcclusionBackfaceRendering(mat, occlusionBackfaceRenderingEnabled);
      }
    }
  });
};

let currentSceneEnvironment: { envMap: THREE.Texture; intensity: number } | null = null;

let warnedVertexLightingEnv = false;

const applySceneEnvironmentToMaterial = (mat: CustomShaderMaterial) => {
  const overrideEnvMap = mat.userData.envMapOverride as THREE.Texture | undefined;
  const overrideIntensity = mat.userData.envMapIntensityOverride as number | undefined;

  let envMap = overrideEnvMap ?? currentSceneEnvironment?.envMap ?? null;
  if (envMap && mat.userData.vertexLighting) {
    if (!warnedVertexLightingEnv) {
      console.warn(
        'Vertex lighting does not support env-map (IBL); ignoring scene environment for those materials'
      );
      warnedVertexLightingEnv = true;
    }
    envMap = null;
  }
  const intensity = overrideIntensity ?? currentSceneEnvironment?.intensity ?? 1;

  if (!!envMap !== !!mat.envMap) {
    mat.needsUpdate = true;
  }
  mat.envMap = envMap;
  mat.uniforms.envMap.value = envMap;
  mat.uniforms.envMapIntensity.value = intensity;
  // PMREM output is a render-target texture (not a CubeTexture), so no flip.
  mat.uniforms.flipEnvMap.value = 1;
};

export const setSceneEnvironment = (
  scene: THREE.Scene,
  env: { envMap: THREE.Texture; intensity: number } | null
) => {
  currentSceneEnvironment = env;
  scene.traverse(obj => {
    const materials = (obj as THREE.Mesh).material;
    for (const mat of Array.isArray(materials) ? materials : [materials]) {
      if (mat instanceof CustomShaderMaterial) {
        applySceneEnvironmentToMaterial(mat);
      }
    }
  });
};

export const precompileOcclusionShaderVariants = (
  scene: THREE.Scene,
  renderer: THREE.WebGLRenderer,
  camera: THREE.Camera
) => {
  setOcclusionBackfaceRendering(scene, true);
  renderer.compile(scene, camera);
  setOcclusionBackfaceRendering(scene, false);
  renderer.compile(scene, camera);
};

export const resetCustomShaderGlobals = () => {
  globalConfig = {};
  occlusionBackfaceRenderingEnabled = false;
  shadowCastSideOverride = null;
  currentSceneEnvironment = null;
  playerShadowPos.set(0, 0, 0);
  playerShadowParams.set(0, 0, 0, 0);
  psRingData.fill(0);
  occlusionParams.set(0, 0, 0, 0);
};

const playerShadowPos = new THREE.Vector3();
const playerShadowParams = new THREE.Vector4(0, 0, 0, 0);
// flat packing: [0..7] = outer ring receiverY (angles 0-7), [8..15] = inner ring (angles 0-7)
const psRingData = new Float32Array(16);

export const getPlayerShadowUniforms = () => ({
  playerShadowPos,
  playerShadowParams,
  psRingData,
});

const occlusionStart = new THREE.Vector3();
const occlusionEnd = new THREE.Vector3();
const occlusionParams = new THREE.Vector4(0, 0, 0, 0);

export const getOcclusionUniforms = () => ({
  occlusionStart,
  occlusionEnd,
  occlusionParams,
});

// World units spanned by one device pixel at unit camera distance:
// `2*tan(fov/2) / drawingBufferHeight`. `unitsPerPx` in the shader is this times
// `distanceToCamera`. Shared by reference into every material's `unitsPerPxScale`
// uniform (mirrors the `playerShadowPos` pattern) and mutated once per frame, so
// the analytic-AA footprint tracks the real FOV + resolution + devicePixelRatio
// instead of the legacy hardcoded 0.001 rad/px. Default ≈ that legacy value.
const aaPixelScale = { value: 0.001 };
const _aaDrawingBufferSize = new THREE.Vector2();

export const updateAaPixelScale = (
  camera: THREE.Camera,
  renderer: THREE.WebGLRenderer,
  refDistance?: number
) => {
  renderer.getDrawingBufferSize(_aaDrawingBufferSize);
  const h = Math.max(1, _aaDrawingBufferSize.y);
  if ((camera as THREE.PerspectiveCamera).isPerspectiveCamera) {
    const fovRad = ((camera as THREE.PerspectiveCamera).fov * Math.PI) / 180;
    aaPixelScale.value = (2 * Math.tan(fovRad / 2)) / h;
  } else if ((camera as THREE.OrthographicCamera).isOrthographicCamera) {
    const oc = camera as THREE.OrthographicCamera;
    // Ortho footprints are distance-independent, but the shader multiplies this by per-fragment
    // distanceToCamera; normalize by the focus distance so it's exact at the framed subject.
    const worldPerPx = (oc.top - oc.bottom) / oc.zoom / h;
    aaPixelScale.value = worldPerPx / Math.max(refDistance ?? 1, 1e-3);
  }
};

/**
 * Creates a minimal `ShaderMaterial` for use as the depth pre-pass override material.
 * It mirrors the Bayer dither discard logic from the main CustomShaderMaterial so that
 * the depth buffer matches what the main pass will actually render.
 *
 * Shares the same uniform objects as `getOcclusionUniforms()` so updates are automatic.
 */
export const buildOcclusionDepthMaterial = (): THREE.ShaderMaterial =>
  new THREE.ShaderMaterial({
    vertexShader: /* glsl */ `
      varying vec3 vWorldPos;
      varying vec3 vWorldNormal;
      // Pin gl_Position invariant so it bit-matches the color material's depth regardless of which
      // optional features (e.g. USE_TANGENT) restructure that shader's vertex stage.
      invariant gl_Position;
      void main() {
        // Shared snippet declares localPos / localNormal / mvPos and writes gl_Position
        // in a form bit-exact with Three.js's project_vertex chunk, so fragments drawn
        // here line up with the color pass depth buffer.
        ${depthExactVertexBody}
        vWorldPos = (modelMatrix * localPos).xyz;
        vWorldNormal = normalize((modelMatrix * vec4(localNormal, 0.0)).xyz);
      }
    `,
    fragmentShader: /* glsl */ `
      varying vec3 vWorldPos;
      varying vec3 vWorldNormal;
      ${softOcclusionPreamble}
      void main() {
        float distanceToCamera = distance(cameraPosition, vWorldPos);
        ${softOcclusionDiscard}
        gl_FragColor = vec4(1.0);
      }
    `,
    uniforms: {
      occlusionStart: { value: occlusionStart },
      occlusionEnd: { value: occlusionEnd },
      occlusionParams: { value: occlusionParams },
    },
  });

/**
 * A bit-exact depth-only override material with no dithering.
 */
export const buildPlainDepthMaterial = (): THREE.ShaderMaterial =>
  new THREE.ShaderMaterial({
    vertexShader: /* glsl */ `
      invariant gl_Position;
      void main() {
        ${depthExactVertexBody}
      }
    `,
    fragmentShader: /* glsl */ `
      void main() {
        gl_FragColor = vec4(1.0);
      }
    `,
    uniforms: {},
  });

const buildDefaultTriplanarParams = (): TriplanarMappingParams => ({
  contrastPreservationFactor: 0.5,
  sharpenFactor: 12.8,
});

/** Gouraud shading for vertex lighting */
const buildRunVertexLightingFragment = (vertexLightingShininess: number) => /* glsl */ `
  // Compute a simple Lambertian diffuse per-vertex, split into direct + indirect.
  // Shadow maps are still sampled per-fragment for crisp edges and only applied to direct.
  vec3 vtxDirectAccum = vec3(0.0);
  vec3 vtxIndirectAccum = vec3(0.0);
  ${vertexLightingShininess > 0 ? 'vec3 vtxSpecAccum = vec3(0.0);' : ''}
  vec3 vtxViewPos = -mvPosition.xyz; // geometry position in view space
  vec3 vtxNormal = normalize(transformedNormal);
  ${vertexLightingShininess > 0 ? 'vec3 vtxViewDir = normalize(vtxViewPos);' : ''}

  IncidentLight vtxDirectLight;

  #if (NUM_DIR_LIGHTS > 0)
    DirectionalLight vtxDirLight;
    #pragma unroll_loop_start
    for (int i = 0; i < NUM_DIR_LIGHTS; i++) {
      vtxDirLight = directionalLights[i];
      getDirectionalLightInfo(vtxDirLight, vtxDirectLight);
      float vtxNdotL = saturate(dot(vtxNormal, vtxDirectLight.direction));
      vtxDirectAccum += vtxDirectLight.color * vtxNdotL;
      ${
        vertexLightingShininess > 0
          ? /* glsl */ `{
        vec3 vtxH = normalize(vtxDirectLight.direction + vtxViewDir);
        float vtxNdotH = saturate(dot(vtxNormal, vtxH));
        vtxSpecAccum += vtxDirectLight.color * pow(vtxNdotH, ${vertexLightingShininess.toFixed(1)});
      }`
          : ''
      }
    }
    #pragma unroll_loop_end
  #endif

  #if (NUM_POINT_LIGHTS > 0)
    PointLight vtxPointLight;
    #pragma unroll_loop_start
    for (int i = 0; i < NUM_POINT_LIGHTS; i++) {
      vtxPointLight = pointLights[i];
      getPointLightInfo(vtxPointLight, vtxViewPos, vtxDirectLight);
      float vtxNdotL = saturate(dot(vtxNormal, vtxDirectLight.direction));
      vtxDirectAccum += vtxDirectLight.color * vtxNdotL;
      ${
        vertexLightingShininess > 0
          ? /* glsl */ `{
        vec3 vtxH = normalize(vtxDirectLight.direction + vtxViewDir);
        float vtxNdotH = saturate(dot(vtxNormal, vtxH));
        vtxSpecAccum += vtxDirectLight.color * pow(vtxNdotH, ${vertexLightingShininess.toFixed(1)});
      }`
          : ''
      }
    }
    #pragma unroll_loop_end
  #endif

  #if (NUM_SPOT_LIGHTS > 0)
    SpotLight vtxSpotLight;
    #pragma unroll_loop_start
    for (int i = 0; i < NUM_SPOT_LIGHTS; i++) {
      vtxSpotLight = spotLights[i];
      getSpotLightInfo(vtxSpotLight, vtxViewPos, vtxDirectLight);
      float vtxNdotL = saturate(dot(vtxNormal, vtxDirectLight.direction));
      vtxDirectAccum += vtxDirectLight.color * vtxNdotL;
      ${
        vertexLightingShininess > 0
          ? /* glsl */ `{
        vec3 vtxH = normalize(vtxDirectLight.direction + vtxViewDir);
        float vtxNdotH = saturate(dot(vtxNormal, vtxH));
        vtxSpecAccum += vtxDirectLight.color * pow(vtxNdotH, ${vertexLightingShininess.toFixed(1)});
      }`
          : ''
      }
    }
    #pragma unroll_loop_end
  #endif

  vtxIndirectAccum += ambientLightColor;

  #if (NUM_HEMI_LIGHTS > 0)
    #pragma unroll_loop_start
    for (int i = 0; i < NUM_HEMI_LIGHTS; i++) {
      vtxIndirectAccum += getHemisphereLightIrradiance(hemisphereLights[i], vtxNormal);
    }
    #pragma unroll_loop_end
  #endif

  vVertexDirect = vtxDirectAccum;
  vVertexIndirect = vtxIndirectAccum;
  ${vertexLightingShininess > 0 ? 'vVertexSpecular = vtxSpecAccum;' : ''}
  `;

// Qualitative Oren-Nayar diffuse factor (Fujii), as a multiplier on the Lambert term.
// Reduces to 1.0 at roughness 0, so it composes cleanly with the stock direct-diffuse line.
const OREN_NAYAR_DIFFUSE_HELPER = /* glsl */ `
float orenNayarDiffuseFactor(vec3 N, vec3 V, vec3 L, float dotNL, float roughness) {
  float dotNV = saturate(dot(N, V));
  float s = dot(L, V) - dotNL * dotNV;
  float t = s <= 0.0 ? 1.0 : max(max(dotNL, dotNV), 1e-3);
  float sig2 = roughness * roughness;
  float A = 1.0 - 0.5 * sig2 / (sig2 + 0.33);
  float B = 0.45 * sig2 / (sig2 + 0.09);
  return A + B * s / t;
}
`;

const STOCK_DIRECT_DIFFUSE_LINE =
  'reflectedLight.directDiffuse += irradiance * BRDF_Lambert( material.diffuseColor );';

// Patches stock `RE_Direct_Physical` to scale only the *direct* diffuse lobe by Oren-Nayar.
// Indirect/IBL diffuse stays Lambert. Throws loudly if a Three upgrade moves the target line.
const buildPhysicalParsFragment = (useOrenNayarDiffuse: boolean | undefined): string => {
  if (!useOrenNayarDiffuse) {
    return '#include <lights_physical_pars_fragment>';
  }
  const chunk = THREE.ShaderChunk.lights_physical_pars_fragment;
  if (!chunk.includes(STOCK_DIRECT_DIFFUSE_LINE)) {
    throw new Error('Oren-Nayar patch: direct-diffuse line not found in lights_physical_pars_fragment');
  }
  const patched = chunk.replace(
    STOCK_DIRECT_DIFFUSE_LINE,
    'reflectedLight.directDiffuse += irradiance * BRDF_Lambert( material.diffuseColor ) * orenNayarDiffuseFactor( geometryNormal, geometryViewDir, directLight.direction, dotNL, material.roughness );'
  );
  return `${OREN_NAYAR_DIFFUSE_HELPER}\n${patched}`;
};

export const buildCustomShaderArgs = (
  {
    roughness = 0.9,
    metalness = 0,
    clearcoat = 0,
    clearcoatRoughness = 0,
    clearcoatNormalMap,
    clearcoatNormalScale = 1,
    iridescence = 0,
    sheen = 0,
    sheenColor = new THREE.Color(0xffffff),
    sheenRoughness = 0,
    color = new THREE.Color(0xffffff),
    transmission = 0,
    ior = 1.5,
    transmissionMap,
    normalScale = 1,
    map,
    uvTransform,
    normalMap,
    normalMapType,
    useDisplacementNormals,
    roughnessMap,
    pomHeightMap,
    emissiveIntensity,
    lightMapIntensity,
    fogMultiplier,
    mapDisableDistance: rawMapDisableDistance,
    mapDisableDistanceAxes = 'xyz',
    mapDisableTransitionThreshold = 20,
    fogShadowFactor = 0.1,
    ambientLightScale = 1,
    ambientDistanceAmp = globalConfig.ambientDistanceAmp,
    heightAlpha,
    transparent,
  }: CustomShaderProps = {},
  {
    customVertexFragment,
    commonShader,
    colorShader,
    lightAttenuationShader,
    normalShader,
    roughnessShader,
    roughnessReverseColorRamp,
    metalnessShader,
    metalnessReverseColorRamp,
    emissiveShader,
    iridescenceShader,
    iridescenceReverseColorRamp,
    displacementShader,
    pomHeightShader,
    pomNormalShader,
    includeNoiseShadersVertex,
    customUniforms,
    constants,
  }: CustomShaderShaders = {},
  {
    antialiasColorShader,
    antialiasRoughnessShader,
    tileBreaking,
    useNoise2,
    enableFog = true,
    usePackedDiffuseNormalGBA,
    readRoughnessMapFromRChannel,
    disableToneMapping: _disableToneMapping,
    inlineEmissiveBypass,
    disabledDirectionalLightIndices,
    disabledSpotLightIndices,
    randomizeUVOffset,
    useGeneratedUVs,
    useWorldSpaceUVs,
    useTriplanarMapping,
    pom,
    noOcclusion,
    vertexLighting = false,
    vertexLightingShininess = 0,
    useOrenNayarDiffuse = true,
  }: CustomShaderOptions = {}
) => {
  const uniforms = THREE.UniformsUtils.merge([
    UniformsLib.common,
    UniformsLib.envmap,
    // UniformsLib.aomap,
    // UniformsLib.lightmap,
    // UniformsLib.emissivemap,
    // UniformsLib.bumpmap,
    UniformsLib.normalmap,
    // UniformsLib.displacementmap,
    UniformsLib.roughnessmap,
    UniformsLib.metalnessmap,
    UniformsLib.fog,
    UniformsLib.lights,
    {
      emissive: { value: new THREE.Color(0x000000) },
      roughness: { value: 1.0 },
      metalness: { value: 0.0 },
      // Not in `UniformsLib.envmap`; lives in the physical material's own uniforms upstream.
      envMapIntensity: { value: 1 },
    },
  ]);
  uniforms.normalScale = { value: new THREE.Vector2(normalScale, normalScale) };

  const triplanarUsesWorldSpace = useTriplanarMapping ? (useWorldSpaceUVs ?? true) : false;
  const generatedUVsUseWorldSpace = useGeneratedUVs ? (useWorldSpaceUVs ?? false) : false;

  const pomTexturing: PomTexturing = useTriplanarMapping
    ? 'triplanar'
    : useGeneratedUVs
      ? 'generated'
      : pom?.tangentSpace
        ? 'tangent'
        : 'baseline';

  if (randomizeUVOffset) {
    uniforms.uvOffsetSeed = { value: 0 };
  }

  uniforms.roughness = { value: roughness };
  uniforms.metalness = { value: metalness };
  uniforms.ior = { value: ior };
  uniforms.clearcoat = { value: clearcoat };
  uniforms.clearcoatRoughness = { value: clearcoatRoughness };
  uniforms.clearcoatNormalMap = { value: clearcoatNormalMap };
  uniforms.clearcoatNormalScale = {
    value: new THREE.Vector2(clearcoatNormalScale, clearcoatNormalScale),
  };
  uniforms.clearcoatNormalMapTransform = { value: clearcoatNormalMap?.matrix };
  uniforms.iridescence = { value: iridescence };
  uniforms.iridescenceIOR = { value: 1.3 };
  uniforms.iridescenceThicknessMinimum = { value: 100 };
  uniforms.iridescenceThicknessMaximum = { value: 400 };
  uniforms.iridescenceThicknessMapTransform = { value: new THREE.Matrix3() };
  if (sheen !== 0) {
    const col = typeof sheenColor === 'number' ? new THREE.Color(sheenColor) : sheenColor.clone();
    uniforms.sheenColor = { value: col.multiplyScalar(sheen) };
    uniforms.sheenRoughness = { value: sheenRoughness };
  }
  uniforms.transmission = { value: transmission };
  uniforms.transmissionMap = { value: transmissionMap };
  uniforms.transmissionSamplerSize = { value: new THREE.Vector2() };
  uniforms.transmissionSamplerMap = { value: null };

  const pomSelfShadow = pom?.selfShadow
    ? {
        lightDir: pom.selfShadow.lightDir,
        steps: pom.selfShadow.steps ?? 12,
        strength: pom.selfShadow.strength ?? 1,
        softness: pom.selfShadow.softness ?? 0.5,
      }
    : null;

  uniforms.curTimeSeconds = { value: 0.0 };
  uniforms.unitsPerPxScale = aaPixelScale;
  if (pom) {
    uniforms.pomDepth = { value: pom.depth };
    if (pom.boundedSilhouette) {
      uniforms.pomBackDepth = { value: null };
      uniforms.pomResolution = { value: new THREE.Vector2(1, 1) };
    }
    if (pomHeightMap) {
      uniforms.pomHeightMap = { value: pomHeightMap };
    }
    if (pomSelfShadow) {
      uniforms.pomShadowLightDir = {
        value: new THREE.Vector3(...pomSelfShadow.lightDir).normalize(),
      };
    }
  }
  uniforms.diffuse = { value: typeof color === 'number' ? new THREE.Color(color) : color };
  uniforms.mapTransform = { value: new THREE.Matrix3().identity() };
  uniforms.uvTransform = { value: uvTransform ?? new THREE.Matrix3().identity() };
  if (emissiveIntensity !== undefined) {
    uniforms.emissiveIntensity = { value: emissiveIntensity };
  }
  uniforms.specularIntensity = { value: 1 };
  uniforms.specularColor = { value: new THREE.Color(0xffffff) };
  if (lightMapIntensity !== undefined) {
    uniforms.lightMapIntensity = { value: lightMapIntensity };
  }
  uniforms.playerShadowPos = { value: playerShadowPos };
  uniforms.playerShadowParams = { value: playerShadowParams };
  uniforms.psRingData = { value: psRingData };
  uniforms.occlusionStart = { value: occlusionStart };
  uniforms.occlusionEnd = { value: occlusionEnd };
  uniforms.occlusionParams = { value: occlusionParams };

  // Bound post-merge so live object references (e.g. a shared per-frame uniform object) survive
  // `UniformsUtils.merge`'s deep clone.
  if (customUniforms) {
    for (const [name, def] of Object.entries(customUniforms)) {
      uniforms[name] = { value: def.value };
    }
  }

  // TODO: enable physically correct lights, look into it at least

  if (
    normalMap &&
    tileBreaking &&
    normalMapType !== undefined &&
    normalMapType !== THREE.TangentSpaceNormalMap
  ) {
    throw new Error('Tile breaking requires a normal map with tangent space');
  }

  if (usePackedDiffuseNormalGBA && !map) {
    throw new Error('Cannot use packed diffuse/normal map without a map');
  }
  if (usePackedDiffuseNormalGBA && normalMap) {
    throw new Error('Cannot use packed diffuse/normal map with a normal map');
  }
  // if (useGeneratedUVs && !map) {
  //   throw new Error('Cannot use generated UVs without a map');
  // }
  if (useTriplanarMapping && useGeneratedUVs) {
    throw new Error(
      'Triplanar mapping cannot be combined with generated UVs (both define how UVs are computed)'
    );
  }
  if (typeof usePackedDiffuseNormalGBA === 'object' && usePackedDiffuseNormalGBA.lut && tileBreaking) {
    throw new Error('LUT and tile breaking are currently broken together');
  }

  if (pom) {
    if (!pomHeightShader && !pomHeightMap) {
      throw new Error(
        '`pom` requires at least one of `shaders.pomHeightShader` (procedural) or `props.pomHeightMap` (heightmap texture)'
      );
    }
    if (pomTexturing === 'triplanar' && !triplanarUsesWorldSpace) {
      throw new Error(
        '`pom` requires world-space triplanar (`useWorldSpaceUVs` must not be false); the POM hit position is world-space'
      );
    }
    if (pomTexturing === 'generated' && !generatedUVsUseWorldSpace) {
      throw new Error(
        '`pom` with `useGeneratedUVs` requires world-space UVs (`useWorldSpaceUVs: true`); the POM hit position is world-space'
      );
    }
    if (pomTexturing === 'baseline' && normalMap) {
      throw new Error(
        '`pom` with baseline/warped UVs cannot use a normal map (no analytic tangent frame for the displaced hit); use `useTriplanarMapping` or `useGeneratedUVs`, or drop the normal map'
      );
    }
    if (pomTexturing === 'baseline' && pomHeightMap) {
      throw new Error(
        '`pom` with `pomHeightMap` requires `useTriplanarMapping` or `useGeneratedUVs` (no UV scheme at the displaced sample point under baseline)'
      );
    }
    if (pomTexturing !== 'triplanar' && (clearcoatNormalMap || usePackedDiffuseNormalGBA)) {
      throw new Error(
        '`pom` without `useTriplanarMapping` cannot use a clearcoat normal map / packed diffuse-normal map'
      );
    }
    if (normalShader) {
      throw new Error('`pom` cannot be combined with `normalShader`; both fully define `normal`');
    }
    if (pom.tangentSpace) {
      if (useTriplanarMapping || useGeneratedUVs) {
        throw new Error(
          '`pom.tangentSpace` marches in the mesh tangent frame and requires the mesh UVs (no `useTriplanarMapping` / `useGeneratedUVs`)'
        );
      }
      if (normalMap) {
        throw new Error(
          '`pom.tangentSpace` does not yet support a normal map (tangent-space normal mapping under tangent POM is future work)'
        );
      }
    }
    const pomTier = pom.tier ?? 'field';
    if (pomTier === 'projectedField' || pomTier === 'grid') {
      if (!pomHeightShader) {
        throw new Error(
          `\`pom.tier: "${pomTier}"\` requires \`shaders.pomHeightShader\` defining \`gridHeight\``
        );
      }
      if (pomHeightMap) {
        throw new Error(`\`pom.tier: "${pomTier}"\` is procedural-only; drop \`props.pomHeightMap\``);
      }
      if (pomTexturing !== 'baseline') {
        throw new Error(
          `\`pom.tier: "${pomTier}"\` owns its own world-grid projection; remove \`useTriplanarMapping\` / \`useGeneratedUVs\` / \`pom.tangentSpace\``
        );
      }
    }
    if (pomTier === 'grid') {
      if (!commonShader) {
        throw new Error(
          '`pom.tier: "grid"` requires `shaders.commonShader` declaring `struct <cellType> {…}` and `<cellType> gridComputeCell(vec2 cellId)`'
        );
      }
      if (typeof pom.cellPitch !== 'number') {
        throw new Error('`pom.tier: "grid"` requires `pom.cellPitch` (square-lattice pitch, world units)');
      }
      if (!pom.cellType) {
        throw new Error('`pom.tier: "grid"` requires `pom.cellType` (the per-cell struct type name)');
      }
    }
    if (pom.hitType) {
      if (pomTier !== 'projectedField') {
        throw new Error('`pom.hitType` is currently supported only on `pom.tier: "projectedField"`');
      }
      if (antialiasColorShader || antialiasRoughnessShader) {
        throw new Error(
          '`pom.hitType` evaluates the cell field once at the hit and shares it across slots; it is incompatible with `antialiasColorShader`/`antialiasRoughnessShader` (which oversample at multiple positions)'
        );
      }
    }
    if (pom.intersect === 'safeStep') {
      if (pomTier !== 'projectedField' && pomTier !== 'grid') {
        throw new Error('`pom.intersect: "safeStep"` requires `pom.tier: "projectedField"` or `"grid"`');
      }
      if (typeof pom.minFeatureWidth !== 'number') {
        throw new Error(
          '`pom.intersect: "safeStep"` requires `pom.minFeatureWidth` (the no-skip stride floor, projected-UV world units)'
        );
      }
    }
    if (pom.intersect === 'analytic') {
      if (pomTier !== 'projectedField') {
        throw new Error(
          '`pom.intersect: "analytic"` is currently supported only on `pom.tier: "projectedField"`'
        );
      }
      if (pom.boundedSilhouette) {
        throw new Error('`pom.intersect: "analytic"` does not support `pom.boundedSilhouette`');
      }
      if (typeof pom.minFeatureWidth !== 'number') {
        throw new Error(
          '`pom.intersect: "analytic"` requires `pom.minFeatureWidth` (used by the `safeStep` fallback)'
        );
      }
    }
  }
  const pomSteps = pom?.steps ?? 24;
  const pomBounded = !!pom?.boundedSilhouette;

  if (roughnessShader && roughnessReverseColorRamp) {
    throw new Error('Cannot use both roughness shader and roughness reverse color ramp');
  }
  if (metalnessShader && metalnessReverseColorRamp) {
    throw new Error('Cannot use both metalness shader and metalness reverse color ramp');
  }
  if (iridescenceShader && iridescenceReverseColorRamp) {
    throw new Error('Cannot use both iridescence shader and iridescence reverse color ramp');
  }

  if (vertexLighting) {
    if (clearcoat || clearcoatRoughness) {
      console.warn('Vertex lighting is incompatible with clearcoat');
    }
    if (iridescence || iridescenceShader) {
      console.warn('Vertex lighting is incompatible with iridescence');
    }
    if (sheen) {
      console.warn('Vertex lighting is incompatible with sheen');
    }
    if (transmission || transmissionMap) {
      console.warn('Vertex lighting is incompatible with transmission');
    }
    if (normalMap) {
      console.warn('Normal maps have no effect on lighting when vertex lighting is enabled');
    }
    // Vertex lighting ignores rect-area lights and env-map IBL (hemisphere diffuse is supported).
  }

  const mapDisableDistance =
    rawMapDisableDistance === undefined ? DEFAULT_MAP_DISABLE_DISTANCE : rawMapDisableDistance;

  // Precomputed mean colors replace per-fragment coarsest-mip fetches (tile-breaking contrast
  // term, triplanar contrast preservation, map disable-distance fade).
  const meanConsumersActive = !!(tileBreaking || useTriplanarMapping);
  const needMapMean = !!map && (meanConsumersActive || typeof mapDisableDistance === 'number');
  const needRoughnessMapMean = !!roughnessMap && meanConsumersActive;
  const needNormalMapMean = !!normalMap && meanConsumersActive;
  const needClearcoatNormalMapMean = !!clearcoatNormalMap && meanConsumersActive;
  if (needMapMean) {
    uniforms.mapMeanColor = { value: getTextureMeanColor(map!) };
  }
  if (needRoughnessMapMean) {
    uniforms.roughnessMapMeanColor = { value: getTextureMeanColor(roughnessMap!) };
  }
  if (needNormalMapMean) {
    uniforms.normalMapMeanColor = { value: getTextureMeanColor(normalMap!) };
  }
  if (needClearcoatNormalMapMean) {
    uniforms.clearcoatNormalMapMeanColor = { value: getTextureMeanColor(clearcoatNormalMap!) };
  }

  const triplanarPosSym = pom ? 'triplanarSamplePos' : 'vTriplanarPos';
  const triplanarNormalSym = pom ? '_pomNormalW' : 'vTriplanarNormal';

  const pomGen = !!pom && pomTexturing === 'generated';
  const pomTangent = !!pom && pomTexturing === 'tangent';
  const pomProjected = !!pom && (pom.tier ?? 'field') === 'projectedField';
  const pomGrid = !!pom && pom.tier === 'grid';
  const cellPitch = pom?.cellPitch ?? 1;
  const cellType = pom?.cellType ?? 'GridCell';
  const hitType = pom?.hitType;
  const pomHitFrame = !!hitType && (pomProjected || pomGrid);
  const pomAnalytic = !!pom && pom.intersect === 'analytic';
  const pomSafe = !!pom && (pom.intersect === 'safeStep' || pomAnalytic);
  const pomLateralDist = !!pom?.lateralDist;
  const minFeatureWidth = pom?.minFeatureWidth ?? 0;
  // Both 'generated' and 'tangent' resolve a marched UV (`_pomGenUv`) at the hit.
  const mapUvSym = pomGen || pomTangent ? '_pomGenUv' : 'vMapUv';

  const usesSceneCtx = !!(
    colorShader ||
    lightAttenuationShader ||
    roughnessShader ||
    metalnessShader ||
    emissiveShader ||
    iridescenceShader
  );

  const hasCustomShaderSnippet = !!(
    commonShader ||
    usesSceneCtx ||
    normalShader ||
    pomNormalShader ||
    displacementShader
  );

  const buildPomDefsFragment = () => {
    if (!pom) {
      return '';
    }
    const lodFadeStart = pom.lodFadeStart ?? 50;
    const lodFadeEnd = lodFadeStart + (pom.lodFadeRange ?? 50);
    const pomRefinement = pom.refinement ?? 'secant';
    const pomBinarySteps = pom.refinementSteps ?? 5;
    const pomRefineSkip = pom.refineSkipThreshold ?? 0.5;
    return buildPomDefs({
      pomSteps,
      pomBounded,
      pomProjected,
      pomGrid,
      cellType,
      pomSafe,
      pomAnalytic,
      minFeatureWidth,
      lodFadeStart,
      lodFadeEnd,
      pomRefinement,
      pomBinarySteps,
      pomRefineSkip,
      pomHasNormalShader: !!pomNormalShader,
      pomJitter: !!(pom.jitter ?? true),
      pomSelfShadow: pomSelfShadow
        ? {
            steps: pomSelfShadow.steps,
            strength: pomSelfShadow.strength,
            softness: pomSelfShadow.softness,
          }
        : undefined,
      pomDebug: pom.debug,
    });
  };

  const buildMapFragment = () => {
    const inner = (() => {
      if (useTriplanarMapping) {
        return /* glsl */ `
        #ifdef USE_MAP
          sampledDiffuseColor_ = triplanarTextureFixContrast(map, ${triplanarPosSym}, vec2(uvTransform[0][0], uvTransform[1][1]), ${triplanarNormalSym}, mapMeanColor);
        #endif`;
      }

      if (!tileBreaking) {
        return /* glsl */ `
        #ifdef USE_MAP
          vec4 sampledDiffuseColor = texture2D( map, ${mapUvSym} );
          sampledDiffuseColor_ = sampledDiffuseColor;
        #endif`;
      }

      return /* glsl */ `sampledDiffuseColor_ = ${buildTileBreakSampleExpr('map', mapUvSym, tileBreaking)};`;
    })();

    if (typeof mapDisableDistance !== 'number') {
      return /* glsl */ `
      #ifdef USE_MAP
        ${usePackedDiffuseNormalGBA ? 'vec3 mapN = vec3(0.);' : ''}
        ${inner}
        ${usePackedDiffuseNormalGBA ? buildUnpackDiffuseNormalGBAFragment(usePackedDiffuseNormalGBA) : ''}
        diffuseColor *= sampledDiffuseColor_;
      #endif`;
    }

    return /* glsl */ `
    #ifdef USE_MAP
      ${usePackedDiffuseNormalGBA ? 'vec3 mapN = vec3(0.);' : ''}

      vec4 averageTextureColor = mapMeanColor;
      if (textureActivation < 0.01) {
        diffuseColor *= averageTextureColor;
      } else {
        ${inner}
        ${usePackedDiffuseNormalGBA ? buildUnpackDiffuseNormalGBAFragment(usePackedDiffuseNormalGBA) : ''}
        diffuseColor = mix(diffuseColor * averageTextureColor, diffuseColor * sampledDiffuseColor_, textureActivation);
      }
    #endif`;
  };

  const buildRoughnessMapFragment = () => {
    const inner = (() => {
      if (useTriplanarMapping && roughnessMap) {
        return /* glsl */ `
          vec3 texelRoughness = triplanarTexture(roughnessMap, ${triplanarPosSym}, vec2(uvTransform[0][0], uvTransform[1][1]), ${triplanarNormalSym}, roughnessMapMeanColor).xyz;
        `;
      }

      if (tileBreaking && roughnessMap)
        return /* glsl */ `vec3 texelRoughness = ${buildTileBreakSampleExpr('roughnessMap', mapUvSym, tileBreaking)}.xyz;`;
      else
        return /* glsl */ `
      vec4 texelRoughness = texture2D( roughnessMap, ${mapUvSym} );
      `;
    })();

    if (typeof mapDisableDistance !== 'number') {
      return /* glsl */ `
      float roughnessFactor = roughness;
      #ifdef USE_ROUGHNESSMAP
        ${inner}
        ${
          readRoughnessMapFromRChannel
            ? 'float channelRoughness = texelRoughness.r;'
            : 'float channelRoughness = texelRoughness.g;'
        }
        roughnessFactor *= channelRoughness;
      #endif`;
    }

    return /* glsl */ `
      float roughnessFactor = roughness;
      #ifdef USE_ROUGHNESSMAP
        if (textureActivation < 0.01) {
        } else {
          ${inner}
          ${
            readRoughnessMapFromRChannel
              ? 'float channelRoughness = texelRoughness.r;'
              : 'float channelRoughness = texelRoughness.g;'
          }
          roughnessFactor = mix(roughnessFactor, roughnessFactor * channelRoughness, textureActivation);
        }
      #endif`;
  };

  const buildNormalMapFragment = () => {
    if (pom) {
      // POM owns the shading normal; the normal map (if any) is combined onto
      // the analytic floor normal in `buildPomNormalApply`.
      return '';
    }
    if (usePackedDiffuseNormalGBA) {
      if (typeof mapDisableDistance === 'number') {
        return /* glsl */ `
        if (textureActivation < 0.01) {
          // we didn't read anything from the normal map, so we can't use it
        } else {
          mapN = mapN * 2.0 - 1.0;
          mapN.xy *= normalScale;

          #ifdef USE_NORMALMAP_TANGENTSPACE
            normal = normalize( tbn * mapN );
          #else
            UNIMPLEMENTED_2
          #endif
        }
        `;
      }

      return /* glsl */ `
        mapN = mapN * 2.0 - 1.0;
        mapN.xy *= normalScale;

        #ifdef USE_NORMALMAP_TANGENTSPACE
          normal = normalize( tbn * mapN );
        #else
          UNIMPLEMENTED_3
        #endif
      `;
    }

    if (!normalMap) {
      return '';
    }

    const normalMapSuffix = /* glsl */ `
    mapN = mapN * 2.0 - 1.0;
    mapN = normalize(mapN);
    mapN.xy *= normalScale;

    #ifdef USE_NORMALMAP_TANGENTSPACE
      normal = normalize( tbn * mapN );
    #else
      UNIMPLEMENTED_4
    #endif`;

    const inner = (() => {
      if (useTriplanarMapping) {
        const transform = triplanarUsesWorldSpace
          ? '(viewMatrix * vec4(perturbedNormal, 0.)).xyz'
          : '(viewMatrix * modelMatrix * vec4(perturbedNormal, 0.)).xyz';
        return /* glsl */ `
          vec3 perturbedNormal = triplanarTextureNormalMap(normalMap, ${triplanarPosSym}, vec2(uvTransform[0][0], uvTransform[1][1]), ${triplanarNormalSym}, normalScale, normalMapMeanColor).xyz;
          normal = normalize(${transform});
          `;
      }

      if (tileBreaking)
        return /* glsl */ `
    vec3 mapN = ${buildTileBreakSampleExpr('normalMap', 'vNormalMapUv', tileBreaking)}.xyz;

    ${normalMapSuffix}
  `;
      else return '#include <normal_fragment_maps>';
    })();

    if (typeof mapDisableDistance !== 'number') {
      return inner;
    }

    return /* glsl */ `
      vec3 baseNormal = normal;
      if (textureActivation < 0.01) {
      } else {
        ${inner}
        normal = mix(baseNormal, normal, textureActivation);
      }`;
  };

  const buildClearcoatNormalMapFragment = () => {
    if (!clearcoatNormalMap) {
      return '';
    }

    const inner = (() => {
      if (useTriplanarMapping) {
        const transform = triplanarUsesWorldSpace
          ? '(viewMatrix * vec4(perturbedClearcoatNormal, 0.)).xyz'
          : '(viewMatrix * modelMatrix * vec4(perturbedClearcoatNormal, 0.)).xyz';
        return /* glsl */ `
          vec3 perturbedClearcoatNormal = triplanarTextureNormalMap(clearcoatNormalMap, ${triplanarPosSym}, vec2(uvTransform[0][0], uvTransform[1][1]), ${triplanarNormalSym}, clearcoatNormalScale, clearcoatNormalMapMeanColor).xyz;
          clearcoatNormal = normalize(${transform});
          `;
      }

      if (tileBreaking) {
        return /* glsl */ `
    vec3 clearcoatMapN = ${buildTileBreakSampleExpr('clearcoatNormalMap', 'vClearcoatNormalMapUv', tileBreaking)}.xyz;

    clearcoatMapN = clearcoatMapN * 2.0 - 1.0;
    clearcoatMapN = normalize(clearcoatMapN);
    clearcoatMapN.xy *= clearcoatNormalScale;

    #ifdef USE_NORMALMAP_TANGENTSPACE
      clearcoatNormal = normalize( tbn2 * clearcoatMapN );
    #else
      UNIMPLEMENTED_5
    #endif
`;
      } else {
        return /* glsl */ `
#ifdef USE_CLEARCOAT_NORMALMAP

	vec3 clearcoatMapN = texture2D( clearcoatNormalMap, vClearcoatNormalMapUv ).xyz * 2.0 - 1.0;
	clearcoatMapN.xy *= clearcoatNormalScale;

	clearcoatNormal = normalize( tbn2 * clearcoatMapN );

#endif
`;
      }
    })();

    if (typeof mapDisableDistance !== 'number') {
      return inner;
    }

    return /* glsl */ `
      vec3 baseClearcoatNormal = clearcoatNormal;
      if (textureActivation < 0.01) {
      } else {
        ${inner}
        clearcoatNormal = mix(baseClearcoatNormal, clearcoatNormal, textureActivation);
      }`;
  };

  const buildCustomUniformDecls = (forVertex: boolean): string => {
    if (!customUniforms) {
      return '';
    }
    return Object.entries(customUniforms)
      .filter(([, def]) => (forVertex ? def.vertex : true))
      .map(([name, def]) => `uniform ${def.type} ${name};`)
      .join('\n');
  };

  // User constants baked into the fragment GLSL as `#define`s (override a slot's `#ifndef` default).
  // Parenthesized scalars so they compose in expressions; floats forced to a decimal literal.
  const buildConstantDefines = (): string => {
    if (!constants) {
      return '';
    }
    const lit = (def: NonNullable<typeof constants>[string]): string => {
      switch (def.type) {
        case 'float':
          return `(${def.value.toFixed(6)})`;
        case 'int':
          return `(${Math.trunc(def.value)})`;
        case 'bool':
          return def.value ? 'true' : 'false';
        default:
          return `${def.type}(${def.value.map(n => n.toFixed(6)).join(', ')})`;
      }
    };
    return Object.entries(constants)
      .map(([name, def]) => `#define ${name} ${lit(def)}`)
      .join('\n');
  };

  return {
    fog: true,
    lights: true,
    dithering: false,
    transparent: transparent ?? false,
    uniforms,
    vertexShader: /* glsl */ `
#define STANDARD
varying vec3 vViewPosition;
#ifdef USE_TRANSMISSION
  varying vec3 vWorldPosition;
#endif

#include <common>
#include <uv_pars_vertex>
#include <displacementmap_pars_vertex>
#include <color_pars_vertex>
${enableFog ? '#include <fog_pars_vertex>' : ''}
#include <normal_pars_vertex>
#include <morphtarget_pars_vertex>
#include <skinning_pars_vertex>
#include <shadowmap_pars_vertex>
#include <logdepthbuf_pars_vertex>
#include <clipping_planes_pars_vertex>
${vertexLighting ? '#include <lights_pars_begin>' : ''}

${vertexLighting ? 'varying vec3 vVertexDirect;' : ''}
${vertexLighting ? 'varying vec3 vVertexIndirect;' : ''}
${vertexLighting && vertexLightingShininess > 0 ? 'varying vec3 vVertexSpecular;' : ''}

${includeNoiseShadersVertex ? noiseShaders : ''}

${useGeneratedUVs ? GeneratedUVsFragment : ''}

${buildCustomUniformDecls(true)}

${displacementShader || ''}

${useDisplacementNormals ? 'attribute vec3 displacementNormal;' : ''}

uniform float curTimeSeconds;
varying vec3 vWorldPos;
varying vec3 vObjectNormal;
varying vec3 vWorldNormal;
uniform mat3 uvTransform;
#ifdef USE_NORMALMAP
uniform vec2 normalScale;
varying float vTerminatorSoftenGate;
#endif
${randomizeUVOffset ? 'uniform float uvOffsetSeed;' : ''}
${randomizeUVOffset ? hashSeedToVec2GLSL : ''}
${useTriplanarMapping ? 'varying vec3 vTriplanarPos;' : ''}
${useTriplanarMapping ? 'varying vec3 vTriplanarNormal;' : ''}
${useTriplanarMapping && randomizeUVOffset ? hashSeedToVec3GLSL : ''}
${pomTangent ? '#ifndef USE_TANGENT\nattribute vec4 tangent;\n#endif\nvarying vec3 vWorldTangent;' : ''}

// Keep gl_Position bit-identical to the depth-prepass material's regardless of which optional
// features (USE_TANGENT, etc.) are compiled in, so the prepass depth-test match holds.
invariant gl_Position;

void main() {
  #include <color_vertex>
  #include <morphcolor_vertex>

  #include <beginnormal_vertex>
  #include <morphnormal_vertex>
  #include <skinbase_vertex>
  #include <skinnormal_vertex>
  #include <defaultnormal_vertex>
  #include <normal_vertex>

  #include <begin_vertex>
  #include <morphtarget_vertex>
  #include <skinning_vertex>
  ${(() => {
    const normalAttribute = useDisplacementNormals ? 'displacementNormal' : 'objectNormal';
    return displacementShader
      ? /* glsl */ `
    float computedDisplacement = getDisplacement(position, ${normalAttribute}, curTimeSeconds);
    transformed += normalize( ${normalAttribute} ) * computedDisplacement;
  `
      : /* glsl */ `
#ifdef USE_DISPLACEMENTMAP
	transformed += normalize( ${normalAttribute} ) * ( texture2D( displacementMap, vDisplacementMapUv ).x * displacementScale + displacementBias );
#endif
`;
  })()}
  #include <project_vertex>
  #include <logdepthbuf_vertex>
  #include <clipping_planes_vertex>

  vViewPosition = - mvPosition.xyz;

#ifdef USE_NORMALMAP
  vTerminatorSoftenGate = smoothstep(0.0, 0.5, abs(normalScale.x));
#endif

  #include <worldpos_vertex>

  vec4 worldPositionMine = vec4(transformed, 1.);
  #ifdef USE_INSTANCING
    worldPositionMine = instanceMatrix * worldPositionMine;
  #endif
  worldPositionMine = modelMatrix * worldPositionMine;
  vWorldPos = worldPositionMine.xyz;
  #ifdef USE_TRANSMISSION
    vWorldPosition = vWorldPos;
  #endif

  vObjectNormal = normal;
  vec4 worldNormalMine = vec4(normal, 0.);
  #ifdef USE_INSTANCING
    worldNormalMine = instanceMatrix * worldNormalMine;
  #endif
  vWorldNormal = normalize((modelMatrix * worldNormalMine).xyz);

  ${
    pomTangent
      ? /* glsl */ `
  vec4 worldTangentMine = vec4(tangent.xyz, 0.);
  #ifdef USE_INSTANCING
    worldTangentMine = instanceMatrix * worldTangentMine;
  #endif
  vWorldTangent = normalize((modelMatrix * worldTangentMine).xyz);`
      : ''
  }

  ${(() => {
    if (!useTriplanarMapping) return '';
    const pos = triplanarUsesWorldSpace ? 'vWorldPos' : 'position';
    const norm = triplanarUsesWorldSpace ? 'vWorldNormal' : 'vObjectNormal';
    const offsetExpr = randomizeUVOffset
      ? /* glsl */ `
      vec2 _uvScaleVec = vec2(uvTransform[0][0], uvTransform[1][1]);
      vec3 _uvOffset3 = hashSeedToVec3(uvOffsetSeed);
      vec3 _safeScale = vec3(
        _uvScaleVec.x != 0. ? 1. / _uvScaleVec.x : 0.,
        _uvScaleVec.y != 0. ? 1. / _uvScaleVec.y : 0.,
        _uvScaleVec.x != 0. ? 1. / _uvScaleVec.x : 0.
      );
      vTriplanarPos = ${pos} + _uvOffset3 * _safeScale;`
      : /* glsl */ `vTriplanarPos = ${pos};`;
    return /* glsl */ `${offsetExpr}
      vTriplanarNormal = ${norm};`;
  })()}

  #ifdef USE_UV
  ${(() => {
    if (useGeneratedUVs) {
      const uvPos = generatedUVsUseWorldSpace ? 'vWorldPos' : 'position';
      const uvNormal = generatedUVsUseWorldSpace ? 'vWorldNormal' : 'normal';
      return /* glsl */ `
      vUv = generateUV(${uvPos}, ${uvNormal});
      vUv = ( uvTransform * vec3( vUv, 1 ) ).xy;
      `;
    }

    if (randomizeUVOffset) {
      // `randomizeUVOffset` performs UV transformation internally
      return 'vUv = uv.xy;';
    }

    // default uv transform
    return 'vUv = (uvTransform * vec3( uv, 1 )).xy;';
  })()}
  #endif

  ${buildUVVertexFragment(randomizeUVOffset, !!useGeneratedUVs)}

  #if defined(USE_MAP) && defined(USE_UV)
    vMapUv = ( mapTransform * vec3( vUv, 1 ) ).xy;
  #endif
  #if defined(USE_NORMALMAP) && defined(USE_UV)
    vNormalMapUv = ( normalMapTransform * vec3( vUv, 1 ) ).xy;
  #endif
  #if defined(USE_ROUGHNESSMAP) && defined(USE_UV)
    vRoughnessMapUv = ( roughnessMapTransform * vec3( vUv, 1 ) ).xy;
  #endif
  #if defined(USE_METALNESSMAP) && defined(USE_UV)
    vMetalnessMapUv = ( metalnessMapTransform * vec3( vUv, 1 ) ).xy;
  #endif
  #if defined(USE_CLEARCOAT_NORMALMAP)
    vClearcoatNormalMapUv = ( clearcoatNormalMapTransform * vec3( vUv, 1 ) ).xy;
  #endif

  #include <shadowmap_vertex>
  ${enableFog ? '#include <fog_vertex>' : ''}

  ${vertexLighting ? buildRunVertexLightingFragment(vertexLightingShininess) : ''}

  ${customVertexFragment ?? ''}
}`,
    fragmentShader: /* glsl */ `
layout(location = 0) out vec4 outFragColor;
${
  inlineEmissiveBypass
    ? /* glsl */ `
#ifdef INLINE_EMISSIVE_BYPASS
layout(location = 1) out vec4 outEmissiveBypass;
#endif`
    : ''
}

#define STANDARD

#ifdef PHYSICAL
	#define IOR
	#define SPECULAR
#endif

uniform vec3 diffuse;
uniform vec3 emissive;
uniform float roughness;
uniform float metalness;
uniform float opacity;

#ifdef IOR
	uniform float ior;
#endif

#ifdef USE_SPECULAR
	uniform float specularIntensity;
	uniform vec3 specularColor;

	#ifdef USE_SPECULAR_COLORMAP
		uniform sampler2D specularColorMap;
	#endif

	#ifdef USE_SPECULAR_INTENSITYMAP
		uniform sampler2D specularIntensityMap;
	#endif
#endif

#ifdef USE_CLEARCOAT
	uniform float clearcoat;
	uniform float clearcoatRoughness;
#endif

#ifdef USE_IRIDESCENCE
	uniform float iridescence;
	uniform float iridescenceIOR;
	uniform float iridescenceThicknessMinimum;
	uniform float iridescenceThicknessMaximum;
#endif

#ifdef USE_SHEEN
	uniform vec3 sheenColor;
	uniform float sheenRoughness;

	#ifdef USE_SHEEN_COLORMAP
		uniform sampler2D sheenColorMap;
	#endif

	#ifdef USE_SHEEN_ROUGHNESSMAP
		uniform sampler2D sheenRoughnessMap;
	#endif
#endif

varying vec3 vViewPosition;
// transmission_pars_fragment declares modelMatrix itself
#ifndef USE_TRANSMISSION
uniform mat4 modelMatrix;
#endif
${vertexLighting ? 'varying vec3 vVertexDirect;' : ''}
${vertexLighting ? 'varying vec3 vVertexIndirect;' : ''}
${vertexLighting && vertexLightingShininess > 0 ? 'varying vec3 vVertexSpecular;' : ''}

#include <common>
#include <packing>
#include <color_pars_fragment>
#include <uv_pars_fragment>
#include <map_pars_fragment>
#include <alphamap_pars_fragment>
#include <alphatest_pars_fragment>
#include <aomap_pars_fragment>
#include <lightmap_pars_fragment>
#include <emissivemap_pars_fragment>
#include <iridescence_fragment>
#include <cube_uv_reflection_fragment>
#include <envmap_common_pars_fragment>
#include <envmap_physical_pars_fragment>
${enableFog ? '#include <fog_pars_fragment>' : ''}
#include <lights_pars_begin>
#include <normal_pars_fragment>
${buildPhysicalParsFragment(useOrenNayarDiffuse)}
#include <transmission_pars_fragment>
#include <shadowmap_pars_fragment>
// #include <bumpmap_pars_fragment>
#include <normalmap_pars_fragment>
#include <clearcoat_pars_fragment>
#include <iridescence_pars_fragment>
#include <roughnessmap_pars_fragment>
#include <metalnessmap_pars_fragment>
#include <logdepthbuf_pars_fragment>
#include <clipping_planes_pars_fragment>

uniform float curTimeSeconds;
uniform float unitsPerPxScale;
varying vec3 vWorldPos;

#ifdef USE_NORMALMAP
varying float vTerminatorSoftenGate;
#endif

// Bound direct light by the geometric (pre-normal-map) horizon so a normal map
// can't illuminate micro-facets whose underlying face points away from the light.
// Faded in by normal-map intensity (vertex-derived gate) so flat (un-perturbed) surfaces
// keep their full terminator — nothing to sparkle there, and the dimming otherwise fights
// Oren-Nayar.
float softenTerminator(vec3 geoN, vec3 lightDir) {
#ifdef USE_NORMALMAP
  float soft = smoothstep(-0.2, 0.5, dot(geoN, lightDir));
  return mix(1.0, soft, vTerminatorSoftenGate);
#else
  return 1.0;
#endif
}
${buildPomUniformDecls(!!pom, pomBounded, !!pomHeightMap, !!pomSelfShadow)}

uniform vec3 playerShadowPos;
uniform vec4 playerShadowParams; // x=radius, y=intensity, z=centerReceiverY, w=maxReceiverY (highest probe, for early-out)
uniform float psRingData[16]; // [0..7]: outer ring receiverY (angles 0-7), [8..15]: inner ring (angles 0-7)

${softOcclusionPreamble}

varying vec3 vObjectNormal;
varying vec3 vWorldNormal;
uniform mat3 uvTransform;
${needMapMean ? 'uniform vec4 mapMeanColor;' : ''}
${needRoughnessMapMean ? 'uniform vec4 roughnessMapMeanColor;' : ''}
${needNormalMapMean ? 'uniform vec4 normalMapMeanColor;' : ''}
${needClearcoatNormalMapMean ? 'uniform vec4 clearcoatNormalMapMeanColor;' : ''}
${useTriplanarMapping ? 'varying vec3 vTriplanarPos;' : ''}
${useTriplanarMapping ? 'varying vec3 vTriplanarNormal;' : ''}
${pomTangent ? 'varying vec3 vWorldTangent;' : ''}

${normalShader ? 'uniform mat3 normalMatrix;' : ''}
// ${usePackedDiffuseNormalGBA ? 'uniform vec2 normalScale;' : ''}
${typeof usePackedDiffuseNormalGBA === 'object' ? 'uniform sampler2D diffuseLUT;' : ''}

struct SceneCtx {
  vec2 vUv;
  vec4 diffuseColor;
  // Camera distance + the anisotropic pixel-footprint half-width in world units
  // (\`aaFootprint\` = one pixel stretched by 1/NdotV toward grazing), mirrored
  // from main() so slots reuse them. \`aaFootprint\` is the footprint to drive
  // analytic AA (edge widen + fade-to-mean) with; the isotropic value, if ever
  // needed, is \`distanceToCamera * unitsPerPxScale\`. Globals
  // \`cameraPosition\`/\`vWorldPos\`/\`vWorldNormal\` are already in slot scope.
  float distanceToCamera;
  float aaFootprint;
};

${commonShaderCode}
${noiseShaders}
${useNoise2 ? noise2Shaders : ''}

// Helpers emitted before user shaders so user shaders can call them.
${
  tileBreaking?.type === 'neyret'
    ? tileBreakingNeyretFragment.replace(
        '#define Z 8.',
        `#define Z ${(tileBreaking.patchScale ?? 8).toFixed(4)}`
      )
    : ''
}
${
  useTriplanarMapping
    ? buildTriplanarDefsFragment(
        typeof useTriplanarMapping === 'boolean'
          ? buildDefaultTriplanarParams()
          : { ...buildDefaultTriplanarParams(), ...useTriplanarMapping },
        (sampler, uv, mean) => buildTileBreakSampleExpr(sampler, uv, tileBreaking, mean),
        tileBreaking ? 'neyret' : 'none'
      )
    : ''
}
${pomGen ? GeneratedUVsFragment : ''}
${
  pomTangent
    ? /* glsl */ `
// World-space gradients of vUv.x / vUv.y, measured once per fragment from screen-space
// derivatives (filled by pomComputeUvGradients() in main(), before the march). They encode each
// UV axis's direction AND its true per-world-unit rate — including the profile V axis, whose rate
// (param-per-world-unit) a fixed uvTransform scale gets wrong, making V-keyed relief over-parallax
// and shear with view angle.
vec3 _pomGradU;
vec3 _pomGradV;

// Cotangent solve (Mikkelsen): the world-space gradient of a UV channel f is
// (cross(dpdy,N)*dFdx(f) + cross(N,dpdx)*dFdy(f)) / det. Call from uniform control flow only.
void pomComputeUvGradients() {
  vec3 dpdx = dFdx(vWorldPos);
  vec3 dpdy = dFdy(vWorldPos);
  vec2 duvdx = dFdx(vUv);
  vec2 duvdy = dFdy(vUv);
  vec3 n = normalize(vWorldNormal);
  vec3 r1 = cross(dpdy, n);
  vec3 r2 = cross(n, dpdx);
  float det = dot(dpdx, r1);
  float idet = abs(det) > 1e-12 ? 1. / det : 0.;
  _pomGradU = (r1 * duvdx.x + r2 * duvdy.x) * idet;
  _pomGradV = (r1 * duvdx.y + r2 * duvdy.y) * idet;
}

// Recover the mesh UV at a marched world point by projecting its in-plane offset onto the measured
// UV gradients (which are ⊥ N, so the depth component drops out). At the base surface this is vUv.
vec2 pomMeshUv(vec3 p) {
  vec3 off = p - vWorldPos;
  return vUv + vec2(dot(off, _pomGradU), dot(off, _pomGradV));
}

// World units per unit of vUv.x / vUv.y — the inverse of the measured UV gradient lengths. Lets a
// material snippet convert UV-space deltas to world units so isotropic patterns (round pits, hex
// grids, …) stay round regardless of the swept profile's V parameterization. Tangent-space POM only.
vec2 pomUvWorldScale() {
  float lu = length(_pomGradU);
  float lv = length(_pomGradV);
  return vec2(lu > 1e-9 ? 1. / lu : 0., lv > 1e-9 ? 1. / lv : 0.);
}`
    : ''
}

${hasCustomShaderSnippet ? proceduralMaterialAACode : ''}
${pomProjected || pomGrid ? proceduralMaterialGridCode : ''}
${pomGrid ? `#define GRID_PITCH ${cellPitch.toFixed(6)}` : ''}
${buildConstantDefines()}

${buildCustomUniformDecls(false)}

${commonShader ?? ''}

${colorShader ?? ''}
${lightAttenuationShader ?? ''}
${normalShader ?? ''}
${roughnessShader ?? ''}
${metalnessShader ?? ''}
${emissiveShader ?? ''}
${[roughnessReverseColorRamp, metalnessReverseColorRamp, iridescenceReverseColorRamp].some(p => p && (p.colorSpace ?? 'srgb') === 'srgb') ? ReverseColorRampCommonFunctions : ''}
${roughnessReverseColorRamp ? buildReverseColorRampGenerator('roughnessFromColor', roughnessReverseColorRamp) : ''}
${metalnessReverseColorRamp ? buildReverseColorRampGenerator('metalnessFromColor', metalnessReverseColorRamp) : ''}
${iridescenceShader ?? ''}
${iridescenceReverseColorRamp ? buildReverseColorRampGenerator('iridescenceFromColor', iridescenceReverseColorRamp) : ''}
${pomHeightShader ?? ''}
${pomProjected ? 'float getPomHeight(vec3 p, vec3 N, float t) { return gridHeight(domProject(p, domAxis(N)), t); }' : ''}
${pomSafe && !pomLateralDist ? 'float gridLateralDist(vec2 uv) { return 0.; }' : ''}
${pomGrid ? `float getPomHeight(vec3 p, vec3 N, float t) { vec2 uv = domProject(p, domAxis(N)); vec2 cid = floor(uv / GRID_PITCH); return gridHeight(GridCtx((fract(uv / GRID_PITCH) - 0.5) * GRID_PITCH, cid, t), gridComputeCell(cid)); }` : ''}
${pom ? buildPomHeightSources({ hasHeightShader: !!pomHeightShader, hasHeightMap: !!pomHeightMap, pomTexturing }) : ''}
${pom && pomNormalShader ? pomNormalShader : ''}
${pomHitFrame ? `${hitType} _pomHitData; // one cell-field eval at the hit, shared by every slot below` : ''}
${pomHitFrame && colorShader ? 'vec4 getFragColor(vec3 baseColor, vec3 p, vec3 n, float t, SceneCtx ctx) { return gridColor(_pomHitData, baseColor, ctx); }' : ''}
${pomHitFrame && lightAttenuationShader ? 'vec2 getLightAttenuation(vec3 p, vec3 n, float t, SceneCtx ctx) { return gridAttenuation(_pomHitData, ctx); }' : ''}
${pomHitFrame && roughnessShader ? 'float getCustomRoughness(vec3 p, vec3 n, float baseRoughness, float t, SceneCtx ctx) { return gridRoughness(_pomHitData, baseRoughness, ctx); }' : ''}
${pomHitFrame && pomNormalShader ? 'vec3 getPomNormal(vec3 p, vec3 N, float depth, float t, float aa) { return gridNormal(_pomHitData, N, depth, aa); }' : ''}
${buildPomDefsFragment()}

void main() {
	#include <clipping_planes_fragment>

  float distanceToCamera = distance(cameraPosition, vWorldPos);

  ${!noOcclusion ? softOcclusionDiscard : ''}

  float unitsPerPx = distanceToCamera * unitsPerPxScale;
  ${
    mapDisableDistanceAxes === 'xz'
      ? 'float texDisableDistance = distance(cameraPosition.xz, vWorldPos.xz);'
      : 'float texDisableDistance = distanceToCamera;'
  }

  ${buildTextureDisableFragment(mapDisableDistance, mapDisableTransitionThreshold)}

  ${buildHeightAlphaEarlyOut(heightAlpha)}

  vec4 diffuseColor = vec4(diffuse, opacity);
  ${
    !noOcclusion
      ? /* glsl */ `
  if (highlightFactor > 0.) {
    diffuseColor.rgb = mix(diffuseColor.rgb, vec3(0.3, 0.3, 0.3), highlightFactor * 0.7);
  }

  bool hasBackfaceHit = false;
  if (!gl_FrontFacing) {
    // Only render backfaces when the soft occlusion cylinder is active.
    if (occlusionParams.z < 0.5) {
      discard;
    }

    // basline light screen-space dither
    if (getBayer4x4(gl_FragCoord.xy) > 0.85) {
      discard;
    }

    // Smooth distance-based scale keeps checkerboard cell size roughly consistent across distances.
    float _bf_distMult = round(max(1. / distanceToCamera, 0.1) * 16.) / 16.;

    float worldBayer = getTriplanarBayer(vWorldPos * vec3(30., 50., 30.) * _bf_distMult, normalize(vWorldNormal), 1.3);

    // if our fragment far from the player, fade out the dither into fully transparent backfaces
    float distanceToPlayer = distance(playerShadowPos, vWorldPos);
    float farFactor = smoothstep(8., 30., distanceToPlayer) * 1.1;

    if (worldBayer > (1. - farFactor) * 0.5) {
      discard;
    } else {
      hasBackfaceHit = true;
    }
  }
  `
      : ''
  }

  #if !defined(USE_UV)
    vec2 vUv = vec2(0.);
  #endif
  #if !defined(USE_UV) || !defined(USE_MAP)
    vec2 vMapUv = vec2(0.);
  #endif

  ${
    hasCustomShaderSnippet
      ? /* glsl */ `aaWorldFootprint = unitsPerPx / max(abs(dot(normalize(vWorldNormal), vWorldPos - cameraPosition)) / distanceToCamera, 0.1);
  aaUvFootprint = vec2(length(vec2(dFdx(vUv.x), dFdy(vUv.x))), length(vec2(dFdx(vUv.y), dFdy(vUv.y))));
  {
    vec3 _dp1 = dFdx(vWorldPos), _dp2 = dFdy(vWorldPos);
    vec2 _duv1 = dFdx(vUv), _duv2 = dFdy(vUv);
    vec3 _fn = normalize(vWorldNormal);
    vec3 _dp2p = cross(_dp2, _fn), _dp1p = cross(_fn, _dp1);
    vec3 _t = _dp2p * _duv1.x + _dp1p * _duv2.x;
    vec3 _b = _dp2p * _duv1.y + _dp1p * _duv2.y;
    if (dot(_t, _t) > 1e-14) { uvFrameT = normalize(_t); }
    if (dot(_b, _b) > 1e-14) { uvFrameB = normalize(_b); }
  }`
      : ''
  }
  ${usesSceneCtx ? 'SceneCtx ctx = SceneCtx(vMapUv, diffuseColor, distanceToCamera, aaWorldFootprint);' : ''}

  ${pomTangent ? 'pomComputeUvGradients();' : ''}
  ${pom ? buildPomMainBlock(pomBounded, pomProjected, pomGrid, pomSafe, pomAnalytic, pomTexturing, pom.normalEps, pomSelfShadow ? { strength: pomSelfShadow.strength } : null, !!pomHeightMap, pomHitFrame && pomProjected ? '_pomHitData = gridComputeHit(domProject(_pomHit, domAxis(_pomNormalW)));' : null) : ''}

	ReflectedLight reflectedLight = ReflectedLight( vec3( 0.0 ), vec3( 0.0 ), vec3( 0.0 ), vec3( 0.0 ) );
	vec3 totalEmissiveRadiance = emissive;
	#include <logdepthbuf_fragment>
  vec4 sampledDiffuseColor_ = diffuseColor;
	${buildMapFragment()}

	#include <color_fragment>
  ${usesSceneCtx ? 'ctx.diffuseColor = diffuseColor;' : ''}
	#include <alphamap_fragment>
	#include <alphatest_fragment>
  ${buildRoughnessMapFragment()}
	#include <metalnessmap_fragment>
	#include <normal_fragment_begin>
  ${buildNormalMapFragment()}
  ${
    normalMap && !pom
      ? /* glsl */ `
  // Degenerate tangent frame (swept-mesh caps: analytic tangent ∥ normal → NaN bitangent) → keep
  // the geometric normal so a NaN can't poison lighting + screen-space post-processing.
  normal = dot(normal, normal) > 0.5 ? normal : nonPerturbedNormal;`
      : ''
  }
  ${pom ? buildPomNormalApply(pomTexturing, !!normalMap, !!pom.applyReliefNormal) : ''}

	#include <clearcoat_normal_fragment_begin>
	// #include <clearcoat_normal_fragment_maps>
  #if defined(USE_CLEARCOAT) && defined(USE_CLEARCOAT_NORMALMAP)
    ${buildClearcoatNormalMapFragment()}
  #endif

  ${buildRunColorShaderFragment(colorShader, antialiasColorShader, !!pom)}

  ${
    normalShader
      ? /* glsl */ `
  normal = getCustomNormal(vWorldPos, vObjectNormal, curTimeSeconds);
  normal = normalize(normalMatrix * normal);
  `
      : ''
  }

  ${roughnessShader ? buildRoughnessShaderFragment(antialiasRoughnessShader) : ''}
  ${roughnessReverseColorRamp ? 'roughnessFactor = roughnessFromColor(sampledDiffuseColor_.rgb);' : ''}

  ${
    metalnessShader
      ? 'metalnessFactor = getCustomMetalness(vWorldPos, vObjectNormal, roughnessFactor, curTimeSeconds, ctx);'
      : ''
  }
  ${metalnessReverseColorRamp ? 'metalnessFactor = metalnessFromColor(sampledDiffuseColor_.rgb);' : ''}

	#include <emissivemap_fragment>
  ${
    emissiveShader
      ? /* glsl */ `
    totalEmissiveRadiance = getCustomEmissive(${pom ? '_pomHit' : 'vWorldPos'}, totalEmissiveRadiance, curTimeSeconds, ctx);
  `
      : ''
  }

	// accumulation
  ${
    vertexLighting
      ? VERTEX_LIGHTING_FRAGMENT
      : /* glsl */ `
	#include <lights_physical_fragment>
  ${buildRunIridescenceShaderFragment(iridescenceShader)}
  ${iridescenceReverseColorRamp ? 'material.iridescence = iridescenceFromColor(sampledDiffuseColor_.rgb);' : ''}

	// #include <lights_fragment_begin>
  ${buildLightsFragmentBegin(disabledDirectionalLightIndices, disabledSpotLightIndices, ambientLightScale, ambientDistanceAmp)}
	#include <lights_fragment_maps>
	#include <lights_fragment_end>

	// modulation
	#include <aomap_fragment>
  ${pomSelfShadow ? buildPomSelfShadowApply() : ''}
  ${buildRunLightAttenuationFragment(lightAttenuationShader, !!pom)}

	vec3 totalDiffuse = reflectedLight.directDiffuse + reflectedLight.indirectDiffuse;
	vec3 totalSpecular = reflectedLight.directSpecular + reflectedLight.indirectSpecular;
  `
  }

  ${PLAYER_SHADOW_FRAGMENT}

	#include <transmission_fragment>

	vec3 outgoingLight = totalDiffuse + totalSpecular${inlineEmissiveBypass ? '' : ' + totalEmissiveRadiance'};

	#ifdef USE_SHEEN

		// Sheen energy compensation approximation calculation can be found at the end of
		// https://drive.google.com/file/d/1T0D1VSyR4AllqIJTQAraEIzjlb5h4FKH/view?usp=sharing
		float sheenEnergyComp = 1. - 0.157 * max3(material.sheenColor);

		outgoingLight = outgoingLight * sheenEnergyComp + sheenSpecularDirect + sheenSpecularIndirect;
	#endif

	#ifdef USE_CLEARCOAT
    float dotNVcc = saturate(dot( geometryClearcoatNormal, geometryViewDir ));
		vec3 Fcc = F_Schlick(material.clearcoatF0, material.clearcoatF90, dotNVcc);
		outgoingLight = outgoingLight * (1. - material.clearcoat * Fcc) + (clearcoatSpecularDirect + clearcoatSpecularIndirect) * material.clearcoat;
	#endif

	// #include <opaque_fragment>
  #ifdef OPAQUE
  diffuseColor.a = 1.;
  #endif

  #ifdef USE_TRANSMISSION
  diffuseColor.a *= material.transmissionAlpha;
  #endif

  ${buildHeightAlphaFragment(heightAlpha)}

  outFragColor = vec4( outgoingLight, diffuseColor.a );

  ${
    !noOcclusion
      ? /* glsl */ `if (hasBackfaceHit) outFragColor.rgb = mix(outFragColor.rgb, vec3(168. / 255., 190. / 255., 155. / 255.), 0.01);`
      : ''
  }

  ${
    inlineEmissiveBypass
      ? /* glsl */ `
  // Route all emissive (uniform + map + getCustomEmissive, already summed into
  // totalEmissiveRadiance) to the second MRT output → emissiveRT, which skips tone
  // mapping and blooms. Raw linear + un-fogged (FinalPass fogs the composite).
  // Coverage = emissive luminance, so a dark base reads through 0-alpha and bright
  // detail composites over it.
#ifdef INLINE_EMISSIVE_BYPASS
  outEmissiveBypass = vec4(totalEmissiveRadiance, clamp(dot(totalEmissiveRadiance, vec3(0.2126, 0.7152, 0.0722)), 0., 1.));
#endif`
      : ''
  }

	${
    enableFog
      ? /* glsl */ `
  #ifdef USE_FOG
    #ifdef FOG_EXP2
      float fogFactor = 1.0 - exp( - fogDensity * fogDensity * vFogDepth * vFogDepth );
    #else
      float fogFactor = smoothstep( fogNear, fogFar, vFogDepth );
    #endif
    ${typeof fogMultiplier === 'number' ? /* glsl */ `fogFactor *= ${fogMultiplier.toFixed(4)};` : ''}
    // \`totalShadow\` is 0 if the fragment is fully shadowed and 1 if it is fully lit
    vec3 shadowColor = vec3(0.);
    float fogShadowFactor = ${fogShadowFactor.toFixed(4)};
    vec3 shadowedFogColor = mix(fogColor, shadowColor, fogShadowFactor * (1. - totalShadow) * min(fogFactor * 1.5, 1.));
    outFragColor.rgb = mix(outFragColor.rgb, shadowedFogColor, fogFactor);
  #endif
  `
      : ''
  }
	// #include <premultiplied_alpha_fragment>
  #ifdef PREMULTIPLIED_ALPHA
    outFragColor.rgb *= outFragColor.a;
  #endif

  ${pom ? buildPomDebug(pom.debug) : ''}
}`,
    glslVersion: THREE.GLSL3,
  };
};

export class CustomShaderMaterial extends THREE.ShaderMaterial {
  /**
   * Used to determine behavior when the player walks/lands on this surface for things like sound effects.
   */
  public materialClass: MaterialClass = MaterialClass.Default;
  public flatShading: boolean = false;
  public isMeshStandardMaterial = false;
  public isMeshPhysicalMaterial = false;

  public specularMap?: THREE.Texture;
  public specularIntensity: number = 1;
  public specularColor: THREE.Color = new THREE.Color(0xffffff);

  public roughness: number = 1;
  public metalness: number = 0;
  public ior: number = 1.5;

  public map?: THREE.Texture;
  /** Read by `WebGLPrograms.getParameters` to drive the `USE_ENVMAP` defines. */
  public envMap: THREE.Texture | null = null;

  public normalScale?: THREE.Vector2;
  public normalMap?: THREE.Texture;
  public normalMapType?: THREE.NormalMapTypes;
  public roughnessMap?: THREE.Texture;
  public metalnessMap?: THREE.Texture;

  public clearcoat?: number;
  public clearcoatRoughness?: number;
  public clearcoatNormalMap?: THREE.Texture;
  public clearcoatNormalScale?: THREE.Vector2;

  constructor(args: THREE.ShaderMaterialParameters, materialClass: MaterialClass) {
    super(args);
    this.materialClass = materialClass;
  }

  public setCurTimeSeconds(curTimeSeconds: number) {
    this.uniforms.curTimeSeconds.value = curTimeSeconds;
  }

  public get color(): THREE.Color {
    return (this.uniforms.diffuse as any).value;
  }

  public set color(color: THREE.Color) {
    (this.uniforms.diffuse as any).value = color;
  }
}

export const buildCustomShader = (
  props: CustomShaderProps = {},
  shaders?: CustomShaderShaders,
  opts?: CustomShaderOptions
) => {
  const mat = new CustomShaderMaterial(
    buildCustomShaderArgs(props, shaders, opts),
    opts?.materialClass ?? MaterialClass.Default
  );

  if (props.name) {
    mat.name = props.name;
  }
  if (props.side !== null && props.side !== undefined) {
    mat.side = props.side;
  }

  if (opts?.usePackedDiffuseNormalGBA) {
    mat.defines.USE_NORMALMAP_TANGENTSPACE = '1';
    mat.defines.USE_NORMALMAP = '1';
    mat.uniforms.normalScale = { value: new THREE.Vector2(props.normalScale ?? 1, props.normalScale ?? 1) };
  }

  if (props.clearcoat || props.clearcoatRoughness) {
    mat.clearcoat = props.clearcoat ?? 0;
    mat.clearcoatRoughness = props.clearcoatRoughness ?? 0;
  }
  if (props.clearcoatNormalMap) {
    mat.clearcoatNormalMap = props.clearcoatNormalMap;
    mat.clearcoatNormalScale = new THREE.Vector2(
      props.clearcoatNormalScale ?? 1,
      props.clearcoatNormalScale ?? 1
    );
  }

  if (props.iridescence || shaders?.iridescenceShader) {
    mat.defines.USE_IRIDESCENCE = '1';
  }

  if (props.sheen) {
    mat.defines.USE_SHEEN = '1';
  }

  mat.metalness = props.metalness ?? 0;
  mat.roughness = props.roughness ?? 1;

  mat.defines.PHYSICAL = '1';
  mat.defines.USE_UV = '1';
  if (props.map) {
    (mat as any).map = props.map;
    mat.uniforms.map.value = props.map;
  }
  if (props.normalMap) {
    mat.normalMap = props.normalMap;
    mat.normalMapType = props.normalMapType ?? THREE.TangentSpaceNormalMap;
    mat.normalScale = new THREE.Vector2(props.normalScale ?? 1, props.normalScale ?? 1);
    mat.uniforms.normalMap.value = props.normalMap;
  }
  if (props.roughnessMap) {
    mat.roughnessMap = props.roughnessMap;
    mat.uniforms.roughnessMap.value = props.roughnessMap;
  }
  if (props.metalnessMap) {
    mat.metalnessMap = props.metalnessMap;
    mat.uniforms.metalnessMap.value = props.metalnessMap;
  }
  if (props.pomHeightMap && mat.uniforms.pomHeightMap) {
    mat.uniforms.pomHeightMap.value = props.pomHeightMap;
  }
  if (props.clearcoatNormalMap) {
    mat.clearcoatNormalMap = props.clearcoatNormalMap;
    mat.uniforms.clearcoatNormalMap.value = props.clearcoatNormalMap;
  }
  if (props.emissiveIntensity !== undefined) {
    (mat as any).emissiveIntensity = props.emissiveIntensity;
    mat.uniforms.emissiveIntensity.value = props.emissiveIntensity;
  }
  if (props.transparent) {
    (mat as any).transparent = props.transparent;
  }
  if (typeof props.opacity === 'number') {
    (mat as any).opacity = props.opacity;
    mat.uniforms.opacity.value = props.opacity;
  }
  if (typeof props.alphaTest === 'number') {
    (mat as any).alphaTest = props.alphaTest;
    mat.uniforms.alphaTest.value = props.alphaTest;
  }
  if (props.transmission) {
    mat.defines.USE_TRANSMISSION = '1';
    mat.uniforms.transmission.value = props.transmission;
  }
  if (props.transmissionMap) {
    mat.defines.USE_TRANSMISSION = '1';
    mat.defines.USE_TRANSMISSIONMAP = '1';
    mat.uniforms.transmissionMap.value = props.transmissionMap;
  }
  if (opts?.vertexLighting) {
    mat.userData.vertexLighting = true;
  }
  if (props.envMap) {
    mat.userData.envMapOverride = props.envMap;
  }
  if (props.envMapIntensity !== undefined) {
    mat.userData.envMapIntensityOverride = props.envMapIntensity;
  }
  applySceneEnvironmentToMaterial(mat);
  if (typeof opts?.usePackedDiffuseNormalGBA === 'object') {
    const dataTexture = new THREE.DataTexture(
      opts.usePackedDiffuseNormalGBA.lut,
      256,
      1,
      THREE.RGBAFormat,
      THREE.UnsignedByteType,
      THREE.UVMapping,
      THREE.ClampToEdgeWrapping,
      THREE.ClampToEdgeWrapping,
      THREE.NearestFilter,
      THREE.NearestFilter
    );
    dataTexture.needsUpdate = true;
    mat.uniforms.diffuseLUT = { value: dataTexture };
  }

  mat.needsUpdate = true;
  mat.uniformsNeedUpdate = true;

  if (opts?.disableToneMapping) {
    mat.userData.emissiveBypass = true;
  }
  if (opts?.inlineEmissiveBypass) {
    if (opts.disableToneMapping) {
      throw new Error(
        'inlineEmissiveBypass and disableToneMapping are mutually exclusive (inline-MRT vs whole-mesh bypass).'
      );
    }
    mat.defines.INLINE_EMISSIVE_BYPASS = '1';
    mat.userData.inlineEmissiveBypass = true;
  }
  if (opts?.noOcclusion) {
    mat.userData.occlusionExclude = true;
  } else {
    setMaterialOcclusionBackfaceRendering(mat, occlusionBackfaceRenderingEnabled);
  }

  if (opts?.pom?.boundedSilhouette) {
    mat.userData.skipDepthPrepass = true;
    mat.depthFunc = THREE.LessEqualDepth;
    mat.depthWrite = true;
    mat.depthTest = true;
    mat.userData[POM_BOUNDED_SILHOUETTE_FLAG] = true;
  }

  if (opts?.randomizeUVOffset) {
    mat.onBeforeRender = (_renderer, _scene, _camera, _geometry, object) => {
      const explicit = (object as { userData?: { uvOffsetSeed?: number } }).userData?.uvOffsetSeed;
      const seed =
        typeof explicit === 'number' && Number.isFinite(explicit)
          ? explicit
          : ((object.id * 2654435761) >>> 0) / 4294967296;
      mat.uniforms.uvOffsetSeed.value = seed;
      // this is needed, otherwise this won't be set properly when the order of objects changes and stuff like that
      mat.uniformsNeedUpdate = true;
    };
  }

  return mat;
};
