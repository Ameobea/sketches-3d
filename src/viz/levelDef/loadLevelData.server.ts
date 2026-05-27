import { existsSync, readFileSync } from 'fs';
import { join } from 'path';
import { dev } from '$app/environment';

import type { GeneratorFn } from './generatorTypes';
import { GENERATED_NODE_USERDATA_KEY, isObjectGroup } from './levelDefTreeUtils';
import { getAssetsDir } from './levelPaths.server';
import { readLevelSourceFiles } from './levelSourceFiles.server';
import { SHADER_GLSL_FIELDS, resolveGlslPath } from './shaderFiles.server';
import { LevelDefSchema, LevelDefRawSchema, normalizeRawDefColors } from './types';
import type { LevelDef, ObjectDef, ObjectGroupDef } from './types';

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

  const resolvedAssets = Object.fromEntries(
    Object.entries(rawResult.data.assets).map(([assetId, assetDef]) => {
      if (assetDef.type === 'geoscript' && 'file' in assetDef) {
        const codePath = assetDef.file.startsWith('__ASSETS__/')
          ? join(getAssetsDir(), assetDef.file.slice('__ASSETS__/'.length))
          : join(levelDir, assetDef.file);
        const code = readFileSync(codePath, 'utf-8');
        const { file: _file, ...rest } = assetDef;
        return [assetId, { ...rest, type: 'geoscript' as const, code }];
      }
      return [assetId, assetDef];
    })
  );

  const resolvedMaterials = rawResult.data.materials
    ? Object.fromEntries(
        Object.entries(rawResult.data.materials).map(([matId, matDef]) => {
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
      )
    : rawResult.data.materials;

  const inlinedDef = { ...rawResult.data, assets: resolvedAssets, materials: resolvedMaterials };

  const result = LevelDefSchema.safeParse(inlinedDef);
  if (!result.success) {
    const msg = result.error.issues.map(i => `  ${i.path.join('.')}: ${i.message}`).join('\n');
    throw new Error(`[loadLevelData] Invalid level def "${name}" (after inlining):\n${msg}`);
  }

  return result.data;
};
