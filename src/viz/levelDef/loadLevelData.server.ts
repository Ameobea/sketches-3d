import { existsSync, readFileSync } from 'fs';
import { join } from 'path';
import { dev } from '$app/environment';

import type { GeneratorFn } from './generatorTypes';
import { GENERATED_NODE_USERDATA_KEY, isObjectGroup } from './levelDefTreeUtils';
import { getAssetsDir } from './levelPaths.server';
import { readLevelSourceFiles } from './levelSourceFiles.server';
import { SHADER_GLSL_FIELDS, resolveGlslPath } from './shaderFiles.server';
import { resolveExternalParent, resolveLibraryMaterials } from './libraryMaterials.server';
import { inlineGeotoyMaterialTextures, resolveGeotoyMaterial } from './geotoyMaterials.server';
import { compMaterialKey } from 'src/geoscript/runner/bakeComposition';
import { resolveMaterialExtends } from './materialExtends.server';
import { LevelDefSchema, LevelDefRawSchema, normalizeRawDefColors } from './types';
import type {
  GeotoyCompositionAssetDef,
  GeotoyCompositionAssetDefRaw,
  LevelDef,
  MaterialDef,
  ObjectDef,
  ObjectGroupDef,
  TextureDef,
} from './types';
import {
  getCompositionLatest,
  getCompositionVersion,
  getGeotoyAPIBaseURL,
  isTreeDefV1,
} from 'src/geoscript/geotoyAPIClient';

/**
 * Pre-compiled generator modules for production.  Vite processes this glob at
 * build time, turning each matched `.gen.ts` into a lazy chunk in the SSR
 * bundle.  The keys are repo-root-relative paths like `/src/levels/t/platforms.gen.ts`.
 */
const prodGeneratorLoaders = import.meta.glob<{ default: GeneratorFn }>('/src/levels/**/*.gen.ts');

const loadGeneratorModule = async (filePath: string): Promise<GeneratorFn> => {
  if (dev) {
    // In dev, use the Vite dev server's ssrLoadModule.
    //
    // Invalidate first so edits are always reflected without restart.
    const server = (globalThis as Record<string, any>).__viteDevServer;
    if (!server) {
      throw new Error('[loadGeneratorModule] Vite dev server not available on globalThis');
    }
    const mods = server.moduleGraph.getModulesByFile(filePath);
    if (mods) {
      for (const mod of mods) server.moduleGraph.invalidateModule(mod);
    }
    const mod = await server.ssrLoadModule(filePath);
    return (mod.default ?? mod) as GeneratorFn;
  }

  const key = filePath.replace(process.cwd(), '');
  const loader = prodGeneratorLoaders[key];
  if (!loader) {
    throw new Error(
      `[loadGeneratorModule] No pre-built generator for "${key}". ` +
        `Known generators: ${Object.keys(prodGeneratorLoaders).join(', ')}`
    );
  }
  const mod = await loader();
  return mod.default;
};

const markGeneratedNode = (node: ObjectDef | ObjectGroupDef): ObjectDef | ObjectGroupDef => {
  const nextUserData = { ...(node.userData ?? {}), [GENERATED_NODE_USERDATA_KEY]: true };
  if (isObjectGroup(node)) {
    return {
      ...node,
      userData: nextUserData,
      children: node.children.map(markGeneratedNode),
    };
  }

  return {
    ...node,
    userData: nextUserData,
  };
};

/**
 * Resolves a `geotoyComposition` asset by fetching its tree from the geotoy backend and
 * inlining it, so the client receives a self-contained payload (no compositions-API auth at
 * level load). Private/unshared comps resolve via `GEOTOY_ADMIN_TOKEN`; missing, inaccessible,
 * or non-v1 comps are hard failures.
 */
const resolveCompositionAsset = async (
  assetId: string,
  def: GeotoyCompositionAssetDefRaw,
  synthesized: Record<string, TextureDef>,
  autoImported: Record<string, MaterialDef>
): Promise<GeotoyCompositionAssetDef> => {
  const adminToken = process.env.GEOTOY_ADMIN_TOKEN || undefined;
  const baseUrl = getGeotoyAPIBaseURL();
  let version;
  try {
    version =
      def.version !== undefined
        ? await getCompositionVersion(
            def.compositionId,
            def.version,
            globalThis.fetch,
            undefined,
            adminToken,
            baseUrl
          )
        : await getCompositionLatest(def.compositionId, globalThis.fetch, undefined, adminToken, baseUrl);
  } catch (err) {
    throw new Error(
      `[loadLevelData] Failed to resolve geotoyComposition asset "${assetId}" (composition ${def.compositionId}): ${err instanceof Error ? err.message : String(err)}`
    );
  }
  if (!isTreeDefV1(version.tree)) {
    throw new Error(
      `[loadLevelData] geotoyComposition asset "${assetId}" (composition ${def.compositionId}) returned a non-v1 tree`
    );
  }
  const resolved: GeotoyCompositionAssetDef = { ...def, tree: version.tree };
  if (version.metadata?.preludeEjected) resolved.preludeEjected = true;

  const palette = version.metadata?.materials;
  if (palette) {
    const defId = palette.defaultMaterialID;
    if (defId != null) resolved.defaultMaterialName = palette.materials[defId]?.name;

    // Dedup palette materials by geotoy name (first wins) — the runtime `set_material` name list and
    // the auto-import source both derive from it.
    const byName = new Map<string, MaterialDef>();
    for (const m of Object.values(palette.materials)) if (!byName.has(m.name)) byName.set(m.name, m);
    resolved.materialNames = [...byName.keys()];

    // Auto-import each palette material as an anonymous `__comp:` level material so unmapped
    // composition meshes render the composition's own material instead of the placeholder. Prod
    // imports only names not overridden by `materialMap` (lean load); dev imports all so the editor
    // can revert any row to its composition default.
    const explicit = def.materialMap ?? {};
    await Promise.all(
      [...byName].map(async ([name, paletteDef]) => {
        if (!dev && name in explicit) return;
        try {
          autoImported[compMaterialKey(assetId, name)] = await inlineGeotoyMaterialTextures(
            paletteDef,
            synthesized,
            `composition ${def.compositionId} material "${name}"`
          );
        } catch (err) {
          console.warn(
            `[loadLevelData] composition "${assetId}": failed to auto-import material "${name}": ${err instanceof Error ? err.message : String(err)}`
          );
        }
      })
    );
  } else {
    console.warn(
      `[loadLevelData] geotoyComposition asset "${assetId}" (composition ${def.compositionId}) has no material palette in metadata; \`set_material\` calls in its tree may fail`
    );
  }
  return resolved;
};

/**
 * Reads a level definition from `<levelsDir>/<name>/def.json`, merges any
 * optional `materials.json` and `objects.json` sidecar files, auto-discovers
 * `.geo` files from the level's `geo/` subdirectory, resolves any `file`
 * references in geoscript assets (inlining the code from disk), validates
 * the result, and returns it.
 *
 * In development, also fixes the `$schema` field in each file on disk when
 * it's missing or stale, so IDEs get autocomplete and inline validation
 * automatically.
 *
 * Intended for use in SvelteKit `+page.server.ts` load functions so the level
 * definition is baked into the page response rather than fetched separately.
 */
export const loadLevelData = async (name: string): Promise<LevelDef> => {
  const { levelDir, def: json } = readLevelSourceFiles(name, { syncSchemas: dev });

  const generators = json.generators as
    | Record<string, { file: string; params?: Record<string, unknown> }>
    | undefined;
  if (generators && Object.keys(generators).length > 0) {
    const runGeneratorsInTree = async (
      nodes: import('./types').ObjectGroupDef['children']
    ): Promise<void> => {
      for (const node of nodes) {
        if (!isObjectGroup(node)) continue;
        const groupNode = node as import('./types').ObjectGroupDef;
        if (groupNode.generator) {
          const genDef = generators[groupNode.generator];
          if (!genDef) {
            throw new Error(
              `[loadLevelData] Group "${groupNode.id}" references unknown generator "${groupNode.generator}"`
            );
          }
          const genPath = join(levelDir, genDef.file);
          if (!existsSync(genPath)) {
            throw new Error(
              `[loadLevelData] Generator "${genDef.file}" for group "${groupNode.id}" not found in level "${name}"`
            );
          }
          const fn = await loadGeneratorModule(genPath);
          console.log(
            `[loadLevelData] Running generator "${groupNode.generator}" for group "${groupNode.id}"...`
          );
          const result = await fn({
            def: json as import('./types').LevelDefRaw,
            physics: (json.physics as import('./types').ScenePhysicsDef) ?? {},
            params: genDef.params ?? {},
          });
          groupNode.children = result.objects.map(markGeneratedNode);
        }
        await runGeneratorsInTree(groupNode.children);
      }
    };

    await runGeneratorsInTree(json.objects ?? []);
  }

  const normalized = normalizeRawDefColors(json);

  const rawResult = LevelDefRawSchema.safeParse(normalized);
  if (!rawResult.success) {
    const msg = rawResult.error.issues.map(i => `  ${i.path.join('.')}: ${i.message}`).join('\n');
    throw new Error(`[loadLevelData] Invalid level def "${name}":\n${msg}`);
  }

  const withLibrary = resolveLibraryMaterials(rawResult.data);
  // Flattening `extends` can itself pull in geotoy/library parents and synthesize their textures.
  const synthesizedTextures: Record<string, TextureDef> = {};
  // Anonymous materials auto-imported from composition palettes; merged into `materials` below.
  const autoImportedMaterials: Record<string, MaterialDef> = {};
  const flatMaterials = withLibrary.materials
    ? await resolveMaterialExtends(withLibrary.materials, resolveExternalParent, synthesizedTextures)
    : withLibrary.materials;

  // Asset + material resolution are independent and both make geotoy-backend round-trips; overlap them.
  const [resolvedAssets, resolvedMaterials] = await Promise.all([
    Promise.all(
      Object.entries(withLibrary.assets).map(async ([assetId, assetDef]) => {
        if (assetDef.type === 'geoscript' && 'file' in assetDef) {
          const codePath = assetDef.file.startsWith('__ASSETS__/')
            ? join(getAssetsDir(), assetDef.file.slice('__ASSETS__/'.length))
            : join(levelDir, assetDef.file);
          const code = readFileSync(codePath, 'utf-8');
          const { file: _file, ...rest } = assetDef;
          return [assetId, { ...rest, type: 'geoscript' as const, code }];
        }
        if (assetDef.type === 'geotoyComposition') {
          return [
            assetId,
            await resolveCompositionAsset(assetId, assetDef, synthesizedTextures, autoImportedMaterials),
          ];
        }
        return [assetId, assetDef];
      })
    ).then(Object.fromEntries),
    flatMaterials
      ? Promise.all(
          Object.entries(flatMaterials).map(async ([matId, matDef]) => {
            if (matDef.type === 'geotoyMaterial')
              return [matId, await resolveGeotoyMaterial(matDef.materialId, synthesizedTextures, matId)];
            if (matDef.type !== 'customShader' || !matDef.shaders) return [matId, matDef];
            const shaders = { ...matDef.shaders };
            for (const field of SHADER_GLSL_FIELDS) {
              const val = shaders[field];
              if (val !== null && typeof val === 'object' && 'file' in val) {
                shaders[field] = readFileSync(resolveGlslPath(levelDir, val.file), 'utf-8');
              }
            }
            return [matId, { ...matDef, shaders }];
          })
        ).then(Object.fromEntries)
      : Promise.resolve(flatMaterials),
  ]);

  const mergedTextures = Object.keys(synthesizedTextures).length
    ? { ...withLibrary.textures, ...synthesizedTextures }
    : withLibrary.textures;
  const mergedMaterials = Object.keys(autoImportedMaterials).length
    ? { ...(resolvedMaterials ?? {}), ...autoImportedMaterials }
    : resolvedMaterials;
  const inlinedDef = {
    ...withLibrary,
    assets: resolvedAssets,
    materials: mergedMaterials,
    textures: mergedTextures,
  };

  const result = LevelDefSchema.safeParse(inlinedDef);
  if (!result.success) {
    const msg = result.error.issues.map(i => `  ${i.path.join('.')}: ${i.message}`).join('\n');
    throw new Error(`[loadLevelData] Invalid level def "${name}" (after inlining):\n${msg}`);
  }

  return result.data;
};
