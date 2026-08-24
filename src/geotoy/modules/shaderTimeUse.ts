import type { MaterialDef } from 'src/geoscript/materials';
import type { CustomShaderMatDef } from 'src/viz/materials/schema';

// One alternation rather than two passes: leftmost-match wins, so a `/*` sitting inside a
// `//` comment can't swallow the code after it (and hide a real read).
const stripComments = (src: string) => src.replace(/\/\*[\s\S]*?\*\/|\/\/[^\n]*/g, ' ');

const countMatches = (src: string, re: RegExp) => src.match(re)?.length ?? 0;

/** Slots that take the uniform declare it as a parameter, so a bare mention proves nothing —
 *  only references beyond the declarations are reads. Slots that don't (`pomNormalShader`)
 *  subtract nothing and stay conservative. */
const shaderReadsTime = (src: string | undefined) => {
  if (!src) {
    return false;
  }
  const stripped = stripComments(src);
  return (
    countMatches(stripped, /\bcurTimeSeconds\b/g) > countMatches(stripped, /\bfloat\s+curTimeSeconds\b/g)
  );
};

/** Every GLSL-bearing slot except the shared chunk, which gets the conservative treatment. */
type ShaderSlots = NonNullable<CustomShaderMatDef['shaders']>;
type TimeCapableSlot = Exclude<
  { [K in keyof ShaderSlots]-?: string extends NonNullable<ShaderSlots[K]> ? K : never }[keyof ShaderSlots],
  'commonShader'
>;

const TIME_CAPABLE_SLOTS = [
  'customVertexFragment',
  'colorShader',
  'lightAttenuationShader',
  'normalShader',
  'stackIndexShader',
  'roughnessShader',
  'metalnessShader',
  'emissiveShader',
  'iridescenceShader',
  'displacementShader',
  'pomHeightShader',
  'pomNormalShader',
] as const satisfies readonly TimeCapableSlot[];

// A slot added to `CustomShaderShaders` but missed here would silently stop animating, with no
// error and nothing to fail a test — so make it a type error naming the missing slot.
type UncheckedSlot = Exclude<TimeCapableSlot, (typeof TIME_CAPABLE_SLOTS)[number]>;
const _allSlotsChecked: UncheckedSlot extends never ? true : UncheckedSlot = true;
void _allSlotsChecked;

/**
 * Whether a material's output can vary with time — i.e. whether the render loop has to keep
 * running for it. `commonShader` is treated conservatively: its helpers are callable from
 * every slot, so any mention of the uniform at all counts.
 */
export const materialIsAnimated = (def: MaterialDef): boolean => {
  const shaders = (def as { shaders?: Record<string, unknown> }).shaders;
  if (!shaders) {
    return false;
  }
  const common = shaders.commonShader;
  if (typeof common === 'string' && countMatches(stripComments(common), /\bcurTimeSeconds\b/g) > 0) {
    return true;
  }
  return TIME_CAPABLE_SLOTS.some(slot => shaderReadsTime(shaders[slot] as string | undefined));
};
