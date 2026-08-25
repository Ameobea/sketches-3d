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
import { referencedTabIds } from 'src/geoscript/treeCodegen';
import { resolveMaterialExtends } from './materialExtends.server';
import { LevelDefSchema, LevelDefRawSchema, isGeotoyTextureRaw, normalizeRawDefColors } from './types';
import type {
  AnyLevelTextureDef,
  CompositionTabDef,
  GeotoyCompositionAssetDef,
  GeotoyCompositionAssetDefRaw,
  GeotoyTextureDefRaw,
  LevelDef,
  LevelTextureDef,
  LevelTextureDefRaw,
  MaterialDef,
  ObjectDef,
  ObjectGroupDef,
} from './types';
import { canonicalizeInputs, djb2Hash } from './paramVariants';
import {
  getCompositionLatest,
  getCompositionVersion,
  getGeotoyAPIBaseURL,
  isCompositionDocV2,
  readVersionMetadata,
  type CompositionVersion,
  type CompositionVersionMetadata,
  type TreeEntry,
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

type CompositionDocCache = Map<string, Promise<CompositionVersion>>;

/**
 * Fetch a composition version (latest when `version` is omitted) and validate the v2 container.
 * Deduped through `cache` per level load — so an asset and a texture entry referencing the same
 * composition share one round-trip AND resolve `latest` to the same version.
 */
const fetchCompositionDoc = (
  compositionId: number,
  version: number | undefined,
  label: string,
  cache: CompositionDocCache
): Promise<CompositionVersion> => {
  const key = `${compositionId}@${version ?? 'latest'}`;
  let p = cache.get(key);
  if (!p) {
    p = (async () => {
      const adminToken = process.env.GEOTOY_ADMIN_TOKEN || undefined;
      const baseUrl = getGeotoyAPIBaseURL();
      let resolved;
      try {
        resolved =
          version !== undefined
            ? await getCompositionVersion(
                compositionId,
                version,
                globalThis.fetch,
                undefined,
                adminToken,
                baseUrl
              )
            : await getCompositionLatest(compositionId, globalThis.fetch, undefined, adminToken, baseUrl);
      } catch (err) {
        throw new Error(
          `[loadLevelData] Failed to resolve ${label} (composition ${compositionId}): ${err instanceof Error ? err.message : String(err)}`
        );
      }
      if (!isCompositionDocV2(resolved.tree)) {
        throw new Error(
          `[loadLevelData] ${label} (composition ${compositionId}) returned a non-v2 composition container`
        );
      }
      return resolved;
    })();
    cache.set(key, p);
  }
  return p;
};

const buildTabDef = (
  entry: TreeEntry,
  meta: CompositionVersionMetadata | undefined,
  render: boolean
): CompositionTabDef => {
  const tabMeta = meta?.tabs?.[entry.id];
  const out: CompositionTabDef = { id: entry.id, kind: entry.kind, tree: entry.tree };
  if (tabMeta?.preludeEjected) out.preludeEjected = true;
  if (tabMeta?.kind === 'texture' && tabMeta.textureParams && Object.keys(tabMeta.textureParams).length > 0) {
    out.textureParams = tabMeta.textureParams;
  }
  if (render) out.render = true;
  return out;
};

/**
 * Widen `runSet` (tab ids, entry tab first) with tabs pulled in transitively by qualified
 * imports (`from "<tabId>:…"`) anywhere in the set — mirrors Geotoy's `buildRunInput` scan.
 */
const closeOverImportedTabs = (runSet: string[], byId: Map<string, TreeEntry>): void => {
  for (let i = 0; i < runSet.length; i += 1) {
    for (const ref of referencedTabIds(byId.get(runSet[i])!.tree)) {
      if (byId.has(ref) && !runSet.includes(ref)) runSet.push(ref);
    }
  }
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
  synthesized: Record<string, AnyLevelTextureDef>,
  autoImported: Record<string, MaterialDef>,
  docCache: CompositionDocCache
): Promise<GeotoyCompositionAssetDef> => {
  const version = await fetchCompositionDoc(
    def.compositionId,
    def.version,
    `geotoyComposition asset "${assetId}"`,
    docCache
  );
  const trees = version.tree.trees;
  let meshEntry;
  if (def.tab !== undefined) {
    meshEntry = trees.find(t => t.id === def.tab);
    if (!meshEntry || meshEntry.kind !== 'mesh') {
      const meshTabs = trees.filter(t => t.kind === 'mesh').map(t => `"${t.id}"`);
      throw new Error(
        `[loadLevelData] geotoyComposition asset "${assetId}" (composition ${def.compositionId}): tab "${def.tab}" ${meshEntry ? 'is not a mesh tab' : 'not found'}; mesh tabs: ${meshTabs.join(', ')}`
      );
    }
  } else {
    meshEntry = trees.find(t => t.kind === 'mesh');
    if (!meshEntry) {
      throw new Error(
        `[loadLevelData] geotoyComposition asset "${assetId}" (composition ${def.compositionId}) has no mesh tree`
      );
    }
  }
  const resolved: GeotoyCompositionAssetDef = { ...def, tree: meshEntry.tree, treeId: meshEntry.id };
  // Per-tab now; this path binds the selected mesh tree, so read that tab's flag. Validated
  // rather than optional-chained: baking with the wrong prelude changes the geometry.
  const versionMeta = readVersionMetadata(version.metadata);
  if (versionMeta?.tabs?.[meshEntry.id]?.preludeEjected) resolved.preludeEjected = true;

  const byId = new Map(trees.map(t => [t.id, t]));
  const textureTabIds = new Set(trees.filter(t => t.kind === 'texture').map(t => t.id));
  // Texture tabs referenced by auto-imported palette materials; filled during inlining below.
  const refTabIds = new Set<string>();

  const palette = versionMeta?.materials;
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
            `composition ${def.compositionId} material "${name}"`,
            { assetId, compTabIds: textureTabIds, refTabIds }
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

  // Run set mirrors Geotoy's: the mesh tab, then material-referenced texture tabs (render
  // deps — their roots are side-effect-imported so `render_texture` fires), then tabs pulled
  // in transitively by qualified imports anywhere in the set.
  const runSet = [meshEntry.id, ...refTabIds];
  closeOverImportedTabs(runSet, byId);
  if (runSet.length > 1) {
    resolved.depTabs = runSet.slice(1).map(id => buildTabDef(byId.get(id)!, versionMeta, refTabIds.has(id)));
  }
  return resolved;
};

/**
 * Resolves a geotoy-sourced texture entry (`{ geotoyComposition, tab?, output }`) by inlining
 * the target texture tab + its transitive import deps as a standalone run payload. Entries
 * resolving to the same composition version + inputs share a run `key` so the client runs the
 * program once. `docCache` dedupes backend fetches within one level load.
 */
const resolveGeotoyTexture = async (
  texName: string,
  def: GeotoyTextureDefRaw,
  docCache: CompositionDocCache
): Promise<LevelTextureDef> => {
  const label = `geotoy texture "${texName}"`;
  const version = await fetchCompositionDoc(def.geotoyComposition, def.version, label, docCache);
  const trees = version.tree.trees;
  const textureTabs = trees.filter(t => t.kind === 'texture');
  let entry;
  if (def.tab !== undefined) {
    entry = trees.find(t => t.id === def.tab);
    if (!entry || entry.kind !== 'texture') {
      throw new Error(
        `[loadLevelData] ${label} (composition ${def.geotoyComposition}): tab "${def.tab}" ${entry ? 'is not a texture tab' : 'not found'}; texture tabs: ${textureTabs.map(t => `"${t.id}"`).join(', ')}`
      );
    }
  } else {
    if (textureTabs.length !== 1) {
      throw new Error(
        `[loadLevelData] ${label} (composition ${def.geotoyComposition}) has ${textureTabs.length} texture tabs; specify one with \`tab\`: ${textureTabs.map(t => `"${t.id}"`).join(', ')}`
      );
    }
    entry = textureTabs[0];
  }

  const versionMeta = readVersionMetadata(version.metadata);
  const knownOutputs = versionMeta?.tabs?.[entry.id];
  if (knownOutputs?.kind === 'texture' && knownOutputs.textureOutputs?.length) {
    if (!knownOutputs.textureOutputs.some(o => o.name === def.output)) {
      console.warn(
        `[loadLevelData] ${label}: output "${def.output}" not among tab "${entry.id}"'s last-known outputs (${knownOutputs.textureOutputs.map(o => o.name).join(', ')}); the run may not produce it`
      );
    }
  }

  const byId = new Map(trees.map(t => [t.id, t]));
  const runSet = [entry.id];
  closeOverImportedTabs(runSet, byId);
  const inputsKey = def.inputs && Object.keys(def.inputs).length > 0 ? canonicalizeInputs(def.inputs) : '';
  const run = {
    key: `${def.geotoyComposition}@${version.id}:${entry.id}${inputsKey ? `:${djb2Hash(inputsKey)}` : ''}`,
    rootTabId: entry.id,
    tabs: runSet.map(id => buildTabDef(byId.get(id)!, versionMeta, false)),
    ...(def.inputs ? { inputs: def.inputs } : {}),
  };
  return { kind: 'geotoyProcedural', tab: entry.id, output: def.output, run };
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
  const synthesizedTextures: Record<string, AnyLevelTextureDef> = {};
  // Anonymous materials auto-imported from composition palettes; merged into `materials` below.
  const autoImportedMaterials: Record<string, MaterialDef> = {};
  const flatMaterials = withLibrary.materials
    ? await resolveMaterialExtends(withLibrary.materials, resolveExternalParent, synthesizedTextures)
    : withLibrary.materials;

  const compositionDocCache = new Map<string, Promise<CompositionVersion>>();

  // Asset + material + texture resolution are independent and all make geotoy-backend
  // round-trips; overlap them.
  const [resolvedAssets, resolvedMaterials, resolvedTextures] = await Promise.all([
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
            await resolveCompositionAsset(
              assetId,
              assetDef,
              synthesizedTextures,
              autoImportedMaterials,
              compositionDocCache
            ),
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
    withLibrary.textures
      ? Promise.all(
          Object.entries(withLibrary.textures as Record<string, LevelTextureDefRaw>).map(
            async ([texName, texDef]) => [
              texName,
              isGeotoyTextureRaw(texDef)
                ? await resolveGeotoyTexture(texName, texDef, compositionDocCache)
                : texDef,
            ]
          )
        ).then(Object.fromEntries)
      : Promise.resolve(withLibrary.textures),
  ]);

  const mergedTextures = Object.keys(synthesizedTextures).length
    ? { ...resolvedTextures, ...synthesizedTextures }
    : resolvedTextures;
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
