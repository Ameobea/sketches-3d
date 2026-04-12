import * as THREE from 'three';

import { buildCustomShader } from 'src/viz/shaders/customShader';
import { buildCustomBasicShader } from 'src/viz/shaders/customBasicShader';
import { MaterialClass } from 'src/viz/shaders/customShader.types';
import type {
  CustomShaderOptions,
  CustomShaderProps,
  CustomShaderShaders,
} from 'src/viz/shaders/customShader.types';
import type { MaterialDef, ShaderPropsJson, ShaderOptionsJson, ShaderShadersJson } from './types';

const SIDE_MAP: Record<NonNullable<ShaderPropsJson['side']>, THREE.Side> = {
  front: THREE.FrontSide,
  back: THREE.BackSide,
  double: THREE.DoubleSide,
};

const MATERIAL_CLASS_MAP: Record<NonNullable<ShaderOptionsJson['materialClass']>, MaterialClass> = {
  default: MaterialClass.Default,
  rock: MaterialClass.Rock,
  crystal: MaterialClass.Crystal,
  instakill: MaterialClass.Instakill,
};

const resolveShaderProps = (
  propsJson: ShaderPropsJson,
  textures: ReadonlyMap<string, THREE.Texture>
): CustomShaderProps => {
  const props: CustomShaderProps = {};

  if (propsJson.color !== undefined) props.color = propsJson.color;
  if (propsJson.roughness !== undefined) props.roughness = propsJson.roughness;
  if (propsJson.metalness !== undefined) props.metalness = propsJson.metalness;
  if (propsJson.normalScale !== undefined) props.normalScale = propsJson.normalScale;
  if (propsJson.emissiveIntensity !== undefined) props.emissiveIntensity = propsJson.emissiveIntensity;
  if (propsJson.lightMapIntensity !== undefined) props.lightMapIntensity = propsJson.lightMapIntensity;
  if (propsJson.opacity !== undefined) props.opacity = propsJson.opacity;
  if (propsJson.alphaTest !== undefined) props.alphaTest = propsJson.alphaTest;
  if (propsJson.transparent !== undefined) props.transparent = propsJson.transparent;
  if (propsJson.transmission !== undefined) props.transmission = propsJson.transmission;
  if (propsJson.ior !== undefined) props.ior = propsJson.ior;
  if (propsJson.clearcoat !== undefined) props.clearcoat = propsJson.clearcoat;
  if (propsJson.clearcoatRoughness !== undefined) props.clearcoatRoughness = propsJson.clearcoatRoughness;
  if (propsJson.clearcoatNormalScale !== undefined)
    props.clearcoatNormalScale = propsJson.clearcoatNormalScale;
  if (propsJson.iridescence !== undefined) props.iridescence = propsJson.iridescence;
  if (propsJson.sheen !== undefined) props.sheen = propsJson.sheen;
  if (propsJson.sheenColor !== undefined) props.sheenColor = propsJson.sheenColor;
  if (propsJson.sheenRoughness !== undefined) props.sheenRoughness = propsJson.sheenRoughness;
  if (propsJson.fogMultiplier !== undefined) props.fogMultiplier = propsJson.fogMultiplier;
  if (propsJson.fogShadowFactor !== undefined) props.fogShadowFactor = propsJson.fogShadowFactor;
  if (propsJson.ambientLightScale !== undefined) props.ambientLightScale = propsJson.ambientLightScale;
  if (propsJson.mapDisableDistance !== undefined) props.mapDisableDistance = propsJson.mapDisableDistance;
  if (propsJson.mapDisableTransitionThreshold !== undefined)
    props.mapDisableTransitionThreshold = propsJson.mapDisableTransitionThreshold;
  if (propsJson.ambientDistanceAmp !== undefined) props.ambientDistanceAmp = propsJson.ambientDistanceAmp;
  if (propsJson.reflection !== undefined) props.reflection = propsJson.reflection;
  if (propsJson.heightAlpha !== undefined) props.heightAlpha = propsJson.heightAlpha;

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

  if (propsJson.uvScale !== undefined) {
    props.uvTransform = new THREE.Matrix3().scale(propsJson.uvScale[0], propsJson.uvScale[1]);
  }

  return props;
};

const resolveShaderOptions = (optionsJson: ShaderOptionsJson): CustomShaderOptions => {
  const options: CustomShaderOptions = {};

  if (optionsJson.useTriplanarMapping !== undefined)
    options.useTriplanarMapping = optionsJson.useTriplanarMapping as any;
  if (optionsJson.useGeneratedUVs !== undefined) options.useGeneratedUVs = optionsJson.useGeneratedUVs;
  if (optionsJson.useWorldSpaceGeneratedUVs !== undefined)
    options.useWorldSpaceGeneratedUVs = optionsJson.useWorldSpaceGeneratedUVs;
  if (optionsJson.tileBreaking !== undefined) options.tileBreaking = optionsJson.tileBreaking;
  if (optionsJson.enableFog !== undefined) options.enableFog = optionsJson.enableFog;
  if (optionsJson.antialiasColorShader !== undefined)
    options.antialiasColorShader = optionsJson.antialiasColorShader;
  if (optionsJson.antialiasRoughnessShader !== undefined)
    options.antialiasRoughnessShader = optionsJson.antialiasRoughnessShader;
  if (optionsJson.readRoughnessMapFromRChannel !== undefined)
    options.readRoughnessMapFromRChannel = optionsJson.readRoughnessMapFromRChannel;
  if (optionsJson.disableToneMapping !== undefined)
    options.disableToneMapping = optionsJson.disableToneMapping;
  if (optionsJson.randomizeUVOffset !== undefined) options.randomizeUVOffset = optionsJson.randomizeUVOffset;
  if (optionsJson.useNoise2 !== undefined) options.useNoise2 = optionsJson.useNoise2;
  if (optionsJson.materialClass !== undefined)
    options.materialClass = MATERIAL_CLASS_MAP[optionsJson.materialClass];

  return options;
};

const resolveShaderShaders = (shadersJson: ShaderShadersJson): CustomShaderShaders => {
  const shaders: CustomShaderShaders = {};
  if (shadersJson.customVertexFragment !== undefined)
    shaders.customVertexFragment = shadersJson.customVertexFragment;
  if (shadersJson.colorShader !== undefined) shaders.colorShader = shadersJson.colorShader;
  if (shadersJson.normalShader !== undefined) shaders.normalShader = shadersJson.normalShader;
  if (shadersJson.roughnessShader !== undefined) shaders.roughnessShader = shadersJson.roughnessShader;
  if (shadersJson.roughnessReverseColorRamp !== undefined)
    shaders.roughnessReverseColorRamp = shadersJson.roughnessReverseColorRamp;
  if (shadersJson.metalnessShader !== undefined) shaders.metalnessShader = shadersJson.metalnessShader;
  if (shadersJson.metalnessReverseColorRamp !== undefined)
    shaders.metalnessReverseColorRamp = shadersJson.metalnessReverseColorRamp;
  if (shadersJson.emissiveShader !== undefined) shaders.emissiveShader = shadersJson.emissiveShader;
  if (shadersJson.iridescenceShader !== undefined) shaders.iridescenceShader = shadersJson.iridescenceShader;
  if (shadersJson.iridescenceReverseColorRamp !== undefined)
    shaders.iridescenceReverseColorRamp = shadersJson.iridescenceReverseColorRamp;
  if (shadersJson.displacementShader !== undefined)
    shaders.displacementShader = shadersJson.displacementShader;
  if (shadersJson.includeNoiseShadersVertex !== undefined)
    shaders.includeNoiseShadersVertex = shadersJson.includeNoiseShadersVertex;
  return shaders;
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
    return buildCustomShader(props, shaders, options);
  }

  if (matDef.type !== 'customBasicShader') {
    throw new Error(`buildMaterial: unhandled material type "${(matDef as { type: string }).type}"`);
  }
  const p = matDef.props ?? {};
  return buildCustomBasicShader(
    {
      color: p.color !== undefined ? new THREE.Color(p.color) : undefined,
      transparent: p.transparent,
      alphaTest: p.alphaTest,
      fogMultiplier: p.fogMultiplier,
    },
    {},
    matDef.options ?? {}
  ) as unknown as THREE.Material;
};
