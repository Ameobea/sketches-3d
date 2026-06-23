import * as THREE from 'three';

import { buildCustomShader } from 'src/viz/shaders/customShader';
import { buildCustomBasicShader } from 'src/viz/shaders/customBasicShader';
import { type MaterialClass, MATERIAL_CLASS_NAMES } from 'src/viz/shaders/customShader.types';
import type {
  CustomShaderOptions,
  CustomShaderProps,
  CustomShaderShaders,
  CustomUniformDef,
  MaterialClassName,
} from 'src/viz/shaders/customShader.types';
import type {
  CustomUniformJson,
  MaterialDef,
  ShaderPropsJson,
  ShaderOptionsJson,
  ShaderShadersJson,
} from './types';

const SIDE_MAP: Record<NonNullable<ShaderPropsJson['side']>, THREE.Side> = {
  front: THREE.FrontSide,
  back: THREE.BackSide,
  double: THREE.DoubleSide,
};

const MATERIAL_CLASS_MAP = Object.fromEntries(
  (Object.entries(MATERIAL_CLASS_NAMES) as [string, MaterialClassName][]).map(([k, v]) => [
    v,
    Number(k) as MaterialClass,
  ])
) as Record<MaterialClassName, MaterialClass>;

/** Keys present in both `Src` and `Dst` whose `Src` value type is assignable to `Dst`. */
type CopyableKeys<Src, Dst> = {
  [K in keyof Src & keyof Dst]: Src[K] extends Dst[K] ? K : never;
}[keyof Src & keyof Dst];

/** Copy each listed key from `src` to `out` when its value is defined. */
const copyDefined = <Src extends object, Dst extends object>(
  out: Dst,
  src: Src,
  keys: readonly CopyableKeys<Src, Dst>[]
): void => {
  for (const k of keys) {
    const v = src[k];
    if (v !== undefined) {
      out[k] = v as Dst[typeof k];
    }
  }
};

const resolveShaderProps = (
  propsJson: ShaderPropsJson,
  textures: ReadonlyMap<string, THREE.Texture>
): CustomShaderProps => {
  const props: CustomShaderProps = {};

  copyDefined(props, propsJson, [
    'color',
    'roughness',
    'metalness',
    'normalScale',
    'emissiveIntensity',
    'lightMapIntensity',
    'envMapIntensity',
    'opacity',
    'alphaTest',
    'transparent',
    'transmission',
    'ior',
    'clearcoat',
    'clearcoatRoughness',
    'clearcoatNormalScale',
    'iridescence',
    'sheen',
    'sheenColor',
    'sheenRoughness',
    'fogMultiplier',
    'fogShadowFactor',
    'ambientLightScale',
    'mapDisableDistance',
    'mapDisableDistanceAxes',
    'mapDisableTransitionThreshold',
    'ambientDistanceAmp',
    'heightAlpha',
  ]);

  if (propsJson.side !== undefined) props.side = SIDE_MAP[propsJson.side];

  const resolveTexture = (key: string | undefined): THREE.Texture | undefined =>
    key !== undefined ? textures.get(key) : undefined;

  props.map = resolveTexture(propsJson.map);
  props.normalMap = resolveTexture(propsJson.normalMap);
  props.roughnessMap = resolveTexture(propsJson.roughnessMap);
  props.metalnessMap = resolveTexture(propsJson.metalnessMap);
  props.lightMap = resolveTexture(propsJson.lightMap);
  props.transmissionMap = resolveTexture(propsJson.transmissionMap);
  props.clearcoatNormalMap = resolveTexture(propsJson.clearcoatNormalMap);
  props.pomHeightMap = resolveTexture(propsJson.pomHeightMap);

  if (propsJson.uvScale !== undefined) {
    props.uvTransform = new THREE.Matrix3().scale(propsJson.uvScale[0], propsJson.uvScale[1]);
  }

  return props;
};

const resolveShaderOptions = (optionsJson: ShaderOptionsJson): CustomShaderOptions => {
  const options: CustomShaderOptions = {};

  copyDefined(options, optionsJson, [
    'useGeneratedUVs',
    'useWorldSpaceUVs',
    'tileBreaking',
    'enableFog',
    'antialiasColorShader',
    'antialiasRoughnessShader',
    'readRoughnessMapFromRChannel',
    'disableToneMapping',
    'randomizeUVOffset',
    'useNoise2',
    'pom',
  ]);

  if (optionsJson.useTriplanarMapping !== undefined)
    options.useTriplanarMapping = optionsJson.useTriplanarMapping as any;
  if (optionsJson.materialClass !== undefined)
    options.materialClass = MATERIAL_CLASS_MAP[optionsJson.materialClass];

  return options;
};

const resolveCustomUniforms = (json: Record<string, CustomUniformJson>): Record<string, CustomUniformDef> => {
  const out: Record<string, CustomUniformDef> = {};
  for (const [name, def] of Object.entries(json)) {
    const { vertex } = def;
    switch (def.type) {
      case 'float':
      case 'int':
        out[name] = { type: def.type, value: def.value, vertex };
        break;
      case 'vec2':
        out[name] = { type: 'vec2', value: new THREE.Vector2().fromArray(def.value), vertex };
        break;
      case 'vec3':
        out[name] = { type: 'vec3', value: new THREE.Vector3().fromArray(def.value), vertex };
        break;
      case 'vec4':
        out[name] = { type: 'vec4', value: new THREE.Vector4().fromArray(def.value), vertex };
        break;
      case 'mat3':
        out[name] = { type: 'mat3', value: new THREE.Matrix3().fromArray(def.value), vertex };
        break;
      case 'mat4':
        out[name] = { type: 'mat4', value: new THREE.Matrix4().fromArray(def.value), vertex };
        break;
    }
  }
  return out;
};

const resolveShaderShaders = (shadersJson: ShaderShadersJson): CustomShaderShaders => {
  const shaders: CustomShaderShaders = {};
  copyDefined(shaders, shadersJson, [
    'customVertexFragment',
    'commonShader',
    'colorShader',
    'lightAttenuationShader',
    'normalShader',
    'roughnessShader',
    'roughnessReverseColorRamp',
    'metalnessShader',
    'metalnessReverseColorRamp',
    'emissiveShader',
    'iridescenceShader',
    'iridescenceReverseColorRamp',
    'displacementShader',
    'includeNoiseShadersVertex',
    'pomHeightShader',
    'pomNormalShader',
    'constants',
  ]);
  if (shadersJson.customUniforms !== undefined)
    shaders.customUniforms = resolveCustomUniforms(shadersJson.customUniforms);
  return shaders;
};

/**
 * Stamp the channels a MaterialDef carries into `mat.userData` so the level loader's
 * post-build propagation (`propagateMatUserDataToEntity` in loadLevelDef.ts) can push
 * them onto owning entities.  Add new material-driven channels here AND in the loader's
 * entity-propagation helper — those two are the matched pair.
 */
export const stampMaterialMetaUserData = (matDef: MaterialDef, mat: THREE.Material) => {
  if (matDef.nonPermeable) {
    mat.userData.nonPermeable = true;
  }
  if (matDef.parkour?.boostSurface) {
    mat.userData.boostSurfaceConfig = matDef.parkour.boostSurface;
  }
  if (matDef.externalVelocityAirDampingFactor) {
    mat.userData.externalVelocityAirDampingFactor = matDef.externalVelocityAirDampingFactor;
  }
  if (matDef.externalVelocityGroundDampingFactor) {
    mat.userData.externalVelocityGroundDampingFactor = matDef.externalVelocityGroundDampingFactor;
  }
};

/**
 * Build a Three.js material from a serializable `MaterialDef` and the resolved texture registry.
 */
export const buildMaterial = (
  matDef: MaterialDef,
  textures: ReadonlyMap<string, THREE.Texture>
): THREE.Material => {
  if (matDef.type === 'customShader') {
    const props = matDef.props ? resolveShaderProps(matDef.props, textures) : {};
    const shaders = matDef.shaders ? resolveShaderShaders(matDef.shaders) : {};
    const options = matDef.options ? resolveShaderOptions(matDef.options) : {};
    if (matDef.nonPermeable) {
      options.noOcclusion = true;
    }
    if (matDef.inlineEmissiveBypass) {
      options.inlineEmissiveBypass = true;
    }
    const mat = buildCustomShader(props, shaders, options);
    stampMaterialMetaUserData(matDef, mat);
    return mat;
  }

  if (matDef.type !== 'customBasicShader') {
    throw new Error(`buildMaterial: unhandled material type "${(matDef as { type: string }).type}"`);
  }
  const p = matDef.props ?? {};
  const mat = buildCustomBasicShader(
    {
      color: p.color !== undefined ? new THREE.Color(p.color) : undefined,
      transparent: p.transparent,
      alphaTest: p.alphaTest,
      fogMultiplier: p.fogMultiplier,
    },
    {},
    matDef.options ?? {}
  ) as unknown as THREE.Material;
  stampMaterialMetaUserData(matDef, mat);
  return mat;
};
