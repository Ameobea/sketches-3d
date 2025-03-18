import * as THREE from 'three';
import { UniformsLib } from 'three';

import commonShaderCode from './common.frag?raw';
import CustomLightsFragmentBegin from './customLightsFragmentBegin.frag?raw';
import tileBreakingFragment from './fasterTileBreakingFixMipmap.frag?raw';
import GeneratedUVsFragment from './generatedUVs.vert?raw';
import noiseShaders from './noise.frag?raw';
import tileBreakingNeyretFragment from './tileBreakingNeyret.frag?raw';
import { buildTriplanarDefsFragment, type TriplanarMappingParams } from './triplanarMapping';
import ssrDefsFragment from './ssr/ssrDefs.frag?raw';
import ssrWriteFragment from './ssr/ssrWrite.frag?raw';

// import noise2Shaders from './noise2.frag?raw';
const noise2Shaders = 'DISABLED TO SAVE SPACE';

const DEFAULT_MAP_DISABLE_DISTANCE = 2000;
const fastFixMipMapTileBreakingScale = (240.2).toFixed(3);

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
  float acc = 0.;
  // 2x oversampling
  for (int i = 0; i < 2; i++) {
    for (int j = 0; j < 2; j++) {
      for (int k = 0; k < 2; k++) {
        vec3 offsetPos = fragPos;
        // TODO use better method, only sample in plane the fragment lies on rather than in 3D
        offsetPos.x += ((float(k) - 1.) * 0.5) * unitsPerPx;
        offsetPos.y += ((float(i) - 1.) * 0.5) * unitsPerPx;
        offsetPos.z += ((float(j) - 1.) * 0.5) * unitsPerPx;
        acc += getCustomRoughness(offsetPos, vNormalAbsolute, roughnessFactor, curTimeSeconds, ctx);
      }
    }
  }
  acc /= 8.;
  roughnessFactor = acc;
`;

const NonAntialiasedRoughnessShaderFragment =
  'roughnessFactor = getCustomRoughness(pos, vNormalAbsolute, roughnessFactor, curTimeSeconds, ctx);';

const buildRoughnessShaderFragment = (antialiasRoughnessShader?: boolean) => {
  if (antialiasRoughnessShader) {
    return AntialiasedRoughnessShaderFragment;
  }

  return NonAntialiasedRoughnessShaderFragment;
};

interface AmbientDistanceAmpParams {
  falloffStartDistance: number;
  falloffEndDistance: number;
  exponent?: number;
  ampFactor: number;
}

let DefaultDistanceAmpParams: AmbientDistanceAmpParams | undefined;

export const setDefaultDistanceAmpParams = (params: AmbientDistanceAmpParams | null | undefined) => {
  DefaultDistanceAmpParams = params ?? undefined;
};

interface ReflectionParams {
  alpha: number;
}

const DefaultReflectionParams: ReflectionParams = Object.freeze({
  alpha: 1,
});

/**
 * Used for determining default behavior like sound effects for when the player lands on a surface
 */
export enum MaterialClass {
  Default,
  Rock,
  Crystal,
  Instakill,
}

export interface CustomShaderProps {
  name?: string;
  side?: THREE.Side;
  roughness?: number;
  metalness?: number;
  clearcoat?: number;
  clearcoatRoughness?: number;
  iridescence?: number;
  color?: number | THREE.Color;
  normalScale?: number;
  map?: THREE.Texture;
  normalMap?: THREE.Texture;
  roughnessMap?: THREE.Texture;
  normalMapType?: THREE.NormalMapTypes;
  /**
   * If set to `true`, an attribute called `displacementNormal` is expected to be set on the geometry.
   *
   * These normals will be used instead of the object normals for displacement mapping.  This is useful
   * if you want to do flat/partially flat shading but still want to use displacement mapping.  If flat
   * shading is used and the object normals are used for displacement mapping, faces tend to fly apart
   * from each other.
   */
  useDisplacementNormals?: boolean;
  uvTransform?: THREE.Matrix3;
  emissiveIntensity?: number;
  lightMap?: THREE.Texture;
  lightMapIntensity?: number;
  transparent?: boolean;
  opacity?: number;
  alphaTest?: number;
  transmission?: number;
  ior?: number;
  transmissionMap?: THREE.Texture;
  fogMultiplier?: number;
  /**
   * If provided, maps will no longer be read once the fragment is this distance from the camera. Set to
   * `null` to disable.
   */
  mapDisableDistance?: number | null;
  /**
   * If provided, the shader will interpolate between read map value and diffuse color within this distance.
   */
  mapDisableTransitionThreshold?: number;
  /**
   * If greater than 0, fog will be darkened by shadows by this amount. A value of 1 means that the fog color
   * of a fully shadowed fragment will be darkened to the shadow color completely.
   */
  fogShadowFactor?: number;
  ambientLightScale?: number;
  /**
   * Controls an effect whereby the amount of ambient light is increased if the fragment is within some distance
   * to the camera.
   *
   * Works in a similar way to exp2 fog but in reverse and with a configurable exponent.
   */
  ambientDistanceAmp?: AmbientDistanceAmpParams;
  /**
   * Controls screen-space reflections.
   */
  reflection?: Partial<ReflectionParams>;
}

interface CustomShaderShaders {
  customVertexFragment?: string;
  colorShader?: string;
  normalShader?: string;
  roughnessShader?: string;
  metalnessShader?: string;
  emissiveShader?: string;
  iridescenceShader?: string;
  displacementShader?: string;
  includeNoiseShadersVertex?: boolean;
}

const buildDefaultTriplanarParams = (): TriplanarMappingParams => ({
  contrastPreservationFactor: 0.5,
  sharpenFactor: 12.8,
});

interface CustomShaderOptions {
  antialiasColorShader?: boolean;
  antialiasRoughnessShader?: boolean;
  tileBreaking?: { type: 'neyret'; patchScale?: number } | { type: 'fastFixMipmap' };
  /**
   * If set, the alternative noise functions in `noise2.frag` will be included
   */
  useNoise2?: boolean;
  enableFog?: boolean;
  /**
   * If set, a normal map will be generated based on derivatives in magnitude of generated diffuse colors.
   *
   * Note that this is a pretty broken implementation right now. There are huge aliasing issues and it looks
   * very bad on surfaces that have a high angle.
   */
  useComputedNormalMap?: boolean;
  /**
   * If set, the provided `map` will be treated as a combined grayscale diffuse + normal map. The diffuse
   * component will be read from the R channel and the normal map will be read from the GBA channels.
   */
  usePackedDiffuseNormalGBA?: boolean | { lut: Uint8Array };
  readRoughnessMapFromRChannel?: boolean;
  disableToneMapping?: boolean;
  disabledDirectionalLightIndices?: number[];
  disabledSpotLightIndices?: number[];
  randomizeUVOffset?: boolean;
  useGeneratedUVs?: boolean;
  useTriplanarMapping?: boolean | Partial<TriplanarMappingParams>;
  materialClass?: MaterialClass;
}

export const buildCustomShaderArgs = (
  {
    roughness = 0.9,
    metalness = 0,
    clearcoat = 0,
    clearcoatRoughness = 0,
    iridescence = 0,
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
    emissiveIntensity,
    lightMapIntensity,
    fogMultiplier,
    mapDisableDistance: rawMapDisableDistance,
    mapDisableTransitionThreshold = 20,
    fogShadowFactor = 0.1,
    ambientLightScale = 1,
    ambientDistanceAmp = DefaultDistanceAmpParams,
    reflection: providedReflectionParams,
  }: CustomShaderProps = {},
  {
    customVertexFragment,
    colorShader,
    normalShader,
    roughnessShader,
    metalnessShader,
    emissiveShader,
    iridescenceShader,
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
    disableToneMapping,
    disabledDirectionalLightIndices,
    disabledSpotLightIndices,
    randomizeUVOffset,
    useGeneratedUVs,
    useTriplanarMapping,
  }: CustomShaderOptions = {}
) => {
  const uniforms = THREE.UniformsUtils.merge([
    UniformsLib.common,
    // UniformsLib.envmap,
    // UniformsLib.aomap,
    UniformsLib.lightmap,
    UniformsLib.emissivemap,
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
  uniforms.normalScale = { type: 'v2', value: new THREE.Vector2(normalScale, normalScale) };

  if (tileBreaking?.type === 'fastFixMipmap') {
    uniforms.noiseSampler = { type: 't', value: buildNoiseTexture() };
  }

  uniforms.roughness = { type: 'f', value: roughness };
  uniforms.metalness = { type: 'f', value: metalness };
  uniforms.ior = { type: 'f', value: ior };
  uniforms.clearcoat = { type: 'f', value: clearcoat };
  uniforms.clearcoatRoughness = { type: 'f', value: clearcoatRoughness };
  uniforms.clearcoatNormal = { type: 'f', value: 0.0 };
  uniforms.iridescence = { type: 'f', value: iridescence };
  uniforms.iridescenceIOR = { type: 'f', value: 1.3 };
  uniforms.iridescenceThicknessMinimum = { type: 'f', value: 100 };
  uniforms.iridescenceThicknessMaximum = { type: 'f', value: 400 };
  uniforms.iridescenceThicknessMapTransform = { type: 'mat3', value: new THREE.Matrix3() };
  uniforms.transmission = { type: 'f', value: transmission };
  uniforms.transmissionMap = { type: 't', value: transmissionMap };
  uniforms.transmissionSamplerSize = { type: 'v2', value: new THREE.Vector2() };
  uniforms.transmissionSamplerMap = { type: 't', value: null };

  uniforms.curTimeSeconds = { type: 'f', value: 0.0 };
  uniforms.diffuse = { type: 'c', value: typeof color === 'number' ? new THREE.Color(color) : color };
  uniforms.mapTransform = { type: 'mat3', value: new THREE.Matrix3().identity() };
  if (uvTransform) {
    uniforms.uvTransform = { type: 'm3', value: uvTransform };
  }
  if (emissiveIntensity !== undefined) {
    uniforms.emissiveIntensity = { type: 'f', value: emissiveIntensity };
  }
  // TODO: Need to handle swapping uvs to `uv2` if light map is provided
  if (lightMapIntensity !== undefined) {
    uniforms.lightMapIntensity = { type: 'f', value: lightMapIntensity };
  }

  const usingSSR = !!providedReflectionParams;

  // TODO: enable physically correct lights, look into it at least

  if (tileBreaking && !map) {
    throw new Error('Tile breaking requires a map');
  }

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
  if (useGeneratedUVs && !map) {
    throw new Error('Cannot use generated UVs without a map');
  }
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

  const buildUVVertexFragment = () => {
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

  const buildRunColorShaderFragment = () => {
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
        vec3 offsetPos = fragPos;
        // TODO use better method, only sample in plane the fragment lies on rather than in 3D
        offsetPos.x += ((float(k) - 1.) * 0.5) * unitsPerPx;
        offsetPos.y += ((float(i) - 1.) * 0.5) * unitsPerPx;
        offsetPos.z += ((float(j) - 1.) * 0.5) * unitsPerPx;
        acc += getFragColor(diffuseColor.xyz, offsetPos, vNormalAbsolute, curTimeSeconds, ctx);
      }
    }
  }
  acc /= 8.;
  diffuseColor = acc;`;
    } else {
      return `
  diffuseColor = getFragColor(diffuseColor.xyz, pos, vNormalAbsolute, curTimeSeconds, ctx);`;
    }
  };

  const buildRunIridescenceShaderFragment = () => {
    if (!iridescenceShader) {
      return '';
    }

    return `
material.iridescence = getCustomIridescence(pos, vNormalAbsolute, material.iridescence, curTimeSeconds, ctx);`;
  };

  const buildLightsFragmentBegin = () => {
    let frag = CustomLightsFragmentBegin.replace(
      '__DIR_LIGHTS_DISABLE__',
      (() => {
        if (!disabledDirectionalLightIndices) {
          return '0';
        }

        return disabledDirectionalLightIndices
          .map(i => `UNROLLED_LOOP_INDEX == ${i.toFixed(0)}`)
          .join(' || ');
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

  const mapDisableDistance =
    rawMapDisableDistance === undefined ? DEFAULT_MAP_DISABLE_DISTANCE : rawMapDisableDistance;
  const buildTextureDisableFragment = () => {
    if (typeof mapDisableDistance !== 'number') {
      return '';
    }

    const startEdge = (mapDisableDistance - mapDisableTransitionThreshold).toFixed(3);
    const endEdge = mapDisableDistance.toFixed(3);

    return `
      float textureActivation = 1. - smoothstep(${startEdge}, ${endEdge}, distanceToCamera);
    `;
  };

  const buildUnpackDiffuseNormalGBAFragment = (params: true | { lut: Uint8Array }) => {
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

  const buildMapFragment = () => {
    const inner = (() => {
      if (useTriplanarMapping) {
        return `
        #ifdef USE_MAP
          sampledDiffuseColor_ = triplanarTextureFixContrast(map, pos, vec2(uvTransform[0][0], uvTransform[1][1]), vNormalAbsolute);
        #endif`;
      }

      if (!tileBreaking) {
        return `
        #ifdef USE_MAP
          vec4 sampledDiffuseColor = texture2D( map, vMapUv );
          sampledDiffuseColor_ = sampledDiffuseColor;
        #endif`;
      }

      return tileBreaking.type === 'neyret'
        ? 'sampledDiffuseColor_ = textureNoTileNeyret(map, vMapUv);'
        : `sampledDiffuseColor_ = textureNoTile(map, noiseSampler, vMapUv, 0., ${fastFixMipMapTileBreakingScale});`;
    })();

    if (typeof mapDisableDistance !== 'number') {
      return `
      #ifdef USE_MAP
        vec4 sampledDiffuseColor_ = vec4(0.);
        ${usePackedDiffuseNormalGBA ? 'vec3 mapN = vec3(0.);' : ''}
        ${inner}
        ${usePackedDiffuseNormalGBA ? buildUnpackDiffuseNormalGBAFragment(usePackedDiffuseNormalGBA) : ''}
        diffuseColor *= sampledDiffuseColor_;
      #endif`;
    }

    return `
    #ifdef USE_MAP
      vec4 sampledDiffuseColor_ = vec4(0.);
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
          vec3 texelRoughness = triplanarTexture(roughnessMap, pos, vec2(uvTransform[0][0], uvTransform[1][1]), vNormalAbsolute).xyz;
        `;
      }

      if (tileBreaking && roughnessMap)
        return tileBreaking.type === 'neyret'
          ? 'vec3 texelRoughness = textureNoTileNeyret(roughnessMap, vMapUv).xyz;'
          : `vec3 texelRoughness = textureNoTile(roughnessMap, noiseSampler, vMapUv, 0., ${fastFixMipMapTileBreakingScale}).xyz;`;
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
          vec3 newWorldNormal = triplanarTextureNormalMap(normalMap, pos, vec2(uvTransform[0][0], uvTransform[1][1]), vWorldNormal, normalScale).xyz;
          // Transform \`newWorldNormal\` from world space to view space
          normal = normalize((viewMatrix * vec4(newWorldNormal, 0.)).xyz);
          `;
      }

      if (tileBreaking)
        return `
    ${
      tileBreaking.type === 'neyret'
        ? 'vec3 mapN = textureNoTileNeyret(normalMap, vMapUv).xyz;'
        : `vec3 mapN = textureNoTile(normalMap, noiseSampler, vMapUv, 0., ${fastFixMipMapTileBreakingScale}).xyz;`
    }

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

  return {
    fog: true,
    lights: true,
    dithering: true,
    uniforms,
    vertexShader: `
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

${includeNoiseShadersVertex ? noiseShaders : ''}

${useGeneratedUVs ? GeneratedUVsFragment : ''}

${displacementShader || ''}

${useDisplacementNormals ? 'attribute vec3 displacementNormal;' : ''}

uniform float curTimeSeconds;
varying vec3 pos;
varying vec3 vNormalAbsolute;
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
  pos = worldPositionMine.xyz;

  #ifdef USE_INSTANCING
    pos = (instanceMatrix * vec4(pos, 1.)).xyz;
  #endif

  vNormalAbsolute = normal;
  vWorldNormal = normalize((modelMatrix * vec4(normal, 0.)).xyz);

  #ifdef USE_UV
  ${(() => {
    if (useGeneratedUVs) {
      return `
      // convert normal into the world space
      vec3 worldNormal = normalize(mat3(modelMatrix[0].xyz, modelMatrix[1].xyz, modelMatrix[2].xyz) * normal);
      vUv = generateUV(pos, worldNormal);
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

  ${buildUVVertexFragment()}

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

  #include <shadowmap_vertex>
  ${enableFog ? '#include <fog_vertex>' : ''}

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

#include <common>
#include <packing>
#include <dithering_pars_fragment>
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

#define srgb2rgb(V) pow( max(V,0.), vec4( 2.2 )  )

uniform float curTimeSeconds;
varying vec3 pos;

#ifndef USE_TRANSMISSION
  varying vec3 vWorldPosition;
#endif

varying vec3 vNormalAbsolute;
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
${iridescenceShader ?? ''}
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

  vec3 fragPos = pos;
  vec3 cameraPos = cameraPosition;
  float distanceToCamera = distance(cameraPos, fragPos);
  float unitsPerPx = abs(2. * distanceToCamera * tan(0.001 / 2.));

  ${buildTextureDisableFragment()}

  vec4 diffuseColor = vec4(diffuse, opacity);

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
	#include <clearcoat_normal_fragment_maps>

  ${buildRunColorShaderFragment()}

  ${
    normalShader
      ? `
  normal = getCustomNormal(pos, vNormalAbsolute, curTimeSeconds);
  normal = normalize(normalMatrix * normal);
  `
      : ''
  }

  ${roughnessShader ? buildRoughnessShaderFragment(antialiasRoughnessShader) : ''}

  ${
    metalnessShader
      ? 'metalnessFactor = getCustomMetalness(pos, vNormalAbsolute, roughnessFactor, curTimeSeconds, ctx);'
      : ''
  }

	#include <emissivemap_fragment>
  ${
    emissiveShader
      ? `
    totalEmissiveRadiance = getCustomEmissive(pos, totalEmissiveRadiance, curTimeSeconds, ctx);
  `
      : ''
  }

	// accumulation
	#include <lights_physical_fragment>
  ${iridescenceShader ? buildRunIridescenceShaderFragment() : ''}
	// #include <lights_fragment_begin>
  ${buildLightsFragmentBegin()}
	#include <lights_fragment_maps>
	#include <lights_fragment_end>

	// modulation
	#include <aomap_fragment>

	vec3 totalDiffuse = reflectedLight.directDiffuse + reflectedLight.indirectDiffuse;
	vec3 totalSpecular = reflectedLight.directSpecular + reflectedLight.indirectSpecular;

	#include <transmission_fragment>

	vec3 outgoingLight = totalDiffuse + totalSpecular + totalEmissiveRadiance;

	#ifdef USE_SHEEN

		// Sheen energy compensation approximation calculation can be found at the end of
		// https://drive.google.com/file/d/1T0D1VSyR4AllqIJTQAraEIzjlb5h4FKH/view?usp=sharing
		float sheenEnergyComp = 1.0 - 0.157 * max3( material.sheenColor );

		outgoingLight = outgoingLight * sheenEnergyComp + sheenSpecular;
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

  outFragColor = vec4( outgoingLight, diffuseColor.a );
  ${
    !disableToneMapping
      ? `
  #if defined( TONE_MAPPING )
	  outFragColor.rgb = toneMapping( outFragColor.rgb );
  #endif

  ${usingSSR ? ssrWriteFragment : ''}
  `
      : ''
  }
	// #include <colorspace_fragment>
  outFragColor = linearToOutputTexel( outFragColor );
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
	// #include <dithering_fragment>
  #ifdef DITHERING
    outFragColor.rgb = dithering( outFragColor.rgb );
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
  /**
   * This flag is set when the material makes use of SSR and expects a second color attachment to be
   * set on the framebuffer.
   *
   * This is read in `defaultPostprocessing.ts` which does some hacky patching of Three.JS to handle
   * binding/unbinding the second buffer as needed to prevent errors that WebGL throws if an output
   * texture is bound but not written to.
   */
  public needsSSRBuffer = false;

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
    mat.defines.USE_CLEARCOAT = '1';
  }

  if (props.iridescence) {
    mat.defines.USE_IRIDESCENCE = '1';
  }

  if (props.reflection) {
    const reflectionParams = { ...DefaultReflectionParams, ...props.reflection };
    // Setting alpha to 1. causes no reflections to be emitted since that's the default value for the cleared buffer
    mat.defines.SSR_ALPHA = Math.min(reflectionParams.alpha, 0.9999).toFixed(4);
  } else {
    mat.defines.SSR_ALPHA = '0.';
  }

  mat.defines.PHYSICAL = '1';
  mat.defines.USE_UV = '1';
  if (props.map) {
    (mat as any).map = props.map;
    mat.uniforms.map.value = props.map;
  }
  if (props.normalMap) {
    (mat as any).normalMap = props.normalMap;
    (mat as any).normalMapType = props.normalMapType ?? THREE.TangentSpaceNormalMap;
    mat.uniforms.normalMap.value = props.normalMap;
  }
  if (props.roughnessMap) {
    (mat as any).roughnessMap = props.roughnessMap;
    mat.uniforms.roughnessMap.value = props.roughnessMap;
  }
  if (props.emissiveIntensity !== undefined) {
    (mat as any).emissiveIntensity = props.emissiveIntensity;
    mat.uniforms.emissiveIntensity.value = props.emissiveIntensity;
  }
  if (props.lightMap) {
    (mat as any).lightMap = props.lightMap;
    mat.uniforms.lightMap.value = props.lightMap;
    mat.uniforms.lightMapIntensity.value = props.lightMapIntensity ?? 1;
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

  return mat;
};
