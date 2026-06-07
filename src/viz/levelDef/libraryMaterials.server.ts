import { existsSync, readFileSync } from 'fs';
import { dirname, join } from 'path';

import { getAssetsDir } from './levelPaths.server';
import { resolveGlslPath, SHADER_GLSL_FIELDS } from './shaderFiles.server';
import {
  LibraryMaterialFileSchema,
  normalizeRawDefColors,
  TEXTURE_SLOTS,
  type LevelDefRaw,
  type MaterialDefRaw,
  type TextureDef,
} from './types';

export const LIBRARY_MATERIAL_PREFIX = '__ASSETS__/materials/';

const isLibraryRef = (ref: string | undefined): ref is string =>
  typeof ref === 'string' && ref.startsWith(LIBRARY_MATERIAL_PREFIX);

const resolveLibraryFilePath = (ref: string): string => {
  const rel = ref.slice('__ASSETS__/'.length);
  return join(getAssetsDir(), `${rel}.json`);
};

const inlineGlsl = (mat: MaterialDefRaw, libFileDir: string): MaterialDefRaw => {
  if (mat.type !== 'customShader' || !mat.shaders) return mat;
  const shaders = { ...mat.shaders };
  for (const field of SHADER_GLSL_FIELDS) {
    const val = (shaders as Record<string, unknown>)[field];
    if (val !== null && typeof val === 'object' && val !== undefined && 'file' in val) {
      (shaders as Record<string, unknown>)[field] = readFileSync(
        resolveGlslPath(libFileDir, (val as { file: string }).file),
        'utf-8'
      );
    }
  }
  return { ...mat, shaders };
};

const prefixTextureRefs = (mat: MaterialDefRaw, prefix: string): MaterialDefRaw => {
  if (mat.type !== 'customShader' || !mat.props) return mat;
  const props = { ...mat.props };
  for (const slot of TEXTURE_SLOTS) {
    const ref = props[slot];
    if (typeof ref === 'string') props[slot] = `${prefix}/${ref}`;
  }
  return { ...mat, props };
};

const collectLibraryRefs = (value: unknown, out: Set<string>): void => {
  if (typeof value === 'string') {
    if (isLibraryRef(value)) out.add(value);
    return;
  }
  if (Array.isArray(value)) {
    for (const v of value) collectLibraryRefs(v, out);
    return;
  }
  if (value !== null && typeof value === 'object') {
    for (const v of Object.values(value)) collectLibraryRefs(v, out);
  }
};

/**
 * Walks the level def's object tree for `"__ASSETS__/materials/..."` string refs (in
 * `material:` fields, behavior params, anywhere) and merges each referenced library file's
 * material + textures into `def.materials` / `def.textures`.
 *
 * Library textures are prefixed with the library ref path so they can't collide with the
 * consuming level's local textures.  The object refs are left as the bare library path,
 * which becomes the key into `def.materials` post-merge.
 */
export const resolveLibraryMaterials = (def: LevelDefRaw): LevelDefRaw => {
  const refs = new Set<string>();
  collectLibraryRefs(def.objects, refs);
  if (refs.size === 0) return def;

  const materials = { ...(def.materials ?? {}) };
  const textures: Record<string, TextureDef> = { ...(def.textures ?? {}) };

  for (const ref of refs) {
    if (ref in materials) continue;
    const filePath = resolveLibraryFilePath(ref);
    if (!existsSync(filePath)) {
      throw new Error(`[resolveLibraryMaterials] Library material file not found for "${ref}": ${filePath}`);
    }
    const raw = normalizeRawDefColors(JSON.parse(readFileSync(filePath, 'utf-8')));
    const parsed = LibraryMaterialFileSchema.safeParse(raw);
    if (!parsed.success) {
      const msg = parsed.error.issues.map(i => `  ${i.path.join('.')}: ${i.message}`).join('\n');
      throw new Error(`[resolveLibraryMaterials] Invalid library material "${ref}":\n${msg}`);
    }

    const libDir = dirname(filePath);
    const inlined = inlineGlsl(parsed.data.material, libDir);
    const prefixed = prefixTextureRefs(inlined, ref);
    materials[ref] = prefixed;

    for (const [texKey, texDef] of Object.entries(parsed.data.textures ?? {})) {
      textures[`${ref}/${texKey}`] = texDef;
    }
  }

  return { ...def, materials, textures };
};
