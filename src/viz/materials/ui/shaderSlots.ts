import { buildDefaultShaders } from 'src/geoscript/geotoyMaterialConvert';
import type { CustomShaderMatDef } from 'src/viz/materials/schema';

/** GLSL slot subset the shader editor manages; mapped to the shared `${slot}Shader` keys. */
export type ShaderSlots = {
  color?: string;
  common?: string;
  lightAttenuation?: string;
  roughness?: string;
  metalness?: string;
  iridescence?: string;
  pomHeight?: string;
  pomNormal?: string;
};

type Shaders = CustomShaderMatDef['shaders'];

const SHADER_SLOT_MAP = {
  color: 'colorShader',
  common: 'commonShader',
  lightAttenuation: 'lightAttenuationShader',
  roughness: 'roughnessShader',
  metalness: 'metalnessShader',
  iridescence: 'iridescenceShader',
  pomHeight: 'pomHeightShader',
  pomNormal: 'pomNormalShader',
} as const;

export const sharedToSlots = (sh: Shaders): ShaderSlots => ({
  color: sh?.colorShader,
  common: sh?.commonShader,
  lightAttenuation: sh?.lightAttenuationShader,
  roughness: sh?.roughnessShader,
  metalness: sh?.metalnessShader,
  iridescence: sh?.iridescenceShader,
  pomHeight: sh?.pomHeightShader,
  pomNormal: sh?.pomNormalShader,
});

/** Map short slot names back to `${slot}Shader` keys, dropping any slot equal to its default
 *  template (absent ⇒ default) while preserving non-GLSL keys like the reverse-color ramps. */
export const slotsToShared = (existing: Shaders, slots: ShaderSlots): Shaders => {
  const defaults = buildDefaultShaders();
  const out: Record<string, unknown> = { ...existing };
  for (const [slot, key] of Object.entries(SHADER_SLOT_MAP)) {
    const v = slots[slot as keyof ShaderSlots];
    if (v && v !== defaults[slot as keyof typeof defaults]) out[key] = v;
    else delete out[key];
  }
  return Object.keys(out).length ? (out as Shaders) : undefined;
};
