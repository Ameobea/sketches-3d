import * as THREE from 'three';
import { UniformsLib } from 'three';

import commonShaderCode from './common.frag?raw';
import softOcclusionPreamble from './softOcclusionPreamble.frag?raw';
import softOcclusionDiscard from './softOcclusionDiscard.frag?raw';
import CustomLightsFragmentBegin from './customLightsFragmentBegin.frag?raw';
import tileBreakingFragment from './fasterTileBreakingFixMipmap.frag?raw';
import GeneratedUVsFragment from './generatedUVs.vert?raw';
import depthExactVertexBody from './depthExactVertex.glsl?raw';
import noiseShaders from './noise.frag?raw';
import tileBreakingNeyretFragment from './tileBreakingNeyret.frag?raw';
import { buildTriplanarDefsFragment, type TriplanarMappingParams } from './triplanarMapping';
import ssrDefsFragment from './ssr/ssrDefs.frag?raw';
import { buildReverseColorRampGenerator, ReverseColorRampCommonFunctions } from './reverseColorRamp';
import { MaterialClass } from './customShader.types';
import type {
  AmbientDistanceAmpParams,
  ReflectionParams,
  CustomShaderProps,
  CustomShaderShaders,
  CustomShaderOptions,
} from './customShader.types';

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
const fastFixMipMapTileBreakingScale = (240.2).toFixed(3);

/**
 * Builds a GLSL expression for sampling a texture using the configured tile-breaking mode.
 * Does not include a swizzle — append `.xyz` etc. as needed at the call site.
 */
const buildTileBreakSampleExpr = (
  sampler: string,
  uv: string,
  tileBreaking: CustomShaderOptions['tileBreaking']
): string => {
  if (!tileBreaking) return `texture2D(${sampler}, ${uv})`;
  if (tileBreaking.type === 'neyret') return `textureNoTileNeyret(${sampler}, ${uv})`;
  return `textureNoTile(${sampler}, noiseSampler, ${uv}, 0., ${fastFixMipMapTileBreakingScale})`;
};

const buildNoiseTexture = (): THREE.DataTexture => {
  const noise = new Float32Array(256 * 256 * 4);
  for (let i = 0; i < noise.length; i++) {
    noise[i] = Math.random();
  }
  const texture = new THREE.DataTexture(
    noise,
    256,
    256,
    THREE.RGBAFormat,
    THREE.FloatType,
    undefined,
    THREE.RepeatWrapping,
    THREE.RepeatWrapping,
    // We need linear interpolation for the noise texture
    THREE.LinearFilter,
    THREE.LinearFilter
  );
  texture.needsUpdate = true;
  return texture;
};

const AntialiasedRoughnessShaderFragment = `
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
  'roughnessFactor = getCustomRoughness(vWorldPos, vObjectNormal, roughnessFactor, curTimeSeconds, ctx);';

const buildRoughnessShaderFragment = (antialiasRoughnessShader?: boolean) => {
  if (antialiasRoughnessShader) {
    return AntialiasedRoughnessShaderFragment;
  }

  return NonAntialiasedRoughnessShaderFragment;
};

const buildUnpackDiffuseNormalGBAFragment = (params: true | { lut: Uint8Array }): string => {
  if (params === true) {
    return `
    mapN = sampledDiffuseColor_.gba;
    sampledDiffuseColor_ = vec4(sampledDiffuseColor_.rrr, 1.);
  `;
  } else {
    return `
    mapN = sampledDiffuseColor_.gba;
    float index = sampledDiffuseColor_.r;
    vec4 lutEntry = texelFetch(diffuseLUT, ivec2(index * 255., 0), 0);
    sampledDiffuseColor_ = lutEntry;
      `;
  }
};

const buildUVVertexFragment = (randomizeUVOffset: boolean | undefined): string => {
  if (randomizeUVOffset) {
    return `
      #ifdef USE_UV
        float modelWorldX = modelMatrix[3][0];
        float modelWorldY = modelMatrix[3][1];
        float modelWorldZ = modelMatrix[3][2];
        vec3 modelWorld = vec3(modelWorldX, modelWorldY, modelWorldZ);

        // hash x, y, z
        float hash = fract(sin(dot(modelWorld, vec3(12.9898, 78.233, 45.164))) * 43758.5453);

        vec2 uvOffset = vec2(
          fract(hash * 3502.2),
          fract(hash * 3200.)
        );

        // Add \`uvUffset\` to \`vUv\` to randomize the UVs.
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
  antialiasColorShader: boolean | undefined
): string => {
  if (!colorShader) {
    return '';
  }

  if (antialiasColorShader) {
    return `
  vec4 acc = vec4(0.);
  // 2x oversampling
  for (int i = 0; i < 2; i++) {
    for (int j = 0; j < 2; j++) {
      for (int k = 0; k < 2; k++) {
        vec3 offsetPos = vWorldPos;
        // TODO use better method, only sample in plane the fragment lies on rather than in 3D
        offsetPos.x += ((float(k) - 1.) * 0.5) * unitsPerPx;
        offsetPos.y += ((float(i) - 1.) * 0.5) * unitsPerPx;
        offsetPos.z += ((float(j) - 1.) * 0.5) * unitsPerPx;
        acc += getFragColor(diffuseColor.xyz, offsetPos, vObjectNormal, curTimeSeconds, ctx);
      }
    }
  }
  acc /= 8.;
  diffuseColor = acc;
  ctx.diffuseColor = diffuseColor;`;
  } else {
    return `
  diffuseColor = getFragColor(diffuseColor.xyz, vWorldPos, vObjectNormal, curTimeSeconds, ctx);
  ctx.diffuseColor = diffuseColor;`;
  }
};

const buildRunIridescenceShaderFragment = (iridescenceShader: string | undefined): string => {
  if (!iridescenceShader) {
    return '';
  }

  return `
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

  return `
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

/**
 * Batch-toggle backface rendering on all CustomShaderMaterial instances in a scene.
 * When `enable` is true, materials that support occlusion get DoubleSide;
 * when false, they get FrontSide (saving vertex shader work for backfaces).
 */
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
 * Eagerly compile both the FrontSide and DoubleSide+BackSide shadow-side variants of
 * every `CustomShaderMaterial` in the scene. Toggling `shadowSide` at runtime changes
 * the program cache key in three.js, so the first occlusion-induced switch causes a
 * fresh shader compile/link — visible as a hitch. Call this once during scene load
 * for scenes that have (or can switch into) third-person mode to pay that cost up
 * front while a loading screen is showing.
 */
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
  psRingData.identity();
  occlusionParams.set(0, 0, 0, 0);
};

/**
 * Shared uniforms for player shadow, referenced by all `CustomShaderMaterial` instances.
 * `playerShadowPos` is updated per-frame with player feet position.
 * `playerShadowParams` packs (radius, intensity, centerReceiverY, centerDropDistance).
 * `psRingData` is a mat4 storing per-angle receiverY for 8 angles on 2 concentric rings
 * (cols 0-1 = outer at radius, cols 2-3 = inner at radius/2), enabling polar bilinear
 * interpolation for partial overhang shadows.
 * Left at default zeros to disable (intensity=0 → early-out in shader).
 */
const playerShadowPos = new THREE.Vector3();
const playerShadowParams = new THREE.Vector4(0, 0, 0, 0);
// mat4 packing: columns 0-1 = outer ring receiverY (angles 0-7), columns 2-3 = inner ring (angles 0-7)
const psRingData = new THREE.Matrix4();

export const getPlayerShadowUniforms = () => ({
  playerShadowPos,
  playerShadowParams,
  psRingData,
});

/**
 * Shared uniforms for soft camera occlusion dithering, referenced by all CustomShaderMaterial instances.
 * `occlusionStart` = player eye position, `occlusionEnd` = camera position.
 * `occlusionParams`: x=revealRadius, y=revealFade, z=active(0|1), w=unused.
 * Set z=0 to disable (shader early-outs with no discard).
 */
const occlusionStart = new THREE.Vector3();
const occlusionEnd = new THREE.Vector3();
const occlusionParams = new THREE.Vector4(0, 0, 0, 0);

export const getOcclusionUniforms = () => ({
  occlusionStart,
  occlusionEnd,
  occlusionParams,
});

/**
 * Creates a minimal ShaderMaterial for use as the depth pre-pass override material.
 * It mirrors the Bayer dither discard logic from the main CustomShaderMaterial so that
 * the depth buffer matches what the main pass will actually render.
 *
 * Shares the same uniform objects as `getOcclusionUniforms()` so updates are automatic.
 */
export const buildOcclusionDepthMaterial = (): THREE.ShaderMaterial =>
  new THREE.ShaderMaterial({
    vertexShader: `
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
    fragmentShader: `
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
 * A bit-exact depth-only override material with no dithering. Used by the depth pre-pass
 * for `noOcclusion` meshes (player, `nonPermeable` walls) — they need their depth written
 * so downstream consumers (SkyStack's `discardIfOccluded`, FinalPass's sky-bypass detection,
 * and the FinalPass emissive composite gate) see them as scene geometry, but they should
 * never get the camera-occlusion dither pattern punched into them.
 */
export const buildPlainDepthMaterial = (): THREE.ShaderMaterial =>
  new THREE.ShaderMaterial({
    vertexShader: `
      #ifdef USE_INSTANCING
        attribute mat4 instanceMatrix;
      #endif
      void main() {
        ${depthExactVertexBody}
      }
    `,
    fragmentShader: `
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
const buildRunVertexLightingFragment = (vertexLightingShininess: number) => `
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
          ? `{
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
          ? `{
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
          ? `{
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

/**
 * Builds the GLSL that computes `heightAlphaFactor` from `vWorldPos.y` and, when the
 * factor is exactly 0, writes black and returns immediately — skipping all texture
 * lookups, lighting, and shadow work.  Emitted near the top of `main()`.
 */
const buildHeightAlphaEarlyOut = (
  heightAlpha: { bottomFade?: [number, number]; topFade?: [number, number] } | undefined
): string => {
  if (!heightAlpha) return '';
  const { bottomFade, topFade } = heightAlpha;
  if (!bottomFade && !topFade) return '';

  const lines: string[] = [];
  if (bottomFade) {
    lines.push(
      `heightAlphaFactor *= smoothstep(${bottomFade[0].toFixed(3)}, ${bottomFade[1].toFixed(3)}, vWorldPos.y);`
    );
  }
  if (topFade) {
    lines.push(
      `heightAlphaFactor *= 1.0 - smoothstep(${topFade[0].toFixed(3)}, ${topFade[1].toFixed(3)}, vWorldPos.y);`
    );
  }

  return `
    float heightAlphaFactor = 1.0;
    ${lines.join('\n    ')}
    if (heightAlphaFactor < 0.001) {
      outFragColor = vec4(0.0, 0.0, 0.0, 1.0);
      return;
    }
  `;
};

/**
 * Applies the height-alpha darkening to `outgoingLight` using the already-computed
 * `heightAlphaFactor`.  Emitted near the end of `main()`.
 */
const buildHeightAlphaFragment = (
  heightAlpha: { bottomFade?: [number, number]; topFade?: [number, number] } | undefined
): string => {
  if (!heightAlpha) return '';
  const { bottomFade, topFade } = heightAlpha;
  if (!bottomFade && !topFade) return '';

  // heightAlphaFactor was already computed by the early-out block; just apply it.
  return `{
    // heightAlphaFactor computed earlier near top of main()
    outgoingLight.rgb = mix(outgoingLight.rgb, vec3(0.0), 1. - heightAlphaFactor);
  }`;
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
    metalnessMap: _metalnessMap,
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
    includeNoiseShadersVertex,
  }: CustomShaderShaders = {},
  {
    antialiasColorShader,
    antialiasRoughnessShader,
    tileBreaking,
    useNoise2,
    enableFog = true,
    useComputedNormalMap,
    usePackedDiffuseNormalGBA,
    readRoughnessMapFromRChannel,
    disableToneMapping: _disableToneMapping,
    disabledDirectionalLightIndices,
    disabledSpotLightIndices,
    randomizeUVOffset,
    useGeneratedUVs,
    useWorldSpaceGeneratedUVs,
    useTriplanarMapping,
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

  if (tileBreaking?.type === 'fastFixMipmap') {
    uniforms.noiseSampler = { value: buildNoiseTexture() };
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
  // TODO: Need to handle swapping uvs to `uv2` if light map is provided
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

  // if (tileBreaking && !map && !normalMap && !clearcoatNormalMap && !roughnessMap && !metalnessMap) {
  //   throw new Error('Tile breaking requires a map');
  // }

  if (
    normalMap &&
    tileBreaking &&
    normalMapType !== undefined &&
    normalMapType !== THREE.TangentSpaceNormalMap
  ) {
    throw new Error('Tile breaking requires a normal map with tangent space');
  }

  if (useComputedNormalMap && normalMap) {
    throw new Error('Cannot use computed normal map with a normal map');
  }

  if (usePackedDiffuseNormalGBA && !map) {
    throw new Error('Cannot use packed diffuse/normal map without a map');
  }
  if (usePackedDiffuseNormalGBA && normalMap) {
    throw new Error('Cannot use packed diffuse/normal map with a normal map');
  }
  if (usePackedDiffuseNormalGBA && useComputedNormalMap) {
    throw new Error('Cannot use packed diffuse/normal map with computed normal map');
  }
  // if (useGeneratedUVs && !map) {
  //   throw new Error('Cannot use generated UVs without a map');
  // }
  if (useTriplanarMapping && (useGeneratedUVs || !!tileBreaking)) {
    // We could technically use it with tile breaking, but at that point we'd be doing up to like
    // 3 * 3 * 3 = 27 texture lookups per fragment which is a bit ridiculous and there's no way
    // it would look good either.
    throw new Error('Triplanar mapping cannot be used with generated UVs or tile breaking');
  }
  // if (useTriplanarMapping && !map) {
  //   throw new Error('Triplanar mapping requires a map');
  // }
  if (typeof usePackedDiffuseNormalGBA === 'object' && usePackedDiffuseNormalGBA.lut && tileBreaking) {
    throw new Error('LUT and tile breaking are currently broken together');
  }

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

  const buildMapFragment = () => {
    const inner = (() => {
      if (useTriplanarMapping) {
        return `
        #ifdef USE_MAP
          sampledDiffuseColor_ = triplanarTextureFixContrast(map, vWorldPos, vec2(uvTransform[0][0], uvTransform[1][1]), vWorldNormal);
        #endif`;
      }

      if (!tileBreaking) {
        return `
        #ifdef USE_MAP
          vec4 sampledDiffuseColor = texture2D( map, vMapUv );
          sampledDiffuseColor_ = sampledDiffuseColor;
        #endif`;
      }

      return `sampledDiffuseColor_ = ${buildTileBreakSampleExpr('map', 'vMapUv', tileBreaking)};`;
    })();

    if (typeof mapDisableDistance !== 'number') {
      return `
      #ifdef USE_MAP
        ${usePackedDiffuseNormalGBA ? 'vec3 mapN = vec3(0.);' : ''}
        ${inner}
        ${usePackedDiffuseNormalGBA ? buildUnpackDiffuseNormalGBAFragment(usePackedDiffuseNormalGBA) : ''}
        diffuseColor *= sampledDiffuseColor_;
      #endif`;
    }

    return `
    #ifdef USE_MAP
      ${usePackedDiffuseNormalGBA ? 'vec3 mapN = vec3(0.);' : ''}

      vec4 averageTextureColor = texture(map, vec2(0.5, 0.5), 99.);
      if (textureActivation < 0.01) {
        diffuseColor *= averageTextureColor;
        // avoid any texture lookups, relieve pressure on the texture unit
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
        return `
          vec3 texelRoughness = triplanarTexture(roughnessMap, vWorldPos, vec2(uvTransform[0][0], uvTransform[1][1]), vWorldNormal).xyz;
        `;
      }

      if (tileBreaking && roughnessMap)
        return `vec3 texelRoughness = ${buildTileBreakSampleExpr('roughnessMap', 'vMapUv', tileBreaking)}.xyz;`;
      else
        return `
      vec4 texelRoughness = texture2D( roughnessMap, vMapUv );
      `;
    })();

    if (typeof mapDisableDistance !== 'number') {
      return `
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

    return `
      float roughnessFactor = roughness;
      #ifdef USE_ROUGHNESSMAP
        if (textureActivation < 0.01) {
          // avoid any texture lookups, relieve pressure on the texture unit
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
    // \/ this works very poorly due to aliasing issues
    if (useComputedNormalMap) {
      return `
      float diffuseMagnitude = diffuseColor.r;
      float dDiffuseX = dFdx(diffuseMagnitude);
      float dDiffuseY = dFdy(diffuseMagnitude);

      float computeNormalBias = 0.1;
      vec3 mapN = normalize(vec3(
        dDiffuseX,
        dDiffuseY,
        1.0 - ((computeNormalBias - 0.1) / 100.0)
      ));

      mapN.xy *= normalScale;

      #ifdef USE_NORMALMAP_TANGENTSPACE
        normal = normalize( tbn * mapN );
      #else
        UNIMPLEMENTED_1
      #endif
    `;
    }

    if (usePackedDiffuseNormalGBA) {
      if (typeof mapDisableDistance === 'number') {
        return `
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

      return `
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

    const normalMapSuffix = `
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
        return `
          vec3 newWorldNormal = triplanarTextureNormalMap(normalMap, vWorldPos, vec2(uvTransform[0][0], uvTransform[1][1]), vWorldNormal, normalScale).xyz;
          // Transform \`newWorldNormal\` from world space to view space
          normal = normalize((viewMatrix * vec4(newWorldNormal, 0.)).xyz);
          `;
      }

      if (tileBreaking)
        return `
    vec3 mapN = ${buildTileBreakSampleExpr('normalMap', 'vMapUv', tileBreaking)}.xyz;

    ${normalMapSuffix}
  `;
      else return '#include <normal_fragment_maps>';
    })();

    if (typeof mapDisableDistance !== 'number') {
      return inner;
    }

    return `
      vec3 baseNormal = normal;
      if (textureActivation < 0.01) {
        // avoid any texture lookups, relieve pressure on the texture unit
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
        return `
          // TODO: I'm pretty sure double-applying uv transform is wrong, here and in all others
          vec3 newClearcoatWorldNormal = triplanarTextureNormalMap(clearcoatNormalMap, vWorldPos, vec2(uvTransform[0][0], uvTransform[1][1]), vWorldNormal, clearcoatNormalScale).xyz;
          // Transform \`newWorldNormal\` from world space to view space
          clearcoatNormal = normalize((viewMatrix * vec4(newClearcoatWorldNormal, 0.)).xyz);
          `;
      }

      if (tileBreaking) {
        return `
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
        return `
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

    return `
      vec3 baseClearcoatNormal = clearcoatNormal;
      if (textureActivation < 0.01) {
        // avoid any texture lookups, relieve pressure on the texture unit
      } else {
        ${inner}
        clearcoatNormal = mix(baseClearcoatNormal, clearcoatNormal, textureActivation);
      }`;
  };

  return {
    fog: true,
    lights: true,
    dithering: false,
    // transparent: heightAlpha ? true : (transparent ?? false),
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
      ? `
    float computedDisplacement = getDisplacement(position, ${normalAttribute}, curTimeSeconds);
    transformed += normalize( ${normalAttribute} ) * computedDisplacement;
  `
      : `
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

  vec4 worldPositionMine = vec4( transformed, 1.0 );
  worldPositionMine = modelMatrix * worldPositionMine;
  vWorldPos = worldPositionMine.xyz;

  #ifdef USE_INSTANCING
    vWorldPos = (instanceMatrix * vec4(vWorldPos, 1.)).xyz;
  #endif

  vObjectNormal = normal;
  vWorldNormal = normalize((modelMatrix * vec4(normal, 0.)).xyz);

  #ifdef USE_UV
  ${(() => {
    if (useGeneratedUVs) {
      const uvPos = useWorldSpaceGeneratedUVs ? 'vWorldPos' : 'position';
      const uvNormal = useWorldSpaceGeneratedUVs ? 'worldNormal' : 'normal';
      return `
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
    return 'vUv = ( uvTransform * vec3( uv, 1 ) ).xy;';
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
    fragmentShader: `
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

uniform vec3 playerShadowPos;
uniform vec4 playerShadowParams; // x=radius, y=intensity, z=centerReceiverY, w=centerDropDist
uniform mat4 psRingData; // cols 0-1: outer ring receiverY (angles 0-7), cols 2-3: inner ring (angles 0-7)

${softOcclusionPreamble}

#ifndef USE_TRANSMISSION
  varying vec3 vWorldPosition;
#endif

varying vec3 vObjectNormal;
varying vec3 vWorldNormal;
uniform mat3 uvTransform;

${normalShader ? 'uniform mat3 normalMatrix;' : ''}
${tileBreaking?.type === 'fastFixMipmap' ? 'uniform sampler2D noiseSampler;' : ''}
// ${useComputedNormalMap || usePackedDiffuseNormalGBA ? 'uniform vec2 normalScale;' : ''}
${typeof usePackedDiffuseNormalGBA === 'object' ? 'uniform sampler2D diffuseLUT;' : ''}

struct SceneCtx {
  vec3 cameraPosition;
  vec2 vUv;
  vec4 diffuseColor;
};

${commonShaderCode}
${noiseShaders}
${useNoise2 ? noise2Shaders : ''}
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
${tileBreaking?.type === 'fastFixMipmap' ? tileBreakingFragment : ''}
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
          : { ...buildDefaultTriplanarParams(), ...useTriplanarMapping }
      )
    : ''
}
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
      ? `
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

  SceneCtx ctx = SceneCtx(cameraPosition, vMapUv, diffuseColor);

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

	#include <clearcoat_normal_fragment_begin>
	// #include <clearcoat_normal_fragment_maps>
  #if defined(USE_CLEARCOAT) && defined(USE_CLEARCOAT_NORMALMAP)
    ${buildClearcoatNormalMapFragment()}
  #endif

  ${buildRunColorShaderFragment(colorShader, antialiasColorShader)}

  ${
    normalShader
      ? `
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
      ? `
    totalEmissiveRadiance = getCustomEmissive(vWorldPos, totalEmissiveRadiance, curTimeSeconds, ctx);
  `
      : ''
  }

	// accumulation
  ${
    vertexLighting
      ? `
  // --- Vertex lighting path ---
  // Lighting was computed per-vertex; we only need per-fragment shadow map sampling here.
  // Shadows only affect direct light; ambient/indirect passes through unshadowed.
  float totalShadow = 1.0;

  #if defined(USE_SHADOWMAP)
    float computedShadow;

    #if (NUM_DIR_LIGHTS > 0) && (NUM_DIR_LIGHT_SHADOWS > 0)
      DirectionalLightShadow vtxDirShadow;
      #pragma unroll_loop_start
      for (int i = 0; i < NUM_DIR_LIGHTS; i++) {
        #if (UNROLLED_LOOP_INDEX < NUM_DIR_LIGHT_SHADOWS)
          vtxDirShadow = directionalLightShadows[i];
          computedShadow = getShadow(directionalShadowMap[i], vtxDirShadow.shadowMapSize, vtxDirShadow.shadowBias, vtxDirShadow.shadowRadius, vDirectionalShadowCoord[i]);
          totalShadow *= computedShadow;
        #endif
      }
      #pragma unroll_loop_end
    #endif

    #if (NUM_SPOT_LIGHTS > 0) && (NUM_SPOT_LIGHT_SHADOWS > 0)
      SpotLightShadow vtxSpotShadow;
      #pragma unroll_loop_start
      for (int i = 0; i < NUM_SPOT_LIGHTS; i++) {
        #if (UNROLLED_LOOP_INDEX < NUM_SPOT_LIGHT_SHADOWS)
          vtxSpotShadow = spotLightShadows[i];
          computedShadow = getShadow(spotShadowMap[i], vtxSpotShadow.shadowMapSize, vtxSpotShadow.shadowBias, vtxSpotShadow.shadowRadius, vSpotLightCoord[i]);
          totalShadow *= computedShadow;
        #endif
      }
      #pragma unroll_loop_end
    #endif

    #if (NUM_POINT_LIGHTS > 0) && (NUM_POINT_LIGHT_SHADOWS > 0)
      PointLightShadow vtxPointShadow;
      #pragma unroll_loop_start
      for (int i = 0; i < NUM_POINT_LIGHTS; i++) {
        #if (UNROLLED_LOOP_INDEX < NUM_POINT_LIGHT_SHADOWS)
          vtxPointShadow = pointLightShadows[i];
          computedShadow = getPointShadow(pointShadowMap[i], vtxPointShadow.shadowMapSize, vtxPointShadow.shadowBias, vtxPointShadow.shadowRadius, vPointShadowCoord[i], vtxPointShadow.shadowCameraNear, vtxPointShadow.shadowCameraFar);
          totalShadow *= computedShadow;
        #endif
      }
      #pragma unroll_loop_end
    #endif
  #endif

  // Apply RECIPROCAL_PI to match PBR energy conservation (BRDF_Lambert divides by PI).
  // Shadows only darken direct light; indirect (ambient + hemisphere) is unshadowed.
  vec3 totalDiffuse = diffuseColor.rgb * RECIPROCAL_PI * (vVertexDirect * totalShadow + vVertexIndirect);
  vec3 totalSpecular = ${vertexLightingShininess > 0 ? 'vVertexSpecular * totalShadow' : 'vec3(0.0)'};
  `
      : `
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

	if (playerShadowParams.y > 0.0) {
		float psRadius = playerShadowParams.x;
		float psCenterReceiverY = playerShadowParams.z;
		float psCenterDropDist = playerShadowParams.w;

		vec2 psDelta = vWorldPos.xz - playerShadowPos.xz;
		float psDist = length(psDelta);
		float psCircle = 1.0 - smoothstep(psRadius * 0.6, psRadius, psDist);

		// Polar bilinear interpolation of receiver Y from ring probes
		// Compute angle index (8 sectors, 45° each)
		float psAngle = atan(psDelta.y, psDelta.x); // -PI to PI
		float psSector = fract(psAngle / 6.2831853) * 8.0; // 0 to 8
		float psSectorFrac = fract(psSector);
		int psIdx0 = int(mod(floor(psSector), 8.0));
		int psIdx1 = int(mod(floor(psSector) + 1.0, 8.0));

		// Look up receiverY from ring mat4 by index, using max() to bias toward closest surface
		// mat4 layout: cols 0-1 = outer ring, cols 2-3 = inner ring
		// psRingData[col][row] where col = i/4, row = i%4
		float psOuterY = max(psRingData[psIdx0 / 4][psIdx0 - (psIdx0 / 4) * 4], psRingData[psIdx1 / 4][psIdx1 - (psIdx1 / 4) * 4]);
		float psInnerY = max(psRingData[2 + psIdx0 / 4][psIdx0 - (psIdx0 / 4) * 4], psRingData[2 + psIdx1 / 4][psIdx1 - (psIdx1 / 4) * 4]);

		// Radial interpolation: center → inner ring → outer ring, biased toward closest surface
		float psRadialT = clamp(psDist / psRadius, 0.0, 1.0);
		float psReceiverY;
		if (psRadialT < 0.5) {
			psReceiverY = max(psCenterReceiverY, psInnerY);
		} else {
			psReceiverY = max(psInnerY, psOuterY);
		}

		// Drop distance derived from final receiverY
		float psDropDist = playerShadowPos.y - psReceiverY;

		// Asymmetric surface check: tight above, gradual bleed below
		float psYDiff = vWorldPos.y - psReceiverY;
		float psOnSurface = psYDiff > 0.0
			? 1.0 - smoothstep(0.0, 0.3, psYDiff)
			: 1.0 - smoothstep(0.0, 1.5, -psYDiff);

		// Skip undersides and vertical walls (vWorldNormal is the actual world-space normal)
		float psNormalUp = smoothstep(0.2, 0.5, vWorldNormal.y);

		// Fade shadow with height above surface
		float psHeightFade = 1.0 - smoothstep(0.0, 40.0, psDropDist);

		float psShadow = psCircle * psOnSurface * psNormalUp * psHeightFade * playerShadowParams.y;
		totalDiffuse *= (1.0 - psShadow);
		totalSpecular *= (1.0 - psShadow);
		totalShadow *= (1.0 - psShadow);
	}

	#include <transmission_fragment>

	vec3 outgoingLight = totalDiffuse + totalSpecular + totalEmissiveRadiance;

	#ifdef USE_SHEEN

		// Sheen energy compensation approximation calculation can be found at the end of
		// https://drive.google.com/file/d/1T0D1VSyR4AllqIJTQAraEIzjlb5h4FKH/view?usp=sharing
		float sheenEnergyComp = 1.0 - 0.157 * max3( material.sheenColor );

		outgoingLight = outgoingLight * sheenEnergyComp + sheenSpecularDirect + sheenSpecularIndirect;
	#endif

	#ifdef USE_CLEARCOAT
    float dotNVcc = saturate( dot( geometryClearcoatNormal, geometryViewDir ) );
		vec3 Fcc = F_Schlick( material.clearcoatF0, material.clearcoatF90, dotNVcc );
		outgoingLight = outgoingLight * ( 1.0 - material.clearcoat * Fcc ) + ( clearcoatSpecularDirect + clearcoatSpecularIndirect ) * material.clearcoat;
	#endif

	// #include <opaque_fragment>
  #ifdef OPAQUE
  diffuseColor.a = 1.0;
  #endif

  #ifdef USE_TRANSMISSION
  diffuseColor.a *= material.transmissionAlpha;
  #endif

  ${buildHeightAlphaFragment(heightAlpha)}

  outFragColor = vec4( outgoingLight, diffuseColor.a );

  ${
    !noOcclusion
      ? `if (hasBackfaceHit) outFragColor.rgb = mix(outFragColor.rgb, vec3(168. / 255., 190. / 255., 155. / 255.), 0.01);`
      : ''
  }

	${
    enableFog
      ? `
  #ifdef USE_FOG
    #ifdef FOG_EXP2
      float fogFactor = 1.0 - exp( - fogDensity * fogDensity * vFogDepth * vFogDepth );
    #else
      float fogFactor = smoothstep( fogNear, fogFar, vFogDepth );
    #endif
    ${typeof fogMultiplier === 'number' ? `fogFactor *= ${fogMultiplier.toFixed(4)};` : ''}
    // \`totalShadow\` is 0 if the fragment is fully shadowed and 1 if it is fully lit
    vec3 shadowColor = vec3( 0.0 );
    float fogShadowFactor = ${fogShadowFactor.toFixed(4)};
    vec3 shadowedFogColor = mix(fogColor, shadowColor, fogShadowFactor * (1. - totalShadow) * clamp(0., 1., fogFactor * 1.5));
    outFragColor.rgb = mix( outFragColor.rgb, shadowedFogColor, fogFactor );
    // outFragColor.w = mix( outFragColor.a, 0., fogFactor );
  #endif
  `
      : ''
  }
	// #include <premultiplied_alpha_fragment>
  #ifdef PREMULTIPLIED_ALPHA
    outFragColor.rgb *= outFragColor.a;
  #endif
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
  /**
   * This flag is set when the material makes use of SSR and expects a second color attachment to be
   * set on the framebuffer.
   *
   * This is read in `defaultPostprocessing.ts` which does some hacky patching of Three.JS to handle
   * binding/unbinding the second buffer as needed to prevent errors that WebGL throws if an output
   * texture is bound but not written to.
   */
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

  if (opts?.useComputedNormalMap || opts?.usePackedDiffuseNormalGBA) {
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
    // Setting alpha to 1. causes no reflections to be emitted since that's the default value for the cleared buffer
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
  if (props.clearcoatNormalMap) {
    mat.clearcoatNormalMap = props.clearcoatNormalMap;
    mat.uniforms.clearcoatNormalMap.value = props.clearcoatNormalMap;
  }
  if (props.emissiveIntensity !== undefined) {
    (mat as any).emissiveIntensity = props.emissiveIntensity;
    mat.uniforms.emissiveIntensity.value = props.emissiveIntensity;
  }
  // if (props.lightMap) {
  //   (mat as any).lightMap = props.lightMap;
  //   mat.uniforms.lightMap.value = props.lightMap;
  //   mat.uniforms.lightMapIntensity.value = props.lightMapIntensity ?? 1;
  // }
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
    // Materials opt into runtime occlusion backface toggling unless explicitly excluded.
    setMaterialOcclusionBackfaceRendering(mat, occlusionBackfaceRenderingEnabled);
  }

  return mat;
};
