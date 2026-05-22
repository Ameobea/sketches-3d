import * as THREE from 'three';
import { UniformsLib } from 'three';

import commonShaderCode from './common.frag?raw';
import softOcclusionPreamble from './softOcclusionPreamble.frag?raw';
import softOcclusionDiscard from './softOcclusionDiscard.frag?raw';
import CustomLightsFragmentBegin from './customLightsFragmentBegin.frag?raw';
import GeneratedUVsFragment from './generatedUVs.vert?raw';
import depthExactVertexBody from './depthExactVertex.glsl?raw';
import noiseShaders from './noise.frag?raw';
import tileBreakingNeyretFragment from './tileBreakingNeyret.frag?raw';
import { buildTriplanarDefsFragment, type TriplanarMappingParams } from './triplanarMapping';
import ssrDefsFragment from './ssr/ssrDefs.frag?raw';
import { buildReverseColorRampGenerator, ReverseColorRampCommonFunctions } from './reverseColorRamp';
import {
  buildPomDefs,
  buildPomMainBlock,
  buildPomUniformDecls,
  buildPomNormalApply,
  buildPomHeightSources,
  buildPomDebug,
  POM_BOUNDED_SILHOUETTE_FLAG,
  type PomTexturing,
} from './pom';
import { MaterialClass } from './customShader.types';
import type {
  AmbientDistanceAmpParams,
  ReflectionParams,
  CustomShaderProps,
  CustomShaderShaders,
  CustomShaderOptions,
} from './customShader.types';
import { buildHeightAlphaEarlyOut, buildHeightAlphaFragment } from './heightAlpha';
import VERTEX_LIGHTING_FRAGMENT from './vertexLighting.frag?raw';
import PLAYER_SHADOW_FRAGMENT from './playerShadow.frag?raw';

export { MaterialClass } from './customShader.types';
export type {
  AmbientDistanceAmpParams,
  ReflectionParams,
  CustomShaderProps,
  CustomShaderShaders,
  CustomShaderOptions,
} from './customShader.types';

// import noise2Shaders from './noise2.frag?raw';
const noise2Shaders = 'DISABLED TO SAVE SPACE';

const DEFAULT_MAP_DISABLE_DISTANCE = 2000;

/**
 * Builds a GLSL expression for sampling a texture using the configured tile-breaking mode.
 * Does not include a swizzle — append `.xyz` etc. as needed at the call site.
 */
const buildTileBreakSampleExpr = (
  sampler: string,
  uv: string,
  tileBreaking: CustomShaderOptions['tileBreaking']
): string =>
  tileBreaking
    ? /* glsl */ `textureNoTileNeyret(${sampler}, ${uv})`
    : /* glsl */ `texture2D(${sampler}, ${uv})`;

const AntialiasedRoughnessShaderFragment = /* glsl */ `
  float roughnessAcc = 0.;
  // 2x oversampling
  for (int i = 0; i < 2; i++) {
    for (int j = 0; j < 2; j++) {
      for (int k = 0; k < 2; k++) {
        vec3 offsetPos = vWorldPos;
        // TODO use better method, only sample in plane the fragment lies on rather than in 3D
        offsetPos.x += ((float(k) - 1.) * 0.5) * unitsPerPx;
        offsetPos.y += ((float(i) - 1.) * 0.5) * unitsPerPx;
        offsetPos.z += ((float(j) - 1.) * 0.5) * unitsPerPx;
        roughnessAcc += getCustomRoughness(offsetPos, vObjectNormal, roughnessFactor, curTimeSeconds, ctx);
      }
    }
  }
  roughnessAcc /= 8.;
  roughnessFactor = roughnessAcc;
`;

const NonAntialiasedRoughnessShaderFragment =
  /* glsl */ `roughnessFactor = getCustomRoughness(vWorldPos, vObjectNormal, roughnessFactor, curTimeSeconds, ctx);`;

const buildRoughnessShaderFragment = (antialiasRoughnessShader?: boolean) => {
  if (antialiasRoughnessShader) {
    return AntialiasedRoughnessShaderFragment;
  }

  return NonAntialiasedRoughnessShaderFragment;
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

const buildUVVertexFragment = (randomizeUVOffset: boolean | undefined): string => {
  if (randomizeUVOffset) {
    return /* glsl */ `
      #ifdef USE_UV
        vec2 uvOffset = hashSeedToVec2(uvOffsetSeed);

        // Add \`uvOffset\` to \`vUv\` to randomize the UVs.
        float uvScaleX = uvTransform[0][0];
        float uvScaleY = uvTransform[1][1];
        mat3 newUVTransform = mat3(
          uvScaleX, 0., 0.,
          0., uvScaleY, 0.,
          uvOffset.x, uvOffset.y, uvOffset.x
        );
        vUv = ( newUVTransform * vec3( vUv, 1 ) ).xy;
      #endif
      `;
  }

  return '';
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
    return /* glsl */ `
  vec4 acc = vec4(0.);
  // 2x oversampling
  for (int i = 0; i < 2; i++) {
    for (int j = 0; j < 2; j++) {
      for (int k = 0; k < 2; k++) {
        vec3 offsetPos = ${posSym};
        // TODO use better method, only sample in plane the fragment lies on rather than in 3D
        offsetPos.x += ((float(k) - 1.) * 0.5) * unitsPerPx;
        offsetPos.y += ((float(i) - 1.) * 0.5) * unitsPerPx;
        offsetPos.z += ((float(j) - 1.) * 0.5) * unitsPerPx;
        acc += getFragColor(diffuseColor.xyz, offsetPos, ${normalSym}, curTimeSeconds, ctx);
      }
    }
  }
  acc /= 8.;
  diffuseColor = acc;
  ctx.diffuseColor = diffuseColor;`;
  } else {
    return /* glsl */ `
  diffuseColor = getFragColor(diffuseColor.xyz, ${posSym}, ${normalSym}, curTimeSeconds, ctx);
  ctx.diffuseColor = diffuseColor;`;
  }
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
    .replace('__AMBIENT_LIGHT_SCALE__', ambientLightScale.toFixed(4))
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

const setMaterialOcclusionBackfaceRendering = (mat: CustomShaderMaterial, enable: boolean) => {
  const targetSide = enable ? THREE.DoubleSide : THREE.FrontSide;
  const targetShadowSide = enable ? THREE.BackSide : null;

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
      #ifdef USE_INSTANCING
        attribute mat4 instanceMatrix;
      #endif
      varying vec3 vWorldPos;
      varying vec3 vWorldNormal;
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
      #ifdef USE_INSTANCING
        attribute mat4 instanceMatrix;
      #endif
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

const DefaultReflectionParams: ReflectionParams = Object.freeze({ alpha: 1 });

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
    metalnessMap: _metalnessMap,
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
    reflection: providedReflectionParams,
    heightAlpha,
    transparent,
  }: CustomShaderProps = {},
  {
    customVertexFragment,
    commonShader,
    colorShader,
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
  }: CustomShaderOptions = {}
) => {
  const uniforms = THREE.UniformsUtils.merge([
    UniformsLib.common,
    // UniformsLib.envmap,
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
      // envMapIntensity: { value: 1 },
    },
  ]);
  uniforms.normalScale = { value: new THREE.Vector2(normalScale, normalScale) };

  const triplanarUsesWorldSpace = useTriplanarMapping ? (useWorldSpaceUVs ?? true) : false;
  const generatedUVsUseWorldSpace = useGeneratedUVs ? (useWorldSpaceUVs ?? false) : false;

  const pomTexturing: PomTexturing = useTriplanarMapping
    ? 'triplanar'
    : useGeneratedUVs
      ? 'generated'
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
    uniforms.sheenColor = {
      value: (() => {
        const col = typeof sheenColor === 'number' ? new THREE.Color(sheenColor) : sheenColor;
        return col.multiplyScalar(sheen);
      })(),
    };
    uniforms.sheenRoughness = { value: sheenRoughness };
  }
  uniforms.transmission = { value: transmission };
  uniforms.transmissionMap = { value: transmissionMap };
  uniforms.transmissionSamplerSize = { value: new THREE.Vector2() };
  uniforms.transmissionSamplerMap = { value: null };

  uniforms.curTimeSeconds = { value: 0.0 };
  if (pom) {
    uniforms.pomDepth = { value: pom.depth };
    if (pom.boundedSilhouette) {
      uniforms.pomBackDepth = { value: null };
      uniforms.pomResolution = { value: new THREE.Vector2(1, 1) };
    }
    if (pomHeightMap) {
      uniforms.pomHeightMap = { value: pomHeightMap };
    }
  }
  uniforms.diffuse = { value: typeof color === 'number' ? new THREE.Color(color) : color };
  uniforms.mapTransform = { value: new THREE.Matrix3().identity() };
  if (uvTransform) {
    uniforms.uvTransform = { value: uvTransform };
  }
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

  const usingSSR = !!providedReflectionParams;

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
  }

  const mapDisableDistance =
    rawMapDisableDistance === undefined ? DEFAULT_MAP_DISABLE_DISTANCE : rawMapDisableDistance;

  const triplanarPosSym = pom ? 'triplanarSamplePos' : 'vTriplanarPos';
  const triplanarNormalSym = pom ? '_pomNormalW' : 'vTriplanarNormal';

  const pomGen = !!pom && pomTexturing === 'generated';
  const mapUvSym = pomGen ? '_pomGenUv' : 'vMapUv';

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
      lodFadeStart,
      lodFadeEnd,
      pomRefinement,
      pomBinarySteps,
      pomRefineSkip,
      pomHasNormalShader: !!pomNormalShader,
      pomDebug: pom.debug,
    });
  };

  const buildMapFragment = () => {
    const inner = (() => {
      if (useTriplanarMapping) {
        return /* glsl */ `
        #ifdef USE_MAP
          sampledDiffuseColor_ = triplanarTextureFixContrast(map, ${triplanarPosSym}, vec2(uvTransform[0][0], uvTransform[1][1]), ${triplanarNormalSym});
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

      vec4 averageTextureColor = texture(map, vec2(0.5, 0.5), 99.);
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
          vec3 texelRoughness = triplanarTexture(roughnessMap, ${triplanarPosSym}, vec2(uvTransform[0][0], uvTransform[1][1]), ${triplanarNormalSym}).xyz;
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
          vec3 perturbedNormal = triplanarTextureNormalMap(normalMap, ${triplanarPosSym}, vec2(uvTransform[0][0], uvTransform[1][1]), ${triplanarNormalSym}, normalScale).xyz;
          normal = normalize(${transform});
          `;
      }

      if (tileBreaking)
        return /* glsl */ `
    vec3 mapN = ${buildTileBreakSampleExpr('normalMap', 'vMapUv', tileBreaking)}.xyz;

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
          vec3 perturbedClearcoatNormal = triplanarTextureNormalMap(clearcoatNormalMap, ${triplanarPosSym}, vec2(uvTransform[0][0], uvTransform[1][1]), ${triplanarNormalSym}, clearcoatNormalScale).xyz;
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

  return {
    fog: true,
    lights: true,
    dithering: false,
    transparent: transparent ?? false,
    uniforms,
    vertexShader: /* glsl */ `
#define STANDARD
varying vec3 vViewPosition;
// #ifdef USE_TRANSMISSION
  varying vec3 vWorldPosition;
// #endif

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

${displacementShader || ''}

${useDisplacementNormals ? 'attribute vec3 displacementNormal;' : ''}

uniform float curTimeSeconds;
varying vec3 vWorldPos;
varying vec3 vObjectNormal;
varying vec3 vWorldNormal;
uniform mat3 uvTransform;
${randomizeUVOffset ? 'uniform float uvOffsetSeed;' : ''}
${randomizeUVOffset ? hashSeedToVec2GLSL : ''}
${useTriplanarMapping ? 'varying vec3 vTriplanarPos;' : ''}
${useTriplanarMapping ? 'varying vec3 vTriplanarNormal;' : ''}
${useTriplanarMapping && randomizeUVOffset ? hashSeedToVec3GLSL : ''}

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

  #include <worldpos_vertex>

  vec4 worldPositionMine = vec4(transformed, 1.);
  worldPositionMine = modelMatrix * worldPositionMine;
  vWorldPos = worldPositionMine.xyz;

  #ifdef USE_INSTANCING
    vWorldPos = (instanceMatrix * vec4(vWorldPos, 1.)).xyz;
  #endif

  vObjectNormal = normal;
  vWorldNormal = normalize((modelMatrix * vec4(normal, 0.)).xyz);

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
      const uvNormal = generatedUVsUseWorldSpace ? 'worldNormal' : 'normal';
      return /* glsl */ `
      vec3 worldNormal = normalize(mat3(modelMatrix[0].xyz, modelMatrix[1].xyz, modelMatrix[2].xyz) * normal);
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

  ${buildUVVertexFragment(randomizeUVOffset)}

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
${usingSSR ? 'layout(location = 1) out vec4 outReflectionData;' : ''}

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
uniform mat4 modelMatrix;
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
#include <lights_physical_pars_fragment>
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
varying vec3 vWorldPos;
${buildPomUniformDecls(!!pom, pomBounded, !!pomHeightMap)}

uniform vec3 playerShadowPos;
uniform vec4 playerShadowParams; // x=radius, y=intensity, z=centerReceiverY, w=maxReceiverY (highest probe, for early-out)
uniform float psRingData[16]; // [0..7]: outer ring receiverY (angles 0-7), [8..15]: inner ring (angles 0-7)

${softOcclusionPreamble}

#ifndef USE_TRANSMISSION
  varying vec3 vWorldPosition;
#endif

varying vec3 vObjectNormal;
varying vec3 vWorldNormal;
uniform mat3 uvTransform;
${useTriplanarMapping ? 'varying vec3 vTriplanarPos;' : ''}
${useTriplanarMapping ? 'varying vec3 vTriplanarNormal;' : ''}

${normalShader ? 'uniform mat3 normalMatrix;' : ''}
// ${usePackedDiffuseNormalGBA ? 'uniform vec2 normalScale;' : ''}
${typeof usePackedDiffuseNormalGBA === 'object' ? 'uniform sampler2D diffuseLUT;' : ''}

struct SceneCtx {
  vec3 cameraPosition;
  vec2 vUv;
  vec4 diffuseColor;
  // Base-mesh world pos/normal, stable across POM displacement (unlike the
  // per-sample \`pos\` / per-pixel \`normal\` color shaders normally see).
  vec3 vWorldPos;
  vec3 vWorldNormal;
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
        (sampler, uv) => buildTileBreakSampleExpr(sampler, uv, tileBreaking),
        tileBreaking ? 'neyret' : 'none'
      )
    : ''
}
${pomGen ? GeneratedUVsFragment : ''}

${commonShader ?? ''}

${colorShader ?? ''}
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
${pom ? buildPomHeightSources({ hasHeightShader: !!pomHeightShader, hasHeightMap: !!pomHeightMap, pomTexturing }) : ''}
${pom && pomNormalShader ? pomNormalShader : ''}
${buildPomDefsFragment()}
${usingSSR ? ssrDefsFragment : ''}

void main() {
	#include <clipping_planes_fragment>

  ${!noOcclusion ? softOcclusionDiscard : ''}

  float distanceToCamera = distance(cameraPosition, vWorldPos);
  float unitsPerPx = abs(2. * distanceToCamera * tan(0.001 / 2.));
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

  SceneCtx ctx = SceneCtx(cameraPosition, vMapUv, diffuseColor, vWorldPos, vWorldNormal);

  ${pom ? buildPomMainBlock(pomBounded, pomTexturing, pom.normalEps) : ''}

	ReflectedLight reflectedLight = ReflectedLight( vec3( 0.0 ), vec3( 0.0 ), vec3( 0.0 ), vec3( 0.0 ) );
	vec3 totalEmissiveRadiance = emissive;
	#include <logdepthbuf_fragment>
  vec4 sampledDiffuseColor_ = diffuseColor;
	${buildMapFragment()}

	#include <color_fragment>
  ctx.diffuseColor = diffuseColor;
	#include <alphamap_fragment>
	#include <alphatest_fragment>
  ${buildRoughnessMapFragment()}
	#include <metalnessmap_fragment>
	#include <normal_fragment_begin>
  ${buildNormalMapFragment()}
  ${pom ? buildPomNormalApply(pomTexturing, !!normalMap) : ''}

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
    totalEmissiveRadiance = getCustomEmissive(vWorldPos, totalEmissiveRadiance, curTimeSeconds, ctx);
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

	vec3 totalDiffuse = reflectedLight.directDiffuse + reflectedLight.indirectDiffuse;
	vec3 totalSpecular = reflectedLight.directSpecular + reflectedLight.indirectSpecular;
  `
  }

  ${PLAYER_SHADOW_FRAGMENT}

	#include <transmission_fragment>

	vec3 outgoingLight = totalDiffuse + totalSpecular + totalEmissiveRadiance;

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
    vec3 shadowedFogColor = mix(fogColor, shadowColor, fogShadowFactor * (1. - totalShadow) * clamp(0., 1., fogFactor * 1.5));
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
  public needsSSRBuffer = false;

  public specularMap?: THREE.Texture;
  public specularIntensity: number = 1;
  public specularColor: THREE.Color = new THREE.Color(0xffffff);

  public roughness: number = 1;
  public metalness: number = 0;
  public ior: number = 1.5;

  public map?: THREE.Texture;

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
  mat.needsSSRBuffer = !!props.reflection;

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

  if (props.reflection) {
    const reflectionParams = { ...DefaultReflectionParams, ...props.reflection };
    mat.defines.SSR_ALPHA = Math.min(reflectionParams.alpha, 0.9999).toFixed(4);
  } else {
    mat.defines.SSR_ALPHA = '0.';
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
