import * as THREE from 'three';
import { UniformsLib } from 'three';

import commonShaderCode from './common.frag?raw';
import CustomLightsFragmentBegin from './customLightsFragmentBegin.frag?raw';
import tileBreakingFragment from './fasterTileBreakingFixMipmap.frag?raw';
import GeneratedUVsFragment from './generatedUVs.vert?raw';
import noiseShaders from './noise.frag?raw';
import tileBreakingNeyretFragment from './tileBreakingNeyret.frag?raw';

// import noise2Shaders from './noise2.frag?raw';
const noise2Shaders = 'DISABLED TO SAVE SPACE';

const DEFAULT_MAP_DISABLE_DISTANCE = 200;
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

const NonAntialiasedRoughnessShaderFragment = `roughnessFactor = getCustomRoughness(pos, vNormalAbsolute, roughnessFactor, curTimeSeconds, ctx);`;

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

interface CustomShaderProps {
  name?: string;
  side?: THREE.Side;
  roughness?: number;
  metalness?: number;
  color?: THREE.Color;
  normalScale?: number;
  map?: THREE.Texture;
  normalMap?: THREE.Texture;
  roughnessMap?: THREE.Texture;
  normalMapType?: THREE.NormalMapTypes;
  uvTransform?: THREE.Matrix3;
  emissiveIntensity?: number;
  lightMap?: THREE.Texture;
  lightMapIntensity?: number;
  transparent?: boolean;
  opacity?: number;
  alphaTest?: number;
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
}

interface CustomShaderShaders {
  customVertexFragment?: string;
  colorShader?: string;
  normalShader?: string;
  roughnessShader?: string;
  metalnessShader?: string;
  emissiveShader?: string;
}

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
  useTriplanarMapping?: boolean;
}

export const buildCustomShaderArgs = (
  {
    roughness = 0.9,
    metalness = 0,
    color = new THREE.Color(0xffffff),
    normalScale = 1,
    map,
    uvTransform,
    normalMap,
    normalMapType,
    roughnessMap,
    emissiveIntensity,
    lightMapIntensity,
    fogMultiplier,
    mapDisableDistance: rawMapDisableDistance,
    mapDisableTransitionThreshold = 20,
    fogShadowFactor = 0.1,
    ambientLightScale = 1,
    ambientDistanceAmp = DefaultDistanceAmpParams,
  }: CustomShaderProps = {},
  {
    customVertexFragment,
    colorShader,
    normalShader,
    roughnessShader,
    metalnessShader,
    emissiveShader,
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
  uniforms.ior = { type: 'f', value: 1.5 };
  uniforms.clearcoat = { type: 'f', value: 0.0 };
  uniforms.clearcoatRoughness = { type: 'f', value: 0.0 };
  uniforms.clearcoatNormal = { type: 'f', value: 0.0 };
  uniforms.transmission = { type: 'f', value: 0.0 };

  uniforms.curTimeSeconds = { type: 'f', value: 0.0 };
  uniforms.diffuse = { type: 'c', value: color };
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

        // hash x, y, z
        float hash = fract(sin(dot(vec3(modelWorldX, modelWorldY, modelWorldZ), vec3(12.9898, 78.233, 45.164))) * 43758.5453);

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

  // TODO: Pull out to separate file
  const buildTriplanarDefsFragment = () => `
  // sharpenFactor < 1 smooths, > 1 sharpens
  vec3 generateTriplanarWeights(vec3 normal, float sharpenFactor) {
    vec3 weights = abs(normal);
    weights = pow(weights, vec3(sharpenFactor)); // sharpen to get more weight on the dominant axis
    weights = weights / dot(weights, vec3(1.0)); // normalize
    return weights;
  }

  vec4 triplanarTexture(sampler2D map, vec3 pos, vec2 uvScale, vec3 normal) {
    // TODO: make configurable
    float sharpenFactor = 12.8;
    vec3 weights = generateTriplanarWeights(normal, sharpenFactor);

    // TODO: Avoid sampling tiny-magnitude weights
    vec4 xSample = texture2D(map, pos.yz * uvScale);
    vec4 ySample = texture2D(map, pos.zx * uvScale);
    vec4 zSample = texture2D(map, pos.xy * uvScale);

    return xSample * weights.x + ySample * weights.y + zSample * weights.z;
  }

  vec4 triplanarTextureFixContrast(sampler2D map, vec3 pos, vec2 uvScale, vec3 normal) {
    // TODO: make configurable
    float sharpenFactor = 12.8;
    vec3 weights = generateTriplanarWeights(normal, sharpenFactor);

    // TODO: Avoid sampling tiny-magnitude weights
    vec4 xSample = texture2D(map, pos.yz * uvScale);
    vec4 ySample = texture2D(map, pos.zx * uvScale);
    vec4 zSample = texture2D(map, pos.xy * uvScale);

    vec4 sampled = xSample * weights.x + ySample * weights.y + zSample * weights.z;

    // TODO: Don't run if con factor is 0
    vec4 meanTextureColor = srgb2rgb(texture(map, vec2(0.5, 0.5), 99.));
    // contrast preserving interp. cf https://www.shadertoy.com/view/4dcSDr
    float divisor = sqrt(weights.x * weights.x + weights.y * weights.y + weights.z * weights.z);
    vec4 contrastCorrected = meanTextureColor + (sampled - meanTextureColor) * divisor;
    // TODO: Make mix factor configurable
    sampled = mix(sampled, contrastCorrected, 0.5);

    return sampled;
  }`;

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
          vec4 sampledDiffuseColor = texture2D( map, vUv );
          sampledDiffuseColor_ = sampledDiffuseColor;
        #endif`;
      }

      return tileBreaking.type === 'neyret'
        ? 'sampledDiffuseColor_ = textureNoTileNeyret(map, vUv);'
        : `sampledDiffuseColor_ = textureNoTile(map, noiseSampler, vUv, 0., ${fastFixMipMapTileBreakingScale});`;
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
          ? 'vec3 texelRoughness = textureNoTileNeyret(roughnessMap, vUv).xyz;'
          : `vec3 texelRoughness = textureNoTile(roughnessMap, noiseSampler, vUv, 0., ${fastFixMipMapTileBreakingScale}).xyz;`;
      else
        return `
      vec4 texelRoughness = texture2D( roughnessMap, vUv );
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

      #ifdef USE_TANGENT
        normal = normalize( vTBN * mapN );
      #else
        normal = perturbNormal2Arb( - vViewPosition, normal, mapN, faceDirection );
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

          #ifdef USE_TANGENT
            normal = normalize( vTBN * mapN );
          #else
            normal = perturbNormal2Arb( - vViewPosition, normal, mapN, faceDirection );
          #endif
        }
        `;
      }

      return `
        mapN = mapN * 2.0 - 1.0;
        mapN.xy *= normalScale;

        #ifdef USE_TANGENT
          normal = normalize( vTBN * mapN );
        #else
          normal = perturbNormal2Arb( - vViewPosition, normal, mapN, faceDirection );
        #endif
      `;
    }

    if (!normalMap) {
      return '';
    }

    const normalMapSuffix = `mapN = mapN * 2.0 - 1.0;
    mapN = normalize(mapN);
    mapN.xy *= normalScale;

    #ifdef USE_TANGENT
      normal = normalize( vTBN * mapN );
    #else
      normal = perturbNormal2Arb( - vViewPosition, normal, mapN, faceDirection );
    #endif`;

    const inner = (() => {
      if (useTriplanarMapping) {
        return `
          vec3 mapN = triplanarTexture(normalMap, pos, vec2(uvTransform[0][0], uvTransform[1][1]), vNormalAbsolute).xyz;

          ${normalMapSuffix}`;
      }

      if (tileBreaking)
        return `
    ${
      tileBreaking.type === 'neyret'
        ? 'vec3 mapN = textureNoTileNeyret(normalMap, vUv).xyz;'
        : `vec3 mapN = textureNoTile(normalMap, noiseSampler, vUv, 0., ${fastFixMipMapTileBreakingScale}).xyz;`
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
#include <uv2_pars_vertex>
#include <displacementmap_pars_vertex>
#include <color_pars_vertex>
${enableFog ? '#include <fog_pars_vertex>' : ''}
#include <normal_pars_vertex>
#include <morphtarget_pars_vertex>
#include <skinning_pars_vertex>
#include <shadowmap_pars_vertex>
#include <logdepthbuf_pars_vertex>
#include <clipping_planes_pars_vertex>

${customVertexFragment ? noiseShaders : ''}

${useGeneratedUVs ? GeneratedUVsFragment : ''}

uniform float curTimeSeconds;
varying vec3 pos;
varying vec3 vNormalAbsolute;

void main() {
  #include <begin_vertex>
  #include <morphtarget_vertex>
  #include <skinning_vertex>
  #include <displacementmap_vertex>
  #include <project_vertex>
  #include <logdepthbuf_vertex>
  #include <clipping_planes_vertex>
  #include <worldpos_vertex>
  vec4 worldPositionMine = vec4( transformed, 1.0 );
  worldPositionMine = modelMatrix * worldPositionMine;
  pos = worldPositionMine.xyz;

  #ifdef USE_INSTANCING
    pos = (instanceMatrix * vec4(pos, 1.)).xyz;
  #endif

  #include <beginnormal_vertex>
  #include <morphnormal_vertex>
  #include <skinbase_vertex>
  #include <skinnormal_vertex>
  #include <defaultnormal_vertex>
  #include <normal_vertex>

  vNormalAbsolute = normal;

  #ifdef USE_UV
  ${(() => {
    if (useGeneratedUVs) {
      return `
      // convert normal into the world space
      vec3 worldNormal = normalize(mat3(modelMatrix[0].xyz, modelMatrix[1].xyz, modelMatrix[2].xyz) * normal);
      vUv = generateUV(pos, worldNormal);
      ${randomizeUVOffset ? '' : 'vUv = ( uvTransform * vec3( vUv, 1 ) ).xy;'}`;
    }

    if (randomizeUVOffset) {
      // `randomizeUVOffset` performs UV transformation internally
      return 'vUv = uv;';
    }

    // default uv transform
    return 'vUv = ( uvTransform * vec3( uv, 1 ) ).xy;';
  })()}
  #endif

  ${buildUVVertexFragment()}

  #include <uv2_vertex>
  #include <color_vertex>
  #include <morphcolor_vertex>

  vViewPosition = - mvPosition.xyz;

  #include <shadowmap_vertex>
  ${enableFog ? '#include <fog_vertex>' : ''}

  ${customVertexFragment ?? ''}
}`,
    fragmentShader: `
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
#ifdef SPECULAR
	uniform float specularIntensity;
	uniform vec3 specularColor;
	#ifdef USE_SPECULARINTENSITYMAP
		uniform sampler2D specularIntensityMap;
	#endif
	#ifdef USE_SPECULARCOLORMAP
		uniform sampler2D specularColorMap;
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
	#ifdef USE_SHEENCOLORMAP
		uniform sampler2D sheenColorMap;
	#endif
	#ifdef USE_SHEENROUGHNESSMAP
		uniform sampler2D sheenRoughnessMap;
	#endif
#endif
varying vec3 vViewPosition;
#include <common>
#include <packing>
#include <dithering_pars_fragment>
#include <color_pars_fragment>
#include <uv_pars_fragment>
#include <uv2_pars_fragment>
#include <map_pars_fragment>
#include <alphamap_pars_fragment>
#include <alphatest_pars_fragment>
#include <aomap_pars_fragment>
#include <lightmap_pars_fragment>
#include <emissivemap_pars_fragment>
#include <bsdfs>
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
varying vec3 vWorldPosition;
varying vec3 vNormalAbsolute;
uniform mat3 uvTransform;
${normalShader ? 'uniform mat3 normalMatrix;' : ''}
${tileBreaking?.type === 'fastFixMipmap' ? 'uniform sampler2D noiseSampler;' : ''}
${useComputedNormalMap || usePackedDiffuseNormalGBA ? 'uniform vec2 normalScale;' : ''}
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
${tileBreaking?.type === 'fastFixMipmap' ? tileBreakingFragment : ''}
${
  tileBreaking?.type === 'neyret'
    ? tileBreakingNeyretFragment.replace(
        '#define Z 8.',
        `#define Z ${(tileBreaking.patchScale ?? 8).toFixed(4)}`
      )
    : ''
}
${useTriplanarMapping ? buildTriplanarDefsFragment() : ''}

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

  SceneCtx ctx = SceneCtx(cameraPosition, vUv, diffuseColor);

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
      ? `metalnessFactor = getCustomMetalness(pos, vNormalAbsolute, roughnessFactor, curTimeSeconds, ctx);`
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
		float dotNVcc = saturate( dot( geometry.clearcoatNormal, geometry.viewDir ) );
		vec3 Fcc = F_Schlick( material.clearcoatF0, material.clearcoatF90, dotNVcc );
		outgoingLight = outgoingLight * ( 1.0 - material.clearcoat * Fcc ) + clearcoatSpecular * material.clearcoat;
	#endif
	#include <output_fragment>
	${!disableToneMapping ? '#include <tonemapping_fragment>' : ''}
	#include <encodings_fragment>
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
    gl_FragColor.rgb = mix( gl_FragColor.rgb, shadowedFogColor, fogFactor );
    // gl_FragColor.w = mix( gl_FragColor.a, 0., fogFactor );
  #endif
  `
      : ''
  }
	#include <premultiplied_alpha_fragment>
	#include <dithering_fragment>
}`,
  };
};

class CustomShaderMaterial extends THREE.ShaderMaterial {
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
  const mat = new CustomShaderMaterial(buildCustomShaderArgs(props, shaders, opts));

  if (props.name) {
    mat.name = props.name;
  }
  if (props.side !== null && props.side !== undefined) {
    mat.side = props.side;
  }

  if (opts?.useComputedNormalMap || opts?.usePackedDiffuseNormalGBA) {
    mat.defines.TANGENTSPACE_NORMALMAP = '1';
    mat.uniforms.normalScale = { value: new THREE.Vector2(props.normalScale ?? 1, props.normalScale ?? 1) };
  }

  if (props.map) {
    (mat as any).map = props.map;
    (mat as any).uniforms.map.value = props.map;
  }
  if (props.normalMap) {
    (mat as any).normalMap = props.normalMap;
    (mat as any).normalMapType = props.normalMapType ?? THREE.TangentSpaceNormalMap;
    (mat as any).uniforms.normalMap.value = props.normalMap;
  }
  if (props.roughnessMap) {
    (mat as any).roughnessMap = props.roughnessMap;
    (mat as any).uniforms.roughnessMap.value = props.roughnessMap;
  }
  if (props.emissiveIntensity !== undefined) {
    (mat as any).emissiveIntensity = props.emissiveIntensity;
    (mat as any).uniforms.emissiveIntensity.value = props.emissiveIntensity;
  }
  if (props.lightMap) {
    (mat as any).lightMap = props.lightMap;
    (mat as any).uniforms.lightMap.value = props.lightMap;
    (mat as any).uniforms.lightMapIntensity.value = props.lightMapIntensity ?? 1;
  }
  if (props.transparent) {
    (mat as any).transparent = props.transparent;
  }
  if (typeof props.opacity === 'number') {
    (mat as any).opacity = props.opacity;
    (mat as any).uniforms.opacity.value = props.opacity;
  }
  if (typeof props.alphaTest === 'number') {
    (mat as any).alphaTest = props.alphaTest;
    (mat as any).uniforms.alphaTest.value = props.alphaTest;
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
    (mat as any).uniforms.diffuseLUT = { value: dataTexture };
  }

  mat.needsUpdate = true;
  mat.uniformsNeedUpdate = true;

  return mat;
};
