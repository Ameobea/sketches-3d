import { existsSync, readFileSync, writeFileSync } from 'fs';
import { join } from 'path';

import { getAssetsDir } from './levelPaths.server';
import type { MaterialDef, MaterialDefRaw } from './types';

/** Shader fields that may be externalized to `.glsl` files via `{ file }` in the raw def. */
export const SHADER_GLSL_FIELDS = [
  'customVertexFragment',
  'commonShader',
  'colorShader',
  'lightAttenuationShader',
  'normalShader',
  'roughnessShader',
  'metalnessShader',
  'emissiveShader',
  'iridescenceShader',
  'displacementShader',
  'pomHeightShader',
  'pomNormalShader',
] as const;

/** Resolves a shader `file` reference to an absolute path (`__ASSETS__/` → shared assets dir). */
export const resolveGlslPath = (levelDir: string, file: string): string =>
  file.startsWith('__ASSETS__/')
    ? join(getAssetsDir(), file.slice('__ASSETS__/'.length))
    : join(levelDir, file);

const isFileRef = (v: unknown): v is { file: string } =>
  typeof v === 'object' && v !== null && 'file' in v && typeof (v as { file: unknown }).file === 'string';

/**
 * The level editor only ever holds the *merged* material def — `loadLevelData`
 * inlines `{ file }` shader references into GLSL strings at load. So a material
 * saved back from the editor would otherwise write those inlined strings
 * straight into `materials.json`, clobbering the externalized `.glsl` files.
 *
 * Given the incoming (merged) def and the previous on-disk raw def, re-attaches
 * each `{ file }` reference the previous def used. If the incoming GLSL differs
 * from the file (e.g. a future shader-editing UI changed it), the edit is
 * written back to the `.glsl` file so the reference is preserved rather than
 * silently lost.
 */
export const externalizeShaderFiles = (
  incoming: MaterialDef,
  prevRaw: MaterialDefRaw | undefined,
  levelDir: string
): MaterialDefRaw => {
  if (
    incoming.type !== 'customShader' ||
    !incoming.shaders ||
    prevRaw?.type !== 'customShader' ||
    !prevRaw.shaders
  ) {
    return incoming as MaterialDefRaw;
  }

  const prevShaders = prevRaw.shaders as Record<string, unknown>;
  const shaders: Record<string, unknown> = { ...incoming.shaders };

  for (const field of SHADER_GLSL_FIELDS) {
    const prevVal = prevShaders[field];
    const incomingVal = shaders[field];
    if (!isFileRef(prevVal) || typeof incomingVal !== 'string') continue;

    const path = resolveGlslPath(levelDir, prevVal.file);
    if (!existsSync(path)) continue; // referenced file is gone; leave the GLSL inline

    if (readFileSync(path, 'utf-8') !== incomingVal) writeFileSync(path, incomingVal, 'utf-8');
    shaders[field] = { file: prevVal.file };
  }

  return { ...incoming, shaders } as MaterialDefRaw;
};
