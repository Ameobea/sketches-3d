import { existsSync, readFileSync } from 'fs';
import { dirname, join } from 'path';

import { getAssetsDir } from './levelPaths.server';
import { resolveGeotoyMaterial } from './geotoyMaterials.server';
import { resolveMaterialExtends } from './materialExtends.server';
import { resolveGlslPath, SHADER_GLSL_FIELDS } from './shaderFiles.server';
import {
  LibraryMaterialFileSchema,
  MaterialDefSchema,
  normalizeRawDefColors,
  TEXTURE_SLOTS,
  type LevelDefRaw,
  type MaterialDef,
  type MaterialDefRaw,
  type MaterialExtendsRef,
  type TextureDef,
} from './types';

export const LIBRARY_MATERIAL_PREFIX = '__ASSETS__/materials/';

const isLibraryRef = (ref: string | undefined): ref is string =>
  typeof ref === 'string' && ref.startsWith(LIBRARY_MATERIAL_PREFIX);

// `__ASSETS__/materials/<sub>/<name>` resolves to either the flat `<sub>/<name>.json`
// or the directory form `<sub>/<name>/<name>.json` (co-locating a material's JSON with
// its `.glsl` slot files). GLSL `{ file }` refs resolve relative to the JSON's dir.
const resolveLibraryFilePath = (ref: string): string => {
  const rel = ref.slice('__ASSETS__/'.length);
  const flat = join(getAssetsDir(), `${rel}.json`);
  if (existsSync(flat)) {
    return flat;
  }
  const base = rel.slice(rel.lastIndexOf('/') + 1);
  return join(getAssetsDir(), rel, `${base}.json`);
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
  if (mat.type !== 'customShader') return mat;
  const out = { ...mat };
  if (mat.props) {
    const props = { ...mat.props };
    for (const slot of TEXTURE_SLOTS) {
      const ref = props[slot];
      if (typeof ref === 'string') props[slot] = `${prefix}/${ref}`;
    }
    out.props = props;
  }
  if (mat.shaders?.customUniforms) {
    const customUniforms = Object.fromEntries(
      Object.entries(mat.shaders.customUniforms).map(([name, def]) => [
        name,
        def.type === 'sampler2D' ? { ...def, value: `${prefix}/${def.value}` } : def,
      ])
    );
    out.shaders = { ...mat.shaders, customUniforms };
  }
  return out;
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
  const objRefs = new Set<string>();
  collectLibraryRefs(def.objects, objRefs);

  const materials = { ...(def.materials ?? {}) };
  const textures: Record<string, TextureDef> = { ...(def.textures ?? {}) };

  const pullIn = (ref: string): void => {
    if (ref in materials) return;
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
  };

  // Only direct object material assignments (`material: "__ASSETS__/…"`) are pulled in here; a
  // library material referenced solely as an `extends` parent is resolved from disk by the extends
  // pass (`resolveExternalParent`), not registered as a level material.
  if (objRefs.size === 0) return def;
  for (const ref of objRefs) {
    pullIn(ref); // pullIn is idempotent (no-ops on refs already registered)
  }

  return { ...def, materials, textures };
};

export const libraryMaterialExists = (ref: string): boolean =>
  isLibraryRef(ref) && existsSync(resolveLibraryFilePath(ref));

/**
 * Resolves a single `__ASSETS__/materials/…` ref into a build-ready material def plus its textures,
 * applying the same GLSL-inlining, texture-prefixing, and `extends`-flattening the full-level load
 * does. Used by the editor to live-build a library material the moment it's assigned.
 */
export const resolveLibraryMaterial = async (
  ref: string
): Promise<{ material: MaterialDef; textures: Record<string, TextureDef> }> => {
  const probe = { version: 1, assets: {}, objects: [{ material: ref }] } as unknown as LevelDefRaw;
  const withLib = resolveLibraryMaterials(probe);
  const textures: Record<string, TextureDef> = { ...(withLib.textures ?? {}) };
  const flat = await resolveMaterialExtends(withLib.materials ?? {}, resolveExternalParent, textures);
  const parsed = MaterialDefSchema.safeParse(flat[ref]);
  if (!parsed.success) {
    const msg = parsed.error.issues.map(i => `  ${i.path.join('.')}: ${i.message}`).join('\n');
    throw new Error(`[resolveLibraryMaterial] Invalid library material "${ref}":\n${msg}`);
  }
  return { material: parsed.data, textures };
};

/**
 * Resolves a library/geotoy `extends` parent to a flattened def, accumulating any textures it pulls
 * in. Injected into `resolveMaterialExtends`; it lives here (not in materialExtends) to avoid an
 * import cycle, since a library parent recurses back through `resolveLibraryMaterial`.
 */
export async function resolveExternalParent(
  ref: Extract<MaterialExtendsRef, { type: 'library' | 'geotoy' }>,
  textures: Record<string, TextureDef>
): Promise<MaterialDefRaw> {
  if (ref.type === 'geotoy') {
    return (await resolveGeotoyMaterial(ref.materialId, textures)) as MaterialDefRaw;
  }
  const { material, textures: libTextures } = await resolveLibraryMaterial(
    `${LIBRARY_MATERIAL_PREFIX}${ref.path}`
  );
  Object.assign(textures, libTextures);
  return material as MaterialDefRaw;
}
