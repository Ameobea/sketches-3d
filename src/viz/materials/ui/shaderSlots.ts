import { SHADER_SLOTS, type CustomShaderMatDef } from 'src/viz/materials/schema';

type EditorSlotEntry = Extract<(typeof SHADER_SLOTS)[number], { editor: string }>;
export type ShaderSlotName = EditorSlotEntry['editor'];
/** GLSL slot subset the shader editor manages, keyed by sidebar short name. */
export type ShaderSlots = Partial<Record<ShaderSlotName, string>>;

const EDITOR_SLOTS = SHADER_SLOTS.filter(s => s.editor !== null) as readonly EditorSlotEntry[];

/** Sidebar slot names for the editor, honoring `pomOnly`. */
export const editorSlotNames = (type: 'physical' | 'basic', pomEnabled: boolean): ShaderSlotName[] =>
  type === 'basic' ? ['color'] : EDITOR_SLOTS.filter(s => pomEnabled || !('pomOnly' in s)).map(s => s.editor);

/** Identity/no-op template for each slot, shown in the editor when a slot is unset; a slot
 *  equal to its template is stored as absent. */
export const buildDefaultShaders = (): Required<ShaderSlots> => ({
  common: `// Shared GLSL emitted before every other slot — declare structs, constants, and
// helper functions used by multiple slots here so the logic lives in one place.`,
  color: `vec4 getFragColor(vec3 baseColor, vec3 pos, vec3 normal, float curTimeSeconds, SceneCtx ctx) {
  return vec4(baseColor, 1.0);
}`,
  lightAttenuation: `// Returns (directMul, indirectMul) in [0,1], scaling direct/indirect light for
// procedural shadow + AO. (1.0, 1.0) = no attenuation.
vec2 getLightAttenuation(vec3 pos, vec3 normal, float curTimeSeconds, SceneCtx ctx) {
  return vec2(1.0);
}`,
  stackIndex: `// Per-fragment interpolation index for stack-backed texture slots
// (map/normalMap/roughnessMap). Return t in [0, 1]: slice 0 at t=0, the last slice at
// t=1. Only runs when a texture stack is assigned; overrides the stackIndex prop.
float getStackIndex(vec3 pos, vec3 normal, vec2 uv, float curTimeSeconds, SceneCtx ctx) {
  return 0.0;
}`,
  roughness: `float getCustomRoughness(vec3 pos, vec3 normal, float baseRoughness, float curTimeSeconds, SceneCtx ctx) {
  return baseRoughness;
}`,
  metalness: `float getCustomMetalness(vec3 pos, vec3 normal, float baseMetalness, float curTimeSeconds, SceneCtx ctx) {
  return baseMetalness;
}`,
  emissive: `// Emissive radiance in linear color; HDR values (>1) bloom harder. \`e\` is the base
// emissive (color x map x intensity). Author un-fogged; with inline emissive bypass
// enabled this skips tone mapping and feeds the bloom pass.
vec3 getCustomEmissive(vec3 pos, vec3 e, float curTimeSeconds, SceneCtx ctx) {
  return e;
}`,
  iridescence: `float getCustomIridescence(vec3 pos, vec3 normal, float baseIridescence, float curTimeSeconds, SceneCtx ctx) {
  return baseIridescence;
}`,
  pomHeight: `// Carved depth in [0, 1]: 0 = base surface, 1 = a full pom.depth carved inward.
float getPomHeight(vec3 pos, vec3 normal, float curTimeSeconds) {
  return 0.0;
}`,
  pomNormal: `// Closed-form relief normal for the carved POM floor (world space); requires pomHeight.
// \`aa\` is the pixel-footprint half-width in world units — fade detail with reliefAAFade(aa, w).
vec3 getPomNormal(vec3 pos, vec3 N, float depth, float t, float aa) {
  return N;
}`,
});

type Shaders = CustomShaderMatDef['shaders'];

const SHADER_SLOT_MAP = Object.fromEntries(EDITOR_SLOTS.map(s => [s.editor, s.key])) as Record<
  ShaderSlotName,
  EditorSlotEntry['key']
>;

export const sharedToSlots = (sh: Shaders): ShaderSlots =>
  Object.fromEntries(EDITOR_SLOTS.map(s => [s.editor, sh?.[s.key] as string | undefined])) as ShaderSlots;

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
