import { existsSync, readdirSync, readFileSync, writeFileSync } from 'fs';
import { join } from 'path';
import { dev } from '$app/environment';

import { formatLevelJson } from './formatLevelJson';
import type { GeneratorFn } from './generatorTypes';
import { GENERATED_NODE_USERDATA_KEY, isObjectGroup } from './levelDefTreeUtils';
import { getAssetsDir, getLevelDir } from './levelPaths.server';
import { LevelDefSchema, LevelDefRawSchema } from './types';
import type { LevelDef, ObjectDef, ObjectGroupDef } from './types';

/**
 * Pre-compiled generator modules for production.  Vite processes this glob at
 * build time, turning each matched `.gen.ts` into a lazy chunk in the SSR
 * bundle.  The keys are repo-root-relative paths like `/src/levels/t/platforms.gen.ts`.
 */
const prodGeneratorLoaders = import.meta.glob<{ default: GeneratorFn }>('/src/levels/**/*.gen.ts');

const loadGeneratorModule = async (filePath: string): Promise<GeneratorFn> => {
  if (dev) {
    // In dev, use the Vite dev server's ssrLoadModule (set on globalThis by generatorsPlugin).
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

  // Production: use pre-compiled chunks from import.meta.glob.
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
 *
 * In development, reads fresh from disk on every request — supporting the
 * level editor workflow of edit → reload → see changes.
 */
export const loadLevelData = async (name: string): Promise<LevelDef> => {
  const levelDir = getLevelDir(name);
  const filePath = join(levelDir, 'def.json');
  const raw = readFileSync(filePath, 'utf-8');
  const json = JSON.parse(raw);

  // Keep schemas in sync while editing locally without mutating container assets in prod.
  if (dev && json.$schema !== '../schema.json') {
    json.$schema = '../schema.json';
    writeFileSync(filePath, formatLevelJson(json), 'utf-8');
  }

  // Merge materials.json if present (external values win on conflict).
  const materialsPath = join(levelDir, 'materials.json');
  if (existsSync(materialsPath)) {
    const matsJson = JSON.parse(readFileSync(materialsPath, 'utf-8'));
    if (dev && matsJson.$schema !== '../materials-schema.json') {
      matsJson.$schema = '../materials-schema.json';
      writeFileSync(materialsPath, formatLevelJson(matsJson), 'utf-8');
    }
    if (matsJson.textures) json.textures = { ...json.textures, ...matsJson.textures };
    if (matsJson.materials) json.materials = { ...json.materials, ...matsJson.materials };
  }

  // Merge objects.json over def.json objects when present.
  // objects.json entries win on ID conflict; def.json entries not present in objects.json
  // are retained (e.g. generator anchor groups that are structural, not editor-placed).
  const objectsPath = join(levelDir, 'objects.json');
  if (existsSync(objectsPath)) {
    const objsJson = JSON.parse(readFileSync(objectsPath, 'utf-8'));
    if (dev && objsJson.$schema !== '../objects-schema.json') {
      objsJson.$schema = '../objects-schema.json';
      writeFileSync(objectsPath, formatLevelJson(objsJson), 'utf-8');
    }
    const merged = new Map((json.objects ?? []).map((n: { id: string }) => [n.id, n]));
    for (const obj of objsJson.objects ?? []) merged.set(obj.id, obj);
    json.objects = [...merged.values()];
  }

  // Auto-discover *.geo files from the geo/ subdirectory.
  const geoDir = join(levelDir, 'geo');
  if (existsSync(geoDir)) {
    if (!json.assets) json.assets = {};
    for (const file of readdirSync(geoDir).filter((f: string) => f.endsWith('.geo'))) {
      const id = file.slice(0, -4);
      if (!(id in json.assets)) {
        json.assets[id] = { type: 'geoscript', file: `geo/${file}` };
      } else {
        console.warn(
          `[loadLevelData] "${name}": geo/${file} ignored — asset "${id}" already defined in def.json`
        );
      }
    }
  }

  // Run generator modules for any anchor groups that declare a `generator` field.
  // The generator's output becomes the group's children; the group's own transform
  // is fully editable in the level editor.
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
        // Recurse into children (after potential generator fill) to handle nested anchors.
        await runGeneratorsInTree(groupNode.children);
      }
    };

    await runGeneratorsInTree(json.objects ?? []);
  }

  // Parse with the raw schema, which accepts both `code` and `file` geoscript assets.
  const rawResult = LevelDefRawSchema.safeParse(json);
  if (!rawResult.success) {
    const msg = rawResult.error.issues.map(i => `  ${i.path.join('.')}: ${i.message}`).join('\n');
    throw new Error(`[loadLevelData] Invalid level def "${name}":\n${msg}`);
  }

  // Inline any `file` geoscript assets: read the file and substitute `code`.
  // Paths starting with `__ASSETS__/` are resolved relative to getAssetsDir();
  // all other paths are relative to the level directory.
  const resolvedAssets = Object.fromEntries(
    Object.entries(rawResult.data.assets).map(([assetId, assetDef]) => {
      if (assetDef.type === 'geoscript' && 'file' in assetDef) {
        const codePath = assetDef.file.startsWith('__ASSETS__/')
          ? join(getAssetsDir(), assetDef.file.slice('__ASSETS__/'.length))
          : join(levelDir, assetDef.file);
        const code = readFileSync(codePath, 'utf-8');
        return [assetId, { type: 'geoscript' as const, code, includePrelude: assetDef.includePrelude }];
      }
      return [assetId, assetDef];
    })
  );
  const inlinedDef = { ...rawResult.data, assets: resolvedAssets };

  // Validate the fully-inlined def (includes cross-reference checks).
  const result = LevelDefSchema.safeParse(inlinedDef);
  if (!result.success) {
    const msg = result.error.issues.map(i => `  ${i.path.join('.')}: ${i.message}`).join('\n');
    throw new Error(`[loadLevelData] Invalid level def "${name}" (after inlining):\n${msg}`);
  }

  return result.data;
};
